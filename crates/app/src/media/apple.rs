use std::cell::Cell;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetReader, AVAssetReaderStatus, AVAssetReaderTrackOutput, AVAssetTrack, AVAssetWriter, AVAssetWriterInput,
    AVAssetWriterInputPixelBufferAdaptor, AVAssetWriterStatus, AVFileTypeMPEG4, AVFileTypeQuickTimeMovie,
    AVMediaTypeAudio, AVMediaTypeVideo, AVURLAsset,
};
use objc2_core_foundation::{CFDictionary, CFRetained, CFString, CFType, CGAffineTransform, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreate, CGContext, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo};
use objc2_core_media::{
    CMTime, kCMFormatDescriptionExtension_ContentLightLevelInfo,
    kCMFormatDescriptionExtension_MasteringDisplayColorVolume, kCMVideoCodecType_VP9,
};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight,
    CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferPool,
    CVPixelBufferUnlockBaseAddress, kCVPixelFormatType_32BGRA, kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange,
};
use objc2_foundation::{NSDictionary, NSError, NSNumber, NSString};
use objc2_image_io::{
    CGImageSource, kCGImagePropertyGIFDelayTime, kCGImagePropertyGIFDictionary, kCGImagePropertyGIFUnclampedDelayTime,
};
use objc2_video_toolbox::{
    VTRegisterSupplementalVideoDecoderIfAvailable, kVTCompressionPropertyKey_ContentLightLevelInfo,
    kVTCompressionPropertyKey_MasteringDisplayColorVolume, kVTProfileLevel_HEVC_Main10_AutoLevel,
};

use crate::cf::{cf_f64, cf_file_url, downcast_dict, file_url, ns_string, object};
use crate::media::av::{CM_TIMESCALE, format_description, image_size, load_track, thumbnail_options};
use crate::media::plan::{
    AAC_BITRATE, AAC_CHANNELS, AAC_SAMPLE_RATE, Audio, GIF_DEFAULT_DELAY, GIF_MIN_DELAY, GifPlan, VideoPlan, bitrate,
    even, scale_dims,
};
use crate::media::{TranscodeError, orient};

const AAC: u32 = u32::from_be_bytes(*b"aac ");
const LINEAR_PCM: u32 = u32::from_be_bytes(*b"lpcm");
const POLL: Duration = Duration::from_millis(200);
const GRACE: Duration = Duration::from_secs(2);

pub type Progress = Arc<dyn Fn(f64) + Send + Sync>;
type Extra = Vec<(&'static CFString, CFRetained<CFType>)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Codec {
    Hevc,
    H264,
}

pub enum Task {
    Video(VideoPlan),
    Gif(GifPlan),
}

pub fn register_vp9() {
    unsafe { VTRegisterSupplementalVideoDecoderIfAvailable(kCMVideoCodecType_VP9) };
}

pub fn run(task: &Task, progress: &Progress, cancel: &Arc<AtomicBool>) -> Result<(), TranscodeError> {
    return match task {
        Task::Video(plan) => with_fallback(plan.hevc, |codec| encode_video(plan, codec, progress, cancel)),
        Task::Gif(plan) => with_fallback(true, |codec| encode_gif(plan, codec, progress, cancel)),
    };
}

pub fn reorient_video(input: &Path, output: &Path, turns: u8) -> Result<(), TranscodeError> {
    let _ = std::fs::remove_file(output);
    let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&file_url(input), None) };
    let media = unsafe { AVMediaTypeVideo }.expect("video media type");
    let track = load_track(&asset, media).ok().flatten().ok_or(TranscodeError::NoVideo)?;
    let reader = unsafe { AVAssetReader::initWithAsset_error(AVAssetReader::alloc(), &asset) }
        .map_err(|error| ns_error(&error))?;
    let video_out = unsafe {
        AVAssetReaderTrackOutput::initWithTrack_outputSettings(AVAssetReaderTrackOutput::alloc(), &track, None)
    };
    unsafe { video_out.setAlwaysCopiesSampleData(false) };
    let hint = format_description(&track);
    let video_in = unsafe {
        AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings_sourceFormatHint(media, None, hint.as_deref())
    };
    let size = unsafe { track.naturalSize() };
    let transform = orient::turn(unsafe { track.preferredTransform() }, size.width.abs(), size.height.abs(), turns);
    unsafe {
        video_in.setTransform(transform);
        video_in.setExpectsMediaDataInRealTime(false);
    }
    let writer = unsafe {
        AVAssetWriter::initWithURL_fileType_error(
            AVAssetWriter::alloc(),
            &file_url(output),
            AVFileTypeQuickTimeMovie.expect("movie file type"),
        )
    }
    .map_err(|error| ns_error(&error))?;
    if !unsafe { reader.canAddOutput(&video_out) } || !unsafe { writer.canAddInput(&video_in) } {
        return Err(TranscodeError::Failed("cannot copy this video".to_string()));
    }
    unsafe {
        reader.addOutput(&video_out);
        writer.addInput(&video_in);
    }
    let audio = audio_setup(&asset, Audio::Copy)
        .filter(|(audio_out, audio_in)| unsafe { reader.canAddOutput(audio_out) && writer.canAddInput(audio_in) });
    if let Some((audio_out, audio_in)) = &audio {
        unsafe {
            audio_in.setExpectsMediaDataInRealTime(false);
            reader.addOutput(audio_out);
            writer.addInput(audio_in);
        }
    }
    if !unsafe { reader.startReading() } {
        return Err(reader_error(&reader));
    }
    if !unsafe { writer.startWriting() } {
        unsafe { reader.cancelReading() };
        return Err(writer_error(&writer));
    }
    unsafe { writer.startSessionAtSourceTime(CMTime::new(0, CM_TIMESCALE)) };
    let cancel = Arc::new(AtomicBool::new(false));
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let mut pending = 0usize;
    let mut queues = Vec::new();
    for (output_track, input_track) in std::iter::once((&video_out, &video_in))
        .chain(audio.as_ref().map(|(audio_out, audio_in)| (audio_out, audio_in)))
    {
        let queue = DispatchQueue::new("livewall.reorient", None);
        let block = pump_audio(output_track, input_track, &cancel, done_tx.clone());
        unsafe { input_track.requestMediaDataWhenReadyOnQueue_usingBlock(&queue, &block) };
        queues.push(queue);
        pending += 1;
    }
    drop(done_tx);
    while pending > 0 {
        match done_rx.recv_timeout(POLL) {
            Ok(()) => pending -= 1,
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                if unsafe { reader.status() } == AVAssetReaderStatus::Failed
                    || unsafe { writer.status() } == AVAssetWriterStatus::Failed
                {
                    cancel.store(true, Ordering::SeqCst);
                }
            }
        }
    }
    let failed = unsafe { reader.status() } == AVAssetReaderStatus::Failed
        || unsafe { writer.status() } == AVAssetWriterStatus::Failed;
    if failed || !finish_writing(&writer) {
        let error = if unsafe { reader.status() } == AVAssetReaderStatus::Failed {
            reader_error(&reader)
        } else {
            writer_error(&writer)
        };
        unsafe { writer.cancelWriting() };
        let _ = std::fs::remove_file(output);
        return Err(error);
    }
    return Ok(());
}

