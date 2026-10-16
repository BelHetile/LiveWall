use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;

use objc2_av_foundation::{AVAssetImageGenerator, AVURLAsset};
use objc2_core_foundation::{CFDictionary, CFMutableData, CFRetained, CFString, CGSize};
use objc2_core_graphics::CGImage;
use objc2_core_media::{CMTime, kCMTimePositiveInfinity};
use objc2_foundation::NSError;
use objc2_image_io::{CGImageDestination, CGImageSource};
use tempfile::NamedTempFile;
use uuid::Uuid;

use crate::cf::{cf_file_url, file_url};
use crate::media::av::{CM_TIMESCALE, thumbnail_options};
use crate::media::limiter;
use crate::model::ItemKind;

const MAX_PX: u16 = 480;
const VIDEO_FRACTION: f64 = 0.1;
const JPEG_TYPE: &str = "public.jpeg";

pub fn file(thumbs_dir: &Path, id: Uuid) -> PathBuf {
    return thumbs_dir.join(format!("{id}.jpg"));
}

fn generate_sync(id: Uuid, kind: ItemKind, path: &Path, duration: f64, thumbs_dir: &Path) -> Option<PathBuf> {
    let dest = file(thumbs_dir, id);
    if dest.exists() {
        return Some(dest);
    }
    return generate(kind, path, duration, &dest).ok().map(|()| dest);
}

pub async fn ensure(id: Uuid, kind: ItemKind, path: PathBuf, duration: f64, thumbs_dir: PathBuf) -> Option<PathBuf> {
    if file(&thumbs_dir, id).exists() {
        return Some(file(&thumbs_dir, id));
    }
    let permit = limiter().acquire().await.ok()?;
    let task = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        return generate_sync(id, kind, &path, duration, &thumbs_dir);
    });
    return task.await.ok().flatten();
}

fn generate(kind: ItemKind, path: &Path, duration: f64, dest: &Path) -> io::Result<()> {
    let image = match kind {
        ItemKind::Video => video_frame(path, duration)?,
        ItemKind::Gif | ItemKind::Image => image_frame(path)?,
    };
    return write_jpeg(&image, dest);
}

fn video_frame(path: &Path, duration: f64) -> io::Result<CFRetained<CGImage>> {
    let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&file_url(path), None) };
    let generator = unsafe { AVAssetImageGenerator::assetImageGeneratorWithAsset(&asset) };
    let maximum = CGSize::new(f64::from(MAX_PX), f64::from(MAX_PX));
    unsafe {
        generator.setAppliesPreferredTrackTransform(true);
        generator.setMaximumSize(maximum);
        generator.setRequestedTimeToleranceBefore(kCMTimePositiveInfinity);
        generator.setRequestedTimeToleranceAfter(kCMTimePositiveInfinity);
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    let block = block2::RcBlock::new(move |image: *mut CGImage, _time: CMTime, error: *mut NSError| {
        let result = if error.is_null() {
            NonNull::new(image)
                .map(|image| unsafe { CFRetained::retain(image) })
                .ok_or_else(|| "no image produced".to_string())
        } else {
            Err(unsafe { error.as_ref() }
                .map_or_else(|| "unknown error".to_string(), |error| error.localizedDescription().to_string()))
        };
        let _ = sender.send(result);
    });
    let requested = unsafe { CMTime::with_seconds((duration * VIDEO_FRACTION).max(0.0), CM_TIMESCALE) };
    unsafe { generator.generateCGImageAsynchronouslyForTime_completionHandler(requested, &block) };
    let result = receiver.recv().map_err(|_| io::Error::other("video thumbnail task dropped"))?;
    return result.map_err(|message| io::Error::other(format!("cannot generate video thumbnail: {message}")));
}

fn image_frame(path: &Path) -> io::Result<CFRetained<CGImage>> {
    let url = cf_file_url(path).ok_or_else(|| io::Error::other("invalid thumbnail path"))?;
    let source = unsafe { CGImageSource::with_url(&url, None) }.ok_or_else(|| io::Error::other("cannot open image"))?;
    let options = thumbnail_options(usize::from(MAX_PX));
    let options: &CFDictionary = unsafe { (*options).cast_unchecked() };
    return unsafe { source.thumbnail_at_index(0, Some(options)) }
        .ok_or_else(|| io::Error::other("cannot render thumbnail"));
}

fn write_jpeg(image: &CFRetained<CGImage>, dest: &Path) -> io::Result<()> {
    let data = CFMutableData::new(None, 0).ok_or_else(|| io::Error::other("cannot allocate thumbnail data"))?;
    let kind = CFString::from_str(JPEG_TYPE);
    let destination = unsafe { CGImageDestination::with_data(&data, &kind, 1, None) }
        .ok_or_else(|| io::Error::other("cannot create image destination"))?;
    unsafe { destination.add_image(image, None) };
    if !unsafe { destination.finalize() } {
        return Err(io::Error::other("cannot finalize thumbnail"));
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
    fn file_is_named_by_item_id() {
        let id = Uuid::nil();
        let path = file(Path::new("/Thumbs"), id);
        assert_eq!(path, Path::new("/Thumbs/00000000-0000-0000-0000-000000000000.jpg"));
    }

    #[test]
    fn concurrency_matches_the_spec() {
        assert_eq!(crate::media::MEDIA_CONCURRENCY, 3);
        assert_eq!(MAX_PX, 480);
    }
}
