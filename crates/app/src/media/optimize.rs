use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::runtime::Handle;
use tokio::sync::{Notify, OnceCell, mpsc};

use crate::app::{Event, post};
use crate::media::probe::{self, Probe};
use crate::model::{ItemId, ItemKind};
use crate::process::Process;

pub const ESCALATION: Duration = Duration::from_secs(3);
pub const PASSTHROUGH_AUDIO: [&str; 6] = ["aac", "alac", "ac-3", "ec-3", "lpcm", "mp3"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audio {
    Drop,
    Copy,
    Encode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub bit_depth: Option<u32>,
    pub hdr: bool,
    pub transfer: Option<String>,
    pub audio: Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub fps_limit: u32,
    pub max_pixels: u32,
    pub hevc: bool,
}

pub fn has_hevc_videotoolbox(encoders: &str) -> bool {
    return encoders.lines().any(|line| line.split_whitespace().any(|token| token == "hevc_videotoolbox"));
}

pub fn audio_plan(codec: Option<&str>, probed: bool) -> Audio {
    if !probed {
        return Audio::Encode;
    }
    let Some(codec) = codec else {
        return Audio::Drop;
    };
    if PASSTHROUGH_AUDIO.contains(&codec) {
        return Audio::Copy;
    }
    return Audio::Encode;
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn even(value: f64) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        return 2;
    }
    let rounded = value.min(f64::from(u32::MAX)) as u32;
    return (rounded - rounded % 2).max(2);
}

pub fn scale_dims(width: u32, height: u32, max_pixels: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 || max_pixels == 0 {
        return None;
    }
    let longest = width.max(height);
    if longest <= max_pixels {
        return None;
    }
    let factor = f64::from(max_pixels) / f64::from(longest);
    let scaled_width = even((f64::from(width) * factor).round());
    let scaled_height = even((f64::from(height) * factor).round());
    if scaled_width > width || scaled_height > height {
        return None;
    }
    return Some((scaled_width, scaled_height));
}

fn filters(source: &Source, target: &Target) -> Vec<String> {
    let mut filters = Vec::new();
    if let Some((width, height)) = scale_dims(source.width, source.height, target.max_pixels) {
        filters.push(format!("scale={width}:{height}"));
    }
    if target.fps_limit > 0 && source.fps > f64::from(target.fps_limit) {
        filters.push(format!("fps={}", target.fps_limit));
    }
    return filters;
}

fn hdr_trc(transfer: Option<&str>) -> &'static str {
    if transfer.is_some_and(|value| value.contains("2084")) {
        return "smpte2084";
    }
    return "arib-std-b67";
}

fn push(args: &mut Vec<OsString>, value: impl Into<OsString>) {
    args.push(value.into());
}