fn with_fallback(
    hevc: bool,
    mut encode: impl FnMut(Codec) -> Result<(), TranscodeError>,
) -> Result<(), TranscodeError> {
    if hevc {
        match encode(Codec::Hevc) {
            Ok(()) => return Ok(()),
            Err(error @ (TranscodeError::Cancelled | TranscodeError::NoVideo)) => return Err(error),
            Err(error) => tracing::warn!(%error, "HEVC encode failed; retrying with H.264"),
        }
    }
    return encode(Codec::H264);
}

fn encode_video(
    plan: &VideoPlan,
    codec: Codec,
    progress: &Progress,
    cancel: &Arc<AtomicBool>,
) -> Result<(), TranscodeError> {
    let chain: &[Audio] = match plan.audio {
        Audio::Copy => &[Audio::Copy, Audio::Encode, Audio::Drop],
        Audio::Encode => &[Audio::Encode, Audio::Drop],
        Audio::Drop => &[Audio::Drop],
    };
    let mut last = TranscodeError::Failed("cannot encode this video".to_string());
    for audio in chain {
        match encode_once(plan, codec, *audio, progress, cancel) {
            Ok(()) => return Ok(()),
            Err(error @ (TranscodeError::Cancelled | TranscodeError::NoVideo)) => return Err(error),
            Err(error) => {
                tracing::warn!(%error, ?audio, "encode attempt failed");
                last = error;
            }
        }
    }
    let _ = std::fs::remove_file(&plan.output);
    return Err(last);
}

fn encode_once(
    plan: &VideoPlan,
    codec: Codec,
    audio: Audio,
    progress: &Progress,
    cancel: &Arc<AtomicBool>,
) -> Result<(), TranscodeError> {
    let _ = std::fs::remove_file(&plan.output);
    let pipeline = Pipeline::open(plan, codec, audio)?;
    pipeline.start()?;
    pipeline.pump(plan, progress, cancel);
    return pipeline.close(plan, progress, cancel);
}

struct Pipeline {
    reader: Retained<AVAssetReader>,
    writer: Retained<AVAssetWriter>,
    video_out: Retained<AVAssetReaderTrackOutput>,
    video_in: Retained<AVAssetWriterInput>,
    audio: Option<(Retained<AVAssetReaderTrackOutput>, Retained<AVAssetWriterInput>)>,
}

