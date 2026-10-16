use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::runtime::Handle;
use tokio::sync::{Notify, OnceCell, mpsc};

use crate::app::{Event, post};
use crate::jobs;
use crate::media::apple;
use crate::media::probe::{self, Probe};
use crate::model::{ItemId, ItemKind};
use crate::process::Process;

pub const ESCALATION: Duration = Duration::from_secs(3);
pub const PASSTHROUGH_AUDIO: [&str; 5] = ["aac", "alac", "ac-3", "ec-3", "mp3"];
pub const BPP_8BIT: f64 = 0.07;
pub const BPP_10BIT: f64 = 0.10;
pub const MIN_BITRATE: u64 = 2_000_000;
pub const MIN_BITRATE_FLOOR: u64 = 300_000;
pub const REFERENCE_PIXELS: f64 = 1920.0 * 1080.0;
pub const GIF_MIN_DELAY: f64 = 0.02;
pub const GIF_DEFAULT_DELAY: f64 = 0.1;
pub const SOURCE_SCALE_EXPONENT: f64 = 0.75;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SourceRate {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub bit_rate: f64,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Apple,
    Ffmpeg,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoPlan {
    pub input: PathBuf,
    pub output: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub bitrate: u64,
    pub hevc: bool,
    pub ten_bit: bool,
    pub hdr: bool,
    pub primaries: Option<String>,
    pub transfer: Option<String>,
    pub audio: Audio,
    pub duration: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GifPlan {
    pub input: PathBuf,
    pub output: PathBuf,
    pub max_pixels: u32,
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

pub fn route(container: Option<&str>, codec: Option<&str>, av1_hw: bool) -> Route {
    if container.is_some_and(|value| matches!(value, "webm" | "mkv")) {
        return Route::Ffmpeg;
    }
    return match codec {
        Some("vp08") => Route::Ffmpeg,
        Some("av01") if !av1_hw => Route::Ffmpeg,
        _ => Route::Apple,
    };
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
pub fn even(value: f64) -> u32 {
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

fn output_dims(width: u32, height: u32, max_pixels: u32) -> (u32, u32) {
    return scale_dims(width, height, max_pixels).unwrap_or_else(|| (even(f64::from(width)), even(f64::from(height))));
}

#[allow(clippy::cast_precision_loss)]
fn floor_bitrate(width: u32, height: u32) -> f64 {
    let pixels = f64::from(width) * f64::from(height);
    return (MIN_BITRATE as f64) * (pixels / REFERENCE_PIXELS).min(1.0);
}

#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn bitrate(width: u32, height: u32, fps: f64, ten_bit: bool, source: Option<SourceRate>) -> u64 {
    let bpp = if ten_bit { BPP_10BIT } else { BPP_8BIT };
    let fps = if fps.is_finite() && fps > 0.0 { fps } else { 30.0 };
    let pixels = f64::from(width) * f64::from(height);
    let mut rate = (pixels * fps * bpp).max(floor_bitrate(width, height).max(MIN_BITRATE_FLOOR as f64));
    if let Some(cap) = source.and_then(|source| source_cap(source, pixels, fps)) {
        rate = rate.min(cap);
    }
    return rate.max(0.0) as u64;
}

fn source_cap(source: SourceRate, pixels: f64, fps: f64) -> Option<f64> {
    let source_pixels = f64::from(source.width) * f64::from(source.height);
    let source_fps = if source.fps.is_finite() && source.fps > 0.0 { source.fps } else { fps };
    if source_pixels <= 0.0 || !source.bit_rate.is_finite() || source.bit_rate <= 0.0 || pixels > source_pixels {
        return None;
    }
    let ratio = (pixels * fps) / (source_pixels * source_fps);
    return Some(source.bit_rate * ratio.min(1.0).powf(SOURCE_SCALE_EXPONENT));
}

fn source_rate(probed: Option<&Probe>) -> Option<SourceRate> {
    let probed = probed?;
    let codec = probed.codec.as_deref()?;
    if !probe::hardware_decodes(codec) {
        return None;
    }
    let bit_rate = probed.bit_rate?;
    return Some(SourceRate { width: probed.width, height: probed.height, fps: probed.fps, bit_rate });
}

fn capped_fps(fps: f64, limit: u32) -> f64 {
    let base = if fps.is_finite() && fps > 0.0 { fps } else { f64::from(limit.max(1)) };
    if limit > 0 {
        return base.min(f64::from(limit));
    }
    return base;
}

pub fn video_plan(
    input: &Path,
    output: &Path,
    probed: Option<&Probe>,
    fallback: &Source,
    target: &Target,
    duration: f64,
) -> VideoPlan {
    let width = probed.map_or(fallback.width, |probe| probe.width);
    let height = probed.map_or(fallback.height, |probe| probe.height);
    let source_fps = probed.map_or(fallback.fps, |probe| probe.fps);
    let bit_depth = probed.and_then(|probe| probe.bit_depth).or(fallback.bit_depth).unwrap_or(8);
    let hdr = probed.map_or(fallback.hdr, |probe| probe.hdr);
    let transfer = probed.and_then(|probe| probe.transfer.clone()).or_else(|| fallback.transfer.clone());
    let audio = probed.map_or(fallback.audio, |probe| audio_plan(probe.audio_codec.as_deref(), true));
    let (width, height) = output_dims(width, height, target.max_pixels);
    let fps = capped_fps(source_fps, target.fps_limit);
    let ten_bit = bit_depth > 8;
    return VideoPlan {
        input: input.to_path_buf(),
        output: output.to_path_buf(),
        width,
        height,
        fps,
        bitrate: bitrate(width, height, fps, ten_bit, source_rate(probed)),
        hevc: target.hevc,
        ten_bit,
        hdr,
        primaries: probed.and_then(|probe| probe.color_primaries.clone()),
        transfer,
        audio,
        duration,
    };
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
    let (width, height) = output_dims(source.width, source.height, target.max_pixels);
    let ten_bit = source.bit_depth.is_some_and(|depth| depth > 8);
    push(&mut args, "-b:v");
    push(&mut args, bitrate(width, height, capped_fps(source.fps, target.fps_limit), ten_bit, None).to_string());
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
    pub kind: ItemKind,
    pub input: PathBuf,
    pub thumbs_dir: PathBuf,
    pub ffmpeg: Option<PathBuf>,
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
    jobs: Arc<Mutex<BTreeMap<ItemId, JobRecord>>>,
    hevc: OnceCell<bool>,
}

pub struct Queue {
    tx: mpsc::UnboundedSender<Job>,
    shared: Arc<Shared>,
    handle: Handle,
}

impl Queue {
    pub fn new(handle: &Handle) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared { jobs: Arc::new(Mutex::new(BTreeMap::new())), hevc: OnceCell::new() });
        let mut worker = Worker { shared: Arc::clone(&shared), handle: handle.clone() };
        handle.spawn(async move {
            worker.run(rx).await;
        });
        return Self { tx, shared, handle: handle.clone() };
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
            jobs::evict_after(&self.handle, Arc::clone(&self.shared.jobs), item, JobRecord::terminal);
        }
    }
}

impl JobRecord {
    fn terminal(&self) -> bool {
        return matches!(self.state, JobState::Done { .. } | JobState::Failed { .. } | JobState::Cancelled);
    }
}

enum Outcome {
    Success,
    Cancelled,
    Failed(String),
}

struct Worker {
    shared: Arc<Shared>,
    handle: Handle,
}

impl Worker {
    async fn run(&mut self, mut rx: mpsc::UnboundedReceiver<Job>) {
        while let Some(job) = rx.recv().await {
            let item = job.item;
            let worker = Worker { shared: Arc::clone(&self.shared), handle: self.handle.clone() };
            if let Err(error) = tokio::spawn(async move { worker.run_job(job).await }).await {
                tracing::warn!(%item, %error, "optimize job failed unexpectedly");
                self.fail(item, format!("optimize failed: {error}"));
            }
        }
    }

    fn set_state(&self, item: ItemId, state: JobState) {
        if let Ok(mut jobs) = self.shared.jobs.lock()
            && let Some(record) = jobs.get_mut(&item)
        {
            record.state = state;
        }
    }

    fn finish(&self, item: ItemId, state: JobState) {
        self.set_state(item, state);
        jobs::evict_after(&self.handle, Arc::clone(&self.shared.jobs), item, JobRecord::terminal);
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

    fn progress_callback(&self, item: ItemId, duration: f64) -> Arc<dyn Fn(f64) + Send + Sync> {
        let shared = Arc::clone(&self.shared);
        let start = Instant::now();
        return Arc::new(move |fraction: f64| {
            let elapsed = start.elapsed().as_secs_f64();
            let speed = if elapsed > 0.0 && duration > 0.0 { Some(fraction * duration / elapsed) } else { None };
            if let Ok(mut jobs) = shared.jobs.lock()
                && let Some(record) = jobs.get_mut(&item)
                && matches!(record.state, JobState::Running { .. })
            {
                record.state = JobState::Running { percent: fraction.clamp(0.0, 1.0), speed };
            }
        });
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

        let result = match self.transcode(&job, &temp, &notify).await {
            Outcome::Cancelled => {
                let _ = std::fs::remove_file(&temp);
                self.finish(item, JobState::Cancelled);
                return;
            }
            Outcome::Failed(reason) => Err(reason),
            Outcome::Success => install(&job, &temp).await,
        };
        match result {
            Ok(optimized) => {
                self.finish(item, JobState::Done { path: optimized.path.clone() });
                post(Event::OptimizeDone { item, result: Ok(optimized) });
            }
            Err(reason) => {
                let _ = std::fs::remove_file(&temp);
                self.fail(item, reason);
            }
        }
    }

    async fn transcode(&self, job: &Job, temp: &Path, notify: &Arc<Notify>) -> Outcome {
        if job.kind == ItemKind::Gif {
            return self.run_apple_gif(job, temp, notify).await;
        }
        let (source, probed, duration) = self.probe_source(job).await;
        let codec = probed.as_ref().and_then(|probe| probe.codec.clone());
        let container = probed.as_ref().and_then(|probe| probe.container.clone());
        let av1_hw = probed.as_ref().is_some_and(|probe| probe.av1_hw);
        if codec.as_deref() == Some("vp09") {
            apple::register_vp9();
        }
        return match route(container.as_deref(), codec.as_deref(), av1_hw) {
            Route::Apple => self.run_apple_video(job, temp, probed.as_ref(), &source, duration, notify).await,
            Route::Ffmpeg => self.run_ffmpeg(job, temp, &source, duration, notify).await,
        };
    }

    async fn run_apple_video(
        &self,
        job: &Job,
        temp: &Path,
        probed: Option<&Probe>,
        source: &Source,
        duration: f64,
        notify: &Arc<Notify>,
    ) -> Outcome {
        let target = Target { fps_limit: job.fps_limit, max_pixels: job.max_pixels, hevc: true };
        let plan = video_plan(&job.input, temp, probed, source, &target, duration);
        if plan.width < 2 || plan.height < 2 {
            return Outcome::Failed("cannot read the video size".to_string());
        }
        return self.run_apple(job.item, apple::Task::Video(plan), duration, notify).await;
    }

    async fn run_apple_gif(&self, job: &Job, temp: &Path, notify: &Arc<Notify>) -> Outcome {
        let plan = GifPlan { input: job.input.clone(), output: temp.to_path_buf(), max_pixels: job.max_pixels };
        return self.run_apple(job.item, apple::Task::Gif(plan), job.duration, notify).await;
    }

    async fn run_apple(&self, item: ItemId, task: apple::Task, duration: f64, notify: &Arc<Notify>) -> Outcome {
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = self.progress_callback(item, duration);
        let worker_cancel = Arc::clone(&cancel);
        let handle = tokio::task::spawn_blocking(move || apple::run(&task, &progress, &worker_cancel));
        let waiter_cancel = Arc::clone(&cancel);
        let notify = Arc::clone(notify);
        let waiter = tokio::spawn(async move {
            notify.notified().await;
            waiter_cancel.store(true, Ordering::SeqCst);
        });
        let joined = handle.await;
        let cancelled = cancel.load(Ordering::SeqCst);
        waiter.abort();
        if cancelled {
            return Outcome::Cancelled;
        }
        return match joined {
            Ok(Ok(())) => Outcome::Success,
            Ok(Err(apple::AppleError::Cancelled)) => Outcome::Cancelled,
            Ok(Err(error)) => Outcome::Failed(error.to_string()),
            Err(error) => Outcome::Failed(format!("optimize task failed: {error}")),
        };
    }

    async fn run_ffmpeg(&self, job: &Job, temp: &Path, source: &Source, duration: f64, notify: &Notify) -> Outcome {
        let Some(ffmpeg) = &job.ffmpeg else {
            return Outcome::Failed("ffmpeg is not installed".to_string());
        };
        let hevc = self.hevc_available(ffmpeg).await;
        let target = Target { fps_limit: job.fps_limit, max_pixels: job.max_pixels, hevc };
        let args = build_args(&job.input, temp, source, &target);

        let mut process = Process::new(ffmpeg.as_os_str().to_owned());
        process.args(args);
        let mut child = match process.spawn_piped() {
            Ok(child) => child,
            Err(error) => return Outcome::Failed(format!("cannot start ffmpeg: {error}")),
        };
        let stdout = child.take_stdout();
        let stderr = child.take_stderr();
        let state = Arc::new(Mutex::new(ProgressState::default()));
        if let Some(stdout) = stdout {
            self.spawn_reader(stdout, job.item, duration, Arc::clone(&state));
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
            return Outcome::Cancelled;
        }

        let stderr_text = match stderr_task {
            Some(task) => task.await.unwrap_or_default(),
            None => String::new(),
        };

        let success = matches!(status, Some(Ok(exit)) if exit.success());
        if !success {
            return Outcome::Failed(failure_reason(&stderr_text));
        }
        return Outcome::Success;
    }

    async fn probe_source(&self, job: &Job) -> (Source, Option<Probe>, f64) {
        let input = job.input.clone();
        let fallback = job.fallback.clone();
        let duration = job.duration;
        let task = tokio::task::spawn_blocking(move || probe::inspect(&input.to_string_lossy(), ItemKind::Video));
        let probed = task.await.ok();
        let total = probed.as_ref().filter(|probe| probe.duration > 0.0).map_or(duration, |probe| probe.duration);
        let source = match &probed {
            Some(probe) if probe.decodable => Source {
                width: probe.width,
                height: probe.height,
                fps: probe.fps,
                bit_depth: probe.bit_depth,
                hdr: probe.hdr,
                transfer: probe.transfer.clone(),
                audio: audio_plan(probe.audio_codec.as_deref(), true),
            },
            _ => fallback,
        };
        return (source, probed, total);
    }

    fn fail(&self, item: ItemId, reason: String) {
        self.finish(item, JobState::Failed { reason: reason.clone() });
        post(Event::OptimizeDone { item, result: Err(reason) });
    }
}

async fn install(job: &Job, temp: &Path) -> Result<Optimized, String> {
    let probe_path = temp.to_path_buf();
    let probed = tokio::task::spawn_blocking(move || probe::inspect(&probe_path.to_string_lossy(), ItemKind::Video))
        .await
        .map_err(|_| "output probe failed".to_string())?;
    if !probed.decodable {
        return Err("optimized output is not playable".to_string());
    }
    let final_path = job.input.with_extension("mp4");
    std::fs::rename(temp, &final_path).map_err(|error| format!("cannot replace media: {error}"))?;
    if final_path != job.input {
        let _ = std::fs::remove_file(&job.input);
    }
    let _ = std::fs::remove_file(crate::media::thumb::file(&job.thumbs_dir, job.item));
    let path = final_path.to_string_lossy().into_owned();
    let _ =
        crate::media::thumb::ensure(job.item, ItemKind::Video, path.clone(), probed.duration, job.thumbs_dir.clone())
            .await;
    return Ok(Optimized { path, probe: probed });
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
    use uuid::Uuid;

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
        assert!(joined.windows(2).any(|pair| pair == ["-b:v", "7741440"]));
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
    fn bitrate_follows_bits_per_pixel_with_a_scaled_floor() {
        assert_eq!(bitrate(2560, 1440, 30.0, false, None), 7_741_440);
        assert_eq!(bitrate(2560, 1440, 30.0, true, None), 11_059_200);
        assert_eq!(bitrate(1920, 1080, 0.0, false, None), 4_354_560);
        assert_eq!(bitrate(640, 360, 30.0, false, None), 483_840);
        assert_eq!(bitrate(0, 0, 30.0, false, None), MIN_BITRATE_FLOOR);
        assert_eq!(bitrate(960, 540, 30.0, false, None), 1_088_640);
    }

    #[test]
    fn bitrate_never_exceeds_a_hardware_source_scaled_down() {
        let four_k = SourceRate { width: 3840, height: 2160, fps: 60.0, bit_rate: 8_550_000.0 };
        assert_eq!(bitrate(2560, 1440, 30.0, false, Some(four_k)), 2_767_303);
        let starved = SourceRate { width: 1920, height: 1080, fps: 30.0, bit_rate: 900_000.0 };
        assert_eq!(bitrate(1920, 1080, 30.0, false, Some(starved)), 900_000);
        let same = SourceRate { width: 1920, height: 1080, fps: 30.0, bit_rate: 3_000_000.0 };
        assert_eq!(bitrate(1920, 1080, 30.0, false, Some(same)), 3_000_000);
        let small = SourceRate { width: 1280, height: 720, fps: 30.0, bit_rate: 1_000_000.0 };
        assert_eq!(bitrate(1920, 1080, 30.0, false, Some(small)), 4_354_560);
    }

    #[test]
    fn source_rate_only_applies_to_hardware_codecs() {
        let mut probe = probe::Probe {
            kind: ItemKind::Video,
            container: Some("mp4".to_string()),
            width: 1920,
            height: 1080,
            fps: 30.0,
            codec: Some("hvc1".to_string()),
            bit_depth: Some(8),
            color_primaries: None,
            transfer: None,
            hdr: false,
            audio: false,
            audio_codec: None,
            duration: 2.0,
            frames: None,
            decodable: true,
            decode_error: None,
            av1_hw: false,
            bit_rate: Some(3_000_000.0),
        };
        assert_eq!(
            source_rate(Some(&probe)),
            Some(SourceRate { width: 1920, height: 1080, fps: 30.0, bit_rate: 3_000_000.0 })
        );
        probe.codec = Some("ap4h".to_string());
        assert_eq!(source_rate(Some(&probe)), None);
        probe.codec = Some("hvc1".to_string());
        probe.bit_rate = None;
        assert_eq!(source_rate(Some(&probe)), None);
    }

    #[test]
    fn plan_caps_fps_and_scales_without_upscaling() {
        let target = Target { fps_limit: 30, max_pixels: 2560, hevc: true };
        let probe = probe::Probe {
            kind: ItemKind::Video,
            container: Some("mp4".to_string()),
            width: 3840,
            height: 2160,
            fps: 60.0,
            codec: Some("hvc1".to_string()),
            bit_depth: Some(10),
            color_primaries: None,
            transfer: Some("SMPTE ST 2084".to_string()),
            hdr: true,
            audio: true,
            audio_codec: Some("aac".to_string()),
            duration: 5.0,
            frames: None,
            decodable: true,
            decode_error: None,
            av1_hw: false,
            bit_rate: None,
        };
        let plan = video_plan(Path::new("/in.mp4"), Path::new("/out.mp4"), Some(&probe), &source(), &target, 5.0);
        assert_eq!((plan.width, plan.height), (2560, 1440));
        assert!((plan.fps - 30.0).abs() < 1e-9);
        assert!(plan.ten_bit);
        assert!(plan.hdr);
        assert_eq!(plan.audio, Audio::Copy);
        assert_eq!(plan.bitrate, bitrate(2560, 1440, 30.0, true, None));
    }

    #[test]
    fn routing_keeps_ffmpeg_for_containers_and_unaccelerated_codecs() {
        assert_eq!(route(Some("mp4"), Some("hvc1"), false), Route::Apple);
        assert_eq!(route(Some("mov"), Some("ap4h"), false), Route::Apple);
        assert_eq!(route(Some("mp4"), Some("vp09"), false), Route::Apple);
        assert_eq!(route(Some("webm"), Some("vp09"), false), Route::Ffmpeg);
        assert_eq!(route(Some("mkv"), Some("h264"), false), Route::Ffmpeg);
        assert_eq!(route(Some("mp4"), Some("vp08"), false), Route::Ffmpeg);
        assert_eq!(route(Some("mp4"), Some("av01"), false), Route::Ffmpeg);
        assert_eq!(route(Some("mp4"), Some("av01"), true), Route::Apple);
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

    fn enqueue_fixture(queue: &Queue, fixture: &Path, dir: &Path) -> ItemId {
        let id = Uuid::new_v4();
        let input = dir.join(format!("{id}.mp4"));
        std::fs::copy(fixture, &input).expect("copy fixture");
        let fallback = Source {
            width: 1920,
            height: 1080,
            fps: 30.0,
            bit_depth: None,
            hdr: false,
            transfer: None,
            audio: Audio::Drop,
        };
        let job = Job {
            item: id,
            kind: ItemKind::Video,
            input,
            thumbs_dir: dir.to_path_buf(),
            ffmpeg: None,
            fps_limit: 30,
            max_pixels: 2560,
            duration: 3.0,
            fallback,
        };
        assert!(queue.enqueue(job));
        return id;
    }

    fn wait_state(runtime: &tokio::runtime::Runtime, queue: &Queue, id: ItemId) -> Option<JobState> {
        return runtime.block_on(async {
            for _ in 0..600 {
                match queue.status(id) {
                    Some(state @ (JobState::Done { .. } | JobState::Failed { .. } | JobState::Cancelled)) => {
                        return Some(state);
                    }
                    None => return None,
                    _ => {}
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            return None;
        });
    }

    #[test]
    fn queue_worker_survives_a_completed_job() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures/clip-hevc-1080p30.mp4");
        if !fixture.exists() {
            eprintln!("skipping queue test: fixture not found");
            return;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let queue = Queue::new(runtime.handle());
        let first = enqueue_fixture(&queue, &fixture, dir.path());
        let first_state = wait_state(&runtime, &queue, first);
        assert!(matches!(first_state, Some(JobState::Done { .. })), "first job: {first_state:?}");
        let second = enqueue_fixture(&queue, &fixture, dir.path());
        let second_state = wait_state(&runtime, &queue, second);
        assert!(matches!(second_state, Some(JobState::Done { .. })), "second job: {second_state:?}");
    }
}
