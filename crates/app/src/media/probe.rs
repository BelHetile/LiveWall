use std::fmt;
use std::path::Path;

use objc2::AnyThread;
use objc2_av_foundation::{
    AVAsset, AVAssetReader, AVAssetReaderStatus, AVAssetReaderTrackOutput, AVAssetTrack, AVMediaTypeAudio,
    AVMediaTypeVideo, AVURLAsset,
};
use objc2_core_foundation::{CFDictionary, CFString, CFType};
use objc2_core_media::{
    CMFormatDescription, kCMFormatDescriptionExtension_BitsPerComponent, kCMFormatDescriptionExtension_ColorPrimaries,
    kCMFormatDescriptionExtension_TransferFunction, kCMFormatDescriptionTransferFunction_ITU_R_2100_HLG,
    kCMFormatDescriptionTransferFunction_SMPTE_ST_2084_PQ, kCMVideoCodecType_AV1, kCMVideoCodecType_H264,
    kCMVideoCodecType_HEVC, kCMVideoCodecType_VP9,
};
use objc2_image_io::CGImageSource;
use objc2_video_toolbox::VTIsHardwareDecodeSupported;
use serde::{Deserialize, Serialize};

use crate::cf::{cf_file_url, cf_i64, cf_string, file_url};
use crate::media::av::{format_description, image_size, load_track};
use crate::model::ItemKind;

const SAMPLE_LIMIT: usize = 4;

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
    pub bit_rate: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    Av1WithoutHardware,
    NonHardwareCodec,
    FpsAboveLimit,
    ResolutionAboveDisplay,
    ContainerUnsupported,
    GifToVideo,
}

impl fmt::Display for Reason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match serde_json::to_value(self) {
            Ok(serde_json::Value::String(label)) => formatter.write_str(&label),
            _ => Err(fmt::Error),
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

const SUPPORTED_CONTAINERS: [&str; 5] = ["mp4", "mov", "m4v", "webm", "mkv"];

pub fn supported_container(container: Option<&str>) -> bool {
    return container.is_some_and(|container| SUPPORTED_CONTAINERS.contains(&container));
}

fn container_of(path: &Path) -> Option<String> {
    return path.extension().and_then(|value| value.to_str()).map(str::to_ascii_lowercase);
}

pub fn inspect(path: &Path, kind: ItemKind) -> Probe {
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
    if probe.kind == ItemKind::Gif {
        return Verdict::NeedsOptimize { reasons: vec![Reason::GifToVideo] };
    }
    if probe.kind != ItemKind::Video {
        return Verdict::Playable;
    }
    let mut reasons = Vec::new();
    if let Some(codec) = &probe.codec {
        if codec == "av01" {
            if !probe.av1_hw {
                reasons.push(Reason::Av1WithoutHardware);
            }
        } else if !hardware_decodes(codec) {
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

fn codec_type(codec: &str) -> Option<u32> {
    return match codec {
        "avc3" => Some(kCMVideoCodecType_H264),
        "hev1" => Some(kCMVideoCodecType_HEVC),
        "av01" => Some(kCMVideoCodecType_AV1),
        "vp09" => Some(kCMVideoCodecType_VP9),
        _ => {
            let bytes = codec.as_bytes();
            if bytes.len() != 4 {
                return None;
            }
            return Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
        }
    };
}

#[cfg(not(test))]
pub fn hardware_decodes(codec: &str) -> bool {
    return codec_type(codec).is_some_and(|codec_type| unsafe { VTIsHardwareDecodeSupported(codec_type) });
}

#[cfg(test)]
pub fn hardware_decodes(codec: &str) -> bool {
    return codec_type(codec).is_some_and(|codec_type| {
        return codec_type == kCMVideoCodecType_H264 || codec_type == kCMVideoCodecType_HEVC;
    });
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
    bit_rate: Option<f64>,
}

fn video_probe(path: &Path) -> Probe {
    let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&file_url(path), None) };
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
        bit_rate: facts.as_ref().and_then(|facts| facts.bit_rate),
    };
}

fn load_video(asset: &AVURLAsset) -> Result<VideoFacts, String> {
    let Some(media_type) = (unsafe { AVMediaTypeVideo }) else {
        return Err("no video media type".to_string());
    };
    return match load_track(asset, media_type)? {
        Some(track) => read_video_facts(asset, &track),
        None => Err("no video track".to_string()),
    };
}

fn read_video_facts(asset: &AVAsset, track: &AVAssetTrack) -> Result<VideoFacts, String> {
    let size = unsafe { track.naturalSize() };
    let transform = unsafe { track.preferredTransform() };
    let width = size.width.abs() * transform.a.abs() + size.height.abs() * transform.c.abs();
    let height = size.height.abs() * transform.d.abs() + size.width.abs() * transform.b.abs();
    let fps = f64::from(unsafe { track.nominalFrameRate() });
    let duration = unsafe { track.timeRange().duration.seconds() };
    let description = format_description(track).ok_or_else(|| "no format description".to_string())?;
    let codec = fourcc(unsafe { description.media_sub_type() });
    let (bit_depth, color_primaries, transfer) = color_info(&description);
    let (decodable, decode_error) = decode_check(asset, track);
    let bit_rate = unsafe { track.estimatedDataRate() };
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
        bit_rate: (bit_rate.is_finite() && bit_rate > 0.0).then(|| f64::from(bit_rate)),
    });
}

fn load_audio(asset: &AVURLAsset) -> Option<String> {
    let media_type = (unsafe { AVMediaTypeAudio })?;
    let track = load_track(asset, media_type).ok().flatten()?;
    let description = format_description(&track)?;
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

fn image_probe(path: &Path, kind: ItemKind) -> Probe {
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
        bit_rate: None,
    };
    let Some(url) = cf_file_url(path) else {
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
    (probe.width, probe.height) = image_size(&source);
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
            bit_rate: None,
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
    fn prores_without_hardware_decode_needs_optimize() {
        let probe = video("ap4h", 1920, 1080, 30.0, false);
        assert_eq!(verdict(&probe, policy()), Verdict::NeedsOptimize { reasons: vec![Reason::NonHardwareCodec] });
    }

    #[test]
    fn images_are_playable_and_gifs_need_optimize() {
        let mut probe = video("png", 1920, 1080, 0.0, false);
        probe.kind = ItemKind::Image;
        probe.decodable = true;
        assert_eq!(verdict(&probe, policy()), Verdict::Playable);
        let mut probe = video("gif", 1920, 1080, 0.0, false);
        probe.kind = ItemKind::Gif;
        probe.decodable = true;
        assert_eq!(verdict(&probe, policy()), Verdict::NeedsOptimize { reasons: vec![Reason::GifToVideo] });
    }

    #[test]
    fn codec_type_maps_alternative_fourccs() {
        assert_eq!(codec_type("hev1"), Some(kCMVideoCodecType_HEVC));
        assert_eq!(codec_type("avc3"), Some(kCMVideoCodecType_H264));
        assert_eq!(codec_type("ap4h"), Some(u32::from_be_bytes(*b"ap4h")));
        assert_eq!(codec_type("nop"), None);
        assert!(!hardware_decodes("vp08"));
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
        assert_eq!(container_of(Path::new("/a/b.MP4")), Some("mp4".to_string()));
        assert_eq!(container_of(Path::new("/a/b")), None);
    }
}