impl Pipeline {
    fn open(plan: &VideoPlan, codec: Codec, audio: Audio) -> Result<Self, TranscodeError> {
        let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&file_url(&plan.input), None) };
        let track = load_track(&asset, unsafe { AVMediaTypeVideo }.expect("video media type"))
            .ok()
            .flatten()
            .ok_or(TranscodeError::NoVideo)?;
        let reader = unsafe { AVAssetReader::initWithAsset_error(AVAssetReader::alloc(), &asset) }
            .map_err(|error| ns_error(&error))?;
        let video_out = unsafe {
            AVAssetReaderTrackOutput::initWithTrack_outputSettings(
                AVAssetReaderTrackOutput::alloc(),
                &track,
                Some(&reader_video_settings(plan, codec)),
            )
        };
        unsafe { video_out.setAlwaysCopiesSampleData(false) };
        if !unsafe { reader.canAddOutput(&video_out) } {
            return Err(TranscodeError::Failed("cannot read this video".to_string()));
        }
        unsafe { reader.addOutput(&video_out) };

        let writer = new_writer(&plan.output)?;
        let transform = unsafe { track.preferredTransform() };
        let (width, height) = stored_dims(plan.width, plan.height, transform);
        let video_in = video_input(plan, codec, (width, height), &hdr_metadata(&track));
        unsafe { video_in.setTransform(orient::fit(transform, f64::from(width), f64::from(height))) };
        if !unsafe { writer.canAddInput(&video_in) } {
            return Err(TranscodeError::Failed("the video encoder is not available".to_string()));
        }
        unsafe { writer.addInput(&video_in) };

        let audio = if audio == Audio::Drop { None } else { audio_setup(&asset, audio) };
        if let Some((audio_out, audio_in)) = &audio {
            if !unsafe { writer.canAddInput(audio_in) } || !unsafe { reader.canAddOutput(audio_out) } {
                return Err(TranscodeError::Failed("cannot transfer the audio track".to_string()));
            }
            unsafe {
                audio_in.setExpectsMediaDataInRealTime(false);
                writer.addInput(audio_in);
                reader.addOutput(audio_out);
            }
        }
        return Ok(Self { reader, writer, video_out, video_in, audio });
    }

    fn start(&self) -> Result<(), TranscodeError> {
        if !unsafe { self.reader.startReading() } {
            return Err(reader_error(&self.reader));
        }
        if !unsafe { self.writer.startWriting() } {
            unsafe { self.reader.cancelReading() };
            return Err(writer_error(&self.writer));
        }
        unsafe { self.writer.startSessionAtSourceTime(CMTime::new(0, CM_TIMESCALE)) };
        return Ok(());
    }

    fn pump(&self, plan: &VideoPlan, progress: &Progress, cancel: &Arc<AtomicBool>) {
        let (done_tx, done_rx) = mpsc::channel::<()>();
        let video_queue = DispatchQueue::new("livewall.optimize.video", None);
        let video_block = pump_video(plan, self, progress, cancel, done_tx.clone());
        unsafe { self.video_in.requestMediaDataWhenReadyOnQueue_usingBlock(&video_queue, &video_block) };
        let mut pending = 1usize;
        if let Some((audio_out, audio_in)) = &self.audio {
            let audio_queue = DispatchQueue::new("livewall.optimize.audio", None);
            let audio_block = pump_audio(audio_out, audio_in, cancel, done_tx.clone());
            unsafe { audio_in.requestMediaDataWhenReadyOnQueue_usingBlock(&audio_queue, &audio_block) };
            pending += 1;
        }
        drop(done_tx);
        let mut deadline: Option<Instant> = None;
        while pending > 0 {
            match done_rx.recv_timeout(POLL) {
                Ok(()) => pending -= 1,
                Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {
                    if deadline.is_some_and(|at| Instant::now() >= at) {
                        return;
                    }
                    if deadline.is_none() && self.stalled(cancel) {
                        unsafe { self.reader.cancelReading() };
                        deadline = Some(Instant::now() + GRACE);
                    }
                }
            }
        }
    }

    fn stalled(&self, cancel: &AtomicBool) -> bool {
        return cancel.load(Ordering::SeqCst)
            || unsafe { self.reader.status() } == AVAssetReaderStatus::Failed
            || unsafe { self.writer.status() } == AVAssetWriterStatus::Failed;
    }

    fn close(&self, plan: &VideoPlan, progress: &Progress, cancel: &AtomicBool) -> Result<(), TranscodeError> {
        let failure = if cancel.load(Ordering::SeqCst) {
            Some(TranscodeError::Cancelled)
        } else if unsafe { self.reader.status() } == AVAssetReaderStatus::Failed {
            Some(reader_error(&self.reader))
        } else if unsafe { self.writer.status() } == AVAssetWriterStatus::Failed {
            Some(writer_error(&self.writer))
        } else {
            None
        };
        if let Some(error) = failure {
            unsafe { self.writer.cancelWriting() };
            let _ = std::fs::remove_file(&plan.output);
            return Err(error);
        }
        if plan.duration > 0.0 {
            unsafe { self.writer.endSessionAtSourceTime(CMTime::with_seconds(plan.duration, CM_TIMESCALE)) };
        }
        if !finish_writing(&self.writer) {
            return Err(writer_error(&self.writer));
        }
        progress(1.0);
        return Ok(());
    }
}

fn pump_video(
    plan: &VideoPlan,
    pipeline: &Pipeline,
    progress: &Progress,
    cancel: &Arc<AtomicBool>,
    done: mpsc::Sender<()>,
) -> RcBlock<dyn Fn()> {
    let reader = pipeline.reader.clone();
    let output = pipeline.video_out.clone();
    let input = pipeline.video_in.clone();
    let progress = Arc::clone(progress);
    let cancel = Arc::clone(cancel);
    let duration = plan.duration;
    let min_step = (1.0 / plan.fps) - 0.002;
    let next_keep = Cell::new(f64::NEG_INFINITY);
    return RcBlock::new(move || {
        while unsafe { input.isReadyForMoreMediaData() } {
            if cancel.load(Ordering::SeqCst) {
                unsafe { reader.cancelReading() };
                finish(&input, &done);
                return;
            }
            let Some(sample) = (unsafe { output.copyNextSampleBuffer() }) else {
                finish(&input, &done);
                return;
            };
            let time = unsafe { sample.presentation_time_stamp().seconds() };
            if time + 0.0001 < next_keep.get() {
                continue;
            }
            if !unsafe { input.appendSampleBuffer(&sample) } {
                unsafe { reader.cancelReading() };
                finish(&input, &done);
                return;
            }
            next_keep.set(time + min_step);
            if duration > 0.0 {
                progress((time / duration).clamp(0.0, 1.0));
            }
        }
    });
}

fn pump_audio(
    output: &Retained<AVAssetReaderTrackOutput>,
    input: &Retained<AVAssetWriterInput>,
    cancel: &Arc<AtomicBool>,
    done: mpsc::Sender<()>,
) -> RcBlock<dyn Fn()> {
    let output = output.clone();
    let input = input.clone();
    let cancel = Arc::clone(cancel);
    return RcBlock::new(move || {
        while unsafe { input.isReadyForMoreMediaData() } {
            if cancel.load(Ordering::SeqCst) {
                finish(&input, &done);
                return;
            }
            let Some(sample) = (unsafe { output.copyNextSampleBuffer() }) else {
                finish(&input, &done);
                return;
            };
            if !unsafe { input.appendSampleBuffer(&sample) } {
                finish(&input, &done);
                return;
            }
        }
    });
}

fn finish(input: &AVAssetWriterInput, done: &mpsc::Sender<()>) {
    unsafe { input.markAsFinished() };
    let _ = done.send(());
}

fn stored_dims(width: u32, height: u32, transform: CGAffineTransform) -> (u32, u32) {
    if transform.b.abs() > 0.5 || transform.c.abs() > 0.5 {
        return (height, width);
    }
    return (width, height);
}

