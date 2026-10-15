use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use livewall::proto::{self, ErrorCode, Request, RequestEnvelope, Response, ResponseBody};
use serde_json::{Value, json};

const POLL: Duration = Duration::from_millis(200);
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(15);
const JOB_TIMEOUT: Duration = Duration::from_mins(2);

fn main() -> ExitCode {
    if std::env::var("LW_E2E").as_deref() != Ok("1") {
        println!("e2e_media: skipped (set LW_E2E=1 to run)");
        return ExitCode::SUCCESS;
    }
    let fixtures = Fixtures::load();
    if fixtures.is_none() {
        println!("e2e_media: skipped (run scripts/fixtures.sh first)");
        return ExitCode::SUCCESS;
    }
    let fixtures = fixtures.expect("fixtures");
    let guard = Guard::capture();
    let result = std::panic::catch_unwind(|| {
        run(&fixtures);
        run_unplayable(&fixtures);
        run_cancel(&fixtures);
    });
    drop(guard);
    return match result {
        Ok(()) => {
            println!("e2e_media: ok");
            ExitCode::SUCCESS
        }
        Err(_) => ExitCode::FAILURE,
    };
}

struct Fixtures {
    av1: Option<PathBuf>,
    uhd60: PathBuf,
    corrupt: PathBuf,
    text: PathBuf,
    vp9: PathBuf,
}

impl Fixtures {
    fn load() -> Option<Self> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
        let uhd60 = root.join("clip-4k-hevc-60.mp4");
        let corrupt = root.join("corrupt.mp4");
        let text = root.join("notes.txt");
        let vp9 = root.join("clip-vp9.webm");
        if !uhd60.exists() || !corrupt.exists() || !text.exists() || !vp9.exists() {
            return None;
        }
        let av1 = ["clip-av1-1080p.mp4", "clip-av1-1080p30.mp4", "clip-av1.mp4"]
            .map(|name| root.join(name))
            .into_iter()
            .find(|path| path.exists());
        return Some(Self { av1, uhd60, corrupt, text, vp9 });
    }
}

