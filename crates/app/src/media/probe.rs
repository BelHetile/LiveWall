use std::ptr::NonNull;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_av_foundation::{
    AVAsset, AVAssetReader, AVAssetReaderStatus, AVAssetReaderTrackOutput, AVAssetTrack, AVMediaTypeAudio,
    AVMediaTypeVideo, AVURLAsset,
};
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CFType, CFURL};
use objc2_core_media::{
    CMFormatDescription, kCMFormatDescriptionExtension_BitsPerComponent, kCMFormatDescriptionExtension_ColorPrimaries,
    kCMFormatDescriptionExtension_Depth, kCMFormatDescriptionExtension_TransferFunction,
    kCMFormatDescriptionTransferFunction_ITU_R_2100_HLG, kCMFormatDescriptionTransferFunction_SMPTE_ST_2084_PQ,
    kCMVideoCodecType_AV1,
};
use objc2_foundation::{NSArray, NSError, NSString, NSURL};
use objc2_image_io::{CGImageSource, kCGImagePropertyPixelHeight, kCGImagePropertyPixelWidth};
use objc2_video_toolbox::VTIsHardwareDecodeSupported;
use serde::{Deserialize, Serialize};

use crate::model::ItemKind;

pub const SAMPLE_LIMIT: usize = 4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct Probe {
    pub kind: ItemKind,
    pub container: Option<String>,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub codec: Option<String>,
    pub bit_depth: Option<u32>,
    pub color_primaries: Option<String>,
    pub transfer: Option<String>,
    pub hdr: bool,
    pub audio: bool,
    pub audio_codec: Option<String>,
    pub duration: f64,
    pub frames: Option<usize>,
    pub decodable: bool,
    pub decode_error: Option<String>,
    pub av1_hw: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    Av1WithoutHardware,
    NonHardwareCodec,
    FpsAboveLimit,
    ResolutionAboveDisplay,
    ContainerUnsupported,
}

impl Reason {
    pub fn label(self) -> &'static str {
        return match self {
            Reason::Av1WithoutHardware => "av1-without-hardware",
            Reason::NonHardwareCodec => "non-hardware-codec",
            Reason::FpsAboveLimit => "fps-above-limit",
            Reason::ResolutionAboveDisplay => "resolution-above-display",
            Reason::ContainerUnsupported => "container-unsupported",
        };
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "kebab-case")]
pub enum Verdict {
    Playable,
    NeedsOptimize { reasons: Vec<Reason> },
    Unsupported { reason: String },
}

#[derive(Debug, Clone, Copy)]
pub struct Policy {
    pub fps_limit: u32,
    pub max_pixels: u32,
}

pub fn av1_hardware() -> bool {
    return unsafe { VTIsHardwareDecodeSupported(kCMVideoCodecType_AV1) };
}

pub const SUPPORTED_CONTAINERS: [&str; 5] = ["mp4", "mov", "m4v", "webm", "mkv"];

pub fn supported_container(container: Option<&str>) -> bool {
    return container.is_some_and(|container| SUPPORTED_CONTAINERS.contains(&container));
}

pub fn container_of(path: &str) -> Option<String> {
    return std::path::Path::new(path).extension().and_then(|value| value.to_str()).map(str::to_ascii_lowercase);
}

pub fn inspect(path: &str, kind: ItemKind) -> Probe {
    return match kind {
        ItemKind::Video => video_probe(path),
        ItemKind::Gif | ItemKind::Image => image_probe(path, kind),
    };
}

pub fn verdict(probe: &Probe, policy: Policy) -> Verdict {
    if !probe.decodable {
        if probe.kind == ItemKind::Video && supported_container(probe.container.as_deref()) {
            return Verdict::NeedsOptimize { reasons: vec![Reason::ContainerUnsupported] };
        }
        let reason = probe.decode_error.clone().unwrap_or_else(|| "cannot decode media".to_string());
        return Verdict::Unsupported { reason };
    }
    if probe.kind != ItemKind::Video {
        return Verdict::Playable;
    }
    let mut reasons = Vec::new();
    if let Some(codec) = &probe.codec {
        if codec == "av01" && !probe.av1_hw {
            reasons.push(Reason::Av1WithoutHardware);
        }
        if matches!(codec.as_str(), "vp09" | "vp08") {
            reasons.push(Reason::NonHardwareCodec);
        }
    }
    if probe.fps > f64::from(policy.fps_limit) {
        reasons.push(Reason::FpsAboveLimit);
    }
    if policy.max_pixels > 0 && (probe.width > policy.max_pixels || probe.height > policy.max_pixels) {
        reasons.push(Reason::ResolutionAboveDisplay);
    }
    if reasons.is_empty() {
        return Verdict::Playable;
    }
    reasons.sort_unstable();
    reasons.dedup();
    return Verdict::NeedsOptimize { reasons };
}