fn new_writer(output: &Path) -> Result<Retained<AVAssetWriter>, TranscodeError> {
    let writer = unsafe {
        AVAssetWriter::initWithURL_fileType_error(
            AVAssetWriter::alloc(),
            &file_url(output),
            AVFileTypeMPEG4.expect("mp4 file type"),
        )
    }
    .map_err(|error| ns_error(&error))?;
    unsafe { writer.setShouldOptimizeForNetworkUse(true) };
    return Ok(writer);
}

fn video_input(plan: &VideoPlan, codec: Codec, size: (u32, u32), extra: &Extra) -> Retained<AVAssetWriterInput> {
    let input = unsafe {
        AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
            AVMediaTypeVideo.expect("video media type"),
            Some(&writer_video_settings(plan, codec, size, extra)),
        )
    };
    unsafe { input.setExpectsMediaDataInRealTime(false) };
    return input;
}

fn reader_video_settings(plan: &VideoPlan, codec: Codec) -> Retained<NSDictionary<NSString, AnyObject>> {
    let format = if codec == Codec::Hevc && plan.ten_bit {
        kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange
    } else {
        kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
    };
    let key = NSString::from_str("PixelFormatType");
    let value = NSNumber::numberWithUnsignedInt(format);
    let keys: [&NSString; 1] = [&key];
    let values: [&AnyObject; 1] = [&value];
    return NSDictionary::from_slices(&keys, &values);
}

fn hdr_metadata(track: &AVAssetTrack) -> Extra {
    let Some(description) = format_description(track) else {
        return Vec::new();
    };
    let pairs = unsafe {
        [
            (
                kCMFormatDescriptionExtension_MasteringDisplayColorVolume,
                kVTCompressionPropertyKey_MasteringDisplayColorVolume,
            ),
            (kCMFormatDescriptionExtension_ContentLightLevelInfo, kVTCompressionPropertyKey_ContentLightLevelInfo),
        ]
    };
    return pairs
        .into_iter()
        .filter_map(|(source, target)| unsafe { description.extension(source) }.map(|value| (target, value)))
        .collect();
}

fn compression_settings(plan: &VideoPlan, codec: Codec, extra: &Extra) -> Retained<NSDictionary<NSString, AnyObject>> {
    let bit_rate = NSNumber::numberWithUnsignedLongLong(plan.bitrate);
    let frame_rate = NSNumber::numberWithInteger(fps_integer(plan.fps));
    let key_interval = NSNumber::numberWithInteger(fps_integer(plan.fps) * 2);
    let mut keys: Vec<&NSString> = vec![
        unsafe { objc2_av_foundation::AVVideoAverageBitRateKey }.expect("bitrate key"),
        unsafe { objc2_av_foundation::AVVideoExpectedSourceFrameRateKey }.expect("frame rate key"),
        unsafe { objc2_av_foundation::AVVideoMaxKeyFrameIntervalKey }.expect("key frame key"),
    ];
    let mut values: Vec<&AnyObject> = vec![&bit_rate, &frame_rate, &key_interval];
    let profile = NSString::from_str(&unsafe { kVTProfileLevel_HEVC_Main10_AutoLevel }.to_string());
    if codec == Codec::Hevc && plan.ten_bit {
        keys.push(unsafe { objc2_av_foundation::AVVideoProfileLevelKey }.expect("profile key"));
        values.push(&profile);
    }
    if codec == Codec::Hevc && plan.hdr {
        for (key, value) in extra {
            keys.push(ns_string(key));
            values.push(object(&**value));
        }
    }
    return NSDictionary::from_slices(&keys, &values);
}

fn writer_video_settings(
    plan: &VideoPlan,
    codec: Codec,
    (width, height): (u32, u32),
    extra: &Extra,
) -> Retained<NSDictionary<NSString, AnyObject>> {
    let width = NSNumber::numberWithUnsignedInt(width);
    let height = NSNumber::numberWithUnsignedInt(height);
    let codec_type = if codec == Codec::Hevc {
        unsafe { objc2_av_foundation::AVVideoCodecTypeHEVC }.expect("hevc codec")
    } else {
        unsafe { objc2_av_foundation::AVVideoCodecTypeH264 }.expect("h264 codec")
    };
    let scaling = unsafe { objc2_av_foundation::AVVideoScalingModeResizeAspectFill }.expect("scaling mode");
    let colors = color_properties(plan);
    let compression = compression_settings(plan, codec, extra);
    let keys: [&NSString; 6] = [
        unsafe { objc2_av_foundation::AVVideoCodecKey }.expect("codec key"),
        unsafe { objc2_av_foundation::AVVideoWidthKey }.expect("width key"),
        unsafe { objc2_av_foundation::AVVideoHeightKey }.expect("height key"),
        unsafe { objc2_av_foundation::AVVideoScalingModeKey }.expect("scaling key"),
        unsafe { objc2_av_foundation::AVVideoColorPropertiesKey }.expect("color key"),
        unsafe { objc2_av_foundation::AVVideoCompressionPropertiesKey }.expect("compression key"),
    ];
    let values: [&AnyObject; 6] = [codec_type, &width, &height, scaling, &colors, &compression];
    return NSDictionary::from_slices(&keys, &values);
}