#[allow(clippy::too_many_lines)]
fn run(fixtures: &Fixtures) {
    let sandbox = Sandbox::new("media");
    sandbox.launch();

    let mut paths = vec![
        fixtures.uhd60.to_string_lossy().into_owned(),
        fixtures.corrupt.to_string_lossy().into_owned(),
        fixtures.text.to_string_lossy().into_owned(),
        fixtures.vp9.to_string_lossy().into_owned(),
    ];
    if let Some(av1) = &fixtures.av1 {
        paths.push(av1.to_string_lossy().into_owned());
    }

    let expected = paths.len();
    let added = sandbox.ok(Request::LibraryAdd { paths });
    let results = added["results"].as_array().expect("per-path results").clone();
    assert_eq!(results.len(), expected, "one result per input: {added}");

    let text = result_for(&results, "notes.txt");
    assert_eq!(text["status"].as_str(), Some("skipped"), "txt was not skipped: {text}");

    let uhd = result_for(&results, "clip-4k-hevc-60.mp4");
    assert_eq!(uhd["status"].as_str(), Some("imported"), "4K60 was not imported: {uhd}");
    assert_eq!(uhd["verdict"]["verdict"].as_str(), Some("needs-optimize"), "4K60 verdict: {uhd}");
    let reasons = uhd["verdict"]["reasons"].as_array().expect("reasons");
    assert!(reasons.iter().any(|reason| reason == "fps-above-limit"), "4K60 reasons: {uhd}");
    assert!(reasons.iter().any(|reason| reason == "resolution-above-display"), "4K60 reasons: {uhd}");
    let uhd_id = id_of(uhd);

    let corrupt = result_for(&results, "corrupt.mp4");
    let corrupt_id = match corrupt["status"].as_str() {
        Some("imported") => {
            assert_eq!(corrupt["verdict"]["verdict"].as_str(), Some("needs-optimize"), "corrupt verdict: {corrupt}");
            Some(id_of(corrupt))
        }
        Some("rejected") => None,
        other => panic!("unexpected corrupt status {other:?}: {corrupt}"),
    };

    let vp9 = result_for(&results, "clip-vp9.webm");
    assert_eq!(vp9["status"].as_str(), Some("imported"), "VP9 webm was not imported: {vp9}");
    assert_eq!(vp9["verdict"]["verdict"].as_str(), Some("needs-optimize"), "VP9 verdict: {vp9}");
    let vp9_id = id_of(vp9);

    let av1_id = fixtures.av1.as_ref().map(|av1| {
        let name = av1.file_name().and_then(|value| value.to_str()).expect("name");
        let entry = result_for(&results, name);
        assert_eq!(entry["status"].as_str(), Some("imported"), "AV1 was not imported: {entry}");
        assert_eq!(entry["verdict"]["verdict"].as_str(), Some("needs-optimize"), "AV1 on M1 verdict: {entry}");
        return id_of(entry);
    });

    let max_px = display_max_px(&sandbox);

    let uhd_status = wait_optimize(&sandbox, &uhd_id, &["done", "failed"]);
    assert_eq!(uhd_status["state"].as_str(), Some("done"), "4K60 optimize: {uhd_status}");
    let uhd_item = library_item(&sandbox, &uhd_id).expect("4K60 item");
    assert_eq!(uhd_item["needs_optimize"], json!(false), "4K60 still flagged: {uhd_item}");
    assert_eq!(uhd_item["optimized"], json!(true), "4K60 not marked optimized: {uhd_item}");
    let uhd_probe = probe_path(&sandbox, uhd_item["path"].as_str().expect("path"));
    assert_eq!(uhd_probe["probe"]["codec"].as_str(), Some("hvc1"), "4K60 output codec: {uhd_probe}");
    assert!(uhd_probe["probe"]["decodable"].as_bool() == Some(true), "4K60 output not playable: {uhd_probe}");
    assert!(
        uhd_probe["probe"]["height"].as_u64().unwrap_or(0) <= u64::from(max_px),
        "4K60 not downscaled under {max_px}: {uhd_probe}"
    );
    assert!(
        uhd_probe["probe"]["width"].as_u64().unwrap_or(0) <= u64::from(max_px),
        "4K60 not downscaled under {max_px}: {uhd_probe}"
    );
    assert!(uhd_probe["probe"]["fps"].as_f64().unwrap_or(120.0) <= 30.5, "4K60 fps not capped: {uhd_probe}");
    assert!(thumbnail(&sandbox, &uhd_id).exists(), "missing 4K60 thumbnail");

    let vp9_status = wait_optimize(&sandbox, &vp9_id, &["done", "failed"]);
    assert_eq!(vp9_status["state"].as_str(), Some("done"), "VP9 optimize: {vp9_status}");
    let vp9_item = library_item(&sandbox, &vp9_id).expect("VP9 item");
    let vp9_probe = probe_path(&sandbox, vp9_item["path"].as_str().expect("path"));
    assert_eq!(vp9_probe["probe"]["codec"].as_str(), Some("hvc1"), "VP9 output codec: {vp9_probe}");
    assert_eq!(vp9_probe["probe"]["decodable"].as_bool(), Some(true), "VP9 output not playable: {vp9_probe}");
    assert!(thumbnail(&sandbox, &vp9_id).exists(), "missing VP9 thumbnail");

    if let Some(av1_id) = &av1_id {
        let av1_status = wait_optimize(&sandbox, av1_id, &["done", "failed"]);
        assert_eq!(av1_status["state"].as_str(), Some("done"), "AV1 optimize: {av1_status}");
        let av1_item = library_item(&sandbox, av1_id).expect("AV1 item");
        let av1_probe = probe_path(&sandbox, av1_item["path"].as_str().expect("path"));
        assert_eq!(av1_probe["probe"]["codec"].as_str(), Some("hvc1"), "AV1 output codec: {av1_probe}");
        assert_eq!(av1_probe["probe"]["decodable"].as_bool(), Some(true), "AV1 output not playable: {av1_probe}");
    }

    if let Some(corrupt_id) = &corrupt_id {
        let corrupt_status = wait_optimize(&sandbox, corrupt_id, &["done", "failed"]);
        assert_eq!(corrupt_status["state"].as_str(), Some("failed"), "corrupt optimize: {corrupt_status}");
        assert!(corrupt_status["reason"].as_str().is_some(), "corrupt failure has no reason: {corrupt_status}");
        assert!(library_item(&sandbox, corrupt_id).is_none(), "corrupt item was not removed");
    }

    let probe = sandbox.ok(Request::Probe { path: fixtures.uhd60.to_string_lossy().into_owned() });
    assert_eq!(probe["probe"]["codec"].as_str(), Some("hvc1"), "probe codec: {probe}");
    assert!(probe["probe"]["height"].as_u64().unwrap_or(0) >= 2160, "probe height: {probe}");
    assert_eq!(probe["verdict"]["verdict"].as_str(), Some("needs-optimize"), "probe verdict: {probe}");

    let tools = sandbox.ok(Request::Tools);
    let entries = tools["tools"].as_array().expect("tools").clone();
    assert_eq!(entries.len(), 3, "three tools reported: {tools}");
    for entry in &entries {
        assert!(!entry["missing"].as_bool().unwrap_or(true), "tool missing: {entry}");
        assert!(entry["version"].as_str().is_some(), "tool version: {entry}");
    }

    let denied = sandbox.request(Request::ToolsInstall { tool: "ffmpeg".into(), consent: false });
    match denied.body {
        ResponseBody::Err(body) => assert_eq!(body.code, ErrorCode::ConsentRequired, "{body:?}"),
        ResponseBody::Ok { .. } => panic!("tools-install without consent must fail"),
    }

    sandbox.quit();
}

