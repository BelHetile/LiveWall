import AppKit
import Darwin
import Foundation
import Testing
@testable import LiveWallCore
@testable import LiveWallEngine

private final class StillProbeBundleToken {}

@Suite(.serialized, .enabled(if: EngineE2E.enabled)) @MainActor struct StillSyncE2ETests {
    private var probeURL: URL {
        return Bundle(for: StillProbeBundleToken.self).bundleURL
            .deletingLastPathComponent()
            .appendingPathComponent("LiveWallStillProbe")
    }

    private func screen(for displayID: UInt32) -> NSScreen? {
        return NSScreen.screens.first { ScreenDescriptor.displayID(of: $0) == displayID }
    }

    private func captureOriginals() -> [(UInt32, URL?)] {
        return NSScreen.screens.compactMap { screen in
            guard let id = ScreenDescriptor.displayID(of: screen) else {
                return nil
            }
            return (id, NSWorkspace.shared.desktopImageURL(for: screen))
        }
    }

    private func waitUntil(timeout: TimeInterval = 5, _ condition: () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition() {
                return true
            }
            pumpRunLoop(0.05)
        }
        return condition()
    }

    private func restoreOriginals(_ originals: [(UInt32, URL?)], timeout: TimeInterval = 5) {
        for (id, url) in originals {
            guard let url, let screen = screen(for: id) else {
                continue
            }
            try? NSWorkspace.shared.setDesktopImageURL(url, for: screen, options: [:])
        }
        _ = waitUntil(timeout: timeout) { originalsStillPresent(originals) }
    }

    private func originalsStillPresent(_ originals: [(UInt32, URL?)]) -> Bool {
        for (id, url) in originals {
            guard let screen = screen(for: id) else {
                continue
            }
            let current = NSWorkspace.shared.desktopImageURL(for: screen)?.standardizedFileURL
            if current != url?.standardizedFileURL {
                return false
            }
        }
        return true
    }

    private func ownedScreensShow(_ url: URL, originals: [(UInt32, URL?)]) -> Bool {
        for (id, original) in originals where original != nil {
            guard let screen = screen(for: id) else {
                continue
            }
            if NSWorkspace.shared.desktopImageURL(for: screen)?.standardizedFileURL != url.standardizedFileURL {
                return false
            }
        }
        return true
    }

    private func stillFile(in directory: URL) -> URL? {
        guard let entries = try? FileManager.default.contentsOfDirectory(
            at: directory,
            includingPropertiesForKeys: nil
        ) else {
            return nil
        }
        return entries.first { $0.pathExtension == "png" }
    }

    private func makeRoot(_ tag: String) -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("lwstill-\(tag)-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    @Test func takeoverAppliesStillAndRestoreReturnsOriginals() async throws {
        _ = NSApplication.shared
        try #require(!NSScreen.screens.isEmpty)

        let originals = captureOriginals()
        let root = makeRoot("e2e")
        let service = StillSyncService(
            workspace: NSWorkspaceDesktopPicture(),
            store: SwapStateStore(url: root.appendingPathComponent("swap-state.json")),
            stillDirectory: root.appendingPathComponent("stills")
        )
        defer {
            restoreOriginals(originals)
            try? FileManager.default.removeItem(at: root)
        }

        try await service.takeover(StillRequest(url: Fixtures.still, kind: .image))
        let still = try #require(service.currentStillURL)

        let applied = waitUntil { ownedScreensShow(still, originals: originals) }
        #expect(applied, "still not applied on all owned screens")

        try await service.restore()

        let restored = waitUntil { originalsStillPresent(originals) }
        #expect(restored, "desktop picture not restored after stop")
    }

    @Test func crashRecoveryRestoresOriginals() async throws {
        _ = NSApplication.shared
        try #require(!NSScreen.screens.isEmpty)

        let originals = captureOriginals()
        let root = makeRoot("crash")
        let swapURL = root.appendingPathComponent("swap-state.json")
        let stillsURL = root.appendingPathComponent("stills")

        let probe = probeURL
        try #require(
            FileManager.default.isExecutableFile(atPath: probe.path),
            "still probe not built at \(probe.path)"
        )

        let process = Process()
        process.executableURL = probe
        process.arguments = [Fixtures.still.path, swapURL.path, stillsURL.path]
        process.standardOutput = Pipe()
        process.standardError = Pipe()

        defer {
            if process.isRunning {
                kill(process.processIdentifier, SIGKILL)
                process.waitUntilExit()
            }
            restoreOriginals(originals)
            try? FileManager.default.removeItem(at: root)
        }

        try process.run()

        let tookOver = waitUntil(timeout: 15) {
            guard FileManager.default.fileExists(atPath: swapURL.path),
                  let still = stillFile(in: stillsURL) else {
                return false
            }
            return ownedScreensShow(still, originals: originals)
        }
        try #require(tookOver, "helper did not take over the desktop in time")

        kill(process.processIdentifier, SIGKILL)
        process.waitUntilExit()

        let service = StillSyncService(
            workspace: NSWorkspaceDesktopPicture(),
            store: SwapStateStore(url: swapURL),
            stillDirectory: stillsURL
        )
        try await service.recoverIfNeeded()

        #expect(!FileManager.default.fileExists(atPath: swapURL.path), "persisted swap state not cleared")
        let restored = waitUntil { originalsStillPresent(originals) }
        #expect(restored, "originals not restored after crash recovery")
    }
}