fn color_properties(plan: &VideoPlan) -> Retained<NSDictionary<NSString, AnyObject>> {
    let wide = plan.hdr || plan.primaries.as_deref() == Some("ITU_R_2020");
    let primaries = if wide {
        unsafe { objc2_av_foundation::AVVideoColorPrimaries_ITU_R_2020 }
    } else if plan.primaries.as_deref() == Some("P3_D65") {
        unsafe { objc2_av_foundation::AVVideoColorPrimaries_P3_D65 }
    } else {
        unsafe { objc2_av_foundation::AVVideoColorPrimaries_ITU_R_709_2 }
    }
    .expect("primaries");
    let transfer = if plan.transfer.as_deref().is_some_and(|value| value.contains("HLG")) {
        unsafe { objc2_av_foundation::AVVideoTransferFunction_ITU_R_2100_HLG }
    } else if plan.hdr {
        unsafe { objc2_av_foundation::AVVideoTransferFunction_SMPTE_ST_2084_PQ }
    } else {
        unsafe { objc2_av_foundation::AVVideoTransferFunction_ITU_R_709_2 }
    }
    .expect("transfer");
    let matrix = if wide {
        unsafe { objc2_av_foundation::AVVideoYCbCrMatrix_ITU_R_2020 }
    } else {
        unsafe { objc2_av_foundation::AVVideoYCbCrMatrix_ITU_R_709_2 }
    }
    .expect("matrix");
    let keys: [&NSString; 3] = [
        unsafe { objc2_av_foundation::AVVideoColorPrimariesKey }.expect("primaries key"),
        unsafe { objc2_av_foundation::AVVideoTransferFunctionKey }.expect("transfer key"),
        unsafe { objc2_av_foundation::AVVideoYCbCrMatrixKey }.expect("matrix key"),
    ];
    let values: [&AnyObject; 3] = [primaries, transfer, matrix];
    return NSDictionary::from_slices(&keys, &values);
}

fn audio_setup(
    asset: &AVURLAsset,
    audio: Audio,
) -> Option<(Retained<AVAssetReaderTrackOutput>, Retained<AVAssetWriterInput>)> {
    let media = unsafe { AVMediaTypeAudio }?;
    let track = load_track(asset, media).ok().flatten()?;
    return match audio {
        Audio::Copy => {
            let hint = format_description(&track);
            let output = unsafe {
                AVAssetReaderTrackOutput::initWithTrack_outputSettings(AVAssetReaderTrackOutput::alloc(), &track, None)
            };
            let input = unsafe {
                AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings_sourceFormatHint(
                    media,
                    None,
                    hint.as_deref(),
                )
            };
            Some((output, input))
        }
        Audio::Encode => {
            let output = unsafe {
                AVAssetReaderTrackOutput::initWithTrack_outputSettings(
                    AVAssetReaderTrackOutput::alloc(),
                    &track,
                    Some(&pcm_reader_settings()),
                )
            };
            let input = unsafe {
                AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(media, Some(&aac_writer_settings()))
            };
            Some((output, input))
        }
        Audio::Drop => None,
    };
}

fn pcm_reader_settings() -> Retained<NSDictionary<NSString, AnyObject>> {
    let format = NSNumber::numberWithUnsignedInt(LINEAR_PCM);
    let depth = NSNumber::numberWithUnsignedInt(16);
    let float = NSNumber::numberWithBool(false);
    let big_endian = NSNumber::numberWithBool(false);
    let non_interleaved = NSNumber::numberWithBool(false);
    let rate = NSNumber::numberWithUnsignedInt(AAC_SAMPLE_RATE);
    let channels = NSNumber::numberWithUnsignedInt(AAC_CHANNELS);
    let keys: [&NSString; 7] = [
        &NSString::from_str("AVFormatIDKey"),
        &NSString::from_str("AVLinearPCMBitDepthKey"),
        &NSString::from_str("AVLinearPCMIsFloatKey"),
        &NSString::from_str("AVLinearPCMIsBigEndianKey"),
        &NSString::from_str("AVLinearPCMIsNonInterleaved"),
        &NSString::from_str("AVSampleRateKey"),
        &NSString::from_str("AVNumberOfChannelsKey"),
    ];
    let values: [&AnyObject; 7] = [&format, &depth, &float, &big_endian, &non_interleaved, &rate, &channels];
    return NSDictionary::from_slices(&keys, &values);
}

fn aac_writer_settings() -> Retained<NSDictionary<NSString, AnyObject>> {
    let format = NSNumber::numberWithUnsignedInt(AAC);
    let rate = NSNumber::numberWithUnsignedInt(AAC_SAMPLE_RATE);
    let channels = NSNumber::numberWithUnsignedInt(AAC_CHANNELS);
    let bit_rate = NSNumber::numberWithUnsignedInt(AAC_BITRATE);
    let keys: [&NSString; 4] = [
        &NSString::from_str("AVFormatIDKey"),
        &NSString::from_str("AVSampleRateKey"),
        &NSString::from_str("AVNumberOfChannelsKey"),
        &NSString::from_str("AVEncoderBitRateKey"),
    ];
    let values: [&AnyObject; 4] = [&format, &rate, &channels, &bit_rate];
    return NSDictionary::from_slices(&keys, &values);
}

fn ns_error(error: &NSError) -> TranscodeError {
    return TranscodeError::Failed(error.localizedDescription().to_string());
}

fn reader_error(reader: &AVAssetReader) -> TranscodeError {
    return unsafe { reader.error() }
        .map_or_else(|| TranscodeError::Failed("cannot read this video".to_string()), |error| ns_error(&error));
}

fn writer_error(writer: &AVAssetWriter) -> TranscodeError {
    return unsafe { writer.error() }.map_or_else(
        || TranscodeError::Failed("cannot write the optimized video".to_string()),
        |error| ns_error(&error),
    );
}

fn finish_writing(writer: &AVAssetWriter) -> bool {
    let (sender, receiver) = mpsc::channel();
    let handler = RcBlock::new(move || {
        let _ = sender.send(());
    });
    unsafe { writer.finishWritingWithCompletionHandler(&handler) };
    let _ = receiver.recv();
    return unsafe { writer.status() } == AVAssetWriterStatus::Completed;
}

struct Gif {
    source: CFRetained<CGImageSource>,
    delays: Vec<f64>,
    plan: VideoPlan,
}

