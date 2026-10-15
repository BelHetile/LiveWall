use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use livewall::proto::{self, ApplyTarget, Request, RequestEnvelope, Response, ResponseBody};

const POLL: Duration = Duration::from_millis(200);
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(15);
const TAKEOVER_TIMEOUT: Duration = Duration::from_secs(20);
const RESTORE_TIMEOUT: Duration = Duration::from_secs(15);

fn main() -> ExitCode {
    if std::env::var("LW_E2E").as_deref() != Ok("1") {
        println!("e2e_desktop: skipped (set LW_E2E=1 to run)");
        return ExitCode::SUCCESS;
    }
    let Some(fixture) = fixture() else {
        println!("e2e_desktop: skipped (run scripts/fixtures.sh; no fixture video found)");
        return ExitCode::SUCCESS;
    };

    let guard = Guard::capture();
    let result = std::panic::catch_unwind(|| {
        clean_takeover_and_restore(&fixture);
        crash_recovery(&fixture);
    });
    drop(guard);

    return match result {
        Ok(()) => {
            println!("e2e_desktop: ok");
            ExitCode::SUCCESS
        }
        Err(_) => ExitCode::FAILURE,
    };
}

fn fixture() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
    for name in ["clip-h264-720p60.mp4", "clip-hevc-1080p30.mp4", "portrait.mp4"] {
        let candidate = root.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    return None;
}

fn clean_takeover_and_restore(fixture: &Path) {
    println!("e2e_desktop: clean takeover/restore");
    let sandbox = Sandbox::new("clean");
    let original = current_pictures();
    let mut child = sandbox.launch();
    let response = sandbox.add_and_apply(fixture);
    assert!(response, "library add/apply failed");
    let still = sandbox.await_owned();
    assert!(still.contains("Stills"), "desktop still was not applied: {still}");

    sandbox.quit(&mut child);
    sandbox.await_idle();
    await_pictures(&original, RESTORE_TIMEOUT);
    assert_eq!(current_pictures(), original, "desktop picture was not restored exactly");
}

fn crash_recovery(fixture: &Path) {
    println!("e2e_desktop: crash recovery");
    let sandbox = Sandbox::new("crash");
    let original = current_pictures();
    let mut child = sandbox.launch();
    assert!(sandbox.add_and_apply(fixture), "library add/apply failed");
    let still = sandbox.await_owned();
    assert!(still.contains("Stills"), "desktop still was not applied: {still}");
    assert!(sandbox.swap_state_exists(), "swap state was not persisted");

    kill(&mut child);

    let mut child = sandbox.launch();
    let retaken = sandbox.await_owned();
    assert!(retaken.contains("Stills"), "desktop still was not retaken after crash: {retaken}");
    sandbox.quit(&mut child);
    sandbox.await_idle();
    await_pictures(&original, RESTORE_TIMEOUT);
    assert_eq!(current_pictures(), original, "desktop picture was not restored after crash");
    assert!(!sandbox.swap_state_exists(), "swap state was not cleared");
}

struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    fn new(label: &str) -> Self {
        let dir = tempfile::Builder::new().prefix(&format!("livewall-e2e-{label}-")).tempdir().expect("tempdir");
        return Self { dir };
    }

    fn data_dir(&self) -> PathBuf {
        return self.dir.path().join("data");
    }

    fn socket(&self) -> PathBuf {
        return self.dir.path().join("control.sock");
    }

    fn swap_state_exists(&self) -> bool {
        return self.data_dir().join("swap-state.json").exists();
    }

    fn launch(&self) -> Child {
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
        wait_for_socket(&self.socket(), LAUNCH_TIMEOUT);
        return child;
    }

    fn add_and_apply(&self, fixture: &Path) -> bool {
        let added = self.request(Request::LibraryAdd { paths: vec![fixture.to_string_lossy().into_owned()] });
        let ResponseBody::Ok { data } = added.body else { return false };
        if data["added"].as_u64().unwrap_or(0) != 1 {
            return false;
        }
        let list = self.request(Request::LibraryList);
        let ResponseBody::Ok { data } = list.body else { return false };
        let Some(items) = data["items"].as_array() else { return false };
        let Some(id) = items.first().and_then(|item| item["id"].as_str()) else { return false };
        let applied = self.request(Request::Apply { item: Some(id.to_string()), target: ApplyTarget::Default });
        return matches!(applied.body, ResponseBody::Ok { .. });
    }

    fn await_owned(&self) -> String {
        let deadline = Instant::now() + TAKEOVER_TIMEOUT;
        loop {
            if let Some(current) = self.owned_current() {
                return current;
            }
            assert!(Instant::now() < deadline, "desktop takeover did not complete");
            std::thread::sleep(POLL);
        }
    }

    fn await_idle(&self) {
        let deadline = Instant::now() + RESTORE_TIMEOUT;
        while Instant::now() < deadline {
            if self.request_opt(Request::Status).is_none() {
                return;
            }
            std::thread::sleep(POLL);
        }
    }

    fn owned_current(&self) -> Option<String> {
        let response = self.request_opt(Request::Desktop)?;
        let ResponseBody::Ok { data } = response.body else { return None };
        let surfaces = data["surfaces"].as_array()?;
        if surfaces.is_empty() {
            return None;
        }
        let all_owned = surfaces.iter().all(|surface| surface["state"].as_str() == Some("owned"));
        if !all_owned {
            return None;
        }
        for surface in surfaces {
            let current = surface["current"].as_str().unwrap_or_default();
            if !current.contains("Stills") {
                return None;
            }
        }
        let current = surfaces.iter().find_map(|surface| surface["current"].as_str().map(ToString::to_string))?;
        return Some(current);
    }

    fn quit(&self, child: &mut Child) {
        let _ = self.request(Request::Quit);
        let _ = child.wait();
    }

    fn request(&self, request: Request) -> Response {
        return self.request_opt(request).expect("ipc request");
    }

    fn request_opt(&self, request: Request) -> Option<Response> {
        let mut stream = UnixStream::connect(self.socket()).ok()?;
        stream.set_read_timeout(Some(Duration::from_secs(30))).ok()?;
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
}

fn wait_for_socket(socket: &Path, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if UnixStream::connect(socket).is_ok() {
            return;
        }
        std::thread::sleep(POLL);
    }
    panic!("LiveWall socket did not appear at {}", socket.display());
}

fn await_pictures(expected: &[String], timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if current_pictures() == expected {
            return;
        }
        std::thread::sleep(POLL);
    }
}

fn kill(child: &mut Child) {
    unsafe { libc::kill(child.id().cast_signed(), libc::SIGKILL) };
    let _ = child.wait();
}

fn current_pictures() -> Vec<String> {
    let output = osascript(&["-e", "tell application \"System Events\" to get picture of every desktop"]);
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let unquoted = trimmed.trim_matches('"');
    return unquoted.split(", ").map(|value| value.trim_matches('"').to_string()).collect();
}

struct Guard {
    pictures: Vec<String>,
}

impl Guard {
    fn capture() -> Self {
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

fn escape(value: &str) -> String {
    return value.replace('\\', "\\\\").replace('"', "\\\"");
}

fn osascript(args: &[&str]) -> String {
    let output = Command::new("osascript").args(args).output().expect("osascript");
    return String::from_utf8_lossy(&output.stdout).into_owned();
}
