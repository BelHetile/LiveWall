use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::runtime::Handle;
use tokio::sync::Notify;

use crate::app::{Event, post};
use crate::import::{self, Transfer};
use crate::media::probe;
use crate::model::Item;
use crate::process::Process;

pub const MIN_HEIGHT: u32 = 720;
pub const MAX_HEIGHT: u32 = 2160;
pub const H264_CEILING: u32 = 1080;
pub const ESCALATION: Duration = Duration::from_secs(3);
pub const EVICT_AFTER: Duration = Duration::from_mins(10);
pub const LW_PROGRESS: &str = "LWPROG ";
pub const LW_POST: &str = "LWPOST ";

pub fn clamp_height(pixels: u32) -> u32 {
    return pixels.clamp(MIN_HEIGHT, MAX_HEIGHT);
}

pub fn preferred_codec(cap: u32, av1_hw: bool) -> Option<&'static str> {
    if cap <= H264_CEILING {
        return Some("avc1");
    }
    if av1_hw {
        return None;
    }
    return Some("vp9");
}

pub fn selector(max_height: u32, av1_hw: bool) -> String {
    let cap = clamp_height(max_height);
    let preferred = match preferred_codec(cap, av1_hw) {
        Some(codec) => format!("[vcodec^={codec}]"),
        None => String::new(),
    };
    let avoid = if av1_hw { String::new() } else { "[vcodec!*=av01]".to_string() };
    let best = format!("bv*[height<={cap}]{preferred}+ba[ext=m4a]");
    let fallback_split = format!("bv*[height<={cap}]{avoid}+ba");
    let merged = format!("b[height<={cap}]{avoid}");
    let single = format!("b[height<={cap}]");
    return format!("{best}/{fallback_split}/{merged}/{single}/b");
}

fn merge_format() -> &'static str {
    return "mp4";
}

#[derive(Debug, Deserialize)]
struct RawProgress {
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    downloaded_bytes: Option<u64>,
    #[serde(default)]
    total_bytes: Option<u64>,
    #[serde(default)]
    total_bytes_estimate: Option<u64>,
    #[serde(default)]
    speed: Option<f64>,
    #[serde(default)]
    eta: Option<f64>,
}

#[derive(Debug, Default, Clone)]
pub struct ProgressTracker {
    stages: BTreeMap<String, usize>,
    percent: f64,
}

impl ProgressTracker {
    #[allow(clippy::cast_precision_loss)]
    fn update(&mut self, raw: &RawProgress) -> f64 {
        let Some(downloaded) = raw.downloaded_bytes else {
            return self.percent;
        };
        let total = raw.total_bytes.or(raw.total_bytes_estimate).filter(|total| *total > 0);
        let fraction = total.map_or(0.0, |total| (downloaded as f64 / total as f64).clamp(0.0, 1.0));
        let key = raw.filename.clone().unwrap_or_default();
        let next = self.stages.len();
        let stage = *self.stages.entry(key).or_insert(next);
        let mapped = match stage {
            0 => 0.9 * fraction,
            _ => 0.9 + 0.1 * fraction,
        };
        self.percent = self.percent.max(mapped.clamp(0.0, 1.0));
        return self.percent;
    }
}