fn run_unplayable(fixtures: &Fixtures) {
    let sandbox = Sandbox::new("unplayable");
    sandbox.launch();
    sandbox.ok(Request::SettingsSet { patch: json!({ "optimize_on_import": false }) });
    let added = sandbox.ok(Request::LibraryAdd { paths: vec![fixtures.corrupt.to_string_lossy().into_owned()] });
    let results = added["results"].as_array().expect("results").clone();
    assert_eq!(results[0]["status"].as_str(), Some("imported"), "corrupt not stored: {}", results[0]);
    let id = id_of(&results[0]);
    let item = library_item(&sandbox, &id).expect("item");
    assert_eq!(item["playable"], json!(false), "corrupt should be non-playable: {item}");
    assert_eq!(item["needs_optimize"], json!(true), "corrupt should need optimize: {item}");

    let response = sandbox.request(Request::Apply {
        item: Some(id.clone()),
        target: proto::ApplyTarget::Default,
        transition: None,
    });
    match response.body {
        ResponseBody::Err(body) => assert_eq!(body.code, ErrorCode::Media, "apply on non-playable: {body:?}"),
        ResponseBody::Ok { .. } => panic!("apply on a non-playable item must fail"),
    }
    assert!(library_item(&sandbox, &id).is_some(), "item removed by the rejected apply");
    sandbox.quit();
}

fn run_cancel(fixtures: &Fixtures) {
    let sandbox = Sandbox::new("cancel");
    let tools_dir = sandbox.data_dir().join("Tools");
    std::fs::create_dir_all(&tools_dir).expect("tools dir");
    write_stub(&tools_dir.join("ffmpeg"), STUB_FFMPEG);
    sandbox.launch();

    sandbox.ok(Request::SettingsSet { patch: json!({ "optimize_on_import": false }) });
    let added = sandbox.ok(Request::LibraryAdd { paths: vec![fixtures.uhd60.to_string_lossy().into_owned()] });
    let results = added["results"].as_array().expect("results").clone();
    let id = id_of(&results[0]);

    sandbox.ok(Request::Optimize { id: id.clone() });
    let temp = sandbox.data_dir().join("Wallpapers").join(format!("{id}.optimize.mp4"));
    let original = sandbox.data_dir().join("Wallpapers").join(format!("{id}.mp4"));
    wait_for_path(&temp);
    assert!(original.exists(), "original disappeared before cancel");

    sandbox.ok(Request::OptimizeCancel { id: id.clone() });
    let status = wait_optimize(&sandbox, &id, &["cancelled", "done", "failed"]);
    assert_eq!(status["state"].as_str(), Some("cancelled"), "cancel status: {status}");

    assert!(!temp.exists(), "cancel left a partial file at {}", temp.display());
    assert!(original.exists(), "cancel destroyed the original at {}", original.display());
    let item = library_item(&sandbox, &id).expect("item survives cancel");
    assert_eq!(item["needs_optimize"], json!(true), "cancelled item should stay unoptimized: {item}");

    sandbox.quit();
}

