import AVFoundation
import CoreGraphics
import LiveWallCore
import Testing
@testable import LiveWallEngine

@Suite struct StillExtractorTests {
    @Test func extractsVideoFrameAtTime() async throws {
        let atStart = try await StillExtractor.extract(from: Fixtures.hevcClip, kind: .video, at: .zero)
        #expect(atStart.width > 0)
        #expect(atStart.height > 0)

        let midway = try await StillExtractor.extract(
            from: Fixtures.hevcClip,
            kind: .video,
            at: CMTime(seconds: 1, preferredTimescale: 600)
        )
        #expect(midway.width > 0)
        #expect(midway.height > 0)
    }

    @Test func clampsTimeBeyondDuration() async throws {
        let image = try await StillExtractor.extract(
            from: Fixtures.h264Clip,
            kind: .video,
            at: CMTime(seconds: 999, preferredTimescale: 600)
        )
        #expect(image.width > 0)
    }

    @Test func extractsGifFirstFrame() async throws {
        let image = try await StillExtractor.extract(from: Fixtures.animation, kind: .gif)
        #expect(image.width == 160)
        #expect(image.height == 120)
    }

    @Test func extractsImagePassthrough() async throws {
        let image = try await StillExtractor.extract(from: Fixtures.still, kind: .image)
        #expect(image.width == 320)
        #expect(image.height == 240)
    }

    @Test func rejectsEmptyVideo() async {
        await #expect(throws: LiveWallError.self) {
            _ = try await StillExtractor.extract(from: Fixtures.zeroBytes, kind: .video)
        }
    }

    @Test func rejectsEmptyImage() async {
        await #expect(throws: LiveWallError.self) {
            _ = try await StillExtractor.extract(from: Fixtures.zeroBytes, kind: .image)
        }
    }
}
