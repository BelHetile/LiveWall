use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(150);
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(20);
const TAKEOVER_TIMEOUT: Duration = Duration::from_secs(20);
const RESTORE_TIMEOUT: Duration = Duration::from_secs(30);
const SETTLE: Duration = Duration::from_secs(4);
const MAX_LINE: usize = 64 * 1024;

const MISSING: &str = "11111111-1111-1111-1111-111111111111";

const PNG: [u8; 116] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x08, 0x00, 0x00, 0x00, 0x08, 0x08, 0x00, 0x00, 0x00, 0x00, 0xe1, 0x64, 0xe1, 0x57, 0x00, 0x00, 0x00, 0x3b, 0x49,
    0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x60, 0x50, 0x70, 0x48, 0x68, 0x58, 0x70, 0xe0, 0x01, 0x03, 0x87, 0x86, 0x47,
    0x46, 0xc7, 0x8a, 0x13, 0x2f, 0x18, 0x04, 0x0c, 0x02, 0x0a, 0x26, 0x6c, 0xb8, 0xf0, 0x81, 0x41, 0xc2, 0x22, 0xa2,
    0x62, 0xc6, 0x8e, 0x1b, 0x3f, 0xe0, 0x4a, 0x18, 0xa0, 0x4a, 0x38, 0x18, 0xa0, 0x4a, 0x04, 0x18, 0xa0, 0x4a, 0x24,
    0x00, 0x2a, 0x04, 0x1f, 0x01, 0xf3, 0x86, 0xe3, 0xff, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42,
    0x60, 0x82,
];

#[test]
fn ipc_end_to_end() {
    if std::env::var("LW_E2E").as_deref() != Ok("1") {
        println!("e2e_ipc: skipped (set LW_E2E=1 to run)");
        return;
    }
    let guard = Guard::capture();
    every_verb();
    settle(&guard);
    wallpaper_transitions();
    settle(&guard);
    second_instance_exits();
    stale_socket_recovers();
    settle(&guard);
    quit_removes_socket_and_restores();
    settle(&guard);
    concurrent_clients();
    oversize_line_rejected();
    settle(&guard);
    drop(guard);
    println!("e2e_ipc: ok");
}

