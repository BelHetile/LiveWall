import AppKit
import Foundation

@MainActor
public final class SystemSnapshotService {
    public private(set) var snapshot: SystemSnapshot
    public var onChange: ((SystemSnapshot) -> Void)?

    private let fullscreen: FullscreenDetector
    private let coverage: CoverageDetector
    private let audio: AudioPlayingDetector
    private let apps: FrontmostDetector
    private let display: DisplayAsleepDetector
    private let pollInterval: TimeInterval
    private let selfBundleID: String?

    private var lockObserver: LockStateObserver?
    private var locked = false
    private var timer: Timer?
    private var started = false

    public init(
        fullscreen: FullscreenDetector,
        coverage: CoverageDetector,
        audio: AudioPlayingDetector,
        apps: FrontmostDetector,
        display: DisplayAsleepDetector,
        pollInterval: TimeInterval = 2,
        selfBundleID: String? = Bundle.main.bundleIdentifier
    ) {
        self.fullscreen = fullscreen
        self.coverage = coverage
        self.audio = audio
        self.apps = apps
        self.display = display
        self.pollInterval = pollInterval
        self.selfBundleID = selfBundleID
        self.snapshot = SystemSnapshot(selfBundleID: selfBundleID)
    }

    public func start() {
        guard !started else {
            return
        }
        started = true
        let refreshHandler: @MainActor () -> Void = { [weak self] in
            self?.refresh()
        }
        display.startObserving(refreshHandler)
        apps.startObserving(refreshHandler)
        audio.startObserving(refreshHandler)
        lockObserver = LockStateObserver { [weak self] locked in
            self?.setLocked(locked)
        }
        scheduleTimer()
        refresh()
    }

    public func stop() {
        guard started else {
            return
        }
        started = false
        display.stopObserving()
        apps.stopObserving()
        audio.stopObserving()
        lockObserver?.invalidate()
        lockObserver = nil
        timer?.invalidate()
        timer = nil
    }

    public func refresh() {
        let next = currentSnapshot()
        guard next != snapshot else {
            return
        }
        snapshot = next
        onChange?(next)
    }

    public func evaluate(_ config: RuleConfig) -> RuleAction {
        return RuleEvaluator.evaluate(snapshot, config: config)
    }

    func setLocked(_ locked: Bool) {
        self.locked = locked
        refresh()
    }

    private func currentSnapshot() -> SystemSnapshot {
        return SystemSnapshot(
            displayAsleep: display.isDisplayAsleep,
            screenLocked: locked,
            fullscreenAppPresent: fullscreen.isFullscreenAppPresent(),
            coverageRatio: coverage.maxCoverageRatio(),
            frontmostBundleID: apps.frontmostBundleID,
            runningBundleIDs: apps.runningBundleIDs,
            selfBundleID: selfBundleID
        )
    }

    private func scheduleTimer() {
        let timer = Timer(timeInterval: pollInterval, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.refresh()
            }
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
    }

    isolated deinit {
        if started {
            display.stopObserving()
            apps.stopObserving()
            audio.stopObserving()
        }
        lockObserver?.invalidate()
        timer?.invalidate()
    }
}