struct VideoFacts {
    codec: Option<String>,
    width: u32,
    height: u32,
    fps: f64,
    bit_depth: Option<u32>,
    color_primaries: Option<String>,
    transfer: Option<String>,
    duration: f64,
    decodable: bool,
    decode_error: Option<String>,
}

fn video_probe(path: &str) -> Probe {
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) };
    let facts = load_video(&asset);
    let audio_codec = load_audio(&asset);
    let (facts, decode_error) = match facts {
        Ok(facts) => (Some(facts), None),
        Err(reason) => (None, Some(reason)),
    };
    let decodable = facts.as_ref().is_some_and(|facts| facts.decodable);
    return Probe {
        kind: ItemKind::Video,
        container: container_of(path),
        width: facts.as_ref().map_or(0, |facts| facts.width),
        height: facts.as_ref().map_or(0, |facts| facts.height),
        fps: facts.as_ref().map_or(0.0, |facts| facts.fps),
        codec: facts.as_ref().and_then(|facts| facts.codec.clone()),
        bit_depth: facts.as_ref().and_then(|facts| facts.bit_depth),
        color_primaries: facts.as_ref().and_then(|facts| facts.color_primaries.clone()),
        transfer: facts.as_ref().and_then(|facts| facts.transfer.clone()),
        hdr: facts.as_ref().is_some_and(|facts| is_hdr(facts.transfer.as_deref())),
        audio: audio_codec.is_some(),
        audio_codec,
        duration: facts.as_ref().map_or(0.0, |facts| facts.duration),
        frames: None,
        decodable,
        decode_error: facts.as_ref().and_then(|facts| facts.decode_error.clone()).or(decode_error),
        av1_hw: av1_hardware(),
    };
}

fn load_video(asset: &Retained<AVURLAsset>) -> Result<VideoFacts, String> {
    let Some(media_type) = (unsafe { AVMediaTypeVideo }) else {
        return Err("no video media type".to_string());
    };
    let owned = Retained::clone(asset);
    let (sender, receiver) = std::sync::mpsc::channel();
    let block = block2::RcBlock::new(move |tracks: *mut NSArray<AVAssetTrack>, error: *mut NSError| {
        let result = if error.is_null() {
            NonNull::new(tracks)
                .map(|tracks| unsafe { tracks.as_ref() })
                .map_or_else(|| Err("no video track".to_string()), |tracks| read_video_facts(&owned, tracks))
        } else {
            Err(unsafe { error.as_ref() }.map_or_else(
                || "cannot load video tracks".to_string(),
                |error| error.localizedDescription().to_string(),
            ))
        };
        let _ = sender.send(result);
    });
    unsafe { asset.loadTracksWithMediaType_completionHandler(media_type, &block) };
    return receiver.recv().unwrap_or_else(|_| Err("video track load was dropped".to_string()));
}

fn read_video_facts(asset: &AVAsset, tracks: &NSArray<AVAssetTrack>) -> Result<VideoFacts, String> {
    if tracks.count() == 0 {
        return Err("no video track".to_string());
    }
    let track = tracks.objectAtIndex(0);
    let size = unsafe { track.naturalSize() };
    let transform = unsafe { track.preferredTransform() };
    let width = size.width.abs() * transform.a.abs() + size.height.abs() * transform.c.abs();
    let height = size.height.abs() * transform.d.abs() + size.width.abs() * transform.b.abs();
    let fps = f64::from(unsafe { track.nominalFrameRate() });
    let duration = unsafe { track.timeRange().duration.seconds() };
    let descriptions = unsafe { track.formatDescriptions() };
    if descriptions.count() == 0 {
        return Err("no format description".to_string());
    }
    let object = descriptions.objectAtIndex(0);
    let pointer = NonNull::from(&*object).cast::<CMFormatDescription>();
    let description = unsafe { CFRetained::retain(pointer) };
    let codec = fourcc(unsafe { description.media_sub_type() });
    let (bit_depth, color_primaries, transfer) = color_info(&description);
    let (decodable, decode_error) = decode_check(asset, &track);
    return Ok(VideoFacts {
        codec: Some(codec),
        width: dim(width),
        height: dim(height),
        fps,
        bit_depth,
        color_primaries,
        transfer,
        duration: if duration.is_finite() { duration.max(0.0) } else { 0.0 },
        decodable,
        decode_error,
    });
}

fn load_audio(asset: &AVURLAsset) -> Option<String> {
    let media_type = (unsafe { AVMediaTypeAudio })?;
    let (sender, receiver) = std::sync::mpsc::channel();
    let block = block2::RcBlock::new(move |tracks: *mut NSArray<AVAssetTrack>, error: *mut NSError| {
        let codec = if error.is_null() {
            NonNull::new(tracks).map(|tracks| unsafe { tracks.as_ref() }).and_then(read_audio_codec)
        } else {
            None
        };
        let _ = sender.send(codec);
    });
    unsafe { asset.loadTracksWithMediaType_completionHandler(media_type, &block) };
    return receiver.recv().unwrap_or(None);
}

