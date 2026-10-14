import AVFoundation
import CoreGraphics
import CryptoKit
import Foundation
import ImageIO
import UniformTypeIdentifiers
import LiveWallCore

public typealias ThumbnailDecoder = @Sendable (URL) async throws -> CGImage

public enum ThumbnailDecoders {
    public static let live: ThumbnailDecoder = { url in
        let fileExtension = url.pathExtension.lowercased()
        let type = UTType(filenameExtension: fileExtension)
        if fileExtension == "gif" || type?.conforms(to: .image) == true {
            return try decodeImageSource(url)
        }
        return try await decodeVideoFrame(url)
    }

    static func decodeImageSource(_ url: URL) throws -> CGImage {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              CGImageSourceGetCount(source) > 0 else {
            throw LiveWallError.mediaRejected(reason: "thumbnail decode failed: \(url.lastPathComponent)")
        }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: Int(ThumbnailService.maximumDimension)
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else {
            throw LiveWallError.mediaRejected(reason: "thumbnail decode failed: \(url.lastPathComponent)")
        }
        return image
    }

    static func decodeVideoFrame(_ url: URL) async throws -> CGImage {
        let asset = AVURLAsset(url: url)
        let generator = AVAssetImageGenerator(asset: asset)
        generator.appliesPreferredTrackTransform = true
        generator.maximumSize = CGSize(
            width: ThumbnailService.maximumDimension,
            height: ThumbnailService.maximumDimension
        )
        do {
            return try await generator.image(at: CMTime(value: 0, timescale: 600)).image
        } catch {
            throw LiveWallError.mediaRejected(reason: "thumbnail decode failed: \(url.lastPathComponent)")
        }
    }
}

