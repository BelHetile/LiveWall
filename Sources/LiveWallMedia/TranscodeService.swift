import CoreMedia
import Foundation
import LiveWallCore
import LiveWallTools
#if canImport(VideoToolbox)
import VideoToolbox
#endif

public enum VideoToolboxHEVCEncoder {
    public static func isSupported() -> Bool {
        #if canImport(VideoToolbox)
        var list: CFArray?
        guard VTCopyVideoEncoderList(nil, &list) == noErr,
              let encoders = list as? [[String: Any]] else {
            return false
        }
        let key = kVTVideoEncoderList_CodecType as String
        let hevc = Int(kCMVideoCodecType_HEVC)
        return encoders.contains { ($0[key] as? Int) == hevc }
        #else
        return false
        #endif
    }
}

public enum TranscodeAudioMode: Sendable, Equatable {
    case passthrough
    case reencode
    case drop
}

public enum TranscodeAudioOutcome: Sendable, Equatable {
    case passthrough
    case reencoded
    case dropped(message: String)

    public var message: String? {
        if case let .dropped(message) = self {
            return message
        }
        return nil
    }
}

public struct TranscodeParameters: Sendable, Equatable {
    public let maxHeight: Int
    public let fpsLimit: Int

    public init(maxHeight: Int, fpsLimit: Int = 30) {
        self.maxHeight = maxHeight
        self.fpsLimit = TranscodeArguments.clampFpsLimit(fpsLimit)
    }
}

public struct TranscodeResult: Sendable, Equatable {
    public let outputURL: URL
    public let videoCodec: String
    public let audio: TranscodeAudioOutcome

    public init(outputURL: URL, videoCodec: String, audio: TranscodeAudioOutcome) {
        self.outputURL = outputURL
        self.videoCodec = videoCodec
        self.audio = audio
    }
}

public enum TranscodeArguments {
    public static let minimumVideoBitrate = 2_000_000
    public static let eightBitBitsPerPixel = 0.07
    public static let tenBitBitsPerPixel = 0.10
    public static let fpsLimitRange = 15...120
    public static let reencodeAudioBitrate = "176k"
    public static let reencodeAudioSampleRate = 48000
    public static let reencodeAudioChannels = 2
    public static let dropAudioMessage = "added without sound"
    public static let hevcEncoder = "hevc_videotoolbox"
    public static let h264Encoder = "h264_videotoolbox"
    public static let tenBitPixelFormat = "yuv420p10le"
    public static let eightBitPixelFormat = "yuv420p"
    public static let passthroughAudioCodecs: Set<String> = ["aac", "alac", "ac3", "eac3", "mp3"]

    public static func clampFpsLimit(_ value: Int) -> Int {
        return min(fpsLimitRange.upperBound, max(fpsLimitRange.lowerBound, value))
    }

    public static func videoCodec(hevcAvailable: Bool) -> String {
        return hevcAvailable ? hevcEncoder : h264Encoder
    }

    public static func pixelFormat(bitDepth: Int?, hevc: Bool) -> String {
        if hevc, (bitDepth ?? 8) > 8 {
            return tenBitPixelFormat
        }
        return eightBitPixelFormat
    }

    public static func makeEven(_ value: Int) -> Int {
        return value - (value % 2)
    }

    public static func scaleFilter(sourceHeight: Int?, targetHeight: Int) -> String? {
        let evenTarget = makeEven(targetHeight)
        if let sourceHeight, sourceHeight <= evenTarget {
            return nil
        }
        return "scale=-2:min(ih\\,\(evenTarget))"
    }

