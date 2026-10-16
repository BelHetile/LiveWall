use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::media::probe::{self, Probe};

const PASSTHROUGH_AUDIO: [&str; 5] = ["aac", "alac", "ac-3", "ec-3", "mp3"];
const BPP_8BIT: f64 = 0.07;
const BPP_10BIT: f64 = 0.10;
const MIN_BITRATE: u64 = 2_000_000;
const MIN_BITRATE_FLOOR: u64 = 300_000;
const REFERENCE_PIXELS: f64 = 1920.0 * 1080.0;
pub const GIF_MIN_DELAY: f64 = 0.02;
pub const GIF_DEFAULT_DELAY: f64 = 0.1;
pub const GIF_TINY_DELAY: f64 = 0.011;
pub const AAC_SAMPLE_RATE: u32 = 48_000;
pub const AAC_CHANNELS: u32 = 2;
pub const AAC_BITRATE: u32 = 176_000;
const SOURCE_SCALE_EXPONENT: f64 = 0.75;

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

impl From<&Probe> for Source {
    fn from(probe: &Probe) -> Self {
        return Self {
            width: probe.width,
            height: probe.height,
            fps: probe.fps,
            bit_depth: probe.bit_depth,
            hdr: probe.hdr,
            transfer: probe.transfer.clone(),
            audio: audio_plan(probe.audio_codec.as_deref(), true),
        };
    }
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
            push(&mut args, format!("{}k", AAC_BITRATE / 1000));
            push(&mut args, "-ar");
            push(&mut args, AAC_SAMPLE_RATE.to_string());
            push(&mut args, "-ac");
            push(&mut args, AAC_CHANNELS.to_string());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ItemKind;

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
    fn audio_plan_covers_every_case() {
        assert_eq!(audio_plan(Some("aac"), true), Audio::Copy);
        assert_eq!(audio_plan(Some("opus"), true), Audio::Encode);
        assert_eq!(audio_plan(None, true), Audio::Drop);
        assert_eq!(audio_plan(None, false), Audio::Encode);
        assert_eq!(audio_plan(Some("aac"), false), Audio::Encode);
    }
}