impl Gif {
    #[allow(clippy::cast_precision_loss)]
    fn open(plan: &GifPlan) -> Result<Self, TranscodeError> {
        let source = open_image(&plan.input)?;
        let count = unsafe { source.count() };
        if count == 0 {
            return Err(TranscodeError::Failed("the GIF has no frames".to_string()));
        }
        let (source_width, source_height) = image_size(&source);
        if source_width == 0 || source_height == 0 {
            return Err(TranscodeError::Failed("cannot read the GIF size".to_string()));
        }
        let (width, height) = scale_dims(source_width, source_height, plan.max_pixels)
            .unwrap_or_else(|| (even(f64::from(source_width)), even(f64::from(source_height))));
        let delays: Vec<f64> = (0..count).map(|index| frame_delay(&source, index)).collect();
        let duration: f64 = delays.iter().sum();
        let fps = if duration > 0.0 { (count as f64 / duration).clamp(1.0, 50.0) } else { 10.0 };
        let plan = VideoPlan {
            input: plan.input.clone(),
            output: plan.output.clone(),
            width,
            height,
            fps,
            bitrate: bitrate(width, height, fps, false, None),
            hevc: true,
            ten_bit: false,
            hdr: false,
            primaries: None,
            transfer: None,
            audio: Audio::Drop,
            duration,
        };
        return Ok(Self { source, delays, plan });
    }

    fn write(
        &self,
        writer: &AVAssetWriter,
        input: &AVAssetWriterInput,
        adaptor: &AVAssetWriterInputPixelBufferAdaptor,
        progress: &Progress,
        cancel: &AtomicBool,
    ) -> Result<(), TranscodeError> {
        let Some(pool) = (unsafe { adaptor.pixelBufferPool() }) else {
            return Err(TranscodeError::Failed("cannot allocate video frames".to_string()));
        };
        let (width, height) = (self.plan.width, self.plan.height);
        let mut elapsed = 0.0f64;
        for (index, delay) in self.delays.iter().enumerate() {
            if cancel.load(Ordering::SeqCst) {
                return Err(TranscodeError::Cancelled);
            }
            let image = thumbnail(&self.source, index, width.max(height))?;
            let buffer = pooled_buffer(&pool)?;
            draw(&buffer, &image)?;
            wait_ready(writer, input, cancel)?;
            let time = unsafe { CMTime::with_seconds(elapsed, CM_TIMESCALE) };
            if !unsafe { adaptor.appendPixelBuffer_withPresentationTime(&buffer, time) } {
                return Err(writer_error(writer));
            }
            elapsed += delay;
            if self.plan.duration > 0.0 {
                progress((elapsed / self.plan.duration).clamp(0.0, 1.0));
            }
        }
        return Ok(());
    }
}

fn encode_gif(
    plan: &GifPlan,
    codec: Codec,
    progress: &Progress,
    cancel: &Arc<AtomicBool>,
) -> Result<(), TranscodeError> {
    let _ = std::fs::remove_file(&plan.output);
    let gif = Gif::open(plan)?;
    let writer = new_writer(&plan.output)?;
    let input = video_input(&gif.plan, codec, (gif.plan.width, gif.plan.height), &Vec::new());
    if !unsafe { writer.canAddInput(&input) } {
        return Err(TranscodeError::Failed("the video encoder is not available".to_string()));
    }
    unsafe { writer.addInput(&input) };
    let adaptor = unsafe {
        AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(
            &input,
            Some(&pixel_buffer_attributes(gif.plan.width, gif.plan.height)),
        )
    };
    if !unsafe { writer.startWriting() } {
        return Err(writer_error(&writer));
    }
    unsafe { writer.startSessionAtSourceTime(CMTime::new(0, CM_TIMESCALE)) };
    if let Err(error) = gif.write(&writer, &input, &adaptor, progress, cancel) {
        unsafe { writer.cancelWriting() };
        let _ = std::fs::remove_file(&plan.output);
        return Err(error);
    }
    unsafe { input.markAsFinished() };
    if gif.plan.duration > 0.0 {
        unsafe { writer.endSessionAtSourceTime(CMTime::with_seconds(gif.plan.duration, CM_TIMESCALE)) };
    }
    if !finish_writing(&writer) {
        return Err(writer_error(&writer));
    }
    progress(1.0);
    return Ok(());
}

