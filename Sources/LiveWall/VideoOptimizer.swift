import AppKit
import AVFoundation

/// Сведения о видео для панели и решения «стоит ли оптимизировать».
struct VideoInfo: Equatable {
    let width: Int
    let height: Int
    let fps: Double
    let codec: String
    let isHEVC: Bool
    let duration: Double

    var summary: String {
        let fpsText = fps > 0 ? "\(Int(fps.rounded())) fps" : "? fps"
        return "\(width)×\(height) · \(fpsText) · \(codec)"
    }
}

/// Отмена перекодирования из любого потока.
final class CancelToken {
    private let lock = NSLock()
    private var cancelled = false

    var isCancelled: Bool {
        lock.lock(); defer { lock.unlock() }
        return cancelled
    }

    func cancel() {
        lock.lock(); cancelled = true; lock.unlock()
    }
}

enum OptimizeError: LocalizedError {
    case noVideo, cancelled, failed(String)

    var errorDescription: String? {
        switch self {
        case .noVideo: return L("The file has no video track.")
        case .cancelled: return L("Cancelled.")
        case .failed(let text): return text
        }
    }
}

/// Делает «лёгкую» копию видео: HEVC, не больше экрана, не чаще заданного fps.
enum VideoOptimizer {
    /// Размер в пикселях, который должно покрывать видео: самый большой экран (не меньше Full HD).
    static func targetScreenPixels() -> CGSize {
        var w: CGFloat = 0
        var h: CGFloat = 0
        for screen in NSScreen.screens {
            let k = screen.backingScaleFactor
            w = max(w, screen.frame.width * k)
            h = max(h, screen.frame.height * k)
        }
        return CGSize(width: max(w, 1920), height: max(h, 1080))
    }

    static func info(for url: URL) async -> VideoInfo? {
        let asset = AVURLAsset(url: url)
        guard let track = try? await asset.loadTracks(withMediaType: .video).first else { return nil }
        let size = (try? await track.load(.naturalSize)) ?? .zero
        let transform = (try? await track.load(.preferredTransform)) ?? .identity
        let shown = CGRect(origin: .zero, size: size).applying(transform)
        let fps = Double((try? await track.load(.nominalFrameRate)) ?? 0)
        var codec = "?"
        var hevc = false
        if let desc = (try? await track.load(.formatDescriptions))?.first {
            let sub = CMFormatDescriptionGetMediaSubType(desc)
            codec = codecName(sub)
            hevc = (sub == kCMVideoCodecType_HEVC || fourCC(sub) == "hev1")
        }
        let duration = (try? await asset.load(.duration)).map { CMTimeGetSeconds($0) } ?? 0
        return VideoInfo(width: Int(abs(shown.width).rounded()), height: Int(abs(shown.height).rounded()),
                         fps: fps, codec: codec, isHEVC: hevc, duration: duration.isFinite ? duration : 0)
    }

    /// Есть ли выигрыш: видео больше экрана или чаще лимита кадров.
    static func needsOptimization(_ info: VideoInfo, fpsLimit: Double, target: CGSize) -> Bool {
        guard info.width > 0, info.height > 0 else { return false }
        let scale = max(target.width / CGFloat(info.width), target.height / CGFloat(info.height))
        let tooBig = scale < 0.9
        let tooFast = info.fps > fpsLimit + 1
        return tooBig || tooFast
    }

    static func fourCC(_ code: FourCharCode) -> String {
        let bytes = [24, 16, 8, 0].map { UInt8((code >> UInt32($0)) & 0xFF) }
        return (String(bytes: bytes, encoding: .ascii) ?? "?").trimmingCharacters(in: .whitespaces)
    }

    static func codecName(_ code: FourCharCode) -> String {
        switch fourCC(code) {
        case "avc1", "avc3": return "H.264"
        case "hvc1", "hev1": return "HEVC"
        case "ap4h", "ap4x", "apcn", "apch", "apcs", "apco": return "ProRes"
        case "vp09": return "VP9"
        case "av01": return "AV1"
        case "jpeg": return "Motion JPEG"
        default: return fourCC(code)
        }
    }