fn read_audio_codec(tracks: &NSArray<AVAssetTrack>) -> Option<String> {
    if tracks.count() == 0 {
        return None;
    }
    let track = tracks.objectAtIndex(0);
    let descriptions = unsafe { track.formatDescriptions() };
    if descriptions.count() == 0 {
        return None;
    }
    let object = descriptions.objectAtIndex(0);
    let pointer = NonNull::from(&*object).cast::<CMFormatDescription>();
    let description = unsafe { CFRetained::retain(pointer) };
    return Some(fourcc(unsafe { description.media_sub_type() }).trim().to_ascii_lowercase());
}

fn decode_check(asset: &AVAsset, track: &AVAssetTrack) -> (bool, Option<String>) {
    let reader = match unsafe { AVAssetReader::initWithAsset_error(AVAssetReader::alloc(), asset) } {
        Ok(reader) => reader,
        Err(error) => return (false, Some(error.localizedDescription().to_string())),
    };
    let output = unsafe {
        AVAssetReaderTrackOutput::initWithTrack_outputSettings(AVAssetReaderTrackOutput::alloc(), track, None)
    };
    if !unsafe { reader.canAddOutput(&output) } {
        return (false, Some("cannot add video output".to_string()));
    }
    unsafe { reader.addOutput(&output) };
    if !unsafe { reader.startReading() } {
        return (false, reader_error(&reader));
    }
    let mut samples = 0usize;
    while samples < SAMPLE_LIMIT {
        if unsafe { output.copyNextSampleBuffer() }.is_none() {
            break;
        }
        samples += 1;
    }
    if samples == 0 || unsafe { reader.status() } == AVAssetReaderStatus::Failed {
        return (false, reader_error(&reader));
    }
    return (true, None);
}

fn reader_error(reader: &AVAssetReader) -> Option<String> {
    return unsafe { reader.error() }.map(|error| error.localizedDescription().to_string());
}

fn image_probe(path: &str, kind: ItemKind) -> Probe {
    let mut probe = Probe {
        kind,
        container: container_of(path),
        width: 0,
        height: 0,
        fps: 0.0,
        codec: None,
        bit_depth: None,
        color_primaries: None,
        transfer: None,
        hdr: false,
        audio: false,
        audio_codec: None,
        duration: 0.0,
        frames: None,
        decodable: false,
        decode_error: None,
        av1_hw: false,
    };
    let Some(url) = CFURL::from_file_path(path) else {
        probe.decode_error = Some("invalid path".to_string());
        return probe;
    };
    let Some(source) = (unsafe { CGImageSource::with_url(&url, None) }) else {
        probe.decode_error = Some("cannot open image".to_string());
        return probe;
    };
    let count = unsafe { source.count() };
    if count == 0 {
        probe.decode_error = Some("image has no frames".to_string());
        return probe;
    }
    if let Some(properties) = unsafe { source.properties_at_index(0, None) } {
        let properties: &CFDictionary<CFString, CFType> = unsafe { properties.cast_unchecked() };
        probe.width = cf_i64(properties.get(unsafe { kCGImagePropertyPixelWidth }).as_deref())
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0);
        probe.height = cf_i64(properties.get(unsafe { kCGImagePropertyPixelHeight }).as_deref())
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0);
    }
    probe.frames = Some(count);
    probe.decodable = true;
    return probe;
}

fn color_info(description: &CMFormatDescription) -> (Option<u32>, Option<String>, Option<String>) {
    let Some(extensions) = (unsafe { description.extensions() }) else {
        return (None, None, None);
    };
    let extensions: &CFDictionary<CFString, CFType> = unsafe { extensions.cast_unchecked::<CFString, CFType>() };
    let bit_depth = cf_i64(extensions.get(unsafe { kCMFormatDescriptionExtension_BitsPerComponent }).as_deref())
        .or_else(|| cf_i64(extensions.get(unsafe { kCMFormatDescriptionExtension_Depth }).as_deref()))
        .and_then(|value| u32::try_from(value).ok());
    let color_primaries = cf_string(extensions.get(unsafe { kCMFormatDescriptionExtension_ColorPrimaries }).as_deref());
    let transfer = cf_string(extensions.get(unsafe { kCMFormatDescriptionExtension_TransferFunction }).as_deref());
    return (bit_depth, color_primaries, transfer);
}

fn is_hdr(transfer: Option<&str>) -> bool {
    let Some(transfer) = transfer else {
        return false;
    };
    return transfer == unsafe { kCMFormatDescriptionTransferFunction_SMPTE_ST_2084_PQ }.to_string()
        || transfer == unsafe { kCMFormatDescriptionTransferFunction_ITU_R_2100_HLG }.to_string();
}