fn wait_ready(writer: &AVAssetWriter, input: &AVAssetWriterInput, cancel: &AtomicBool) -> Result<(), TranscodeError> {
    while !unsafe { input.isReadyForMoreMediaData() } {
        if cancel.load(Ordering::SeqCst) {
            return Err(TranscodeError::Cancelled);
        }
        if unsafe { writer.status() } == AVAssetWriterStatus::Failed {
            return Err(writer_error(writer));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    return Ok(());
}

fn open_image(path: &Path) -> Result<CFRetained<CGImageSource>, TranscodeError> {
    let Some(url) = cf_file_url(path) else {
        return Err(TranscodeError::Failed("invalid GIF path".to_string()));
    };
    return unsafe { CGImageSource::with_url(&url, None) }
        .ok_or_else(|| TranscodeError::Failed("cannot open the GIF".to_string()));
}

fn frame_delay(source: &CGImageSource, index: usize) -> f64 {
    let Some(properties) = (unsafe { source.properties_at_index(index, None) }) else {
        return GIF_DEFAULT_DELAY;
    };
    let properties: &CFDictionary<CFString, CFType> = unsafe { properties.cast_unchecked() };
    let Some(gif) = properties.get(unsafe { kCGImagePropertyGIFDictionary }) else {
        return GIF_DEFAULT_DELAY;
    };
    let Some(gif) = downcast_dict(Some(&gif)) else {
        return GIF_DEFAULT_DELAY;
    };
    let delay = cf_f64(gif.get(unsafe { kCGImagePropertyGIFUnclampedDelayTime }).as_deref())
        .or_else(|| cf_f64(gif.get(unsafe { kCGImagePropertyGIFDelayTime }).as_deref()));
    return delay.filter(|value| *value > 0.0).map_or(GIF_DEFAULT_DELAY, |value| value.max(GIF_MIN_DELAY));
}

fn thumbnail(source: &CGImageSource, index: usize, max_pixel: u32) -> Result<CFRetained<CGImage>, TranscodeError> {
    let options = thumbnail_options(max_pixel as usize);
    let options: &CFDictionary = unsafe { options.cast_unchecked() };
    return unsafe { source.thumbnail_at_index(index, Some(options)) }
        .ok_or_else(|| TranscodeError::Failed("cannot decode a GIF frame".to_string()));
}

fn pixel_buffer_attributes(width: u32, height: u32) -> Retained<NSDictionary<NSString, AnyObject>> {
    let format = NSNumber::numberWithUnsignedInt(kCVPixelFormatType_32BGRA);
    let width = NSNumber::numberWithUnsignedInt(width);
    let height = NSNumber::numberWithUnsignedInt(height);
    let keys: [&NSString; 3] =
        [&NSString::from_str("PixelFormatType"), &NSString::from_str("Width"), &NSString::from_str("Height")];
    let values: [&AnyObject; 3] = [&format, &width, &height];
    return NSDictionary::from_slices(&keys, &values);
}

fn pooled_buffer(pool: &CVPixelBufferPool) -> Result<CFRetained<CVPixelBuffer>, TranscodeError> {
    let mut out: *mut CVPixelBuffer = std::ptr::null_mut();
    let status = unsafe { CVPixelBufferPool::create_pixel_buffer(None, pool, NonNull::from(&mut out)) };
    let Some(buffer) = NonNull::new(out).filter(|_| status == 0) else {
        return Err(TranscodeError::Failed("cannot allocate a video frame".to_string()));
    };
    return Ok(unsafe { CFRetained::from_raw(buffer) });
}

#[allow(clippy::cast_precision_loss)]
fn draw(buffer: &CVPixelBuffer, image: &CGImage) -> Result<(), TranscodeError> {
    unsafe { CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::empty()) };
    let result = (|| {
        let width = CVPixelBufferGetWidth(buffer);
        let height = CVPixelBufferGetHeight(buffer);
        let bytes = CVPixelBufferGetBytesPerRow(buffer);
        let base = CVPixelBufferGetBaseAddress(buffer);
        let Some(space) = objc2_core_graphics::CGColorSpace::new_device_rgb() else {
            return Err(TranscodeError::Failed("cannot create a color space".to_string()));
        };
        let info = CGImageByteOrderInfo::Order32Little.0 | CGImageAlphaInfo::PremultipliedFirst.0;
        let Some(context) = (unsafe { CGBitmapContextCreate(base, width, height, 8, bytes, Some(&space), info) })
        else {
            return Err(TranscodeError::Failed("cannot create a drawing context".to_string()));
        };
        let rect = CGRect::new(CGPoint::ZERO, CGSize::new(width as f64, height as f64));
        CGContext::set_rgb_fill_color(Some(&context), 0.0, 0.0, 0.0, 1.0);
        CGContext::fill_rect(Some(&context), rect);
        CGContext::draw_image(Some(&context), rect, Some(image));
        return Ok(());
    })();
    unsafe { CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::empty()) };
    return result;
}

