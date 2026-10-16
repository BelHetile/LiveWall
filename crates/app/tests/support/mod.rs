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
    child: RefCell<Option<Child>>,
}

impl Sandbox {
    pub fn new(label: &str) -> Self {
        let dir = tempfile::Builder::new().prefix(&format!("livewall-e2e-{label}-")).tempdir().expect("tempdir");
        return Self { dir, child: RefCell::new(None) };
    }

    pub fn path(&self) -> &Path {
        return self.dir.path();
    }

    pub fn data_dir(&self) -> PathBuf {
        return self.dir.path().join("data");
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
    }
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

impl Sandbox {
    pub fn write_png(&self) -> PathBuf {
        return self.write_png_named("icon.png");
    }

    pub fn write_png_named(&self, name: &str) -> PathBuf {
        let path = self.path().join(name);
        std::fs::write(&path, PNG).expect("write png");
        return path;
    }
}
