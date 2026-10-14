import AVFoundation
import CoreGraphics
import CryptoKit
import Foundation
import ImageIO
import LiveWallCore
import UniformTypeIdentifiers

public struct StillRequest: Equatable, Sendable {
    public let url: URL
    public let kind: ItemKind
    public let stillTime: Double

    public init(url: URL, kind: ItemKind, stillTime: Double = 0) {
        self.url = url
        self.kind = kind
        self.stillTime = stillTime
    }
}

@MainActor
public final class StillRenderer {
    public let directory: URL

    public init(directory: URL) {
        self.directory = directory
    }

    public func stillFileURL(for request: StillRequest) -> URL {
        let key = "\(request.url.standardizedFileURL.path)|\(request.kind.rawValue)|\(request.stillTime)"
        let digest = SHA256.hash(data: Data(key.utf8))
        let name = digest.map { String(format: "%02x", $0) }.joined()
        return directory.appendingPathComponent("still-\(name).png")
    }

    @discardableResult
    public func render(_ request: StillRequest) async throws -> URL {
        let file = stillFileURL(for: request)
        if FileManager.default.fileExists(atPath: file.path) {
            return file
        }
        let time = CMTime(seconds: request.stillTime, preferredTimescale: 600)
        let image = try await StillExtractor.extract(from: request.url, kind: request.kind, at: time)
        let data = try StillRenderer.pngData(from: image, path: request.url.path)
        try AtomicJSON.writeData(data, to: file)
        return file
    }

    private static func pngData(from image: CGImage, path: String) throws -> Data {
        let data = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(
            data,
            UTType.png.identifier as CFString,
            1,
            nil
        ) else {
            throw LiveWallError.stillUnavailable(path: path, reason: "png encoder unavailable")
        }
        CGImageDestinationAddImage(destination, image, nil)
        guard CGImageDestinationFinalize(destination) else {
            throw LiveWallError.stillUnavailable(path: path, reason: "png encode failed")
        }
        return data as Data
    }
}
