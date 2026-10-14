import CoreGraphics
import Foundation
import ImageIO
import LiveWallCore
import Testing
@testable import LiveWallEngine

@Suite struct GifTimelineTests {
    @Test func readsNativeDelays() throws {
        let source = try #require(CGImageSourceCreateWithURL(Fixtures.animation as CFURL, nil))
        let timeline = GifTimeline.read(from: source)
        #expect(timeline.frameCount == 8)
        #expect(timeline.frameDelays.allSatisfy { abs($0 - 0.1) < 0.001 })
        #expect(abs(timeline.totalDuration - 0.8) < 0.001)
    }

    @Test func reportsLoopCount() throws {
        let source = try #require(CGImageSourceCreateWithURL(Fixtures.animation as CFURL, nil))
        #expect(GifTimeline.loopCount(source: source) == 0)
    }

    @Test func outOfRangeDelayFallsBack() {
        let timeline = GifTimeline(frameDelays: [0.05, 0.25], loopCount: 0)
        #expect(timeline.delay(at: 0) == 0.05)
        #expect(timeline.delay(at: 1) == 0.25)
        #expect(timeline.delay(at: 5) == GifTimeline.defaultFrameDelay)
        #expect(timeline.delay(at: -1) == GifTimeline.defaultFrameDelay)
    }
}

@Suite struct GifFrameCacheTests {
    private func makeImage(side: Int) throws -> CGImage {
        let context = try #require(CGContext(
            data: nil,
            width: side,
            height: side,
            bitsPerComponent: 8,
            bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ))
        return try #require(context.makeImage())
    }

    @Test @MainActor func boundsByFrameCount() throws {
        let image = try makeImage(side: 8)
        let cache = GifFrameCache(maxFrames: 3, maxBytes: .max) { _ in image }
        for index in 0..<50 {
            _ = cache.image(at: index)
            #expect(cache.cachedFrameCount <= 3)
        }
        #expect(cache.cachedFrameCount == 3)
    }

    @Test @MainActor func boundsByBytes() throws {
        let image = try makeImage(side: 8)
        let perFrame = GifFrameCache.decodedByteSize(image)
        let cache = GifFrameCache(maxFrames: 1000, maxBytes: perFrame * 2) { _ in image }
        for index in 0..<20 {
            _ = cache.image(at: index)
            #expect(cache.cachedBytes <= perFrame * 2)
            #expect(cache.cachedFrameCount <= 2)
        }
    }

    @Test @MainActor func keepsFrameNearestCurrent() throws {
        let image = try makeImage(side: 8)
        let cache = GifFrameCache(maxFrames: 2, maxBytes: .max) { _ in image }
        _ = cache.image(at: 0)
        _ = cache.image(at: 1)
        _ = cache.image(at: 10)
        _ = cache.image(at: 9)
        #expect(cache.cachedFrameCount == 2)
        #expect(cache.image(at: 10) != nil)
    }

    @Test @MainActor func oversizedFrameIsNotCached() throws {
        let oversized = try makeImage(side: 64)
        let small = try makeImage(side: 8)
        let cache = GifFrameCache(maxFrames: 10, maxBytes: GifFrameCache.decodedByteSize(small)) { _ in oversized }
        let returned = cache.image(at: 0)
        #expect(returned != nil)
        #expect(cache.cachedFrameCount == 0)
        #expect(cache.cachedBytes == 0)
    }

    @Test @MainActor func nilDecodeReturnsNil() throws {
        let cache = GifFrameCache(maxFrames: 4, maxBytes: .max) { _ in nil }
        #expect(cache.image(at: 0) == nil)
        #expect(cache.cachedFrameCount == 0)
    }
}

@Suite struct GifNodeTests {
    @Test @MainActor func readsTimelineOnInit() throws {
        let node = try GifNode(url: Fixtures.animation)
        defer { node.pause() }
        #expect(node.timeline.frameCount == 8)
        #expect(node.currentFrameIndex == 0)
        #expect(node.isPlaying == false)
    }

    @Test @MainActor func rejectsEmptyFile() throws {
        #expect(throws: LiveWallError.self) {
            _ = try GifNode(url: Fixtures.zeroBytes)
        }
    }

    @Test @MainActor func rejectsNonImageFile() throws {
        #expect(throws: LiveWallError.self) {
            _ = try GifNode(url: Fixtures.corrupt)
        }
    }

    @Test @MainActor func stepsFramesWhilePlayingAndHoldsWhenPaused() throws {
        let node = try GifNode(url: Fixtures.animation)
        defer { node.pause() }
        node.play()
        #expect(node.isPlaying)

        let deadline = Date().addingTimeInterval(1.0)
        while node.currentFrameIndex == 0 && Date() < deadline {
            pumpRunLoop(0.05)
        }
        #expect(node.currentFrameIndex != 0)

        node.pause()
        #expect(node.isPlaying == false)
        let held = node.currentFrameIndex
        pumpRunLoop(0.3)
        #expect(node.currentFrameIndex == held)
    }
}