#[allow(clippy::cast_possible_truncation)]
fn fps_integer(fps: f64) -> isize {
    if !fps.is_finite() || fps < 1.0 {
        return 30;
    }
    return fps.round().min(240.0) as isize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::plan::{Source, Target, build_args, video_plan};
    use crate::media::probe;
    use crate::model::ItemKind;
    use std::path::PathBuf;
    use std::process::Command;

    fn fixtures() -> PathBuf {
        return Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures");
    }

    fn scratch() -> PathBuf {
        return std::env::temp_dir().join("livewall-apple-smoke");
    }

    fn probe_output(path: &Path) -> (String, u32, u32, f64, f64, u64) {
        let text = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=codec_name,width,height,r_frame_rate",
                "-show_entries",
                "format=duration,size",
                "-of",
                "default=nw=1",
            ])
            .arg(path)
            .output()
            .expect("ffprobe");
        let text = String::from_utf8_lossy(&text.stdout).into_owned();
        let field = |name: &str| -> String {
            return text
                .lines()
                .find_map(|line| line.strip_prefix(name).and_then(|value| value.strip_prefix('=')))
                .unwrap_or_default()
                .to_string();
        };
        let rate = field("r_frame_rate");
        let (num, den) = rate.split_once('/').unwrap_or(("0", "1"));
        let fps = num.parse::<f64>().unwrap_or(0.0) / den.parse::<f64>().unwrap_or(1.0);
        return (
            field("codec_name"),
            field("width").parse().unwrap_or(0),
            field("height").parse().unwrap_or(0),
            fps,
            field("duration").parse().unwrap_or(0.0),
            field("size").parse().unwrap_or(0),
        );
    }

    fn audio_codec(path: &Path) -> String {
        let text = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "a:0",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "default=nw=1",
            ])
            .arg(path)
            .output()
            .expect("ffprobe");
        return String::from_utf8_lossy(&text.stdout).trim().trim_start_matches("codec_name=").to_string();
    }

    fn pix_fmt(path: &Path) -> String {
        let text = Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=pix_fmt", "-of", "default=nw=1"])
            .arg(path)
            .output()
            .expect("ffprobe");
        return String::from_utf8_lossy(&text.stdout).trim().to_string();
    }

    fn transcode_video(input: &Path, output: &Path, max_pixels: u32, fps_limit: u32) {
        let probe = probe::inspect(input, ItemKind::Video);
        let source = Source::from(&probe);
        let target = Target { fps_limit, max_pixels, hevc: true };
        let plan = video_plan(input, output, Some(&probe), &source, &target, probe.duration);
        let progress: Arc<dyn Fn(f64) + Send + Sync> = Arc::new(|_| {});
        let cancel = Arc::new(AtomicBool::new(false));
        run(&Task::Video(plan), &progress, &cancel).expect("transcode");
    }

    #[test]
    #[ignore = "requires local target/fixtures and writes to a scratch dir"]
    #[allow(clippy::too_many_lines)]
    fn smoke_apple_transcodes() {
        let dir = scratch();
        std::fs::create_dir_all(&dir).expect("scratch");
        let fixtures = fixtures();
        if !fixtures.join("clip-4k-hevc-60.mp4").exists() {
            eprintln!("skipping smoke test: fixtures not found at {}", fixtures.display());
            return;
        }
        let only = std::env::var("SMOKE_CASE").unwrap_or_default();

        if only.is_empty() || only == "fourk" {
            let four_k = dir.join("fourk.mp4");
            transcode_video(&fixtures.join("clip-4k-hevc-60.mp4"), &four_k, 2560, 30);
            let (codec, width, height, fps, _, size) = probe_output(&four_k);
            assert_eq!(codec, "hevc");
            assert_eq!((width, height), (2560, 1440));
            assert!(fps <= 30.0, "fps {fps}");
            assert!(size > 0);
            assert!(probe::inspect(&four_k, ItemKind::Video).decodable);
        }

        if only.is_empty() || only == "hevc" {
            let hevc = dir.join("hevc.mp4");
            transcode_video(&fixtures.join("clip-hevc-1080p30.mp4"), &hevc, 2560, 30);
            let (codec, width, height, fps, duration, _) = probe_output(&hevc);
            assert_eq!(codec, "hevc");
            assert_eq!((width, height), (1920, 1080));
            assert!(fps <= 30.0, "fps {fps}");
            assert!((duration - 3.0).abs() < 0.5, "duration {duration}");
            assert!(probe::inspect(&hevc, ItemKind::Video).decodable);
        }

        if only.is_empty() || only == "sixty" {
            let sixty = dir.join("sixty.mp4");
            transcode_video(&fixtures.join("clip-h264-720p60.mp4"), &sixty, 2560, 30);
            let (codec, width, height, fps, _, _) = probe_output(&sixty);
            assert_eq!(codec, "hevc");
            assert_eq!((width, height), (1280, 720));
            assert!(fps <= 30.0, "fps {fps}");
            assert!(probe::inspect(&sixty, ItemKind::Video).decodable);
        }

        if only.is_empty() || only == "tenbit" {
            let input = dir.join("tenbit-src.mp4");
            if input.exists() {
                let out = dir.join("tenbit.mp4");
                transcode_video(&input, &out, 2560, 30);
                let (codec, width, height, _, _, _) = probe_output(&out);
                assert_eq!(codec, "hevc");
                assert_eq!((width, height), (1920, 1080));
                assert!(pix_fmt(&out).contains("10"), "pix_fmt {}", pix_fmt(&out));
                assert!(probe::inspect(&out, ItemKind::Video).decodable);
            }
        }

        if only.is_empty() || only == "audio" {
            let input = dir.join("audio-src.mp4");
            if input.exists() {
                let out = dir.join("audio.mp4");
                transcode_video(&input, &out, 2560, 30);
                let (codec, _, _, _, _, _) = probe_output(&out);
                assert_eq!(codec, "hevc");
                assert_eq!(audio_codec(&out), "aac");
                assert!(probe::inspect(&out, ItemKind::Video).decodable);
            }
        }

        if only.is_empty() || only == "h264" {
            let input = fixtures.join("clip-hevc-1080p30.mp4");
            let out = dir.join("h264.mp4");
            let probe = probe::inspect(&input, ItemKind::Video);
            let source = Source::from(&probe);
            let target = Target { fps_limit: 30, max_pixels: 2560, hevc: false };
            let plan = video_plan(&input, &out, Some(&probe), &source, &target, probe.duration);
            let progress: Arc<dyn Fn(f64) + Send + Sync> = Arc::new(|_| {});
            let cancel = Arc::new(AtomicBool::new(false));
            run(&Task::Video(plan), &progress, &cancel).expect("h264 transcode");
            let (codec, width, height, _, _, _) = probe_output(&out);
            assert_eq!(codec, "h264");
            assert_eq!((width, height), (1920, 1080));
            assert!(probe::inspect(&out, ItemKind::Video).decodable);
        }

        if only.is_empty() || only == "webm" {
            let input = fixtures.join("clip-vp9.webm");
            if input.exists() {
                let out = dir.join("webm.mp4");
                let probe = probe::inspect(&input, ItemKind::Video);
                let source = Source::from(&probe);
                let target = Target { fps_limit: 30, max_pixels: 2560, hevc: true };
                let args = build_args(&input, &out, &source, &target);
                let status = Command::new("ffmpeg").args(&args).status().expect("ffmpeg");
                assert!(status.success());
                let (codec, width, height, _, _, _) = probe_output(&out);
                assert_eq!(codec, "hevc");
                assert!(width > 0 && height > 0);
                assert!(probe::inspect(&out, ItemKind::Video).decodable);
            }
        }

        if only.is_empty() || only == "gif" {
            let gif = dir.join("gif.mp4");
            let gif_plan = GifPlan { input: fixtures.join("anim.gif"), output: gif.clone(), max_pixels: 2560 };
            let progress: Arc<dyn Fn(f64) + Send + Sync> = Arc::new(|_| {});
            let cancel = Arc::new(AtomicBool::new(false));
            run(&Task::Gif(gif_plan), &progress, &cancel).expect("gif transcode");
            let (codec, width, height, _, duration, size) = probe_output(&gif);
            assert_eq!(codec, "hevc");
            assert!(width > 0 && height > 0);
            assert!(duration > 0.0);
            assert!(size > 0);
            assert!(probe::inspect(&gif, ItemKind::Video).decodable);
        }
    }
}
