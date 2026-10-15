use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use livewall::proto::{self, ErrorCode, Request, RequestEnvelope, Response, ResponseBody};

const POLL: Duration = Duration::from_millis(200);
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(15);

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
    let result = std::panic::catch_unwind(|| run(&fixtures));
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

    let corrupt = result_for(&results, "corrupt.mp4");
    assert_eq!(corrupt["status"].as_str(), Some("rejected"), "corrupt file was not rejected: {corrupt}");
    assert!(corrupt["verdict"]["verdict"].as_str() == Some("unsupported"), "corrupt verdict: {corrupt}");

    let uhd = result_for(&results, "clip-4k-hevc-60.mp4");
    assert_eq!(uhd["status"].as_str(), Some("imported"), "4K60 was not imported: {uhd}");
    assert_eq!(uhd["verdict"]["verdict"].as_str(), Some("needs-optimize"), "4K60 verdict: {uhd}");
    let reasons = uhd["verdict"]["reasons"].as_array().expect("reasons");
    assert!(reasons.iter().any(|reason| reason == "fps-above-limit"), "4K60 reasons: {uhd}");
    assert!(reasons.iter().any(|reason| reason == "resolution-above-display"), "4K60 reasons: {uhd}");

    let vp9 = result_for(&results, "clip-vp9.webm");
    match vp9["status"].as_str() {
        Some("rejected") => assert_eq!(vp9["verdict"]["verdict"].as_str(), Some("unsupported"), "VP9 verdict: {vp9}"),
        Some("imported") => {
            assert_eq!(vp9["verdict"]["verdict"].as_str(), Some("needs-optimize"), "VP9 verdict: {vp9}");
        }
        other => panic!("unexpected VP9 status {other:?}: {vp9}"),
    }

    if let Some(av1) = &fixtures.av1 {
        let name = av1.file_name().and_then(|value| value.to_str()).expect("name");
        let entry = result_for(&results, name);
        assert_eq!(entry["status"].as_str(), Some("imported"), "AV1 was not imported: {entry}");
        assert_eq!(entry["verdict"]["verdict"].as_str(), Some("needs-optimize"), "AV1 on M1 verdict: {entry}");
    }

    let mut imported = 0;
    for result in &results {
        if result["status"].as_str() != Some("imported") {
            continue;
        }
        imported += 1;
        let id = result["id"].as_str().expect("item id");
        let stored = sandbox.data_dir().join("Wallpapers").join(format!("{id}.{}", extension(name_of(result))));
        assert!(stored.exists(), "missing stored media at {}", stored.display());
        let undecodable = result["verdict"]["reasons"]
            .as_array()
            .is_some_and(|reasons| reasons.iter().any(|reason| reason == "av1-without-hardware"));
        if undecodable {
            continue;
        }
        let thumbnail = sandbox.data_dir().join("Thumbs").join(format!("{id}.jpg"));
        assert!(thumbnail.exists(), "missing thumbnail for {} at {}", name_of(result), thumbnail.display());
    }
    assert!(imported >= 2, "expected several imports, got {imported}");
    assert_eq!(added["added"].as_u64(), Some(imported as u64));
    assert_eq!(added["skipped"].as_u64(), Some((results.len() - imported) as u64));

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

fn name_of(result: &serde_json::Value) -> &str {
    let path = result["path"].as_str().unwrap_or_default();
    return path.rsplit('/').next().unwrap_or(path);
}

fn extension(name: &str) -> &str {
    return name.rsplit_once('.').map_or("", |(_, extension)| extension);
}

fn result_for<'a>(results: &'a [serde_json::Value], name: &str) -> &'a serde_json::Value {
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

    fn ok(&self, request: Request) -> serde_json::Value {
        let response = self.request(request);
        return match response.body {
            ResponseBody::Ok { data } => data,
            ResponseBody::Err(body) => panic!("unexpected error {body:?}"),
        };
    }

    fn request(&self, request: Request) -> Response {
        let mut stream = UnixStream::connect(self.socket()).expect("connect");
        stream.set_read_timeout(Some(Duration::from_mins(1))).expect("timeout");
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
