use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::time::UNIX_EPOCH;

use objc2_av_foundation::{AVAssetImageGenerator, AVURLAsset};
use objc2_core_foundation::{CFMutableData, CFRetained, CFString, CFURL};
use objc2_core_graphics::CGImage;
use objc2_core_media::{CMTime, kCMTimeZero};
use objc2_foundation::{NSError, NSString, NSURL};
use objc2_image_io::{CGImageDestination, CGImageSource};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::app::{Event, post};
use crate::model::{ItemId, ItemKind};

const PNG_TYPE: &str = "public.png";
const STILL_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "heic", "tif", "tiff"];

pub fn accepts(path: &str) -> bool {
    let Some(extension) = Path::new(path).extension().and_then(|value| value.to_str()) else {
        return false;
    };
    let extension = extension.to_ascii_lowercase();
    return STILL_EXTENSIONS.contains(&extension.as_str());
}

pub fn cache_key(path: &str, mtime: i64, still_time: f32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    hasher.update([0]);
    hasher.update(mtime.to_le_bytes());
    hasher.update([0]);
    hasher.update(still_time.to_bits().to_le_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    return out;
}

pub fn cache_file(stills_dir: &Path, key: &str) -> PathBuf {
    return stills_dir.join(format!("{key}.png"));
}

pub fn mtime_secs(path: &str) -> i64 {
    return fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| i64::try_from(duration.as_secs()).unwrap_or(0));
}

pub fn generate(item: ItemId, kind: ItemKind, path: &str, still_time: f32, dest: PathBuf) {
    match kind {
        ItemKind::Video => generate_video(item, path, still_time, dest),
        ItemKind::Gif | ItemKind::Image => spawn_frame(item, path, dest),
    }
}

fn generate_video(item: ItemId, path: &str, still_time: f32, dest: PathBuf) {
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) };
    let generator = unsafe { AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset) };
    unsafe {
        generator.setAppliesPreferredTrackTransform(true);
        generator.setRequestedTimeToleranceBefore(kCMTimeZero);
        generator.setRequestedTimeToleranceAfter(kCMTimeZero);
    }
    let block = block2::RcBlock::new(move |image: *mut CGImage, _time: CMTime, error: *mut NSError| {
        if !error.is_null() {
            tracing::warn!(item = %item, "still generation failed");
            return;
        }
        let Some(image) = NonNull::new(image) else { return };
        let image = unsafe { CFRetained::retain(image) };
        finish(item, &image, &dest);
    });
    let requested = unsafe { CMTime::with_seconds(f64::from(still_time.max(0.0)), 600) };
    unsafe { generator.generateCGImageAsynchronouslyForTime_completionHandler(requested, &block) };
}

fn spawn_frame(item: ItemId, path: &str, dest: PathBuf) {
    let path = path.to_string();
    tokio::task::spawn_blocking(move || {
        let Some(url) = CFURL::from_file_path(&path) else { return };
        let Some(source) = (unsafe { CGImageSource::with_url(&url, None) }) else { return };
        let Some(image) = (unsafe { source.image_at_index(0, None) }) else { return };
        finish(item, &image, &dest);
    });
}

fn finish(item: ItemId, image: &CFRetained<CGImage>, dest: &Path) {
    match write_png(image, dest) {
        Ok(()) => post(Event::StillReady { item, path: dest.to_path_buf() }),
        Err(error) => tracing::warn!(item = %item, %error, "cannot write still"),
    }
}

fn write_png(image: &CFRetained<CGImage>, dest: &Path) -> io::Result<()> {
    let data = CFMutableData::new(None, 0).ok_or_else(|| io::Error::other("cannot allocate still data"))?;
    let kind = CFString::from_str(PNG_TYPE);
    let destination = unsafe { CGImageDestination::with_data(&data, &kind, 1, None) }
        .ok_or_else(|| io::Error::other("cannot create image destination"))?;
    unsafe { destination.add_image(image, None) };
    if !unsafe { destination.finalize() } {
        return Err(io::Error::other("cannot finalize still"));
    }
    let bytes = unsafe { std::slice::from_raw_parts(data.byte_ptr(), usize::try_from(data.length()).unwrap_or(0)) };
    let dir = dest.parent().unwrap_or_else(|| Path::new("."));
    let mut file = NamedTempFile::new_in(dir)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(dest).map_err(|error| error.error)?;
    return Ok(());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_stable_for_the_same_inputs() {
        assert_eq!(cache_key("/a.mp4", 10, 0.0), cache_key("/a.mp4", 10, 0.0));
        assert_eq!(cache_key("/a.mp4", 10, 0.0).len(), 64);
    }

    #[test]
    fn key_changes_with_every_component() {
        let base = cache_key("/a.mp4", 10, 0.0);
        assert_ne!(base, cache_key("/b.mp4", 10, 0.0));
        assert_ne!(base, cache_key("/a.mp4", 11, 0.0));
        assert_ne!(base, cache_key("/a.mp4", 10, 1.0));
    }

    #[test]
    fn key_distinguishes_close_still_times() {
        assert_ne!(cache_key("/a.mp4", 10, 1.0), cache_key("/a.mp4", 10, 1.000_000_1));
    }

    #[test]
    fn accepts_desktop_image_formats() {
        assert!(accepts("/a.png"));
        assert!(accepts("/a.JPG"));
        assert!(accepts("/a.jpeg"));
        assert!(accepts("/a.heic"));
        assert!(accepts("/a.tiff"));
        assert!(!accepts("/a.gif"));
        assert!(!accepts("/a.webp"));
        assert!(!accepts("/a.mp4"));
        assert!(!accepts("/no-extension"));
    }
}
