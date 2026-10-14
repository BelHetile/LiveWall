import AppKit
import Foundation
import Testing
@testable import LiveWallEngine

@Suite(.serialized, .enabled(if: EngineE2E.enabled)) @MainActor struct PauseRulesE2ETests {
    @Test func realWindowListCoverageAndExclusionMath() throws {
        _ = NSApplication.shared
        let screen = try #require(NSScreen.screens.first)
        let frame = screen.frame

        let window = NSWindow(contentRect: frame, styleMask: [.borderless], backing: .buffered, defer: false)
        window.level = .normal
        window.hasShadow = false
        window.isReleasedWhenClosed = false
        window.orderFrontRegardless()
        defer {
            window.orderOut(nil)
            window.close()
        }
        pumpRunLoop(0.4)

        let provider = CGWindowListProvider()
        let windows = provider.onScreenWindows()
        #expect(!windows.isEmpty, "CGWindowList returned no windows")

        let ours = windows.filter { $0.ownerPID == getpid() }
        let ownSnapshot = try #require(
            ours.first { WindowGeometry.approximatelyEqual($0.bounds, frame, tolerance: 6) },
            "our test window was not found in the CGWindowList snapshot"
        )

        #expect(WindowGeometry.fullscreenAppPresent(windows: [ownSnapshot], screens: [frame], ownPID: 0))
        #expect(!WindowGeometry.fullscreenAppPresent(windows: [ownSnapshot], screens: [frame], ownPID: getpid()))

        let included = WindowGeometry.coverageRatio(of: frame, windows: [ownSnapshot], ownPID: 0)
        #expect(included >= 0.9)
        let excluded = WindowGeometry.coverageRatio(of: frame, windows: [ownSnapshot], ownPID: getpid())
        #expect(excluded == 0)

        let whole = WindowGeometry.maxCoverageRatio(windows: windows, screens: [frame], ownPID: 0)
        #expect(whole >= 0.9)
    }

    @Test func displaySleepWakeNotificationPath() {
        _ = NSApplication.shared
        let detector = NSWorkspaceDisplayAsleepDetector()
        var observed: [Bool] = []
        detector.startObserving { observed.append(detector.isDisplayAsleep) }
        defer { detector.stopObserving() }

        #expect(detector.isDisplayAsleep == false)

        NSWorkspace.shared.notificationCenter.post(name: NSWorkspace.screensDidSleepNotification, object: nil)
        pumpRunLoop(0.2)
        #expect(detector.isDisplayAsleep)
        #expect(observed == [true])

        NSWorkspace.shared.notificationCenter.post(name: NSWorkspace.screensDidWakeNotification, object: nil)
        pumpRunLoop(0.2)
        #expect(!detector.isDisplayAsleep)
        #expect(observed == [true, false])
    }
}