    public static func outputDimensions(
        sourceWidth: Int?,
        sourceHeight: Int?,
        targetHeight: Int
    ) -> (width: Int?, height: Int?) {
        let evenTarget = makeEven(targetHeight)
        guard let sourceHeight, sourceHeight > 0 else {
            return (sourceWidth, evenTarget)
        }
        if sourceHeight <= evenTarget {
            return (sourceWidth, sourceHeight)
        }
        guard let sourceWidth, sourceWidth > 0 else {
            return (nil, evenTarget)
        }
        let scaled = Int((Double(sourceWidth) * Double(evenTarget) / Double(sourceHeight)).rounded())
        return (makeEven(scaled), evenTarget)
    }

    public static func frameRateArgument(sourceFPS: Double?, fpsLimit: Int) -> Int? {
        let limit = clampFpsLimit(fpsLimit)
        guard let sourceFPS, sourceFPS > Double(limit) else {
            return nil
        }
        return limit
    }

    public static func videoBitrate(width: Int?, height: Int?, bitDepth: Int?, fps: Double?) -> Int {
        guard let width, let height, width > 0, height > 0 else {
            return minimumVideoBitrate
        }
        let bitsPerPixel = (bitDepth ?? 8) > 8 ? tenBitBitsPerPixel : eightBitBitsPerPixel
        let frameRate = fps ?? 30
        let bits = bitsPerPixel * Double(width) * Double(height) * frameRate
        return max(minimumVideoBitrate, Int(bits.rounded()))
    }

    public static func audioMode(codec: String?) -> TranscodeAudioMode {
        guard let codec, !codec.isEmpty else {
            return .drop
        }
        let normalized = codec.lowercased()
        if passthroughAudioCodecs.contains(normalized) || normalized == "lpcm" || normalized.hasPrefix("pcm_") {
            return .passthrough
        }
        return .reencode
    }

    public static func colorArguments(
        hevc: Bool,
        primaries: String?,
        transfer: String?,
        space: String?
    ) -> [String] {
        var arguments: [String] = []
        if hevc {
            if let primaries {
                arguments += ["-color_primaries", primaries]
            }
            if let transfer {
                arguments += ["-color_trc", transfer]
            }
            if let space {
                arguments += ["-colorspace", space]
            }
        } else {
            arguments += [
                "-color_primaries", "bt709",
                "-color_trc", "bt709",
                "-colorspace", "bt709"
            ]
        }
        return arguments
    }

    public static func audioArguments(_ mode: TranscodeAudioMode) -> [String] {
        switch mode {
        case .passthrough:
            return ["-c:a", "copy"]
        case .reencode:
            return [
                "-c:a", "aac",
                "-b:a", reencodeAudioBitrate,
                "-ar", String(reencodeAudioSampleRate),
                "-ac", String(reencodeAudioChannels)
            ]
        case .drop:
            return ["-an"]
        }
    }

    public static func arguments(
        input: URL,
        output: URL,
        source: MediaMetadata,
        parameters: TranscodeParameters,
        hevcAvailable: Bool,
        audioMode: TranscodeAudioMode
    ) -> [String] {
        let video = source.video
        let hevc = hevcAvailable
        var arguments = ["-hide_banner", "-loglevel", "error", "-y", "-i", input.path]

        arguments += ["-c:v", videoCodec(hevcAvailable: hevc)]
        arguments += ["-pix_fmt", pixelFormat(bitDepth: video?.bitDepth, hevc: hevc)]

        if let filter = scaleFilter(sourceHeight: video?.height, targetHeight: parameters.maxHeight) {
            arguments += ["-vf", filter]
        }

        let sourceFPS = video?.frameRate
        if let rate = frameRateArgument(sourceFPS: sourceFPS, fpsLimit: parameters.fpsLimit) {
            arguments += ["-r", String(rate)]
        }

        arguments += colorArguments(
            hevc: hevc,
            primaries: video?.colorPrimaries,
            transfer: video?.colorTransfer,
            space: video?.colorSpace
        )

        let dimensions = outputDimensions(
            sourceWidth: video?.width,
            sourceHeight: video?.height,
            targetHeight: parameters.maxHeight
        )
        let cappedFPS = min(sourceFPS ?? Double(parameters.fpsLimit), Double(parameters.fpsLimit))
        let bitrate = videoBitrate(
            width: dimensions.width,
            height: dimensions.height,
            bitDepth: video?.bitDepth,
            fps: cappedFPS
        )
        arguments += ["-b:v", String(bitrate)]

        if hevc {
            arguments += ["-tag:v", "hvc1"]
        }

        arguments += audioArguments(audioMode)
        arguments += ["-movflags", "+faststart"]
        arguments.append(output.path)
        return arguments
    }
}

