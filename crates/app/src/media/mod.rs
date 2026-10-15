pub mod optimize;
pub mod probe;
pub mod thumb;

use std::sync::OnceLock;

use tokio::sync::Semaphore;

pub const MEDIA_CONCURRENCY: usize = 3;

static LIMITER: OnceLock<Semaphore> = OnceLock::new();

pub fn limiter() -> &'static Semaphore {
    return LIMITER.get_or_init(|| Semaphore::new(MEDIA_CONCURRENCY));
}
