import CoreGraphics
import Foundation
import ImageIO
import Testing
import LiveWallCore
@testable import LiveWallMedia

private let repoRoot = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent()
    .deletingLastPathComponent()
    .deletingLastPathComponent()
private let fixturesDirectory = repoRoot.appendingPathComponent("Tests/Fixtures", isDirectory: true)

private func temporaryDirectory(_ label: String) -> URL {
    let url = FileManager.default.temporaryDirectory
        .appendingPathComponent("lwthumb-\(label)-\(UUID().uuidString)", isDirectory: true)
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

private func imageDimensions(_ url: URL) -> (width: Int, height: Int)? {
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
        return nil
    }
    return (image.width, image.height)
}

private func cachedFiles(in directory: URL, prefix: String) -> [String] {
    let contents = (try? FileManager.default.contentsOfDirectory(atPath: directory.path)) ?? []
    return contents.filter { $0.hasPrefix(prefix) }
}

private final class ConcurrencyRecorder: @unchecked Sendable {
    private let lock = NSLock()
    private var active = 0
    private var peak = 0

    func enter() {
        lock.lock()
        active += 1
        peak = max(peak, active)
        lock.unlock()
    }

    func leave() {
        lock.lock()
        active -= 1
        lock.unlock()
    }

    var peakValue: Int {
        lock.lock()
        defer { lock.unlock() }
        return peak
    }
}

@Suite struct ThumbnailServiceTests {
    @Test func writesAndReadsCache() async throws {
        let cache = temporaryDirectory("cache")
        let service = ThumbnailService(cacheDirectory: cache)
        let source = fixturesDirectory.appendingPathComponent("still.png")

        let first = try await service.thumbnail(for: "item-1", source: source)
        #expect(FileManager.default.fileExists(atPath: first.path))
        #expect(first.pathExtension == "jpg")
        let firstAttributes = try FileManager.default.attributesOfItem(atPath: first.path)
        let firstModified = firstAttributes[.modificationDate] as? Date

        let second = try await service.thumbnail(for: "item-1", source: source)
        #expect(second == first)
        let secondModified = try FileManager.default.attributesOfItem(atPath: second.path)[.modificationDate] as? Date
        #expect(secondModified == firstModified)
    }

    @Test func gifThumbnailDecodesFirstFrame() async throws {
        let cache = temporaryDirectory("gif")
        let service = ThumbnailService(cacheDirectory: cache)
        let source = fixturesDirectory.appendingPathComponent("anim.gif")

        let url = try await service.thumbnail(for: "gif-item", source: source)
        let dimensions = try #require(imageDimensions(url))
        #expect(dimensions.width == 160)
        #expect(dimensions.height == 120)
        #expect(max(dimensions.width, dimensions.height) <= Int(ThumbnailService.maximumDimension))
    }

    @Test func downscalesOversizedImages() async throws {
        let cache = temporaryDirectory("downscale")
        let large = CGContext(
            data: nil,
            width: 1000,
            height: 600,
            bitsPerComponent: 8,
            bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        )!.makeImage()!
        let service = ThumbnailService(cacheDirectory: cache, decoder: { _ in large })

        let source = fixturesDirectory.appendingPathComponent("still.png")
        let url = try await service.thumbnail(for: "large-item", source: source)
        let dimensions = try #require(imageDimensions(url))
        #expect(max(dimensions.width, dimensions.height) == Int(ThumbnailService.maximumDimension))
        #expect(dimensions.width == 480)
        #expect(dimensions.height == 288)
    }

    @Test func invalidateRemovesCachedFiles() async throws {
        let cache = temporaryDirectory("invalidate")
        let service = ThumbnailService(cacheDirectory: cache)
        let source = fixturesDirectory.appendingPathComponent("still.png")

        let url = try await service.thumbnail(for: "item-2", source: source)
        #expect(FileManager.default.fileExists(atPath: url.path))

        try await service.invalidate(itemID: "item-2")
        #expect(!FileManager.default.fileExists(atPath: url.path))
        #expect(cachedFiles(in: cache, prefix: "item-2-").isEmpty)
    }

    @Test func replacementLeavesOnlyFreshThumbnail() async throws {
        let cache = temporaryDirectory("replace")
        let service = ThumbnailService(cacheDirectory: cache)
        let sourceDir = temporaryDirectory("replace-source")
        let source = sourceDir.appendingPathComponent("media.png")
        try FileManager.default.copyItem(at: fixturesDirectory.appendingPathComponent("still.png"), to: source)

        let first = try await service.thumbnail(for: "item-3", source: source)
        try FileManager.default.removeItem(at: source)
        try FileManager.default.copyItem(at: fixturesDirectory.appendingPathComponent("anim.gif"), to: source)
        let second = try await service.thumbnail(for: "item-3", source: source)

        #expect(second != first)
        #expect(!FileManager.default.fileExists(atPath: first.path))
        #expect(FileManager.default.fileExists(atPath: second.path))
        #expect(cachedFiles(in: cache, prefix: "item-3-").count == 1)
    }

    @Test func decodeFailureThrowsAndWritesNothing() async throws {
        let cache = temporaryDirectory("failure")
        let service = ThumbnailService(cacheDirectory: cache, decoder: { _ in
            throw LiveWallError.mediaRejected(reason: "decode failed")
        })
        let source = fixturesDirectory.appendingPathComponent("still.png")

        await #expect(throws: LiveWallError.self) {
            try await service.thumbnail(for: "broken-item", source: source)
        }
        #expect(cachedFiles(in: cache, prefix: "broken-item-").isEmpty)
    }

    @Test func respectsConcurrencyCap() async throws {
        let cache = temporaryDirectory("concurrency")
        let sources = temporaryDirectory("concurrency-source")
        let recorder = ConcurrencyRecorder()
        let service = ThumbnailService(cacheDirectory: cache, maxConcurrent: 3, decoder: { url in
            recorder.enter()
            defer { recorder.leave() }
            try await Task.sleep(for: .milliseconds(120))
            return try ThumbnailDecoders.decodeImageSource(url)
        })
        let png = fixturesDirectory.appendingPathComponent("still.png")
        var urls: [URL] = []
        for index in 0..<6 {
            let target = sources.appendingPathComponent("source-\(index).png")
            try FileManager.default.copyItem(at: png, to: target)
            urls.append(target)
        }

        await withTaskGroup(of: Void.self) { group in
            for (index, source) in urls.enumerated() {
                group.addTask {
                    _ = try? await service.thumbnail(for: "cap-\(index)", source: source)
                }
            }
        }

        #expect(recorder.peakValue == 3)
    }
}
