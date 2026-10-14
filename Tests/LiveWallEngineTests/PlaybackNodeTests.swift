import AVFoundation
import AppKit
import CoreGraphics
import LiveWallCore
import Testing
@testable import LiveWallEngine

@Suite struct VideoNodeTests {
    @Test @MainActor func loadsValidClips() async throws {
        let hevc = try await VideoNode(url: Fixtures.hevcClip)
        #expect(hevc.isPlaying == false)
        #expect(hevc.currentTime.seconds == 0)
        #expect(hevc.videoGravity == .resizeAspectFill)

        let h264 = try await VideoNode(url: Fixtures.h264Clip)
        #expect(h264.asset.duration.seconds > 0)
    }

    @Test @MainActor func playAndPauseToggleRate() async throws {
        let node = try await VideoNode(url: Fixtures.hevcClip)
        node.play()
        #expect(node.isPlaying)
        #expect(node.player.rate != 0)
        node.pause()
        #expect(node.isPlaying == false)
        #expect(node.player.rate == 0)
    }

    @Test @MainActor func configuresAudioAndRate() async throws {
        let node = try await VideoNode(url: Fixtures.hevcClip, muted: true, volume: 0.5, rate: 1.5)
        #expect(node.isMuted)
        #expect(node.volume == 0.5)
        #expect(node.rate == 1.5)
        node.setMuted(false)
        node.setVolume(2.0)
        #expect(node.isMuted == false)
        #expect(node.volume == 1.0)
        node.setRate(-1)
        #expect(node.rate == 1.0)
    }

    @Test @MainActor func rejectsCorruptFile() async throws {
        await #expect(throws: LiveWallError.mediaUnreadable(path: Fixtures.corrupt.path, reason: "truncated sample data")) {
            _ = try await VideoNode(url: Fixtures.corrupt)
        }
    }

    @Test @MainActor func rejectsZeroByteFile() async throws {
        await #expect(throws: LiveWallError.mediaUnreadable(path: Fixtures.zeroBytes.path, reason: "empty file")) {
            _ = try await VideoNode(url: Fixtures.zeroBytes)
        }
    }

    @Test @MainActor func rejectsAudioOnlyFile() async throws {
        await #expect(throws: LiveWallError.mediaUnreadable(path: Fixtures.audioOnly.path, reason: "no video track")) {
            _ = try await VideoNode(url: Fixtures.audioOnly)
        }
    }

    @Test @MainActor func rejectsUndecodableCodec() async throws {
        await #expect(throws: LiveWallError.codecUnsupported(path: Fixtures.av1Clip.path)) {
            _ = try await VideoNode(url: Fixtures.av1Clip)
        }
    }
}

@Suite struct ImageNodeTests {
    @Test @MainActor func loadsImagePassthrough() throws {
        let node = try ImageNode(url: Fixtures.still)
        #expect(node.image.width == 320)
        #expect(node.image.height == 240)
        #expect(node.isPlaying == false)
        node.play()
        node.pause()
        #expect(node.isPlaying == false)
    }

    @Test @MainActor func rejectsEmptyFile() throws {
        #expect(throws: LiveWallError.self) {
            _ = try ImageNode(url: Fixtures.zeroBytes)
        }
    }

    @Test @MainActor func rejectsNonImageFile() throws {
        #expect(throws: LiveWallError.self) {
            _ = try ImageNode(url: Fixtures.corrupt)
        }
    }
}