fn cf_string(value: Option<&CFType>) -> Option<String> {
    return value.and_then(|value| value.downcast_ref::<CFString>()).map(ToString::to_string);
}

fn cf_i64(value: Option<&CFType>) -> Option<i64> {
    return value.and_then(|value| value.downcast_ref::<CFNumber>()).and_then(CFNumber::as_i64);
}

fn fourcc(code: u32) -> String {
    return code
        .to_be_bytes()
        .iter()
        .map(|byte| if byte.is_ascii_graphic() { char::from(*byte) } else { '?' })
        .collect();
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn dim(value: f64) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    return value.round().min(f64::from(u32::MAX)) as u32;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(codec: &str, width: u32, height: u32, fps: f64, av1_hw: bool) -> Probe {
        return Probe {
            kind: ItemKind::Video,
            container: Some("mp4".to_string()),
            width,
            height,
            fps,
            codec: Some(codec.to_string()),
            bit_depth: Some(8),
            color_primaries: None,
            transfer: None,
            hdr: false,
            audio: true,
            audio_codec: Some("aac".to_string()),
            duration: 3.0,
            frames: None,
            decodable: true,
            decode_error: None,
            av1_hw,
        };
    }

    fn policy() -> Policy {
        return Policy { fps_limit: 30, max_pixels: 2560 };
    }

    #[test]
    fn hardware_av1_below_limits_is_playable() {
        let probe = video("av01", 1920, 1080, 30.0, true);
        assert_eq!(verdict(&probe, policy()), Verdict::Playable);
    }

    #[test]
    fn av1_without_hardware_needs_optimize() {
        let probe = video("av01", 1920, 1080, 30.0, false);
        assert_eq!(verdict(&probe, policy()), Verdict::NeedsOptimize { reasons: vec![Reason::Av1WithoutHardware] });
    }

    #[test]
    fn vp9_is_never_hardware_friendly() {
        let probe = video("vp09", 1280, 720, 30.0, true);
        assert_eq!(verdict(&probe, policy()), Verdict::NeedsOptimize { reasons: vec![Reason::NonHardwareCodec] });
    }

    #[test]
    fn high_fps_and_oversized_report_every_reason() {
        let probe = video("hvc1", 3840, 2160, 60.0, true);
        assert_eq!(
            verdict(&probe, policy()),
            Verdict::NeedsOptimize { reasons: vec![Reason::FpsAboveLimit, Reason::ResolutionAboveDisplay] }
        );
    }

    #[test]
    fn hevc_1080p30_is_playable() {
        let probe = video("hvc1", 1920, 1080, 30.0, true);
        assert_eq!(verdict(&probe, policy()), Verdict::Playable);
    }

    #[test]
    fn corrupt_media_is_unsupported() {
        let mut probe = video("hvc1", 0, 0, 0.0, false);
        probe.decodable = false;
        probe.decode_error = Some("cannot decode".to_string());
        probe.container = Some("mp3".to_string());
        assert_eq!(verdict(&probe, policy()), Verdict::Unsupported { reason: "cannot decode".to_string() });
    }

    #[test]
    fn undecodable_video_container_needs_optimize() {
        let mut probe = video("vp09", 1280, 720, 30.0, false);
        probe.decodable = false;
        probe.decode_error = Some("cannot open".to_string());
        probe.container = Some("webm".to_string());
        assert_eq!(verdict(&probe, policy()), Verdict::NeedsOptimize { reasons: vec![Reason::ContainerUnsupported] });
    }

    #[test]
    fn container_unsupported_matches_only_supported_video() {
        assert!(supported_container(Some("mp4")));
        assert!(supported_container(Some("mkv")));
        assert!(!supported_container(Some("avi")));
        assert!(!supported_container(None));
    }

    #[test]
    fn images_and_gifs_are_playable() {
        let mut probe = video("png", 1920, 1080, 0.0, false);
        probe.kind = ItemKind::Image;
        probe.decodable = true;
        assert_eq!(verdict(&probe, policy()), Verdict::Playable);
    }

    #[test]
    fn fourcc_decodes_ascii_codes() {
        assert_eq!(fourcc(0x6176_3031), "av01");
        assert_eq!(fourcc(0x6876_6331), "hvc1");
    }

    #[test]
    fn dim_guards_bad_values() {
        assert_eq!(dim(1920.4), 1920);
        assert_eq!(dim(-5.0), 0);
        assert_eq!(dim(f64::NAN), 0);
    }

    #[test]
    fn container_is_lowercased_extension() {
        assert_eq!(container_of("/a/b.MP4"), Some("mp4".to_string()));
        assert_eq!(container_of("/a/b"), None);
    }
}
