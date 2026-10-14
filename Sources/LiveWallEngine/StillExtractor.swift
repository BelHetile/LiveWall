import AVFoundation
import CoreGraphics
import ImageIO
import LiveWallCore

public enum StillExtractor {
    public static func extract(from url: URL, kind: ItemKind, at time: CMTime = .zero) async throws -> CGImage {
        switch kind {
        case .video:
            return try await videoFrame(url: url, time: time)
        case .gif, .image:
            return try firstFrame(url: url)
        }
    }

    private static func videoFrame(url: URL, time: CMTime) async throws -> CGImage {
        let asset = AVURLAsset(url: url)
        let duration: CMTime
        do {
            duration = try await asset.load(.duration)
        } catch {
            throw LiveWallError.mediaUnreadable(path: url.path, reason: "unreadable asset")
        }

        let generator = AVAssetImageGenerator(asset: asset)
        generator.appliesPreferredTrackTransform = true
        generator.requestedTimeToleranceBefore = .positiveInfinity
        generator.requestedTimeToleranceAfter = .positiveInfinity

        let requested = time.seconds.isFinite ? time.seconds : 0
        let upperBound = max(0, duration.seconds - 0.001)
        let seconds = min(max(0, requested), upperBound)
        do {
            return try await generator.image(at: CMTime(seconds: seconds, preferredTimescale: 600)).image
        } catch {
            throw LiveWallError.mediaUnreadable(path: url.path, reason: "frame extraction failed")
        }
    }

    private static func firstFrame(url: URL) throws -> CGImage {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              CGImageSourceGetCount(source) > 0,
              let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
            throw LiveWallError.mediaUnreadable(path: url.path, reason: "unreadable image")
        }
        return image
    }
}
