import Foundation
import Testing
@testable import LiveWallEngine

private final class ProbeBundleToken {}

@Suite(.enabled(if: EngineE2E.enabled)) struct PlaybackE2ETests {
    private var probeURL: URL {
        return Bundle(for: ProbeBundleToken.self).bundleURL
            .deletingLastPathComponent()
            .appendingPathComponent("LiveWallPlaybackProbe")
    }

    @Test func videoPlaysInWallpaperWindow() throws {
        let probe = probeURL
        try #require(
            FileManager.default.isExecutableFile(atPath: probe.path),
            "playback probe not built at \(probe.path)"
        )

        let process = Process()
        process.executableURL = probe
        process.arguments = [Fixtures.hevcClip.path, "3"]
        let stdout = Pipe()
        process.standardOutput = stdout
        process.standardError = Pipe()

        try process.run()
        process.waitUntilExit()

        let data = stdout.fileHandleForReading.readDataToEndOfFile()
        let output = String(decoding: data, as: UTF8.self)

        #expect(process.terminationStatus == 0, "probe exited \(process.terminationStatus): \(output)")
        #expect(output.contains("PLAYBACK"))
        #expect(output.contains("ready=true"))
        #expect(output.contains("status=2"))
    }
}
