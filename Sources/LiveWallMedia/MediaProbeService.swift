import AVFoundation
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers
import LiveWallCore
import LiveWallTools

public enum MediaKind: String, Sendable, Codable, Equatable {
    case video
    case image
    case animatedImage
}

public enum MediaVerdict: Sendable, Equatable {
    case native
    case transcodeToHEVC(warning: String)
    case rejected(reason: String)
}

public struct MediaProbeResult: Sendable, Equatable {
    public let kind: MediaKind?
    public let verdict: MediaVerdict
    public let metadata: MediaMetadata?

    public init(kind: MediaKind?, verdict: MediaVerdict, metadata: MediaMetadata?) {
        self.kind = kind
        self.verdict = verdict
        self.metadata = metadata
    }
}

public struct MediaProbeService: Sendable {
    public static let av1TranscodeWarning =
        "This video uses AV1, which this Mac has no hardware decoder for. It will be converted to HEVC so it plays smoothly."

    public static let acceptedVideoExtensions: Set<String> = ["mp4", "mov", "m4v", "webm", "mkv"]
    public static let rejectedVideoExtensions: Set<String> = ["avi", "flv", "wmv", "ogv"]

    private let resolver: any ToolExecutableResolving
    private let runner: ProcessRunner
    private let av1HardwareSupport: @Sendable () -> Bool

    public init(
        resolver: any ToolExecutableResolving = ToolLocator.system,
        runner: ProcessRunner = ProcessRunner(),
        av1HardwareSupport: @escaping @Sendable () -> Bool = AV1HardwareSupport.isSupported
    ) {
        self.resolver = resolver
        self.runner = runner
        self.av1HardwareSupport = av1HardwareSupport
    }

    public func probe(_ url: URL) async throws -> MediaProbeResult {
        let fileManager = FileManager.default
        var isDirectory: ObjCBool = false
        guard fileManager.fileExists(atPath: url.path, isDirectory: &isDirectory), !isDirectory.boolValue else {
            return MediaProbeResult(kind: nil, verdict: .rejected(reason: "file not found"), metadata: nil)
        }
        let attributes = try? fileManager.attributesOfItem(atPath: url.path)
        let size = (attributes?[.size] as? NSNumber)?.intValue ?? 0
        guard size > 0 else {
            return MediaProbeResult(kind: nil, verdict: .rejected(reason: "empty file"), metadata: nil)
        }

        let fileExtension = url.pathExtension.lowercased()
        if Self.rejectedVideoExtensions.contains(fileExtension) {
            return MediaProbeResult(kind: nil, verdict: .rejected(reason: "unsupported container: \(fileExtension)"), metadata: nil)
        }
        let type = UTType(filenameExtension: fileExtension)
        if fileExtension == "gif" || type?.conforms(to: .image) == true {
            return probeImage(url)
        }
        if type?.conforms(to: .movie) == true
            || type?.conforms(to: .video) == true
            || Self.acceptedVideoExtensions.contains(fileExtension) {
            return try await probeVideo(url)
        }
        if type?.conforms(to: .audio) == true {
            return MediaProbeResult(kind: nil, verdict: .rejected(reason: "audio-only files are not supported"), metadata: nil)
        }
        return MediaProbeResult(kind: nil, verdict: .rejected(reason: "unsupported format"), metadata: nil)
    }

    public func probeOrThrow(_ url: URL) async throws -> MediaProbeResult {
        let result = try await probe(url)
        if case let .rejected(reason) = result.verdict {
            throw LiveWallError.mediaRejected(reason: reason)
        }
        return result
    }

    public func readMetadata(_ url: URL) async throws -> MediaMetadata {
        guard let ffprobe = resolver.ffprobeExecutableURL() else {
            throw LiveWallError.toolMissing(tool: "ffprobe")
        }
        let invocation = ProcessInvocation(
            executable: ffprobe,
            arguments: ["-v", "error", "-print_format", "json", "-show_format", "-show_streams", url.path]
        )
        let result = try await runner.run(invocation)
        guard result.exitCode == 0, !result.stdout.isEmpty else {
            throw LiveWallError.mediaRejected(reason: "ffprobe could not read media")
        }
        return try FFProbeMetadataParser.parse(json: result.stdout)
    }

    private func probeImage(_ url: URL) -> MediaProbeResult {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              CGImageSourceGetCount(source) > 0,
              let typeIdentifier = CGImageSourceGetType(source) else {
            return MediaProbeResult(kind: nil, verdict: .rejected(reason: "image decode failed"), metadata: nil)
        }
        let type = UTType(typeIdentifier as String)
        guard type?.conforms(to: .image) == true else {
            return MediaProbeResult(kind: nil, verdict: .rejected(reason: "unsupported image format"), metadata: nil)
        }
        let kind: MediaKind = type?.conforms(to: .gif) == true ? .animatedImage : .image
        return MediaProbeResult(kind: kind, verdict: .native, metadata: nil)
    }

    private func probeVideo(_ url: URL) async throws -> MediaProbeResult {
        let metadata: MediaMetadata
        do {
            metadata = try await readMetadata(url)
        } catch let error as LiveWallError {
            if case .toolMissing = error {
                throw error
            }
            return MediaProbeResult(kind: .video, verdict: .rejected(reason: "corrupt or unreadable media"), metadata: nil)
        }
        guard let video = metadata.video else {
            return MediaProbeResult(kind: .video, verdict: .rejected(reason: "audio-only files are not supported"), metadata: metadata)
        }
        if video.codec == "av1" {
            let verdict: MediaVerdict = av1HardwareSupport()
                ? .native
                : .transcodeToHEVC(warning: Self.av1TranscodeWarning)
            return MediaProbeResult(kind: .video, verdict: verdict, metadata: metadata)
        }
        do {
            try await Self.validateDecodable(url, video: video, duration: metadata.duration)
        } catch {
            return MediaProbeResult(kind: .video, verdict: .rejected(reason: "cannot decode video track"), metadata: metadata)
        }
        return MediaProbeResult(kind: .video, verdict: .native, metadata: metadata)
    }

    static func validateDecodable(_ url: URL, video: VideoMetadata, duration: Double?) async throws {
        let asset = AVURLAsset(url: url)
        let tracks = try await asset.loadTracks(withMediaType: .video)
        guard let track = tracks.first else {
            throw LiveWallError.mediaRejected(reason: "no video track")
        }
        let reader = try AVAssetReader(asset: asset)
        let output = AVAssetReaderTrackOutput(track: track, outputSettings: nil)
        guard reader.canAdd(output) else {
            throw LiveWallError.mediaRejected(reason: "no reader output")
        }
        reader.add(output)
        guard reader.startReading() else {
            throw LiveWallError.mediaRejected(reason: "cannot read media")
        }
        var sampleCount = 0
        while output.copyNextSampleBuffer() != nil {
            sampleCount += 1
        }
        guard reader.status != .failed else {
            throw LiveWallError.mediaRejected(reason: "decode failed")
        }
        guard sampleCount > 0 else {
            throw LiveWallError.mediaRejected(reason: "no decodable frames")
        }
        let expected = video.frameCount ?? Int((duration ?? 0) * (video.frameRate ?? 0))
        if expected > 1, sampleCount * 2 < expected {
            throw LiveWallError.mediaRejected(reason: "truncated media")
        }
    }
}