const STUB_FFMPEG: &str = "#!/bin/sh\ncase \"$*\" in *-encoders*) echo ' V....D hevc_videotoolbox'; exit 0;; esac\nfor last; do :; done\n: > \"$last\"\nsleep 30\n";

fn write_stub(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write stub");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

fn id_of(result: &Value) -> String {
    return result["id"].as_str().expect("item id").to_string();
}

fn wait_optimize(sandbox: &Sandbox, id: &str, terminal: &[&str]) -> Value {
    let deadline = Instant::now() + JOB_TIMEOUT;
    while Instant::now() < deadline {
        let status = sandbox.ok(Request::OptimizeStatus { id: id.to_string() });
        if terminal.contains(&status["state"].as_str().unwrap_or_default()) {
            return status;
        }
        std::thread::sleep(POLL);
    }
    panic!("optimize job {id} did not reach {terminal:?} in time");
}

fn wait_for_path(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if path.exists() {
            return;
        }
        std::thread::sleep(POLL);
    }
    panic!("stub never created {}", path.display());
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn display_max_px(sandbox: &Sandbox) -> u32 {
    let displays = sandbox.ok(Request::Displays);
    let mut max = 0u32;
    for display in displays.as_array().expect("displays") {
        let frame = display["frame"].as_array().expect("frame");
        let scale = display["scale"].as_f64().unwrap_or(1.0);
        let width = frame[2].as_f64().unwrap_or(0.0) * scale;
        let height = frame[3].as_f64().unwrap_or(0.0) * scale;
        max = max.max(width as u32).max(height as u32);
    }
    assert!(max > 0, "no connected displays reported");
    return max;
}

fn library_item(sandbox: &Sandbox, id: &str) -> Option<Value> {
    let library = sandbox.ok(Request::LibraryList);
    return library["items"].as_array()?.iter().find(|item| item["id"].as_str() == Some(id)).cloned();
}

fn probe_path(sandbox: &Sandbox, path: &str) -> Value {
    return sandbox.ok(Request::Probe { path: path.to_string() });
}

fn thumbnail(sandbox: &Sandbox, id: &str) -> PathBuf {
    return sandbox.data_dir().join("Thumbs").join(format!("{id}.jpg"));
}

fn name_of(result: &Value) -> &str {
    let path = result["path"].as_str().unwrap_or_default();
    return path.rsplit('/').next().unwrap_or(path);
}

fn result_for<'a>(results: &'a [Value], name: &str) -> &'a Value {
    return results
        .iter()
        .find(|result| name_of(result) == name)
        .unwrap_or_else(|| panic!("no result for {name}: {results:?}"));
}

struct Sandbox {
    dir: tempfile::TempDir,
    child: std::cell::RefCell<Option<Child>>,
}

impl Sandbox {
    fn new(label: &str) -> Self {
        let dir = tempfile::Builder::new().prefix(&format!("livewall-e2e-{label}-")).tempdir().expect("tempdir");
        return Self { dir, child: std::cell::RefCell::new(None) };
    }

    fn data_dir(&self) -> PathBuf {
        return self.dir.path().join("data");
    }

    fn socket(&self) -> PathBuf {
        return self.dir.path().join("control.sock");
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
        wait_for_socket(&self.socket());
        *self.child.borrow_mut() = Some(child);
    }

    fn quit(&self) {
        let _ = self.request(Request::Quit);
        if let Some(mut child) = self.child.borrow_mut().take() {
            let _ = child.wait();
        }
    }

    fn ok(&self, request: Request) -> Value {
        let response = self.request(request);
        return match response.body {
            ResponseBody::Ok { data } => data,
            ResponseBody::Err(body) => panic!("unexpected error {body:?}"),
        };
    }

    fn request(&self, request: Request) -> Response {
        let mut stream = UnixStream::connect(self.socket()).expect("connect");
        stream.set_read_timeout(Some(Duration::from_mins(2))).expect("timeout");
        stream.write_all(&proto::encode(&RequestEnvelope::new(request))).expect("write");
        stream.flush().expect("flush");
        let mut reader = BufReader::new(stream);
        let mut line = Vec::new();
        reader.read_until(b'\n', &mut line).expect("read");
        assert!(!line.is_empty(), "app closed the connection");
        return proto::decode::<Response>(&line).expect("decode");
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
