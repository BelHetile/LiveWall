import AppKit
import IOKit.ps

final class ScreenLockMonitor: NSObject {
    static let shared = ScreenLockMonitor()

    private var handlers: [(Bool) -> Void] = []
    private var started = false

    func observe(_ handler: @escaping (Bool) -> Void) {
        handlers.append(handler)
        guard !started else { return }
        started = true
        let center = DistributedNotificationCenter.default()
        center.addObserver(self, selector: #selector(locked), name: NSNotification.Name("com.apple.screenIsLocked"),
                           object: nil, suspensionBehavior: .deliverImmediately)
        center.addObserver(self, selector: #selector(unlocked), name: NSNotification.Name("com.apple.screenIsUnlocked"),
                           object: nil, suspensionBehavior: .deliverImmediately)
    }

    @objc private func locked() { notify(true) }
    @objc private func unlocked() { notify(false) }

    private func notify(_ isLocked: Bool) {
        if Thread.isMainThread {
            handlers.forEach { $0(isLocked) }
        } else {
            DispatchQueue.main.async { self.handlers.forEach { $0(isLocked) } }
        }
    }
}

/// Decides whether the wallpaper should pause right now (like Performance → Playback in Wallpaper Engine).
/// Returns a human-readable reason, or nil when playing is fine.
final class PauseMonitor: NSObject {
    static let shared = PauseMonitor()

    var settings = Settings() {
        didSet { evaluate() }
    }
    var onChange: ((String?) -> Void)?

    private(set) var reason: String?
    private var timer: Timer?
    private var powerSource: CFRunLoopSource?
    private var cachedOnBattery = false

    /// Windows only need watching for videos and GIFs: a still image has nothing to pause.
    var needsMonitoring = false {
        didSet {
            guard needsMonitoring != oldValue, started else { return }
            updateTimer()
            evaluate()
        }
    }
    private var screenOff = false
    private var locked = false
    private var started = false

