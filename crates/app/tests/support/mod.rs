#![allow(dead_code)]

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use livewall::proto::{self, Request, RequestEnvelope, Response, ResponseBody};
use serde_json::Value;

pub const POLL: Duration = Duration::from_millis(200);
pub const LAUNCH_TIMEOUT: Duration = Duration::from_secs(20);
pub const TAKEOVER_TIMEOUT: Duration = Duration::from_secs(20);
pub const RESTORE_TIMEOUT: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_mins(2);
const SETTLE: Duration = Duration::from_secs(4);

pub fn send(socket: &Path, request: Request, timeout: Duration) -> Option<Response> {
    let mut stream = UnixStream::connect(socket).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.write_all(&proto::encode(&RequestEnvelope::new(request))).ok()?;
    stream.flush().ok()?;
    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    reader.read_until(b'\n', &mut line).ok()?;
    if line.is_empty() {
        return None;
    }
    return proto::decode::<Response>(&line).ok();
}

pub fn wait_for_socket(socket: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if UnixStream::connect(socket).is_ok() {
            return;
        }
        std::thread::sleep(POLL);
    }
    panic!("LiveWall socket did not appear at {}", socket.display());
}

pub struct Sandbox {
    dir: tempfile::TempDir,
    data: PathBuf,
    child: RefCell<Option<Child>>,
}

impl Sandbox {
    /// The data folder lives under `target/tmp/e2e-data/<label>` and is wiped at the START of a
    /// run, never at the end: wallpapers applied during a test keep pointing at live files after
    /// the run, so a SIGKILL test cannot leave gray placeholders in Mission Control behind.
    pub fn new(label: &str) -> Self {
        let dir = tempfile::Builder::new().prefix(&format!("livewall-e2e-{label}-")).tempdir().expect("tempdir");
        let data = Path::new(env!("CARGO_TARGET_TMPDIR")).join("e2e-data").join(label);
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).expect("e2e data dir");
        return Self { dir, data, child: RefCell::new(None) };
    }

    pub fn path(&self) -> &Path {
        return self.dir.path();
    }

    pub fn data_dir(&self) -> PathBuf {
        return self.data.clone();
    }

    pub fn socket(&self) -> PathBuf {
        return self.dir.path().join("control.sock");
    }

    pub fn launch(&self) {
        let data = self.data_dir();
        std::fs::create_dir_all(&data).expect("data dir");
        let child = Command::new(env!("CARGO_BIN_EXE_LiveWall"))
            .env("LIVEWALL_DATA_DIR", &data)
            .env("LIVEWALL_SOCKET", self.socket())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn LiveWall");
        *self.child.borrow_mut() = Some(child);
        wait_for_socket(&self.socket(), LAUNCH_TIMEOUT);
    }

    pub fn reap(&self) {
        if let Some(mut child) = self.child.borrow_mut().take() {
            let _ = child.wait();
        }
    }

    pub fn kill9(&self) {
        if let Some(mut child) = self.child.borrow_mut().take() {
            unsafe { libc::kill(child.id().cast_signed(), libc::SIGKILL) };
            let _ = child.wait();
        }
    }

    pub fn quit(&self) {
        let _ = self.request(Request::Quit);
        self.reap();
    }

    pub fn ok(&self, request: Request) -> Value {
        let response = self.request(request);
        return match response.body {
            ResponseBody::Ok { data } => data,
            ResponseBody::Err(body) => panic!("unexpected error {body:?}"),
        };
    }

    pub fn request(&self, request: Request) -> Response {
        return self.request_opt(request).expect("ipc request");
    }

    pub fn request_opt(&self, request: Request) -> Option<Response> {
        return send(&self.socket(), request, REQUEST_TIMEOUT);
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.get_mut().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub struct Guard {
    pub pictures: Vec<String>,
}

impl Guard {
    pub fn capture() -> Self {
        forget_choices();
        return Self { pictures: current_pictures() };
    }

    fn restore(&self) {
        if self.pictures.is_empty() {
            return;
        }
        let count = osascript(&["-e", "tell application \"System Events\" to count of desktops"])
            .trim()
            .parse::<usize>()
            .unwrap_or(self.pictures.len());
        let fallback = &self.pictures[0];
        for index in 1..=count.max(1) {
            let picture = self.pictures.get(index - 1).unwrap_or(fallback);
            let script = format!(
                "tell application \"System Events\" to set picture of desktop {index} to \"{}\"",
                escape(picture)
            );
            let _ = osascript(&["-e", &script]);
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.restore();
        forget_choices();
    }
}

const CHOICES: &str = "Library/Containers/com.apple.wallpaper.extension.image/Data/Library/Preferences/com.apple.wallpaper.extension.image";
const CHOICE_COPIES: &str = "Library/Containers/com.apple.wallpaper.extension.image/Data/Library/Caches";
const IMAGE_FILES: &str = "ChoiceRequests.ImageFiles";

pub fn forget_choices() {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return;
    };
    let domain = home.join(CHOICES);
    let Ok(output) = Command::new("defaults").arg("export").arg(&domain).arg("-").output() else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let Ok(plist::Value::Dictionary(mut prefs)) = plist::Value::from_reader_xml(output.stdout.as_slice()) else {
        return;
    };
    let Some(plist::Value::Array(entries)) = prefs.get(IMAGE_FILES).cloned() else {
        return;
    };
    let ours = Path::new(env!("CARGO_TARGET_TMPDIR")).join("e2e-data");
    let copies = home.join(CHOICE_COPIES);
    let (gone, kept): (Vec<_>, Vec<_>) = entries.into_iter().partition(|entry| {
        return choice_path(entry, "originalURL").is_some_and(|path| path.starts_with(&ours));
    });
    if gone.is_empty() {
        return;
    }
    for entry in &gone {
        if let Some(copy) = choice_path(entry, "copyURL").filter(|path| path.parent() == Some(copies.as_path())) {
            let _ = std::fs::remove_file(copy);
        }
    }
    prefs.insert(IMAGE_FILES.to_string(), plist::Value::Array(kept));
    let mut xml = Vec::new();
    if plist::Value::Dictionary(prefs).to_writer_xml(&mut xml).is_err() {
        return;
    }
    let Ok(mut child) = Command::new("defaults").arg("import").arg(&domain).arg("-").stdin(Stdio::piped()).spawn()
    else {
        return;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&xml);
    }
    let _ = child.wait();
}