public actor TranscodeService {
    private let resolver: any ToolExecutableResolving
    private let hevcAvailable: @Sendable () -> Bool
    private var activeSession: ProcessSession?
    private var cancelRequested = false

    public init(
        resolver: any ToolExecutableResolving = ToolLocator.system,
        hevcAvailable: @escaping @Sendable () -> Bool = VideoToolboxHEVCEncoder.isSupported
    ) {
        self.resolver = resolver
        self.hevcAvailable = hevcAvailable
    }

    public func transcode(
        input: URL,
        output: URL,
        source: MediaMetadata,
        parameters: TranscodeParameters
    ) async throws -> TranscodeResult {
        cancelRequested = false
        guard let ffmpeg = resolver.executableURL(for: .ffmpeg) else {
            throw LiveWallError.toolMissing(tool: MediaTool.ffmpeg.executableName)
        }
        let initialMode = TranscodeArguments.audioMode(codec: source.audio?.codec)
        do {
            return try await attempt(
                ffmpeg: ffmpeg,
                input: input,
                output: output,
                source: source,
                parameters: parameters,
                audioMode: initialMode
            )
        } catch let failure as TranscodeAttemptFailure {
            guard initialMode == .reencode else {
                throw LiveWallError.transcodeFailed(reason: failure.stderr)
            }
            do {
                return try await attempt(
                    ffmpeg: ffmpeg,
                    input: input,
                    output: output,
                    source: source,
                    parameters: parameters,
                    audioMode: .drop
                )
            } catch let retryFailure as TranscodeAttemptFailure {
                throw LiveWallError.transcodeFailed(reason: retryFailure.stderr)
            }
        }
    }

    public func cancelActive() {
        cancelRequested = true
        activeSession?.terminate()
    }

    private func attempt(
        ffmpeg: URL,
        input: URL,
        output: URL,
        source: MediaMetadata,
        parameters: TranscodeParameters,
        audioMode: TranscodeAudioMode
    ) async throws -> TranscodeResult {
        if cancelRequested {
            throw CancellationError()
        }
        let hevc = hevcAvailable()
        let arguments = TranscodeArguments.arguments(
            input: input,
            output: output,
            source: source,
            parameters: parameters,
            hevcAvailable: hevc,
            audioMode: audioMode
        )
        let session = try ProcessLauncher.launch(ProcessInvocation(executable: ffmpeg, arguments: arguments))
        activeSession = session
        for await _ in session.lines {}
        let result = try await session.wait()
        activeSession = nil

        if cancelRequested {
            try? FileManager.default.removeItem(at: output)
            throw CancellationError()
        }
        guard result.exitCode == 0, FileManager.default.fileExists(atPath: output.path) else {
            try? FileManager.default.removeItem(at: output)
            throw TranscodeAttemptFailure(stderr: result.stderr)
        }
        return TranscodeResult(
            outputURL: output,
            videoCodec: hevc ? "hevc" : "h264",
            audio: audioOutcome(for: audioMode)
        )
    }

    private func audioOutcome(for mode: TranscodeAudioMode) -> TranscodeAudioOutcome {
        switch mode {
        case .passthrough:
            return .passthrough
        case .reencode:
            return .reencoded
        case .drop:
            return .dropped(message: TranscodeArguments.dropAudioMessage)
        }
    }
}

private struct TranscodeAttemptFailure: Error {
    let stderr: String
}