#[derive(Debug, Deserialize)]
struct RawPost {
    #[serde(default)]
    postprocessor: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JobState {
    Queued,
    Downloading { percent: f64, speed: Option<f64>, eta: Option<f64> },
    Merging,
    Importing,
    Done { item: String, path: String },
    Failed { reason: String },
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobReport {
    pub id: String,
    pub state: &'static str,
    pub progress: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl JobState {
    pub fn report(&self, id: &str) -> JobReport {
        let make = |state, progress, speed, eta, item, path, reason| JobReport {
            id: id.to_string(),
            state,
            progress,
            speed,
            eta,
            item,
            path,
            reason,
        };
        return match self {
            JobState::Queued => make("queued", 0.0, None, None, None, None, None),
            JobState::Downloading { percent, speed, eta } => {
                make("downloading", *percent, *speed, *eta, None, None, None)
            }
            JobState::Merging => make("merging", 0.9, None, None, None, None, None),
            JobState::Importing => make("importing", 1.0, None, None, None, None, None),
            JobState::Done { item, path } => {
                make("done", 1.0, None, None, Some(item.clone()), Some(path.clone()), None)
            }
            JobState::Failed { reason } => make("failed", 0.0, None, None, None, None, Some(reason.clone())),
            JobState::Cancelled => make("cancelled", 0.0, None, None, None, None, None),
        };
    }
}

pub struct Spec {
    pub url: String,
    pub yt_dlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub deno: Option<PathBuf>,
    pub incoming: PathBuf,
    pub wallpapers: PathBuf,
    pub thumbs: PathBuf,
    pub policy: probe::Policy,
    pub height: u32,
    pub av1_hw: bool,
}

fn yt_args(spec: &Spec, job_dir: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec!["--no-playlist".into(), "--newline".into(), "--no-mtime".into()];
    args.push("--ffmpeg-location".into());
    args.push(spec.ffmpeg.as_os_str().to_owned());
    if let Some(deno) = &spec.deno {
        args.push("--js-runtimes".into());
        args.push(format!("deno:{}", deno.display()).into());
    }
    args.push("--progress-template".into());
    args.push("download:LWPROG %(progress)j".into());
    args.push("--progress-template".into());
    args.push("postprocess:LWPOST %(progress)j".into());
    args.push("--format".into());
    args.push(selector(spec.height, spec.av1_hw).into());
    args.push("--merge-output-format".into());
    args.push(merge_format().into());
    args.push("--output".into());
    args.push(job_dir.join("%(title).120B.%(ext)s").into_os_string());
    args.push(spec.url.clone().into());
    return args;
}

struct JobRecord {
    state: JobState,
    cancel: Option<Arc<Notify>>,
    cancelled: bool,
}

pub struct Queue {
    shared: Arc<Mutex<BTreeMap<String, JobRecord>>>,
    handle: Handle,
}

impl Queue {
    pub fn new(handle: Handle) -> Self {
        return Self { shared: Arc::new(Mutex::new(BTreeMap::new())), handle };
    }

    pub fn enqueue(&self, spec: Spec) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        if let Ok(mut jobs) = self.shared.lock() {
            jobs.insert(id.clone(), JobRecord { state: JobState::Queued, cancel: None, cancelled: false });
        }
        let shared = Arc::clone(&self.shared);
        let task_id = id.clone();
        self.handle.spawn(async move {
            run_job(task_id, spec, shared).await;
        });
        return id;
    }

    pub fn status(&self, id: &str) -> Option<JobState> {
        return self.shared.lock().ok()?.get(id).map(|record| record.state.clone());
    }

    pub fn reports(&self) -> Vec<JobReport> {
        let Ok(jobs) = self.shared.lock() else {
            return Vec::new();
        };
        return jobs.iter().map(|(id, record)| record.state.report(id)).collect();
    }

    pub fn complete(&self, id: &str, state: JobState) {
        let Ok(mut jobs) = self.shared.lock() else {
            return;
        };
        if let Some(record) = jobs.get_mut(id) {
            record.state = state;
        }
    }

    pub fn cancel(&self, id: &str) {
        let Ok(mut jobs) = self.shared.lock() else {
            return;
        };
        let Some(record) = jobs.get_mut(id) else {
            return;
        };
        record.cancelled = true;
        if let Some(notify) = &record.cancel {
            notify.notify_one();
        }
        if matches!(record.state, JobState::Queued) {
            record.state = JobState::Cancelled;
        }
    }
}

fn set_state(shared: &Mutex<BTreeMap<String, JobRecord>>, id: &str, state: JobState) {
    if let Ok(mut jobs) = shared.lock()
        && let Some(record) = jobs.get_mut(id)
    {
        record.state = state;
    }
}

fn take_cancelled(shared: &Mutex<BTreeMap<String, JobRecord>>, id: &str) -> bool {
    let Ok(mut jobs) = shared.lock() else {
        return true;
    };
    let Some(record) = jobs.get_mut(id) else {
        return true;
    };
    return record.cancelled;
}

fn register_cancel(shared: &Mutex<BTreeMap<String, JobRecord>>, id: &str, notify: Arc<Notify>) {
    if let Ok(mut jobs) = shared.lock()
        && let Some(record) = jobs.get_mut(id)
    {
        record.cancel = Some(notify);
    }
}

async fn run_job(id: String, spec: Spec, shared: Arc<Mutex<BTreeMap<String, JobRecord>>>) {
    let handle = Handle::current();
    if take_cancelled(&shared, &id) {
        return;
    }
    let job_dir = spec.incoming.join(&id);
    if let Err(error) = std::fs::create_dir_all(&job_dir) {
        fail(&shared, &id, format!("cannot create download dir: {error}"));
        return;
    }

    let notify = Arc::new(Notify::new());
    register_cancel(&shared, &id, Arc::clone(&notify));
    set_state(&shared, &id, JobState::Downloading { percent: 0.0, speed: None, eta: None });

    let args = yt_args(&spec, &job_dir);
    let mut process = Process::new(spec.yt_dlp.as_os_str().to_owned());
    process.args(args);
    let mut child = match process.spawn_piped() {
        Ok(child) => child,
        Err(error) => {
            fail(&shared, &id, format!("cannot start yt-dlp: {error}"));
            return;
        }
    };
    let stdout = child.take_stdout();
    let stderr = child.take_stderr();
    let tracker = Arc::new(Mutex::new(ProgressTracker::default()));
    if let Some(stdout) = stdout {
        spawn_progress(&handle, Arc::clone(&shared), id.clone(), stdout, Arc::clone(&tracker));
    }
    let stderr_task = stderr.map(|stderr| {
        handle.spawn(async move {
            let mut text = String::new();
            let mut reader = BufReader::new(stderr);
            let _ = reader.read_to_string(&mut text).await;
            return text;
        })
    });

    let mut status: Option<io::Result<ExitStatus>> = None;
    let cancelled = tokio::select! {
        result = child.wait() => {
            status = Some(result);
            false
        }
        () = notify.notified() => true,
    };

    if cancelled {
        let _ = child.cancel(ESCALATION).await;
        if let Some(task) = stderr_task {
            let _ = task.await;
        }
        let _ = std::fs::remove_dir_all(&job_dir);
        set_state(&shared, &id, JobState::Cancelled);
        evict(&handle, Arc::clone(&shared), id.clone());
        return;
    }

    let stderr_text = match stderr_task {
        Some(task) => task.await.unwrap_or_default(),
        None => String::new(),
    };
    let success = matches!(status, Some(Ok(exit)) if exit.success());
    if !success {
        let _ = std::fs::remove_dir_all(&job_dir);
        fail(&shared, &id, failure_reason(&stderr_text));
        evict(&handle, Arc::clone(&shared), id);
        return;
    }

    let Some(file) = find_output(&job_dir) else {
        let _ = std::fs::remove_dir_all(&job_dir);
        fail(&shared, &id, "yt-dlp produced no output file");
        evict(&handle, Arc::clone(&shared), id);
        return;
    };

    set_state(&shared, &id, JobState::Importing);
    let inputs = vec![file.to_string_lossy().into_owned()];
    let report = import::run(&inputs, &spec.wallpapers, &spec.thumbs, spec.policy, Transfer::Move).await;
    let _ = std::fs::remove_dir_all(&job_dir);

    let items: Vec<Item> = report.imported.iter().map(|imported| imported.item.clone()).collect();
    if items.is_empty() {
        let reason = report
            .results
            .into_iter()
            .next()
            .and_then(|result| result.reason)
            .unwrap_or_else(|| "import failed".to_string());
        fail(&shared, &id, reason);
    } else {
        post(Event::DownloadImported { job: id.clone(), items });
    }
    evict(&handle, shared, id);
}

fn spawn_progress(
    handle: &Handle,
    shared: Arc<Mutex<BTreeMap<String, JobRecord>>>,
    id: String,
    stdout: tokio::process::ChildStdout,
    tracker: Arc<Mutex<ProgressTracker>>,
) {
    handle.spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(payload) = line.strip_prefix(LW_PROGRESS) {
                if let Ok(raw) = serde_json::from_str::<RawProgress>(payload) {
                    let (percent, speed, eta) = {
                        let Ok(mut tracker) = tracker.lock() else { break };
                        (tracker.update(&raw), raw.speed, raw.eta)
                    };
                    set_state(&shared, &id, JobState::Downloading { percent, speed, eta });
                }
            } else if let Some(payload) = line.strip_prefix(LW_POST)
                && let Ok(raw) = serde_json::from_str::<RawPost>(payload)
                && raw.postprocessor.is_some()
            {
                set_state(&shared, &id, JobState::Merging);
            }
        }
    });
}

