use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use serde::Serialize;
use tokio::runtime::Handle;
use tokio::sync::{Notify, OnceCell, mpsc};

use crate::app::{Event, post};
use crate::jobs::{Lifecycle, Registry};
use crate::media::plan::{self, GifPlan, Route, Source, Target};
use crate::media::probe::{self, Probe};
use crate::media::{TranscodeError, apple, ffmpeg, thumb};
use crate::model::{ItemId, ItemKind};

#[derive(Debug, Clone, PartialEq)]
pub enum JobState {
    Queued,
    Running { percent: f64, speed: Option<f64> },
    Done { path: String },
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
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobReport {
    pub id: String,
    pub state: Phase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl JobReport {
    fn new(id: ItemId, state: Phase) -> Self {
        return Self { id: id.to_string(), state, percent: None, speed: None, path: None, reason: None };
    }
}

impl JobState {
    pub fn report(&self, id: ItemId) -> JobReport {
        return match self {
            JobState::Queued => JobReport::new(id, Phase::Queued),
            JobState::Running { percent, speed } => {
                JobReport { percent: Some(*percent), speed: *speed, ..JobReport::new(id, Phase::Running) }
            }
            JobState::Done { path } => {
                JobReport { percent: Some(1.0), path: Some(path.clone()), ..JobReport::new(id, Phase::Done) }
            }
            JobState::Failed { reason } => {
                JobReport { reason: Some(reason.clone()), ..JobReport::new(id, Phase::Failed) }
            }
            JobState::Cancelled => JobReport::new(id, Phase::Cancelled),
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

struct Shared {
    jobs: Registry<ItemId, JobState>,
    hevc: OnceCell<bool>,
}

impl Shared {
    fn set_progress(&self, item: ItemId, percent: f64, speed: Option<f64>) {
        self.jobs.update(&item, |state| {
            if matches!(state, JobState::Running { .. }) {
                *state = JobState::Running { percent: percent.clamp(0.0, 1.0), speed };
            }
        });
    }
}

pub struct Queue {
    tx: mpsc::UnboundedSender<Job>,
    shared: Arc<Shared>,
}

impl Queue {
    pub fn new(handle: &Handle) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared { jobs: Registry::new(handle.clone()), hevc: OnceCell::new() });
        let mut worker = Worker { shared: Arc::clone(&shared) };
        handle.spawn(async move {
            worker.run(rx).await;
        });
        return Self { tx, shared };
    }

    pub fn enqueue(&self, job: Job) -> bool {
        if !self.shared.jobs.insert(job.item) {
            return false;
        }
        return self.tx.send(job).is_ok();
    }

    pub fn status(&self, item: ItemId) -> Option<JobState> {
        return self.shared.jobs.status(&item);
    }

    pub fn cancel(&self, item: ItemId) {
        self.shared.jobs.cancel(&item);
    }
}

struct Worker {
    shared: Arc<Shared>,
}

impl Worker {
    async fn run(&mut self, mut rx: mpsc::UnboundedReceiver<Job>) {
        while let Some(job) = rx.recv().await {
            let item = job.item;
            let worker = Worker { shared: Arc::clone(&self.shared) };
            if let Err(error) = tokio::spawn(async move { worker.run_job(job).await }).await {
                tracing::warn!(%item, %error, "optimize job failed unexpectedly");
                self.fail(item, format!("optimize failed: {error}"));
            }
        }
    }

    fn set_running(&self, item: ItemId, notify: Arc<Notify>) {
        self.shared.jobs.set_state(&item, JobState::Running { percent: 0.0, speed: None });
        self.shared.jobs.register_cancel(&item, notify);
    }

    fn apple_progress(&self, item: ItemId, duration: f64) -> apple::Progress {
        let shared = Arc::clone(&self.shared);
        let start = Instant::now();
        return Arc::new(move |fraction: f64| {
            let elapsed = start.elapsed().as_secs_f64();
            let speed = if elapsed > 0.0 && duration > 0.0 { Some(fraction * duration / elapsed) } else { None };
            shared.set_progress(item, fraction, speed);
        });
    }

    fn ffmpeg_progress(&self, item: ItemId) -> ffmpeg::Report {
        let shared = Arc::clone(&self.shared);
        return Arc::new(move |percent, speed| shared.set_progress(item, percent, speed));
    }

    async fn run_job(&self, job: Job) {
        let item = job.item;
        if self.shared.jobs.take_cancelled(&item) {
            return;
        }
        let notify = Arc::new(Notify::new());
        self.set_running(item, Arc::clone(&notify));

        let temp = temp_path(&job.input, item);
        let _ = std::fs::remove_file(&temp);

        let result = match self.transcode(&job, &temp, &notify).await {
            Err(TranscodeError::Cancelled) => {
                let _ = std::fs::remove_file(&temp);
                self.shared.jobs.finish(&item, JobState::Cancelled);
                return;
            }
            Err(error) => Err(error.to_string()),
            Ok(()) => install(&job, &temp).await,
        };
        match result {
            Ok(optimized) => {
                self.shared.jobs.finish(&item, JobState::Done { path: optimized.path.clone() });
                post(Event::OptimizeDone { item, result: Ok(optimized) });
            }
            Err(reason) => {
                let _ = std::fs::remove_file(&temp);
                self.fail(item, reason);
            }
        }
    }

    async fn transcode(&self, job: &Job, temp: &Path, notify: &Arc<Notify>) -> Result<(), TranscodeError> {
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
        return match plan::route(container.as_deref(), codec.as_deref(), av1_hw) {
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
    ) -> Result<(), TranscodeError> {
        let target = Target { fps_limit: job.fps_limit, max_pixels: job.max_pixels, hevc: true };
        let video = plan::video_plan(&job.input, temp, probed, source, &target, duration);
        if video.width < 2 || video.height < 2 {
            return Err(TranscodeError::Failed("cannot read the video size".to_string()));
        }
        return self.run_apple(job.item, apple::Task::Video(video), duration, notify).await;
    }

    async fn run_apple_gif(&self, job: &Job, temp: &Path, notify: &Arc<Notify>) -> Result<(), TranscodeError> {
        let gif = GifPlan { input: job.input.clone(), output: temp.to_path_buf(), max_pixels: job.max_pixels };
        return self.run_apple(job.item, apple::Task::Gif(gif), job.duration, notify).await;
    }

    async fn run_apple(
        &self,
        item: ItemId,
        task: apple::Task,
        duration: f64,
        notify: &Arc<Notify>,
    ) -> Result<(), TranscodeError> {
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = self.apple_progress(item, duration);
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
            return Err(TranscodeError::Cancelled);
        }
        return joined.unwrap_or_else(|error| Err(TranscodeError::Failed(format!("optimize task failed: {error}"))));
    }

    async fn run_ffmpeg(
        &self,
        job: &Job,
        temp: &Path,
        source: &Source,
        duration: f64,
        notify: &Notify,
    ) -> Result<(), TranscodeError> {
        let Some(binary) = &job.ffmpeg else {
            return Err(TranscodeError::Failed("ffmpeg is not installed".to_string()));
        };
        let hevc = *self.shared.hevc.get_or_init(|| ffmpeg::supports_hevc(binary)).await;
        let target = Target { fps_limit: job.fps_limit, max_pixels: job.max_pixels, hevc };
        let args = plan::build_args(&job.input, temp, source, &target);
        return ffmpeg::run(binary, args, duration, self.ffmpeg_progress(job.item), notify).await;
    }

    async fn probe_source(&self, job: &Job) -> (Source, Option<Probe>, f64) {
        let input = job.input.clone();
        let fallback = job.fallback.clone();
        let duration = job.duration;
        let task = tokio::task::spawn_blocking(move || probe::inspect(&input, ItemKind::Video));
        let probed = task.await.ok();
        let total = probed.as_ref().filter(|probe| probe.duration > 0.0).map_or(duration, |probe| probe.duration);
        let source = match &probed {
            Some(probe) if probe.decodable => Source::from(probe),
            _ => fallback,
        };
        return (source, probed, total);
    }

    fn fail(&self, item: ItemId, reason: String) {
        self.shared.jobs.finish(&item, JobState::Failed { reason: reason.clone() });
        post(Event::OptimizeDone { item, result: Err(reason) });
    }
}

async fn install(job: &Job, temp: &Path) -> Result<Optimized, String> {
    let probe_path = temp.to_path_buf();
    let probed = tokio::task::spawn_blocking(move || probe::inspect(&probe_path, ItemKind::Video))
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
    let _ = std::fs::remove_file(thumb::file(&job.thumbs_dir, job.item));
    let _ = thumb::ensure(job.item, ItemKind::Video, final_path.clone(), probed.duration, job.thumbs_dir.clone()).await;
    return Ok(Optimized { path: final_path.to_string_lossy().into_owned(), probe: probed });
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
    use crate::media::plan::Audio;
    use std::time::Duration;
    use uuid::Uuid;

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