public actor ThumbnailService {
    public static let maximumDimension: CGFloat = 480
    public static let defaultMaxConcurrent = 3

    private let cacheDirectory: URL
    private let decoder: ThumbnailDecoder
    private let limiter: ThumbnailConcurrencyLimiter

    public init(
        cacheDirectory: URL = LiveWallPaths.thumbsDirectory(),
        maxConcurrent: Int = ThumbnailService.defaultMaxConcurrent,
        decoder: @escaping ThumbnailDecoder = ThumbnailDecoders.live
    ) {
        self.cacheDirectory = cacheDirectory
        self.decoder = decoder
        self.limiter = ThumbnailConcurrencyLimiter(limit: max(1, maxConcurrent))
    }

    public func thumbnail(for itemID: String, source: URL) async throws -> URL {
        let cacheName = "\(Self.sanitize(itemID))-\(Self.contentKey(for: source)).jpg"
        let destination = cacheDirectory.appendingPathComponent(cacheName)
        if FileManager.default.fileExists(atPath: destination.path) {
            return destination
        }
        try FileManager.default.createDirectory(at: cacheDirectory, withIntermediateDirectories: true)

        await limiter.acquire()
        let data: Data
        do {
            data = try await render(source: source)
        } catch {
            await limiter.release()
            throw error
        }
        await limiter.release()

        try data.write(to: destination, options: .atomic)
        removeSiblings(itemID: itemID, keeping: destination)
        return destination
    }

    public func invalidate(itemID: String) throws {
        let prefix = "\(Self.sanitize(itemID))-"
        guard FileManager.default.fileExists(atPath: cacheDirectory.path) else {
            return
        }
        let contents = try FileManager.default.contentsOfDirectory(
            at: cacheDirectory,
            includingPropertiesForKeys: nil
        )
        for file in contents where file.lastPathComponent.hasPrefix(prefix) {
            try? FileManager.default.removeItem(at: file)
        }
    }

    private nonisolated func render(source: URL) async throws -> Data {
        let image: CGImage
        do {
            image = try await decoder(source)
        } catch let error as LiveWallError {
            throw error
        } catch {
            throw LiveWallError.mediaRejected(reason: "thumbnail decode failed: \(source.lastPathComponent)")
        }
        do {
            return try Self.jpegData(from: image, maxDimension: Self.maximumDimension)
        } catch let error as LiveWallError {
            throw error
        } catch {
            throw LiveWallError.mediaRejected(reason: "thumbnail encode failed: \(source.lastPathComponent)")
        }
    }

    private func removeSiblings(itemID: String, keeping destination: URL) {
        let prefix = "\(Self.sanitize(itemID))-"
        guard let contents = try? FileManager.default.contentsOfDirectory(
            at: cacheDirectory,
            includingPropertiesForKeys: nil
        ) else {
            return
        }
        for file in contents where file.lastPathComponent.hasPrefix(prefix)
            && file.lastPathComponent != destination.lastPathComponent {
            try? FileManager.default.removeItem(at: file)
        }
    }

    static func sanitize(_ itemID: String) -> String {
        let invalid = CharacterSet(charactersIn: "/\\:*?\"<>|").union(.controlCharacters).union(.newlines)
        let scalars = itemID.unicodeScalars.map { invalid.contains($0) ? "_" : $0 }
        var result = String(String.UnicodeScalarView(scalars)).trimmingCharacters(in: .whitespacesAndNewlines)
        while result.hasPrefix(".") {
            result.removeFirst()
        }
        return result.isEmpty ? "item" : result
    }

    static func contentKey(for source: URL) -> String {
        let attributes = try? FileManager.default.attributesOfItem(atPath: source.path)
        let size = (attributes?[.size] as? NSNumber)?.intValue ?? 0
        let modified = (attributes?[.modificationDate] as? Date)?.timeIntervalSince1970 ?? 0
        let seed = "\(source.standardizedFileURL.path)|\(size)|\(modified)"
        let digest = SHA256.hash(data: Data(seed.utf8))
        return digest.prefix(8).map { String(format: "%02x", $0) }.joined()
    }

    static func jpegData(from image: CGImage, maxDimension: CGFloat, quality: CGFloat = 0.82) throws -> Data {
        let scaled = try downscale(image, maxDimension: maxDimension)
        let output = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(
            output,
            UTType.jpeg.identifier as CFString,
            1,
            nil
        ) else {
            throw LiveWallError.mediaRejected(reason: "jpeg encoder unavailable")
        }
        CGImageDestinationAddImage(
            destination,
            scaled,
            [kCGImageDestinationLossyCompressionQuality: quality] as CFDictionary
        )
        guard CGImageDestinationFinalize(destination) else {
            throw LiveWallError.mediaRejected(reason: "jpeg encoding failed")
        }
        return output as Data
    }

    static func downscale(_ image: CGImage, maxDimension: CGFloat) throws -> CGImage {
        let width = CGFloat(image.width)
        let height = CGFloat(image.height)
        let longest = max(width, height)
        guard longest > maxDimension, longest > 0 else {
            return image
        }
        let scale = maxDimension / longest
        let targetWidth = max(1, Int((width * scale).rounded()))
        let targetHeight = max(1, Int((height * scale).rounded()))
        let colorSpace = CGColorSpaceCreateDeviceRGB()
        guard let context = CGContext(
            data: nil,
            width: targetWidth,
            height: targetHeight,
            bitsPerComponent: 8,
            bytesPerRow: 0,
            space: colorSpace,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else {
            throw LiveWallError.mediaRejected(reason: "thumbnail resize failed")
        }
        context.interpolationQuality = .high
        context.draw(image, in: CGRect(x: 0, y: 0, width: targetWidth, height: targetHeight))
        guard let scaled = context.makeImage() else {
            throw LiveWallError.mediaRejected(reason: "thumbnail resize failed")
        }
        return scaled
    }
}

private actor ThumbnailConcurrencyLimiter {
    private let limit: Int
    private var active = 0
    private var waiters: [CheckedContinuation<Void, Never>] = []

    init(limit: Int) {
        self.limit = limit
    }

    func acquire() async {
        if active < limit {
            active += 1
            return
        }
        await withCheckedContinuation { continuation in
            waiters.append(continuation)
        }
    }

    func release() {
        if waiters.isEmpty {
            active = max(0, active - 1)
        } else {
            waiters.removeFirst().resume()
        }
    }
}
