use std::ptr::NonNull;
use std::sync::mpsc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_av_foundation::{AVAssetTrack, AVMediaType, AVURLAsset};
use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_core_media::CMFormatDescription;
use objc2_foundation::{NSArray, NSError};
use objc2_image_io::{
    CGImageSource, kCGImagePropertyPixelHeight, kCGImagePropertyPixelWidth,
    kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailWithTransform,
    kCGImageSourceThumbnailMaxPixelSize,
};

use crate::cf::cf_i64;

pub const CM_TIMESCALE: i32 = 600;

pub fn load_track(asset: &AVURLAsset, media: &AVMediaType) -> Result<Option<Retained<AVAssetTrack>>, String> {
    let (sender, receiver) = mpsc::channel();
    let block = RcBlock::new(move |tracks: *mut NSArray<AVAssetTrack>, error: *mut NSError| {
        let result = if error.is_null() {
            Ok(NonNull::new(tracks)
                .map(|tracks| unsafe { tracks.as_ref() })
                .and_then(|tracks| if tracks.count() == 0 { None } else { Some(tracks.objectAtIndex(0)) }))
        } else {
            Err(unsafe { error.as_ref() }
                .map_or_else(|| "cannot load tracks".to_string(), |error| error.localizedDescription().to_string()))
        };
        let _ = sender.send(result);
    });
    unsafe { asset.loadTracksWithMediaType_completionHandler(media, &block) };
    return receiver.recv().unwrap_or_else(|_| Err("track load was dropped".to_string()));
}

pub fn format_description(track: &AVAssetTrack) -> Option<CFRetained<CMFormatDescription>> {
    let descriptions = unsafe { track.formatDescriptions() };
    if descriptions.count() == 0 {
        return None;
    }
    let object = descriptions.objectAtIndex(0);
    let pointer = NonNull::from(&*object).cast::<CMFormatDescription>();
    return Some(unsafe { CFRetained::retain(pointer) });
}

pub fn image_size(source: &CGImageSource) -> (u32, u32) {
    let Some(properties) = (unsafe { source.properties_at_index(0, None) }) else {
        return (0, 0);
    };
    let properties: &CFDictionary<CFString, CFType> = unsafe { properties.cast_unchecked() };
    let width = cf_i64(properties.get(unsafe { kCGImagePropertyPixelWidth }).as_deref())
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    let height = cf_i64(properties.get(unsafe { kCGImagePropertyPixelHeight }).as_deref())
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0);
    return (width, height);
}

pub fn thumbnail_options(max_px: usize) -> CFRetained<CFDictionary<CFString, CFType>> {
    let keys: [&CFString; 3] = [
        unsafe { kCGImageSourceCreateThumbnailFromImageAlways },
        unsafe { kCGImageSourceCreateThumbnailWithTransform },
        unsafe { kCGImageSourceThumbnailMaxPixelSize },
    ];
    let truthy: &CFType = CFBoolean::new(true).as_ref();
    let max = CFNumber::new_i64(i64::try_from(max_px).unwrap_or(i64::MAX));
    let max_ref: &CFType = max.as_ref();
    let values: [&CFType; 3] = [truthy, truthy, max_ref];
    return CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_options_carry_three_keys() {
        let options = thumbnail_options(480);
        let options: &CFDictionary<CFString, CFType> = &options;
        assert_eq!(options.len(), 3);
    }
}
