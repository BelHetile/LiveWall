import Foundation
import LiveWallCore

public enum HDRFormat: String, Sendable, Codable, Equatable {
    case pq
    case hlg
}

public struct VideoMetadata: Sendable, Equatable, Codable {
    public let codec: String
    public let profile: String?
    public let pixelFormat: String?
    public let bitDepth: Int?
    public let colorSpace: String?
    public let colorPrimaries: String?
    public let colorTransfer: String?
    public let hdrFormat: HDRFormat?
    public let width: Int?
    public let height: Int?
    public let frameRate: Double?
    public let frameCount: Int?

    public init(
        codec: String,
        profile: String?,
        pixelFormat: String?,
        bitDepth: Int?,
        colorSpace: String?,
        colorPrimaries: String?,
        colorTransfer: String?,
        hdrFormat: HDRFormat?,
        width: Int?,
        height: Int?,
        frameRate: Double?,
        frameCount: Int?
    ) {
        self.codec = codec
        self.profile = profile
        self.pixelFormat = pixelFormat
        self.bitDepth = bitDepth
        self.colorSpace = colorSpace
        self.colorPrimaries = colorPrimaries
        self.colorTransfer = colorTransfer
        self.hdrFormat = hdrFormat
        self.width = width
        self.height = height
        self.frameRate = frameRate
        self.frameCount = frameCount
    }
}

public struct AudioMetadata: Sendable, Equatable, Codable {
    public let codec: String
    public let profile: String?
    public let channels: Int?
    public let sampleRate: Int?

    public init(codec: String, profile: String?, channels: Int?, sampleRate: Int?) {
        self.codec = codec
        self.profile = profile
        self.channels = channels
        self.sampleRate = sampleRate
    }
}

public struct MediaMetadata: Sendable, Equatable, Codable {
    public let container: String?
    public let duration: Double?
    public let bitRate: Int?
    public let video: VideoMetadata?
    public let audio: AudioMetadata?

    public init(
        container: String?,
        duration: Double?,
        bitRate: Int?,
        video: VideoMetadata?,
        audio: AudioMetadata?
    ) {
        self.container = container
        self.duration = duration
        self.bitRate = bitRate
        self.video = video
        self.audio = audio
    }
}

public enum FFProbeMetadataParser {
    public static func parse(json: String) throws -> MediaMetadata {
        guard let data = json.data(using: .utf8) else {
            throw LiveWallError.mediaRejected(reason: "ffprobe output not utf8")
        }
        return try parse(data)
    }

    public static func parse(_ data: Data) throws -> MediaMetadata {
        let decoded: FFProbeOutput
        do {
            decoded = try JSONDecoder().decode(FFProbeOutput.self, from: data)
        } catch {
            throw LiveWallError.mediaRejected(reason: "unrecognized ffprobe output")
        }
        let streams = decoded.streams ?? []
        let videoStream = streams.first { $0.codecType == "video" }
        let audioStream = streams.first { $0.codecType == "audio" }
        let format = decoded.format

        let video = videoStream.flatMap { stream -> VideoMetadata? in
            guard let codec = stream.codecName else {
                return nil
            }
            let transfer = stream.colorTransfer
            return VideoMetadata(
                codec: codec,
                profile: stream.profile,
                pixelFormat: stream.pixelFormat,
                bitDepth: bitDepth(
                    pixelFormat: stream.pixelFormat,
                    bitsPerRawSample: stream.bitsPerRawSample,
                    profile: stream.profile
                ),
                colorSpace: stream.colorSpace,
                colorPrimaries: stream.colorPrimaries,
                colorTransfer: transfer,
                hdrFormat: hdrFormat(transfer: transfer),
                width: stream.width,
                height: stream.height,
                frameRate: parseRational(stream.avgFrameRate) ?? parseRational(stream.rFrameRate),
                frameCount: stream.nbFrames.flatMap { Int($0) }
            )
        }

        let audio = audioStream.flatMap { stream -> AudioMetadata? in
            guard let codec = stream.codecName else {
                return nil
            }
            return AudioMetadata(
                codec: codec,
                profile: stream.profile,
                channels: stream.channels,
                sampleRate: stream.sampleRate.flatMap { Int($0) }
            )
        }

        return MediaMetadata(
            container: format?.formatName,
            duration: format?.duration.flatMap { Double($0) },
            bitRate: format?.bitRate.flatMap { Int($0) },
            video: video,
            audio: audio
        )
    }

    static func hdrFormat(transfer: String?) -> HDRFormat? {
        switch transfer {
        case "smpte2084":
            return .pq
        case "arib-std-b67":
            return .hlg
        default:
            return nil
        }
    }

    static func bitDepth(pixelFormat: String?, bitsPerRawSample: String?, profile: String?) -> Int? {
        if let pixelFormat,
           let range = pixelFormat.range(of: "p[0-9]+", options: .regularExpression),
           let value = Int(pixelFormat[range].dropFirst()) {
            return value
        }
        if let bitsPerRawSample, let value = Int(bitsPerRawSample), value > 0 {
            return value
        }
        if let profile, profile.contains("10") {
            return 10
        }
        if let profile, profile.contains("12") {
            return 12
        }
        return pixelFormat == nil && profile == nil ? nil : 8
    }

    static func parseRational(_ value: String?) -> Double? {
        guard let value else {
            return nil
        }
        let parts = value.split(separator: "/")
        if parts.count == 2, let numerator = Double(parts[0]), let denominator = Double(parts[1]), denominator != 0 {
            return numerator / denominator
        }
        return Double(value)
    }
}

private struct FFProbeOutput: Decodable {
    let streams: [FFProbeStream]?
    let format: FFProbeFormat?
}

private struct FFProbeStream: Decodable {
    let codecName: String?
    let codecType: String?
    let profile: String?
    let width: Int?
    let height: Int?
    let pixelFormat: String?
    let rFrameRate: String?
    let avgFrameRate: String?
    let colorTransfer: String?
    let colorPrimaries: String?
    let colorSpace: String?
    let sampleRate: String?
    let channels: Int?
    let bitsPerRawSample: String?
    let nbFrames: String?

    enum CodingKeys: String, CodingKey {
        case codecName = "codec_name"
        case codecType = "codec_type"
        case profile
        case width
        case height
        case pixelFormat = "pix_fmt"
        case rFrameRate = "r_frame_rate"
        case avgFrameRate = "avg_frame_rate"
        case colorTransfer = "color_transfer"
        case colorPrimaries = "color_primaries"
        case colorSpace = "color_space"
        case sampleRate = "sample_rate"
        case channels
        case bitsPerRawSample = "bits_per_raw_sample"
        case nbFrames = "nb_frames"
    }
}

private struct FFProbeFormat: Decodable {
    let formatName: String?
    let duration: String?
    let bitRate: String?

    enum CodingKeys: String, CodingKey {
        case formatName = "format_name"
        case duration
        case bitRate = "bit_rate"
    }
}