fn choice_path(entry: &plist::Value, key: &str) -> Option<PathBuf> {
    let request = plist::Value::from_reader(std::io::Cursor::new(entry.as_data()?)).ok()?;
    let url = match request.as_dictionary()?.get(key)? {
        plist::Value::String(url) => url.clone(),
        plist::Value::Dictionary(url) => url.get("relative")?.as_string()?.to_string(),
        _ => return None,
    };
    return Some(PathBuf::from(percent_decode(url.strip_prefix("file://")?)));
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = (bytes[index] == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
        if let Some(byte) = escaped {
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    return String::from_utf8_lossy(&out).into_owned();
}

pub fn current_pictures() -> Vec<String> {
    let output = osascript(&["-e", "tell application \"System Events\" to get picture of every desktop"]);
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    return trimmed.trim_matches('"').split(", ").map(|value| value.trim_matches('"').to_string()).collect();
}

pub fn await_pictures(expected: &[String], timeout: Duration) {
    let deadline = Instant::now() + timeout;
    let mut stable_since: Option<Instant> = None;
    while Instant::now() < deadline {
        if current_pictures() == expected {
            let since = *stable_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= SETTLE {
                return;
            }
        } else {
            stable_since = None;
        }
        std::thread::sleep(POLL);
    }
}

fn escape(value: &str) -> String {
    return value.replace('\\', "\\\\").replace('"', "\\\"");
}

fn osascript(args: &[&str]) -> String {
    let output = Command::new("osascript").args(args).output().expect("osascript");
    return String::from_utf8_lossy(&output.stdout).into_owned();
}

pub const PNG: [u8; 116] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x08, 0x00, 0x00, 0x00, 0x08, 0x08, 0x00, 0x00, 0x00, 0x00, 0xe1, 0x64, 0xe1, 0x57, 0x00, 0x00, 0x00, 0x3b, 0x49,
    0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x60, 0x50, 0x70, 0x48, 0x68, 0x58, 0x70, 0xe0, 0x01, 0x03, 0x87, 0x86, 0x47,
    0x46, 0xc7, 0x8a, 0x13, 0x2f, 0x18, 0x04, 0x0c, 0x02, 0x0a, 0x26, 0x6c, 0xb8, 0xf0, 0x81, 0x41, 0xc2, 0x22, 0xa2,
    0x62, 0xc6, 0x8e, 0x1b, 0x3f, 0xe0, 0x4a, 0x18, 0xa0, 0x4a, 0x38, 0x18, 0xa0, 0x4a, 0x04, 0x18, 0xa0, 0x4a, 0x24,
    0x00, 0x2a, 0x04, 0x1f, 0x01, 0xf3, 0x86, 0xe3, 0xff, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42,
    0x60, 0x82,
];