    /// Перекодирует `source` в `dest` (.mov). Прогресс 0…1 приходит с фоновой очереди.
    static func transcode(source: URL, dest: URL, fpsLimit: Double, target: CGSize,
                          token: CancelToken, progress: @escaping (Double) -> Void) async throws {
        let asset = AVURLAsset(url: source)
        guard let videoTrack = try await asset.loadTracks(withMediaType: .video).first else { throw OptimizeError.noVideo }
        let audioTrack = try? await asset.loadTracks(withMediaType: .audio).first
        let natural = try await videoTrack.load(.naturalSize)
        let transform = try await videoTrack.load(.preferredTransform)
        let nominalFps = try await videoTrack.load(.nominalFrameRate)
        let sourceFps = Double(nominalFps)
        let durationTime = try await asset.load(.duration)
        let duration = CMTimeGetSeconds(durationTime)

        // размер: покрыть экран, но не увеличивать (считаем по повёрнутой картинке)
        let shown = CGRect(origin: .zero, size: natural).applying(transform)
        let shownW = max(abs(shown.width), 1)
        let shownH = max(abs(shown.height), 1)
        let scale = min(1, max(target.width / shownW, target.height / shownH))
        let outW = max(2, Int((abs(natural.width) * scale / 2).rounded()) * 2)
        let outH = max(2, Int((abs(natural.height) * scale / 2).rounded()) * 2)
        let fps = sourceFps > 0 ? min(sourceFps, fpsLimit) : fpsLimit
        // ~0.07 бита на пиксель кадра — для HEVC визуально без потерь на обоях
        let bitrate = max(2_000_000, Int(Double(outW * outH) * fps * 0.07))

        try? FileManager.default.removeItem(at: dest)
        let reader = try AVAssetReader(asset: asset)
        let writer = try AVAssetWriter(outputURL: dest, fileType: .mov)

        let videoOut = AVAssetReaderTrackOutput(track: videoTrack, outputSettings: [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
        ])
        videoOut.alwaysCopiesSampleData = false
        guard reader.canAdd(videoOut) else { throw OptimizeError.failed(L("This video can't be read.")) }
        reader.add(videoOut)

        let keyInterval = max(1, Int(fps.rounded())) * 2
        let videoIn = AVAssetWriterInput(mediaType: .video, outputSettings: [
            AVVideoCodecKey: AVVideoCodecType.hevc,
            AVVideoWidthKey: outW,
            AVVideoHeightKey: outH,
            AVVideoScalingModeKey: AVVideoScalingModeResizeAspectFill,
            AVVideoCompressionPropertiesKey: [
                AVVideoAverageBitRateKey: bitrate,
                AVVideoExpectedSourceFrameRateKey: max(1, Int(fps.rounded())),
                AVVideoMaxKeyFrameIntervalKey: keyInterval,
            ] as [String: Any],
        ])
        videoIn.transform = transform
        videoIn.expectsMediaDataInRealTime = false
        guard writer.canAdd(videoIn) else { throw OptimizeError.failed(L("The HEVC encoder is not available.")) }
        writer.add(videoIn)

        // звук копируем как есть, если контейнер его принимает
        var audioOut: AVAssetReaderTrackOutput?
        var audioIn: AVAssetWriterInput?
        if let track = audioTrack {
            let output = AVAssetReaderTrackOutput(track: track, outputSettings: nil)
            let hint = (try? await track.load(.formatDescriptions))?.first
            let input = AVAssetWriterInput(mediaType: .audio, outputSettings: nil, sourceFormatHint: hint)
            input.expectsMediaDataInRealTime = false
            if reader.canAdd(output) && writer.canAdd(input) {
                reader.add(output)
                writer.add(input)
                audioOut = output
                audioIn = input
            }
        }

        guard reader.startReading() else {
            throw OptimizeError.failed(reader.error?.localizedDescription ?? L("This video can't be read."))
        }
        guard writer.startWriting() else {
            reader.cancelReading()
            throw OptimizeError.failed(writer.error?.localizedDescription ?? L("Could not write the optimized video."))
        }
        writer.startSession(atSourceTime: .zero)

        final class PumpState {
            var nextKeep = -Double.infinity
            var finished = false
        }
        let group = DispatchGroup()
        let minStep = 1.0 / fps - 0.002

        // видео: прореживаем кадры до лимита fps
        let videoState = PumpState()
        group.enter()
        videoIn.requestMediaDataWhenReady(on: DispatchQueue(label: "livewall.optimize.video")) {
            func finish() {
                guard !videoState.finished else { return }
                videoState.finished = true
                videoIn.markAsFinished()
                group.leave()
            }
            while videoIn.isReadyForMoreMediaData {
                if token.isCancelled {
                    reader.cancelReading()
                    finish()
                    return
                }
                guard let sample = videoOut.copyNextSampleBuffer() else {
                    finish()
                    return
                }
                let t = CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sample))
                guard t + 0.0001 >= videoState.nextKeep else { continue }
                if !videoIn.append(sample) {
                    reader.cancelReading()
                    finish()
                    return
                }
                videoState.nextKeep = t + minStep
                if duration > 0 { progress(min(1, max(0, t / duration))) }
            }
        }

        if let output = audioOut, let input = audioIn {
            let audioState = PumpState()
            group.enter()
            input.requestMediaDataWhenReady(on: DispatchQueue(label: "livewall.optimize.audio")) {
                func finish() {
                    guard !audioState.finished else { return }
                    audioState.finished = true
                    input.markAsFinished()
                    group.leave()
                }
                while input.isReadyForMoreMediaData {
                    if token.isCancelled {
                        finish()
                        return
                    }
                    guard let sample = output.copyNextSampleBuffer() else {
                        finish()
                        return
                    }
                    if !input.append(sample) {
                        finish()
                        return
                    }
                }
            }
        }

        await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in
            group.notify(queue: .global()) { c.resume() }
        }

        if token.isCancelled {
            writer.cancelWriting()
            try? FileManager.default.removeItem(at: dest)
            throw OptimizeError.cancelled
        }
        if reader.status == .failed || writer.status == .failed {
            let text = writer.error?.localizedDescription ?? reader.error?.localizedDescription
            writer.cancelWriting()
            try? FileManager.default.removeItem(at: dest)
            throw OptimizeError.failed(text ?? L("Could not write the optimized video."))
        }
        if durationTime.isValid && CMTimeGetSeconds(durationTime) > 0 {
            writer.endSession(atSourceTime: durationTime)
        }
        await writer.finishWriting()
        guard writer.status == .completed else {
            try? FileManager.default.removeItem(at: dest)
            throw OptimizeError.failed(writer.error?.localizedDescription ?? L("Could not write the optimized video."))
        }
        progress(1)
    }
}