    func start() {
        guard !started else { return }
        started = true

        let ws = NSWorkspace.shared.notificationCenter
        ws.addObserver(self, selector: #selector(screenSlept), name: NSWorkspace.screensDidSleepNotification, object: nil)
        ws.addObserver(self, selector: #selector(screenWoke), name: NSWorkspace.screensDidWakeNotification, object: nil)
        ws.addObserver(self, selector: #selector(screenSlept), name: NSWorkspace.willSleepNotification, object: nil)
        ws.addObserver(self, selector: #selector(screenWoke), name: NSWorkspace.didWakeNotification, object: nil)
        ws.addObserver(self, selector: #selector(sessionInactive), name: NSWorkspace.sessionDidResignActiveNotification, object: nil)
        ws.addObserver(self, selector: #selector(sessionActive), name: NSWorkspace.sessionDidBecomeActiveNotification, object: nil)
        ws.addObserver(self, selector: #selector(somethingChanged), name: NSWorkspace.didActivateApplicationNotification, object: nil)
        ws.addObserver(self, selector: #selector(somethingChanged), name: NSWorkspace.didLaunchApplicationNotification, object: nil)
        ws.addObserver(self, selector: #selector(somethingChanged), name: NSWorkspace.didTerminateApplicationNotification, object: nil)
        ws.addObserver(self, selector: #selector(somethingChanged), name: NSWorkspace.activeSpaceDidChangeNotification, object: nil)

        ScreenLockMonitor.shared.observe { [weak self] isLocked in
            self?.locked = isLocked
            self?.evaluate()
        }

        NotificationCenter.default.addObserver(self, selector: #selector(somethingChanged),
                                               name: ProcessInfo.thermalStateDidChangeNotification, object: nil)
        NotificationCenter.default.addObserver(self, selector: #selector(somethingChanged),
                                               name: NSNotification.Name.NSProcessInfoPowerStateDidChange, object: nil)

        startPowerNotifications()
        updateTimer()
        evaluate()
    }

    /// Windows move without notifications, so they are polled, but only while needed
    /// and with tolerance, so macOS can coalesce wakeups with other timers.
    private func updateTimer() {
        timer?.invalidate()
        timer = nil
        guard needsMonitoring else { return }
        let t = Timer(timeInterval: 2.0, repeats: true) { [weak self] _ in
            self?.evaluate()
        }
        t.tolerance = 0.5
        RunLoop.main.add(t, forMode: .common)
        timer = t
    }

    /// The power source rarely changes, so the system tells us instead of polling.
    private func startPowerNotifications() {
        cachedOnBattery = PauseMonitor.onBattery()
        let context = Unmanaged.passUnretained(self).toOpaque()
        let callback: IOPowerSourceCallbackType = { ctx in
            guard let ctx = ctx else { return }
            let monitor = Unmanaged<PauseMonitor>.fromOpaque(ctx).takeUnretainedValue()
            monitor.cachedOnBattery = PauseMonitor.onBattery()
            monitor.evaluate()
        }
        if let source = IOPSNotificationCreateRunLoopSource(callback, context)?.takeRetainedValue() {
            CFRunLoopAddSource(CFRunLoopGetMain(), source, .defaultMode)
            powerSource = source
        }
    }

    @objc private func screenSlept() { screenOff = true; evaluate() }
    @objc private func screenWoke() { screenOff = false; evaluate() }
    @objc private func sessionInactive() { screenOff = true; evaluate() }
    @objc private func sessionActive() { screenOff = false; evaluate() }
    @objc private func somethingChanged() {
        // notifications may arrive off the main thread
        DispatchQueue.main.async { self.evaluate() }
    }

    func evaluate() {
        let r = computeReason()
        if r != reason {
            reason = r
            onChange?(r)
        }
    }

    private func computeReason() -> String? {
        // a still wallpaper has nothing to pause
        guard needsMonitoring else { return nil }
        if screenOff { return L("Display asleep") }
        if locked { return L("Screen locked") }

        let s = settings
        let info = ProcessInfo.processInfo

        if s.pauseWhenHot && (info.thermalState == .serious || info.thermalState == .critical) {
            return L("Mac is hot")
        }
        if s.pauseInLowPower && info.isLowPowerModeEnabled {
            return L("Low Power Mode")
        }
        if s.pauseOnBattery && cachedOnBattery {
            return L("On battery")
        }

        let front = NSWorkspace.shared.frontmostApplication
        for rule in s.appRules {
            if rule.frontmostOnly {
                if front?.bundleIdentifier == rule.bundleID {
                    return LF("%@ is in front", rule.name)
                }
            } else if !NSRunningApplication.runningApplications(withBundleIdentifier: rule.bundleID).isEmpty {
                return LF("%@ is running", rule.name)
            }
        }

        if s.pauseWhenAppFocused, let f = front,
           f.processIdentifier != getpid(),
           f.bundleIdentifier != "com.apple.finder" {
            return LF("%@ is focused", f.localizedName ?? L("Another app"))
        }

        if s.pauseWhenFullscreen || s.pauseWhenMaximized {
            let cov = PauseMonitor.coverage()
            if s.pauseWhenFullscreen && cov.allFullscreen {
                return L("Fullscreen app")
            }
            if s.pauseWhenMaximized && cov.minFraction >= 0.9 {
                return L("Desktop covered by windows")
            }
        }
        return nil
    }

    // MARK: Helpers

    static func onBattery() -> Bool {
        guard let infoRef = IOPSCopyPowerSourcesInfo() else { return false }
        let info = infoRef.takeRetainedValue()
        guard let typeRef = IOPSGetProvidingPowerSourceType(info) else { return false }
        let type = typeRef.takeUnretainedValue() as String
        return type == kIOPSBatteryPowerValue
    }

    /// How much other apps' windows cover each screen.
    /// minFraction: covered share of the usable area on the least covered screen,
    /// allFullscreen: every screen has a full-screen window.
    static func coverage() -> (minFraction: Double, allFullscreen: Bool) {
        let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
        guard let list = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else {
            return (0, false)
        }
        let myPID = getpid()
        var rects: [CGRect] = []
        for w in list {
            guard let layer = w[kCGWindowLayer as String] as? Int, layer == 0 else { continue }
            if let pid = w[kCGWindowOwnerPID as String] as? Int32, pid == myPID { continue }
            if let alpha = w[kCGWindowAlpha as String] as? Double, alpha < 0.05 { continue }
            guard let bounds = w[kCGWindowBounds as String] as? [String: Any],
                  let r = CGRect(dictionaryRepresentation: bounds as CFDictionary) else { continue }
            if r.width < 80 || r.height < 80 { continue }
            rects.append(r)
        }

        let screens = NSScreen.screens
        guard let primary = screens.first, !screens.isEmpty else { return (0, false) }
        let ph = primary.frame.height

        var minFraction = 1.0
        var allFullscreen = true
        for screen in screens {
            // Cocoa (origin at the bottom) -> CoreGraphics (origin at the top)
            let f = screen.frame
            let sr = CGRect(x: f.minX, y: ph - f.maxY, width: f.width, height: f.height)
            let full = rects.contains { r in
                abs(r.minX - sr.minX) < 2 && abs(r.minY - sr.minY) < 2 &&
                    r.width >= sr.width - 2 && r.height >= sr.height - 2
            }
            if !full { allFullscreen = false }

            let vf = screen.visibleFrame
            let vr = CGRect(x: vf.minX, y: ph - vf.maxY, width: vf.width, height: vf.height)
            let nx = 24, ny = 16
            var hit = 0
            for i in 0..<nx {
                for j in 0..<ny {
                    let p = CGPoint(x: vr.minX + (CGFloat(i) + 0.5) * vr.width / CGFloat(nx),
                                    y: vr.minY + (CGFloat(j) + 0.5) * vr.height / CGFloat(ny))
                    if rects.contains(where: { $0.contains(p) }) { hit += 1 }
                }
            }
            minFraction = min(minFraction, Double(hit) / Double(nx * ny))
        }
        return (minFraction, allFullscreen)
    }
}
