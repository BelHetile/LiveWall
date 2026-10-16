use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::runtime::Handle;
use tokio::sync::Notify;

use crate::app::{Event, post};
use crate::import::{self, Transfer};
use crate::jobs::{self, ESCALATION, Lifecycle, Registry};
use crate::media::probe;
use crate::model::Item;
use crate::process::Process;

pub const MIN_HEIGHT: u32 = 720;
pub const MAX_HEIGHT: u32 = 2160;
pub const H264_CEILING: u32 = 1080;
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

impl Lifecycle for JobState {
    fn queued() -> Self {
        return JobState::Queued;
    }

    fn cancelled() -> Self {
        return JobState::Cancelled;
    }

    fn is_queued(&self) -> bool {
        return matches!(self, JobState::Queued);
    }

    fn is_terminal(&self) -> bool {
        return matches!(self, JobState::Done { .. } | JobState::Failed { .. } | JobState::Cancelled);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Queued,
    Downloading,
    Merging,
    Importing,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobReport {
    pub id: String,
    pub state: Phase,
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

impl JobReport {
    fn new(id: &str, state: Phase, progress: f64) -> Self {
        return Self {
            id: id.to_string(),
            state,
            progress,
            speed: None,
            eta: None,
            item: None,
            path: None,
            reason: None,
        };
    }
}

impl JobState {
    pub fn report(&self, id: &str) -> JobReport {
        return match self {
            JobState::Queued => JobReport::new(id, Phase::Queued, 0.0),
            JobState::Downloading { percent, speed, eta } => {
                JobReport { speed: *speed, eta: *eta, ..JobReport::new(id, Phase::Downloading, *percent) }
            }
            JobState::Merging => JobReport::new(id, Phase::Merging, 0.9),
            JobState::Importing => JobReport::new(id, Phase::Importing, 1.0),
            JobState::Done { item, path } => {
                JobReport { item: Some(item.clone()), path: Some(path.clone()), ..JobReport::new(id, Phase::Done, 1.0) }
            }
            JobState::Failed { reason } => {
                JobReport { reason: Some(reason.clone()), ..JobReport::new(id, Phase::Failed, 0.0) }
            }
            JobState::Cancelled => JobReport::new(id, Phase::Cancelled, 0.0),
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
    args.push("mp4".into());
    args.push("--output".into());
    args.push(job_dir.join("%(title).120B.%(ext)s").into_os_string());
    args.push(spec.url.clone().into());
    return args;
}

pub struct Queue {
    jobs: Registry<String, JobState>,
    handle: Handle,
}

impl Queue {
    pub fn new(handle: Handle) -> Self {
        return Self { jobs: Registry::new(handle.clone()), handle };
    }

    pub fn enqueue(&self, spec: Spec) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.jobs.insert(id.clone());
        let jobs = self.jobs.clone();
        let task_id = id.clone();
        self.handle.spawn(async move {
            run_job(task_id, spec, jobs).await;
        });
        return id;
    }

    pub fn status(&self, id: &str) -> Option<JobState> {
        return self.jobs.status(&id.to_string());
    }

    pub fn reports(&self) -> Vec<JobReport> {
        return self.jobs.snapshot().iter().map(|(id, state)| state.report(id)).collect();
    }

    pub fn complete(&self, id: &str, state: JobState) {
        self.jobs.set_state(&id.to_string(), state);
    }

    pub fn cancel(&self, id: &str) {
        self.jobs.cancel(&id.to_string());
    }
}

async fn run_job(id: String, spec: Spec, jobs: Registry<String, JobState>) {
    let handle = Handle::current();
    if jobs.take_cancelled(&id) {
        return;
    }
    let job_dir = spec.incoming.join(&id);
    if let Err(error) = std::fs::create_dir_all(&job_dir) {
        fail(&jobs, &id, format!("cannot create download dir: {error}"));
        return;
    }

    let notify = Arc::new(Notify::new());
    jobs.register_cancel(&id, Arc::clone(&notify));
    jobs.set_state(&id, JobState::Downloading { percent: 0.0, speed: None, eta: None });

    let args = yt_args(&spec, &job_dir);
    let mut process = Process::new(spec.yt_dlp.as_os_str().to_owned());
    process.args(args);
    let mut child = match process.spawn_piped() {
        Ok(child) => child,
        Err(error) => {
            fail(&jobs, &id, format!("cannot start yt-dlp: {error}"));
            return;
        }
    };
    let stdout = child.take_stdout();
    let stderr = child.take_stderr();
    let tracker = Arc::new(Mutex::new(ProgressTracker::default()));
    if let Some(stdout) = stdout {
        spawn_progress(&handle, jobs.clone(), id.clone(), stdout, Arc::clone(&tracker));
    }
    let stderr_task = jobs::drain_stderr(&handle, stderr);

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
        let _ = jobs::join_stderr(stderr_task).await;
        let _ = std::fs::remove_dir_all(&job_dir);
        jobs.finish(&id, JobState::Cancelled);
        return;
    }

    let stderr_text = jobs::join_stderr(stderr_task).await;
    let success = matches!(status, Some(Ok(exit)) if exit.success());
    if !success {
        let _ = std::fs::remove_dir_all(&job_dir);
        fail(&jobs, &id, jobs::failure_reason(&stderr_text, "yt-dlp"));
        return;
    }

    let Some(file) = find_output(&job_dir) else {
        let _ = std::fs::remove_dir_all(&job_dir);
        fail(&jobs, &id, "yt-dlp produced no output file");
        return;
    };

    jobs.set_state(&id, JobState::Importing);
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
        fail(&jobs, &id, reason);
        return;
    }
    post(Event::DownloadImported { job: id.clone(), items });
    jobs.evict(&id);
}

fn spawn_progress(
    handle: &Handle,
    jobs: Registry<String, JobState>,
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
                    jobs.set_state(&id, JobState::Downloading { percent, speed, eta });
                }
            } else if let Some(payload) = line.strip_prefix(LW_POST)
                && let Ok(raw) = serde_json::from_str::<RawPost>(payload)
                && raw.postprocessor.is_some()
            {
                jobs.set_state(&id, JobState::Merging);
            }
        }
    });
}

fn fail(jobs: &Registry<String, JobState>, id: &String, reason: impl Into<String>) {
    jobs.set_state(id, JobState::Failed { reason: reason.into() });
    jobs.evict(id);
}

fn find_output(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let extension = path.extension().map(|value| value.to_string_lossy().to_ascii_lowercase());
        if matches!(extension.as_deref(), Some("part" | "ytdl" | "temp")) {
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
}
