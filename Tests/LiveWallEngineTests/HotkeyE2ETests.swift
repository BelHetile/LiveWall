import Foundation
import Testing
@testable import LiveWallEngine

private final class HotkeyProbeBundleToken {}

@Suite(.serialized, .enabled(if: EngineE2E.enabled)) struct HotkeyE2ETests {
    private var probeURL: URL {
        return Bundle(for: HotkeyProbeBundleToken.self).bundleURL
            .deletingLastPathComponent()
            .appendingPathComponent("LiveWallHotkeyProbe")
    }

    @Test func syntheticComboFiresRegisteredHandler() throws {
        let probe = probeURL
        try #require(
            FileManager.default.isExecutableFile(atPath: probe.path),
            "hotkey probe not built at \(probe.path)"
        )

        let process = Process()
        process.executableURL = probe
        process.arguments = ["6", "2304"]
        let stdout = Pipe()
        process.standardOutput = stdout
        process.standardError = Pipe()

        try process.run()
        process.waitUntilExit()

        let data = stdout.fileHandleForReading.readDataToEndOfFile()
        let output = String(decoding: data, as: UTF8.self)

        #expect(process.terminationStatus == 0, "probe exited \(process.terminationStatus): \(output)")
        #expect(output.contains("HOTKEY FIRED action=next"))
    }
}
