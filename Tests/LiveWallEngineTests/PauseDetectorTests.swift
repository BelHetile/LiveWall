import AppKit
import CoreGraphics
import Foundation
import Testing
@testable import LiveWallEngine

@MainActor
final class FakeWindowProvider: WindowInfoProvider {
    var windows: [WindowSnapshot]
    init(_ windows: [WindowSnapshot]) {
        self.windows = windows
    }
    func onScreenWindows() -> [WindowSnapshot] {
        return windows
    }
}

@MainActor
final class FakeFullscreenDetector: FullscreenDetector {
    var value = false
    func isFullscreenAppPresent() -> Bool {
        return value
    }
}

@MainActor
final class FakeCoverageDetector: CoverageDetector {
    var value = 0.0
    func maxCoverageRatio() -> Double {
        return value
    }
}

@MainActor
final class FakeAudioPlayingDetector: AudioPlayingDetector {
    var value = false
    private(set) var observing = false
    func isExternalAudioPlaying() -> Bool {
        return value
    }
    func startObserving(_ handler: @escaping @MainActor () -> Void) {
        observing = true
    }
    func stopObserving() {
        observing = false
    }
}

@MainActor
final class FakeFrontmostDetector: FrontmostDetector {
    var frontmostBundleID: String?
    var runningBundleIDs: Set<String> = []
    private(set) var observing = false
    func startObserving(_ handler: @escaping @MainActor () -> Void) {
        observing = true
    }
    func stopObserving() {
        observing = false
    }
}

@MainActor
final class FakeDisplayAsleepDetector: DisplayAsleepDetector {
    var isDisplayAsleep = false
    private(set) var handler: (@MainActor () -> Void)?
    func startObserving(_ handler: @escaping @MainActor () -> Void) {
        self.handler = handler
    }
    func stopObserving() {
        handler = nil
    }
    func simulate(_ asleep: Bool) {
        isDisplayAsleep = asleep
        handler?()
    }
}

private let hdScreen = CGRect(x: 0, y: 0, width: 1920, height: 1080)

private func fullScreenWindow(pid: pid_t = 999, layer: Int = 0, owner: String? = "SomeApp") -> WindowSnapshot {
    return WindowSnapshot(bounds: hdScreen, layer: layer, ownerPID: pid, ownerName: owner)
}

@Suite struct WindowGeometryTests {
    @Test func fullscreenMatchesExactScreenBounds() {
        let windows = [fullScreenWindow()]
        #expect(WindowGeometry.fullscreenAppPresent(windows: windows, screens: [hdScreen], ownPID: 1))
    }

    @Test func fullscreenUsedWhenAppearNearFullscreen() {
        let window = WindowSnapshot(
            bounds: CGRect(x: 0, y: 0, width: 1919, height: 1079),
            layer: 0,
            ownerPID: 999,
            ownerName: "SomeApp"
        )
        #expect(WindowGeometry.fullscreenAppPresent(windows: [window], screens: [hdScreen], ownPID: 1))
    }

    @Test func fullscreenRejectsSmallWindow() {
        let window = WindowSnapshot(bounds: CGRect(x: 100, y: 100, width: 800, height: 600), layer: 0, ownerPID: 999)
        #expect(!WindowGeometry.fullscreenAppPresent(windows: [window], screens: [hdScreen], ownPID: 1))
    }

    @Test func fullscreenRejectsOurOwnWindow() {
        #expect(!WindowGeometry.fullscreenAppPresent(windows: [fullScreenWindow(pid: 42)], screens: [hdScreen], ownPID: 42))
    }

    @Test func fullscreenRejectsElevatedLayers() {
        #expect(!WindowGeometry.fullscreenAppPresent(windows: [fullScreenWindow(layer: 24)], screens: [hdScreen], ownPID: 1))
    }

    @Test func fullscreenRejectsSystemOwners() {
        let window = fullScreenWindow(owner: "Window Server")
        #expect(!WindowGeometry.fullscreenAppPresent(windows: [window], screens: [hdScreen], ownPID: 1))
    }

    @Test func fullscreenRejectsWindowOnDifferentScreen() {
        let other = CGRect(x: 3000, y: 0, width: 1920, height: 1080)
        #expect(!WindowGeometry.fullscreenAppPresent(windows: [fullScreenWindow()], screens: [other], ownPID: 1))
    }

    @Test func coverageFullWindowIsOne() {
        #expect(WindowGeometry.coverageRatio(of: hdScreen, windows: [fullScreenWindow()], ownPID: 1) == 1.0)
    }

    @Test func coverageHalfWindowIsHalf() {
        let window = WindowSnapshot(bounds: CGRect(x: 0, y: 0, width: 960, height: 1080), layer: 0, ownerPID: 999)
        let ratio = WindowGeometry.coverageRatio(of: hdScreen, windows: [window], ownPID: 1)
        #expect(abs(ratio - 0.5) < 0.0001)
    }

    @Test func coverageClipsToScreen() {
        let window = WindowSnapshot(bounds: CGRect(x: 0, y: 0, width: 3840, height: 2160), layer: 0, ownerPID: 999)
        #expect(WindowGeometry.coverageRatio(of: hdScreen, windows: [window], ownPID: 1) == 1.0)
    }

    @Test func coverageExcludesOwnWindows() {
        #expect(WindowGeometry.coverageRatio(of: hdScreen, windows: [fullScreenWindow(pid: 42)], ownPID: 42) == 0)
    }

