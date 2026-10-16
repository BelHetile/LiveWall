pub mod apple;
pub mod av;
pub mod ffmpeg;
pub mod optimize;
pub mod plan;
pub mod probe;
pub mod thumb;

use std::sync::OnceLock;

use tokio::sync::Semaphore;

const MEDIA_CONCURRENCY: usize = 3;

static LIMITER: OnceLock<Semaphore> = OnceLock::new();

#[derive(Debug, thiserror::Error)]
pub enum TranscodeError {
    #[error("optimize cancelled")]
    Cancelled,
    #[error("the file has no video track")]
    NoVideo,
    #[error("{0}")]
    Failed(String),
}

fn limiter() -> &'static Semaphore {
    return LIMITER.get_or_init(|| Semaphore::new(MEDIA_CONCURRENCY));
}