pub fn build_args(input: &Path, output: &Path, source: &Source, target: &Target) -> Vec<OsString> {
    let mut args = Vec::new();
    push(&mut args, "-hide_banner");
    push(&mut args, "-nostdin");
    push(&mut args, "-y");
    push(&mut args, "-i");
    push(&mut args, input.as_os_str().to_owned());
    push(&mut args, "-c:v");
    push(&mut args, if target.hevc { "hevc_videotoolbox" } else { "h264_videotoolbox" });
    if target.hevc {
        push(&mut args, "-tag:v");
        push(&mut args, "hvc1");
        if source.bit_depth.is_some_and(|depth| depth > 8) {
            push(&mut args, "-pix_fmt");
            push(&mut args, "p010le");
        }
        if source.hdr {
            push(&mut args, "-color_primaries");
            push(&mut args, "bt2020");
            push(&mut args, "-color_trc");
            push(&mut args, hdr_trc(source.transfer.as_deref()));
            push(&mut args, "-colorspace");
            push(&mut args, "bt2020nc");
            push(&mut args, "-color_range");
            push(&mut args, "tv");
        }
    } else {
        push(&mut args, "-color_primaries");
        push(&mut args, "bt709");
        push(&mut args, "-color_trc");
        push(&mut args, "bt709");
        push(&mut args, "-colorspace");
        push(&mut args, "bt709");
    }
    let chain = filters(source, target);
    if !chain.is_empty() {
        push(&mut args, "-vf");
        push(&mut args, chain.join(","));
    }
    match source.audio {
        Audio::Drop => push(&mut args, "-an"),
        Audio::Copy => {
            push(&mut args, "-c:a");
            push(&mut args, "copy");
        }
        Audio::Encode => {
            push(&mut args, "-c:a");
            push(&mut args, "aac");
            push(&mut args, "-b:a");
            push(&mut args, "176k");
            push(&mut args, "-ar");
            push(&mut args, "48000");
            push(&mut args, "-ac");
            push(&mut args, "2");
        }
    }
    push(&mut args, "-movflags");
    push(&mut args, "+faststart");
    push(&mut args, "-progress");
    push(&mut args, "pipe:1");
    push(&mut args, "-nostats");
    push(&mut args, "-f");
    push(&mut args, "mp4");
    push(&mut args, output.as_os_str().to_owned());
    return args;
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ProgressState {
    pub out_time_us: i64,
    pub speed: Option<f64>,
    pub ended: bool,
}

pub fn apply_line(state: &mut ProgressState, line: &str) {
    let Some((key, value)) = line.split_once('=') else {
        return;
    };
    let key = key.trim();
    let value = value.trim();
    match key {
        "out_time_us" | "out_time_ms" => {
            if let Ok(parsed) = value.parse::<i64>() {
                state.out_time_us = parsed;
            }
        }
        "speed" => state.speed = value.trim_end_matches('x').parse::<f64>().ok(),
        "progress" => state.ended = value == "end",
        _ => {}
    }
}

#[allow(clippy::cast_precision_loss)]
pub fn percent(state: &ProgressState, total_seconds: f64) -> f64 {
    if total_seconds <= 0.0 {
        return if state.ended { 1.0 } else { 0.0 };
    }
    let done = state.out_time_us.max(0) as f64 / 1_000_000.0;
    return (done / total_seconds).clamp(0.0, 1.0);
}

#[derive(Debug, Clone, PartialEq)]
pub enum JobState {
    Queued,
    Running { percent: f64, speed: Option<f64> },
    Done { path: String },
    Failed { reason: String },
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobReport {
    pub id: String,
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl JobState {
    pub fn report(&self, id: ItemId) -> JobReport {
        let id = id.to_string();
        return match self {
            JobState::Queued => JobReport { id, state: "queued", percent: None, speed: None, path: None, reason: None },
            JobState::Running { percent, speed } => {
                JobReport { id, state: "running", percent: Some(*percent), speed: *speed, path: None, reason: None }
            }
            JobState::Done { path } => {
                JobReport { id, state: "done", percent: Some(1.0), speed: None, path: Some(path.clone()), reason: None }
            }
            JobState::Failed { reason } => {
                JobReport { id, state: "failed", percent: None, speed: None, path: None, reason: Some(reason.clone()) }
            }
            JobState::Cancelled => {
                JobReport { id, state: "cancelled", percent: None, speed: None, path: None, reason: None }
            }
        };
    }
}

pub struct Job {
    pub item: ItemId,
    pub input: PathBuf,
    pub thumbs_dir: PathBuf,
    pub ffmpeg: PathBuf,
    pub fps_limit: u32,
    pub max_pixels: u32,
    pub duration: f64,
    pub fallback: Source,
}

struct JobRecord {
    state: JobState,
    cancel: Option<Arc<Notify>>,
    cancelled: bool,
}

struct Shared {
    jobs: Mutex<BTreeMap<ItemId, JobRecord>>,
    hevc: OnceCell<bool>,
}

pub struct Queue {
    tx: mpsc::UnboundedSender<Job>,
    shared: Arc<Shared>,
}

impl Queue {
    pub fn new(handle: &Handle) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared { jobs: Mutex::new(BTreeMap::new()), hevc: OnceCell::new() });
        let mut worker = Worker { shared: Arc::clone(&shared), handle: handle.clone() };
        handle.spawn(async move {
            worker.run(rx).await;
        });
        return Self { tx, shared };
    }

    pub fn enqueue(&self, job: Job) -> bool {
        {
            let Ok(mut jobs) = self.shared.jobs.lock() else {
                return false;
            };
            if jobs.get(&job.item).is_some_and(|record| !record.terminal()) {
                return false;
            }
            jobs.insert(job.item, JobRecord { state: JobState::Queued, cancel: None, cancelled: false });
        }
        return self.tx.send(job).is_ok();
    }

    pub fn status(&self, item: ItemId) -> Option<JobState> {
        return self.shared.jobs.lock().ok()?.get(&item).map(|record| record.state.clone());
    }

    pub fn cancel(&self, item: ItemId) {
        let Ok(mut jobs) = self.shared.jobs.lock() else {
            return;
        };
        let Some(record) = jobs.get_mut(&item) else {
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

impl JobRecord {
    fn terminal(&self) -> bool {
        return matches!(self.state, JobState::Done { .. } | JobState::Failed { .. } | JobState::Cancelled);
    }
}

struct Worker {
    shared: Arc<Shared>,
    handle: Handle,
}

impl Worker {
    async fn run(&mut self, mut rx: mpsc::UnboundedReceiver<Job>) {
        while let Some(job) = rx.recv().await {
            self.run_job(job).await;
        }
    }

    fn set_state(&self, item: ItemId, state: JobState) {
        if let Ok(mut jobs) = self.shared.jobs.lock()
            && let Some(record) = jobs.get_mut(&item)
        {
            record.state = state;
        }
    }

    fn take_cancelled(&self, item: ItemId) -> bool {
        let Ok(mut jobs) = self.shared.jobs.lock() else {
            return true;
        };
        let Some(record) = jobs.get_mut(&item) else {
            return true;
        };
        if record.cancelled {
            record.state = JobState::Cancelled;
            return true;
        }
        return false;
    }

    fn set_running(&self, item: ItemId, notify: Arc<Notify>) {
        if let Ok(mut jobs) = self.shared.jobs.lock()
            && let Some(record) = jobs.get_mut(&item)
        {
            record.state = JobState::Running { percent: 0.0, speed: None };
            record.cancel = Some(notify);
        }
    }

    async fn hevc_available(&self, ffmpeg: &Path) -> bool {
        return *self
            .shared
            .hevc
            .get_or_init(|| async {
                let mut process = Process::new(ffmpeg.as_os_str().to_owned());
                process.args(["-hide_banner", "-encoders"]);
                let Ok(output) = process.output().await else {
                    return false;
                };
                let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
                text.push_str(&String::from_utf8_lossy(&output.stderr));
                return has_hevc_videotoolbox(&text);
            })
            .await;
    }

    fn spawn_reader(
        &self,
        stdout: tokio::process::ChildStdout,
        item: ItemId,
        total: f64,
        state: Arc<Mutex<ProgressState>>,
    ) {
        let shared = Arc::clone(&self.shared);
        self.handle.spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(mut guard) = state.lock() else {
                    break;
                };
                apply_line(&mut guard, &line);
                let percent = percent(&guard, total);
                let speed = guard.speed;
                drop(guard);
                if let Ok(mut jobs) = shared.jobs.lock()
                    && let Some(record) = jobs.get_mut(&item)
                    && matches!(record.state, JobState::Running { .. })
                {
                    record.state = JobState::Running { percent, speed };
                }
            }
        });
    }

    async fn run_job(&self, job: Job) {
        let item = job.item;
        if self.take_cancelled(item) {
            return;
        }
        let notify = Arc::new(Notify::new());
        self.set_running(item, Arc::clone(&notify));

        let temp = temp_path(&job.input, item);
        let _ = std::fs::remove_file(&temp);
        let hevc = self.hevc_available(&job.ffmpeg).await;

        let (source, duration) = self.probe_source(&job).await;
        let target = Target { fps_limit: job.fps_limit, max_pixels: job.max_pixels, hevc };
        let args = build_args(&job.input, &temp, &source, &target);

        let mut process = Process::new(job.ffmpeg.as_os_str().to_owned());
        process.args(args);
        let mut child = match process.spawn_piped() {
            Ok(child) => child,
            Err(error) => {
                self.fail(item, format!("cannot start ffmpeg: {error}"));
                return;
            }
        };
        let stdout = child.take_stdout();
        let stderr = child.take_stderr();
        let state = Arc::new(Mutex::new(ProgressState::default()));
        if let Some(stdout) = stdout {
            self.spawn_reader(stdout, item, duration, Arc::clone(&state));
        }
        let stderr_task = stderr.map(|stderr| {
            self.handle.spawn(async move {
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
            let _ = std::fs::remove_file(&temp);
            self.set_state(item, JobState::Cancelled);
            return;
        }

        let stderr_text = match stderr_task {
            Some(task) => task.await.unwrap_or_default(),
            None => String::new(),
        };

        let success = matches!(status, Some(Ok(exit)) if exit.success());
        if !success {
            let _ = std::fs::remove_file(&temp);
            self.fail(item, failure_reason(&stderr_text));
            return;
        }

        let probed = {
            let probe_path = temp.clone();
            let task =
                tokio::task::spawn_blocking(move || probe::inspect(&probe_path.to_string_lossy(), ItemKind::Video));
            if let Ok(probed) = task.await {
                probed
            } else {
                let _ = std::fs::remove_file(&temp);
                self.fail(item, "output probe failed".to_string());
                return;
            }
        };
        if !probed.decodable {
            let _ = std::fs::remove_file(&temp);
            self.fail(item, "optimized output is not playable".to_string());
            return;
        }

        let final_path = job.input.with_extension("mp4");
        if let Err(error) = std::fs::rename(&temp, &final_path) {
            let _ = std::fs::remove_file(&temp);
            self.fail(item, format!("cannot replace media: {error}"));
            return;
        }
        if final_path != job.input {
            let _ = std::fs::remove_file(&job.input);
        }

        let _ = std::fs::remove_file(crate::media::thumb::file(&job.thumbs_dir, item));
        let path = final_path.to_string_lossy().into_owned();
        let _ =
            crate::media::thumb::ensure(item, ItemKind::Video, path.clone(), probed.duration, job.thumbs_dir.clone())
                .await;

        self.set_state(item, JobState::Done { path: path.clone() });
        post(Event::OptimizeDone { item, result: Ok(Optimized { path, probe: probed }) });
    }

    async fn probe_source(&self, job: &Job) -> (Source, f64) {
        let input = job.input.clone();
        let fallback = job.fallback.clone();
        let duration = job.duration;
        let task = tokio::task::spawn_blocking(move || probe::inspect(&input.to_string_lossy(), ItemKind::Video));
        let probed = task.await.ok();
        let Some(probed) = probed else {
            return (fallback, duration);
        };
        let total = if probed.duration > 0.0 { probed.duration } else { duration };
        return (merge_source(&probed, &fallback), total);
    }

    fn fail(&self, item: ItemId, reason: String) {
        self.set_state(item, JobState::Failed { reason: reason.clone() });
        post(Event::OptimizeDone { item, result: Err(reason) });
    }
}

fn merge_source(probed: &Probe, fallback: &Source) -> Source {
    if probed.decodable {
        return Source {
            width: probed.width,
            height: probed.height,
            fps: probed.fps,
            bit_depth: probed.bit_depth,
            hdr: probed.hdr,
            transfer: probed.transfer.clone(),
            audio: audio_plan(probed.audio_codec.as_deref(), true),
        };
    }
    return fallback.clone();
}

fn failure_reason(stderr: &str) -> String {
    for line in stderr.lines().rev() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    return "ffmpeg failed".to_string();
}

fn temp_path(input: &Path, item: ItemId) -> PathBuf {
    let dir = input.parent().unwrap_or_else(|| Path::new("."));
    return dir.join(format!("{item}.optimize.mp4"));
}

pub struct Optimized {
    pub path: String,
    pub probe: Probe,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> Source {
        return Source {
            width: 1920,
            height: 1080,
            fps: 30.0,
            bit_depth: Some(8),
            hdr: false,
            transfer: None,
            audio: Audio::Copy,
        };
    }

    #[test]
    fn hevc_encoder_detection() {
        let text = " V....D h264_videotoolbox    VideoToolbox H.264 Encoder\n V....D hevc_videotoolbox    VideoToolbox H.265 Encoder";
        assert!(has_hevc_videotoolbox(text));
        assert!(!has_hevc_videotoolbox(" V....D h264_videotoolbox  VideoToolbox H.264 Encoder"));
    }

    #[test]
    fn arguments_for_hevc_with_scale_and_fps_cap() {
        let target = Target { fps_limit: 30, max_pixels: 2560, hevc: true };
        let source = Source { width: 3840, height: 2160, fps: 60.0, ..source() };
        let args = build_args(Path::new("/in/webm"), Path::new("/out/opt.mp4"), &source, &target);
        let joined: Vec<String> = args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert!(joined.contains(&"hevc_videotoolbox".to_string()));
        assert!(joined.windows(2).any(|pair| pair == ["-tag:v", "hvc1"]));
        assert!(joined.windows(2).any(|pair| pair == ["-vf", "scale=2560:1440,fps=30"]));
        assert!(joined.windows(2).any(|pair| pair == ["-c:a", "copy"]));
        assert!(joined.windows(2).any(|pair| pair == ["-movflags", "+faststart"]));
        assert_eq!(joined.last(), Some(&"/out/opt.mp4".to_string()));
    }

    #[test]
    fn h264_fallback_tags_bt709_and_encodes_audio() {
        let target = Target { fps_limit: 30, max_pixels: 2560, hevc: false };
        let source = Source { hdr: true, audio: Audio::Encode, ..source() };
        let args = build_args(Path::new("/in.mp4"), Path::new("/out.mp4"), &source, &target);
        let joined: Vec<String> = args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert!(joined.contains(&"h264_videotoolbox".to_string()));
        assert!(!joined.contains(&"hvc1".to_string()));
        assert!(joined.windows(2).any(|pair| pair == ["-color_trc", "bt709"]));
        assert!(joined.windows(2).any(|pair| pair == ["-c:a", "aac"]));
        assert!(joined.windows(2).any(|pair| pair == ["-b:a", "176k"]));
    }

    #[test]
    fn ten_bit_hdr_uses_main10_and_color_tags() {
        let target = Target { fps_limit: 0, max_pixels: 0, hevc: true };
        let source = Source { bit_depth: Some(10), hdr: true, transfer: Some("SMPTE ST 2084".to_string()), ..source() };
        let args = build_args(Path::new("/in.mp4"), Path::new("/out.mp4"), &source, &target);
        let joined: Vec<String> = args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert!(joined.windows(2).any(|pair| pair == ["-pix_fmt", "p010le"]));
        assert!(joined.windows(2).any(|pair| pair == ["-color_trc", "smpte2084"]));
        assert!(joined.windows(2).any(|pair| pair == ["-color_primaries", "bt2020"]));
    }

    #[test]
    fn missing_audio_drops_the_track() {
        let target = Target { fps_limit: 0, max_pixels: 0, hevc: true };
        let source = Source { audio: Audio::Drop, ..source() };
        let args = build_args(Path::new("/in.mp4"), Path::new("/out.mp4"), &source, &target);
        let joined: Vec<String> = args.iter().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert!(joined.contains(&"-an".to_string()));
    }

    #[test]
    fn scaling_never_upscales_and_keeps_even_dims() {
        assert_eq!(scale_dims(3840, 2160, 2560), Some((2560, 1440)));
        assert_eq!(scale_dims(1920, 1080, 2560), None);
        assert_eq!(scale_dims(1080, 1920, 1080), Some((608, 1080)));
        assert_eq!(scale_dims(0, 0, 2560), None);
        let (width, height) = scale_dims(1921, 1081, 1000).expect("scale");
        assert_eq!(width % 2, 0);
        assert_eq!(height % 2, 0);
        assert!(width <= 1000 && height <= 1000);
    }

    #[test]
    fn progress_lines_drive_percent_and_speed() {
        let mut state = ProgressState::default();
        apply_line(&mut state, "out_time_us=1500000");
        apply_line(&mut state, "speed=1.5x");
        assert!((percent(&state, 3.0) - 0.5).abs() < 1e-9);
        assert_eq!(state.speed, Some(1.5));
        apply_line(&mut state, "progress=end");
        assert!(state.ended);
        assert!((percent(&state, 0.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn audio_plan_covers_every_case() {
        assert_eq!(audio_plan(Some("aac"), true), Audio::Copy);
        assert_eq!(audio_plan(Some("opus"), true), Audio::Encode);
        assert_eq!(audio_plan(None, true), Audio::Drop);
        assert_eq!(audio_plan(None, false), Audio::Encode);
        assert_eq!(audio_plan(Some("aac"), false), Audio::Encode);
    }
}