    @Test func coverageExcludesSystemUILayers() {
        #expect(WindowGeometry.coverageRatio(of: hdScreen, windows: [fullScreenWindow(layer: 24)], ownPID: 1) == 0)
    }

    @Test func coverageSumsMultipleWindows() {
        let left = WindowSnapshot(bounds: CGRect(x: 0, y: 0, width: 960, height: 1080), layer: 0, ownerPID: 999)
        let right = WindowSnapshot(bounds: CGRect(x: 960, y: 0, width: 960, height: 1080), layer: 0, ownerPID: 998)
        #expect(WindowGeometry.coverageRatio(of: hdScreen, windows: [left, right], ownPID: 1) == 1.0)
    }

    @Test func maxCoveragePicksMostCoveredScreen() {
        let second = CGRect(x: 1920, y: 0, width: 1920, height: 1080)
        let half = WindowSnapshot(bounds: CGRect(x: 0, y: 0, width: 960, height: 1080), layer: 0, ownerPID: 999)
        let ratio = WindowGeometry.maxCoverageRatio(windows: [half], screens: [hdScreen, second], ownPID: 1)
        #expect(abs(ratio - 0.5) < 0.0001)
    }
}

@Suite @MainActor struct DetectorTests {
    @Test func fullscreenDetectorUsesInjectedProvider() {
        let provider = FakeWindowProvider([fullScreenWindow()])
        let detector = CGWindowFullscreenDetector(provider: provider, ownPID: 1, screens: { [hdScreen] })
        #expect(detector.isFullscreenAppPresent())
        provider.windows = []
        #expect(!detector.isFullscreenAppPresent())
    }

    @Test func coverageDetectorUsesInjectedProvider() {
        let provider = FakeWindowProvider([fullScreenWindow()])
        let detector = CGWindowCoverageDetector(provider: provider, ownPID: 1, screens: { [hdScreen] })
        #expect(detector.maxCoverageRatio() == 1.0)
        provider.windows = []
        #expect(detector.maxCoverageRatio() == 0)
    }

    @Test func audioExcludesOwnPidOnly() {
        let own = getpid()
        #expect(!CoreAudioPlayingDetector.externalOutputPresent([(pid: own, runningOutput: true)], ownPID: own))
        #expect(CoreAudioPlayingDetector.externalOutputPresent(
            [(pid: own, runningOutput: true), (pid: 4242, runningOutput: true)],
            ownPID: own
        ))
        #expect(!CoreAudioPlayingDetector.externalOutputPresent(
            [(pid: 4242, runningOutput: false)],
            ownPID: own
        ))
    }

    @Test func snapshotServiceCombinesFakes() {
        let fullscreen = FakeFullscreenDetector()
        let coverage = FakeCoverageDetector()
        let audio = FakeAudioPlayingDetector()
        let apps = FakeFrontmostDetector()
        let display = FakeDisplayAsleepDetector()
        let service = SystemSnapshotService(
            fullscreen: fullscreen,
            coverage: coverage,
            audio: audio,
            apps: apps,
            display: display,
            pollInterval: 3600,
            selfBundleID: "io.github.belhetile.livewall"
        )
        var changes: [SystemSnapshot] = []
        service.onChange = { changes.append($0) }

        service.start()
        #expect(display.handler != nil)
        #expect(apps.observing)
        #expect(audio.observing)

        fullscreen.value = true
        coverage.value = 0.95
        apps.frontmostBundleID = "com.example.other"
        apps.runningBundleIDs = ["com.example.other"]
        service.refresh()

        #expect(service.snapshot.fullscreenAppPresent)
        #expect(service.snapshot.coverageRatio == 0.95)
        #expect(service.snapshot.frontmostBundleID == "com.example.other")
        #expect(service.snapshot.runningBundleIDs == ["com.example.other"])
        #expect(service.snapshot.selfBundleID == "io.github.belhetile.livewall")
        #expect(!changes.isEmpty)

        service.setLocked(true)
        #expect(service.snapshot.screenLocked)

        display.simulate(true)
        #expect(service.snapshot.displayAsleep)

        service.stop()
        #expect(!apps.observing)
        #expect(!audio.observing)
    }

    @Test func refreshDoesNotEmitWhenSnapshotUnchanged() {
        let service = SystemSnapshotService(
            fullscreen: FakeFullscreenDetector(),
            coverage: FakeCoverageDetector(),
            audio: FakeAudioPlayingDetector(),
            apps: FakeFrontmostDetector(),
            display: FakeDisplayAsleepDetector(),
            pollInterval: 3600,
            selfBundleID: "io.github.belhetile.livewall"
        )
        var count = 0
        service.onChange = { _ in count += 1 }
        service.refresh()
        service.refresh()
        #expect(count == 0)
    }

    @Test func evaluateDelegatesToRuleEvaluator() {
        let fullscreen = FakeFullscreenDetector()
        let service = SystemSnapshotService(
            fullscreen: fullscreen,
            coverage: FakeCoverageDetector(),
            audio: FakeAudioPlayingDetector(),
            apps: FakeFrontmostDetector(),
            display: FakeDisplayAsleepDetector(),
            pollInterval: 3600,
            selfBundleID: nil
        )
        fullscreen.value = true
        #expect(service.evaluate(RuleConfig(fullscreenAction: .pause)) == .keepRunning)
        service.refresh()
        #expect(service.evaluate(RuleConfig(fullscreenAction: .pause)) == .pause)
    }
}