fn fail(shared: &Mutex<BTreeMap<String, JobRecord>>, id: &str, reason: impl Into<String>) {
    set_state(shared, id, JobState::Failed { reason: reason.into() });
}

fn evict(handle: &Handle, shared: Arc<Mutex<BTreeMap<String, JobRecord>>>, id: String) {
    handle.spawn(async move {
        tokio::time::sleep(EVICT_AFTER).await;
        if let Ok(mut jobs) = shared.lock() {
            jobs.remove(&id);
        }
    });
}

fn failure_reason(stderr: &str) -> String {
    for line in stderr.lines().rev() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    return "yt-dlp failed".to_string();
}

#[allow(clippy::case_sensitive_file_extension_comparisons)]
fn find_output(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.to_string_lossy().to_ascii_lowercase();
        if name.ends_with(".part") || name.ends_with(".ytdl") || name.ends_with(".temp") {
            continue;
        }
        let size = entry.metadata().map_or(0, |metadata| metadata.len());
        if best.as_ref().is_none_or(|(current, _)| size > *current) {
            best = Some((size, path));
        }
    }
    return best.map(|(_, path)| path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_is_clamped_to_the_spec_range() {
        assert_eq!(clamp_height(100), 720);
        assert_eq!(clamp_height(1440), 1440);
        assert_eq!(clamp_height(4000), 2160);
    }

    #[test]
    fn selector_table_matches_the_spec() {
        assert_eq!(
            selector(1080, false),
            "bv*[height<=1080][vcodec^=avc1]+ba[ext=m4a]/bv*[height<=1080][vcodec!*=av01]+ba/b[height<=1080][vcodec!*=av01]/b[height<=1080]/b"
        );
        assert_eq!(
            selector(2160, true),
            "bv*[height<=2160]+ba[ext=m4a]/bv*[height<=2160]+ba/b[height<=2160]/b[height<=2160]/b"
        );
        assert_eq!(
            selector(2160, false),
            "bv*[height<=2160][vcodec^=vp9]+ba[ext=m4a]/bv*[height<=2160][vcodec!*=av01]+ba/b[height<=2160][vcodec!*=av01]/b[height<=2160]/b"
        );
    }

    #[test]
    fn selector_never_offers_av1_without_hardware() {
        assert!(!selector(1080, false).contains("[vcodec^=av01]"));
        assert!(selector(2160, false).contains("[vcodec!*=av01]"));
    }

    #[test]
    fn progress_maps_two_stages_monotonically() {
        let mut tracker = ProgressTracker::default();
        let first = tracker.update(&RawProgress {
            filename: Some("video.f137.mp4".into()),
            downloaded_bytes: Some(50),
            total_bytes: Some(100),
            total_bytes_estimate: None,
            speed: Some(1000.0),
            eta: Some(1.0),
        });
        assert!((first - 0.45).abs() < 1e-9, "{first}");
        let second = tracker.update(&RawProgress {
            filename: Some("audio.f140.m4a".into()),
            downloaded_bytes: Some(50),
            total_bytes: Some(100),
            total_bytes_estimate: None,
            speed: None,
            eta: None,
        });
        assert!((second - 0.95).abs() < 1e-9, "{second}");
        let regressed = tracker.update(&RawProgress {
            filename: Some("audio.f140.m4a".into()),
            downloaded_bytes: Some(10),
            total_bytes: Some(100),
            total_bytes_estimate: None,
            speed: None,
            eta: None,
        });
        assert!((regressed - 0.95).abs() < 1e-9, "progress regressed: {regressed}");
    }

    #[test]
    fn merge_format_is_mp4() {
        assert_eq!(merge_format(), "mp4");
    }
}