fn wallpaper_transitions() {
    println!("e2e_ipc: wallpaper change transitions");
    let sandbox = Sandbox::new("transition");
    sandbox.launch();
    let socket = sandbox.socket();
    let first = sandbox.write_png_named("a.png").to_string_lossy().into_owned();
    let second = sandbox.write_png_named("b.png").to_string_lossy().into_owned();

    let added = ok_json(&socket, &["library", "add", &first]);
    let a_id = added["data"]["results"][0]["id"].as_str().expect("item id").to_string();
    let added = ok_json(&socket, &["library", "add", &second]);
    let b_id = added["data"]["results"][0]["id"].as_str().expect("item id").to_string();
    assert_ne!(a_id, b_id, "library deduplicated distinct files: {added}");

    ok_json(&socket, &["apply", "--item", &a_id, "--target", "default", "--transition", "fade", "--duration", "0.3"]);
    let current = ok_json(&socket, &["current"]);
    assert_eq!(current["data"]["item"], serde_json::json!(a_id), "apply+a transition: {current}");

    let kinds = [
        "simple", "fade", "left", "right", "top", "bottom", "wipe", "wave", "grow", "center", "any", "outer", "random",
        "none",
    ];
    for kind in kinds {
        ok_json(
            &socket,
            &["apply", "--item", &b_id, "--target", "default", "--transition", kind, "--duration", "0.12"],
        );
        ok_json(
            &socket,
            &["apply", "--item", &a_id, "--target", "default", "--transition", kind, "--duration", "0.12"],
        );
    }

    ok_json(
        &socket,
        &[
            "apply",
            "--item",
            &b_id,
            "--target",
            "default",
            "--transition",
            "wave",
            "--duration",
            "0.2",
            "--angle",
            "30",
            "--wave",
            "30x40",
            "--bezier",
            "0.1,0.2,0.3,0.4",
        ],
    );
    ok_json(
        &socket,
        &[
            "apply",
            "--item",
            &a_id,
            "--target",
            "default",
            "--transition",
            "grow",
            "--duration",
            "0.2",
            "--pos",
            "top-right",
            "--invert-y",
        ],
    );
    ok_json(
        &socket,
        &[
            "apply",
            "--item",
            &b_id,
            "--target",
            "default",
            "--transition",
            "grow",
            "--duration",
            "0.2",
            "--pos",
            "100,200",
        ],
    );
    ok_json(&socket, &["next", "--transition", "fade", "--duration", "0.1"]);
    ok_json(&socket, &["previous", "--transition", "wipe"]);
    ok_json(&socket, &["random", "--transition", "grow", "--duration", "0.1"]);

    ok_json(&socket, &["apply", "--item", &a_id, "--target", "default", "--transition", "wipe", "--duration", "3"]);
    ok_json(&socket, &["apply", "--item", &b_id, "--target", "default", "--transition", "fade", "--duration", "0.1"]);
    await_single_node(&socket, &b_id);

    ok_json(&socket, &["settings", "set", r#"{"transition":{"kind":"wipe","angle":90.0}}"#]);
    let settings = ok_json(&socket, &["settings", "get"]);
    assert_eq!(settings["data"]["transition"]["kind"], serde_json::json!("wipe"), "transition stored: {settings}");
    assert_eq!(settings["data"]["transition"]["angle"], serde_json::json!(90.0), "transition angle: {settings}");
    let windows = ok_json(&socket, &["windows"]);
    assert!(windows["data"]["nodes"].as_array().is_some(), "windows report: {windows}");

    let usage = lwctl_run(&socket, &["apply", "--item", &a_id, "--transition", "wipe", "--pos", "nowhere"]);
    assert_eq!(usage.status.code(), Some(64), "invalid --pos should be a usage error");

    ok_json(&socket, &["quit"]);
    sandbox.reap();
}

fn await_single_node(socket: &Path, item: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let windows = ok_json(socket, &["windows"]);
        let nodes = windows["data"]["nodes"].as_array().cloned().unwrap_or_default();
        if nodes.len() == 1 && nodes[0]["item"].as_str() == Some(item) {
            return;
        }
        assert!(Instant::now() < deadline, "nodes did not settle on {item}: {windows}");
        std::thread::sleep(POLL);
    }
}

fn every_verb() {
    println!("e2e_ipc: every request verb through lwctl");
    let sandbox = Sandbox::new("verbs");
    sandbox.launch();
    let socket = sandbox.socket();
    let png = sandbox.write_png();
    let png = png.to_string_lossy().into_owned();

    let added = ok_json(&socket, &["library", "add", &png]);
    assert_eq!(added["data"]["added"], serde_json::json!(1), "library add: {added}");
    let id = added["data"]["results"][0]["id"].as_str().expect("item id").to_string();

    let simple: &[&[&str]] = &[
        &["pause"],
        &["resume"],
        &["toggle-pause"],
        &["mute"],
        &["unmute"],
        &["next"],
        &["previous"],
        &["random"],
        &["status"],
        &["current"],
        &["spaces"],
        &["windows"],
        &["desktop"],
        &["rules"],
        &["menu"],
        &["tools"],
        &["downloads"],
        &["library", "list"],
        &["settings", "get"],
        &["hotkeys", "get"],
        &["login-item", "get"],
    ];
    for args in simple {
        ok_json(&socket, args);
    }

    ok_json(&socket, &["library", "rename", &id, "--title", "Renamed"]);
    ok_json(&socket, &["library", "move", &id]);
    ok_json(&socket, &["library", "remove", MISSING]);
    ok_json(&socket, &["apply", "--item", &id, "--target", "space", "--target-id", "ghost"]);
    ok_json(&socket, &["set-geometry", &id, "--mode", "custom", "--x", "0.1", "--y", "0.2", "--zoom", "2"]);
    ok_json(&socket, &["set-item", &id, "--volume", "0.5", "--speed", "1.5", "--muted", "false", "--still-time", "12"]);
    ok_json(&socket, &["settings", "set", "{}"]);
    ok_json(&socket, &["hotkeys", "set", "random"]);
    ok_json(&socket, &["probe", "/nonexistent.png"]);
    ok_json(&socket, &["optimize-cancel", MISSING]);
    ok_json(&socket, &["download-cancel", MISSING]);

    err(&socket, &["library", "rename", MISSING, "--title", "x"]);
    err(&socket, &["set-item", MISSING, "--volume", "0.5"]);
    err(&socket, &["set-geometry", MISSING, "--mode", "fill"]);
    err(&socket, &["optimize", MISSING]);
    err(&socket, &["optimize-status", MISSING]);
    err(&socket, &["download-status", MISSING]);
    err(&socket, &["tools-install", "ffmpeg"]);
    err(&socket, &["download", "ftp://example.invalid/x", "--no-wait"]);
    err(&socket, &["settings", "set", r#"{"volume":"loud"}"#]);

    let folder = ok_json(&socket, &["folder", "create", "F"]);
    let folder_id = folder["data"]["id"].as_str().expect("folder id").to_string();
    ok_json(&socket, &["folder", "rename", &folder_id, "G"]);
    ok_json(&socket, &["library", "move", &id, "--folder", &folder_id]);
    let moved = ok_json(&socket, &["library", "list"]);
    assert_eq!(moved["data"]["items"][0]["folder"], serde_json::json!(folder_id), "move into folder: {moved}");
    ok_json(&socket, &["folder", "delete", &folder_id]);
    let after = ok_json(&socket, &["library", "list"]);
    assert_eq!(after["data"]["items"].as_array().map(Vec::len), Some(1), "folder delete dropped an item: {after}");
    assert!(after["data"]["items"][0]["folder"].is_null(), "item not moved to root: {after}");
    assert_eq!(after["data"]["folders"].as_array().map(Vec::len), Some(0), "folder not deleted: {after}");
    err(&socket, &["folder", "rename", MISSING, "G"]);
    err(&socket, &["folder", "delete", MISSING]);
    err(&socket, &["library", "move", &id, "--folder", MISSING]);

    ok_json(&socket, &["settings", "set", r#"{"hotkeys":{"random":{"key":"x","modifiers":["control"]}}}"#]);
    let patched = ok_json(&socket, &["settings", "get"]);
    assert_eq!(patched["data"]["hotkeys"]["random"]["key"], serde_json::json!("x"), "nested merge: {patched}");
    assert!(patched["data"]["hotkeys"]["next"].is_object(), "sibling hotkey clobbered: {patched}");
    ok_json(&socket, &["settings", "set", r#"{"hotkeys":{"random":null}}"#]);
    let patched = ok_json(&socket, &["settings", "get"]);
    assert!(patched["data"]["hotkeys"]["random"].is_null(), "nested delete failed: {patched}");
    ok_json(&socket, &["settings", "set", r#"{"tools":{"install_consent":true}}"#]);
    let patched = ok_json(&socket, &["settings", "get"]);
    assert_eq!(patched["data"]["tools"]["install_consent"], serde_json::json!(true), "nested merge: {patched}");
    assert_eq!(patched["data"]["rotate_minutes"], serde_json::json!(30), "unrelated field clobbered: {patched}");

    let login = lwctl_run(&socket, &["login-item", "set", "false"]);
    assert!(matches!(login.status.code(), Some(0 | 1)), "login-item set exit: {:?}", login.status.code());

    ok_json(&socket, &["quit"]);
    sandbox.reap();
}

fn second_instance_exits() {
    println!("e2e_ipc: second instance exits already running");
    let sandbox = Sandbox::new("second");
    sandbox.launch();
    let out = Command::new(env!("CARGO_BIN_EXE_LiveWall"))
        .env("LIVEWALL_DATA_DIR", sandbox.data_dir())
        .env("LIVEWALL_SOCKET", sandbox.socket())
        .stdin(Stdio::null())
        .output()
        .expect("second launch");
    assert_eq!(out.status.code(), Some(0), "second instance exit: {:?}", out.status.code());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("already running"), "second instance said: {text:?}");
    ok_json(&sandbox.socket(), &["quit"]);
    sandbox.reap();
}

fn stale_socket_recovers() {
    println!("e2e_ipc: stale socket after kill -9 is recovered");
    let sandbox = Sandbox::new("stale");
    sandbox.launch();
    sandbox.kill9();
    assert!(sandbox.socket().exists(), "socket did not survive SIGKILL");

    sandbox.launch();
    ok_json(&sandbox.socket(), &["status"]);
    ok_json(&sandbox.socket(), &["quit"]);
    sandbox.reap();
}

fn quit_removes_socket_and_restores() {
    println!("e2e_ipc: quit removes the socket and restores the desktop picture");
    let sandbox = Sandbox::new("quit");
    let socket = sandbox.socket();
    let png = sandbox.write_png();
    let png = png.to_string_lossy().into_owned();
    let original = current_pictures();
    sandbox.launch();

    let added = ok_json(&socket, &["library", "add", &png]);
    let id = added["data"]["results"][0]["id"].as_str().expect("item id").to_string();
    ok_json(&socket, &["apply", "--item", &id, "--target", "default"]);
    await_taken(&socket);
    assert!(sandbox.socket().exists(), "socket missing before quit");

    ok_json(&socket, &["quit"]);
    sandbox.reap();
    assert!(!sandbox.socket().exists(), "quit left the socket behind");
    await_pictures(&original, RESTORE_TIMEOUT);
    assert_eq!(current_pictures(), original, "desktop picture was not restored after quit");
}

fn concurrent_clients() {
    println!("e2e_ipc: 20 concurrent lwctl clients");
    let sandbox = Sandbox::new("concurrent");
    sandbox.launch();
    let socket = sandbox.socket();
    let mut clients = Vec::new();
    for _ in 0..20 {
        let client = Command::new(lwctl())
            .args(["--json", "status"])
            .env("LIVEWALL_SOCKET", &socket)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("lwctl");
        clients.push(client);
    }
    for client in clients {
        let out = client.wait_with_output().expect("wait");
        assert_eq!(out.status.code(), Some(0), "concurrent status failed: {}", String::from_utf8_lossy(&out.stderr));
        let value = parse(&out.stdout);
        assert_eq!(value["status"], serde_json::json!("ok"), "concurrent status body: {value}");
    }
    ok_json(&socket, &["quit"]);
    sandbox.reap();
}

fn oversize_line_rejected() {
    println!("e2e_ipc: oversize line is rejected without killing the server");
    let sandbox = Sandbox::new("oversize");
    sandbox.launch();
    let mut stream = UnixStream::connect(sandbox.socket()).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let mut payload = vec![b'a'; MAX_LINE + 4096];
    payload.push(b'\n');
    let _ = stream.write_all(&payload);
    let _ = stream.flush();
    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    let _ = reader.read_until(b'\n', &mut line);
    if !line.is_empty() {
        let value = parse(&line);
        assert_eq!(value["status"], serde_json::json!("err"), "oversize was not rejected: {value}");
    }
    ok_json(&sandbox.socket(), &["status"]);
    ok_json(&sandbox.socket(), &["quit"]);
    sandbox.reap();
}

fn lwctl() -> &'static Path {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    return PATH.get_or_init(|| {
        let livewall = PathBuf::from(env!("CARGO_BIN_EXE_LiveWall"));
        let candidate = livewall.parent().expect("binary directory").join("lwctl");
        if !candidate.exists() {
            let status = Command::new(env!("CARGO")).args(["build", "-p", "lwctl"]).status();
            assert!(status.is_ok_and(|status| status.success()), "cannot build lwctl");
        }
        assert!(candidate.exists(), "lwctl not found at {}", candidate.display());
        return candidate;
    });
}

fn lwctl_run(socket: &Path, args: &[&str]) -> Output {
    let mut full = vec!["--json"];
    full.extend_from_slice(args);
    return Command::new(lwctl()).args(&full).env("LIVEWALL_SOCKET", socket).output().expect("lwctl");
}

fn ok_json(socket: &Path, args: &[&str]) -> serde_json::Value {
    let out = lwctl_run(socket, args);
    assert_eq!(out.status.code(), Some(0), "lwctl {args:?} exit {:?}: {}", out.status.code(), stderr(&out));
    let value = parse(&out.stdout);
    assert_eq!(value["v"], serde_json::json!(1), "lwctl {args:?} protocol version: {value}");
    assert_eq!(value["status"], serde_json::json!("ok"), "lwctl {args:?} status: {value}");
    return value;
}

fn err(socket: &Path, args: &[&str]) {
    let out = lwctl_run(socket, args);
    assert_eq!(out.status.code(), Some(1), "lwctl {args:?} expected exit 1, got {:?}", out.status.code());
    assert!(stderr(&out).contains("lwctl:"), "lwctl {args:?} stderr: {}", stderr(&out));
}

fn stderr(output: &Output) -> String {
    return String::from_utf8_lossy(&output.stderr).into_owned();
}

fn parse(bytes: &[u8]) -> serde_json::Value {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim();
    return serde_json::from_str(text).unwrap_or_else(|error| panic!("invalid response {text:?}: {error}"));
}

fn await_taken(socket: &Path) {
    let deadline = Instant::now() + TAKEOVER_TIMEOUT;
    loop {
        if let Some(surfaces) = ok_json(socket, &["desktop"])["data"]["surfaces"].as_array().cloned()
            && !surfaces.is_empty()
            && surfaces.iter().all(|surface| {
                surface["state"].as_str() == Some("owned")
                    && surface["current"].as_str().is_some_and(|value| value.contains("Wallpapers"))
            })
        {
            return;
        }
        assert!(Instant::now() < deadline, "desktop takeover did not complete");
        std::thread::sleep(POLL);
    }
}

fn await_pictures(expected: &[String], timeout: Duration) {
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

fn settle(guard: &Guard) {
    await_pictures(&guard.pictures, RESTORE_TIMEOUT);
    assert_eq!(current_pictures(), guard.pictures, "desktop picture did not settle back to the original");
}

fn wait_for_socket(socket: &Path) {
    let deadline = Instant::now() + LAUNCH_TIMEOUT;
    while Instant::now() < deadline {
        if UnixStream::connect(socket).is_ok() {
            return;
        }
        std::thread::sleep(POLL);
    }
    panic!("LiveWall socket did not appear at {}", socket.display());
}

struct Sandbox {
    dir: tempfile::TempDir,
    child: std::cell::RefCell<Option<Child>>,
}

impl Sandbox {
    fn new(label: &str) -> Self {
        let dir = tempfile::Builder::new().prefix(&format!("livewall-e2e-ipc-{label}-")).tempdir().expect("tempdir");
        return Self { dir, child: std::cell::RefCell::new(None) };
    }

    fn data_dir(&self) -> PathBuf {
        return self.dir.path().join("data");
    }

    fn socket(&self) -> PathBuf {
        return self.dir.path().join("control.sock");
    }

    fn write_png(&self) -> PathBuf {
        return self.write_png_named("icon.png");
    }

    fn write_png_named(&self, name: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, PNG).expect("write png");
        return path;
    }

    fn launch(&self) {
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
        wait_for_socket(&self.socket());
    }

    fn reap(&self) {
        if let Some(mut child) = self.child.borrow_mut().take() {
            let _ = child.wait();
        }
    }

    fn kill9(&self) {
        if let Some(mut child) = self.child.borrow_mut().take() {
            unsafe { libc::kill(child.id().cast_signed(), libc::SIGKILL) };
            let _ = child.wait();
        }
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

fn current_pictures() -> Vec<String> {
    let trimmed = osascript(&["-e", "tell application \"System Events\" to get picture of every desktop"]);
    let trimmed = trimmed.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    return trimmed.trim_matches('"').split(", ").map(|value| value.trim_matches('"').to_string()).collect();
}

fn osascript(args: &[&str]) -> String {
    let output = Command::new("osascript").args(args).output().expect("osascript");
    return String::from_utf8_lossy(&output.stdout).into_owned();
}
