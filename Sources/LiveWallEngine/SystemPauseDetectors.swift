import AppKit
import CoreAudio
import CoreGraphics
import Foundation

@MainActor
public final class CGWindowListProvider: WindowInfoProvider {
    public init() {}

    public func onScreenWindows() -> [WindowSnapshot] {
        let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
        guard let raw = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else {
            return []
        }
        let flipHeight = NSScreen.screens.first?.frame.height ?? 0
        var windows: [WindowSnapshot] = []
        windows.reserveCapacity(raw.count)
        for info in raw {
            guard let snapshot = Self.snapshot(from: info, flipHeight: flipHeight) else {
                continue
            }
            windows.append(snapshot)
        }
        return windows
    }

    static func snapshot(from info: [String: Any], flipHeight: CGFloat) -> WindowSnapshot? {
        guard let boundsValue = info[kCGWindowBounds as String] as? NSDictionary,
              let rect = CGRect(dictionaryRepresentation: boundsValue as CFDictionary) else {
            return nil
        }
        let layer = (info[kCGWindowLayer as String] as? NSNumber)?.intValue ?? 0
        let ownerPID = (info[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value ?? 0
        let ownerName = info[kCGWindowOwnerName as String] as? String
        let flipped = CGRect(
            x: rect.minX,
            y: flipHeight - rect.maxY,
            width: rect.width,
            height: rect.height
        )
        return WindowSnapshot(bounds: flipped, layer: layer, ownerPID: ownerPID, ownerName: ownerName)
    }
}

@MainActor
public final class CGWindowFullscreenDetector: FullscreenDetector {
    private let provider: WindowInfoProvider
    private let screens: () -> [CGRect]
    private let ownPID: pid_t

    public init(
        provider: WindowInfoProvider = CGWindowListProvider(),
        ownPID: pid_t = getpid(),
        screens: @escaping () -> [CGRect] = { NSScreen.screens.map(\.frame) }
    ) {
        self.provider = provider
        self.ownPID = ownPID
        self.screens = screens
    }

    public func isFullscreenAppPresent() -> Bool {
        return WindowGeometry.fullscreenAppPresent(
            windows: provider.onScreenWindows(),
            screens: screens(),
            ownPID: ownPID
        )
    }
}

@MainActor
public final class CGWindowCoverageDetector: CoverageDetector {
    private let provider: WindowInfoProvider
    private let screens: () -> [CGRect]
    private let ownPID: pid_t

    public init(
        provider: WindowInfoProvider = CGWindowListProvider(),
        ownPID: pid_t = getpid(),
        screens: @escaping () -> [CGRect] = { NSScreen.screens.map(\.frame) }
    ) {
        self.provider = provider
        self.ownPID = ownPID
        self.screens = screens
    }

    public func maxCoverageRatio() -> Double {
        return WindowGeometry.maxCoverageRatio(
            windows: provider.onScreenWindows(),
            screens: screens(),
            ownPID: ownPID
        )
    }
}

@MainActor
public final class CoreAudioPlayingDetector: AudioPlayingDetector {
    private let ownPID: pid_t
    private var handler: (@MainActor () -> Void)?
    private var boundDevice = AudioObjectID(kAudioObjectUnknown)
    private var systemListeners: [(object: AudioObjectID, address: AudioObjectPropertyAddress, block: AudioObjectPropertyListenerBlock)] = []
    private var deviceListener: (object: AudioObjectID, address: AudioObjectPropertyAddress, block: AudioObjectPropertyListenerBlock)?

    public init(ownPID: pid_t = getpid()) {
        self.ownPID = ownPID
    }

    public func isExternalAudioPlaying() -> Bool {
        guard let device = Self.defaultOutputDevice() else {
            return false
        }
        guard Self.deviceIsRunningSomewhere(device) else {
            return false
        }
        let processes = Self.runningOutputProcesses()
        guard !processes.isEmpty else {
            return true
        }
        return Self.externalOutputPresent(processes, ownPID: ownPID)
    }

    public func startObserving(_ handler: @escaping @MainActor () -> Void) {
        self.handler = handler
        addSystemListener(selector: kAudioHardwarePropertyDefaultOutputDevice)
        addSystemListener(selector: kAudioHardwarePropertyProcessObjectList)
        bindDeviceListener()
    }

    public func stopObserving() {
        handler = nil
        for entry in systemListeners {
            var address = entry.address
            AudioObjectRemovePropertyListenerBlock(entry.object, &address, DispatchQueue.main, entry.block)
        }
        systemListeners.removeAll()
        removeDeviceListener()
    }

    private func addSystemListener(selector: AudioObjectPropertySelector) {
        let object = AudioObjectID(kAudioObjectSystemObject)
        let address = AudioObjectPropertyAddress(
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var mutable = address
        let block: AudioObjectPropertyListenerBlock = { [weak self] _, _ in
            MainActor.assumeIsolated {
                self?.handleSystemChange(selector: selector)
            }
        }
        guard AudioObjectAddPropertyListenerBlock(object, &mutable, DispatchQueue.main, block) == noErr else {
            return
        }
        systemListeners.append((object, mutable, block))
    }

    private func handleSystemChange(selector: AudioObjectPropertySelector) {
        if selector == kAudioHardwarePropertyDefaultOutputDevice {
            bindDeviceListener()
        }
        notify()
    }

    private func bindDeviceListener() {
        guard let device = Self.defaultOutputDevice() else {
            return
        }
        guard device != boundDevice else {
            return
        }
        removeDeviceListener()
        boundDevice = device
        let address = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyDeviceIsRunningSomewhere,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var mutable = address
        let block: AudioObjectPropertyListenerBlock = { [weak self] _, _ in
            MainActor.assumeIsolated {
                self?.notify()
            }
        }
        guard AudioObjectAddPropertyListenerBlock(device, &mutable, DispatchQueue.main, block) == noErr else {
            return
        }
        deviceListener = (device, mutable, block)
    }

    private func removeDeviceListener() {
        guard let entry = deviceListener else {
            return
        }
        var address = entry.address
        AudioObjectRemovePropertyListenerBlock(entry.object, &address, DispatchQueue.main, entry.block)
        deviceListener = nil
        boundDevice = AudioObjectID(kAudioObjectUnknown)
    }

    private func notify() {
        handler?()
    }

    static func externalOutputPresent(_ processes: [(pid: pid_t, runningOutput: Bool)], ownPID: pid_t) -> Bool {
        return processes.contains { $0.runningOutput && $0.pid != ownPID }
    }

    static func defaultOutputDevice() -> AudioObjectID? {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var device = AudioObjectID(kAudioObjectUnknown)
        var size = UInt32(MemoryLayout<AudioObjectID>.size)
        let status = AudioObjectGetPropertyData(
            AudioObjectID(kAudioObjectSystemObject),
            &address,
            0,
            nil,
            &size,
            &device
        )
        guard status == noErr, device != AudioObjectID(kAudioObjectUnknown) else {
            return nil
        }
        return device
    }

    static func deviceIsRunningSomewhere(_ device: AudioObjectID) -> Bool {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyDeviceIsRunningSomewhere,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var running: UInt32 = 0
        var size = UInt32(MemoryLayout<UInt32>.size)
        let status = AudioObjectGetPropertyData(device, &address, 0, nil, &size, &running)
        return status == noErr && running != 0
    }

    static func runningOutputProcesses() -> [(pid: pid_t, runningOutput: Bool)] {
        let processObjects = processObjectList()
        var result: [(pid: pid_t, runningOutput: Bool)] = []
        result.reserveCapacity(processObjects.count)
        for object in processObjects {
            let pid = processPID(object)
            let running = processIsRunningOutput(object)
            result.append((pid, running))
        }
        return result
    }

    static func processObjectList() -> [AudioObjectID] {
        let object = AudioObjectID(kAudioObjectSystemObject)
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyProcessObjectList,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var size: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(object, &address, 0, nil, &size) == noErr else {
            return []
        }
        let count = Int(size) / MemoryLayout<AudioObjectID>.size
        guard count > 0 else {
            return []
        }
        var list = [AudioObjectID](repeating: 0, count: count)
        guard AudioObjectGetPropertyData(object, &address, 0, nil, &size, &list) == noErr else {
            return []
        }
        return list
    }

    static func processPID(_ object: AudioObjectID) -> pid_t {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioProcessPropertyPID,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var pid: pid_t = 0
        var size = UInt32(MemoryLayout<pid_t>.size)
        let status = AudioObjectGetPropertyData(object, &address, 0, nil, &size, &pid)
        return status == noErr ? pid : 0
    }

    static func processIsRunningOutput(_ object: AudioObjectID) -> Bool {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioProcessPropertyIsRunningOutput,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var running: UInt32 = 0
        var size = UInt32(MemoryLayout<UInt32>.size)
        let status = AudioObjectGetPropertyData(object, &address, 0, nil, &size, &running)
        return status == noErr && running != 0
    }
}

@MainActor
public final class NSWorkspaceAppStateDetector: FrontmostDetector {
    private var tokens: [NSObjectProtocol] = []

    public init() {}

    public var frontmostBundleID: String? {
        return NSWorkspace.shared.frontmostApplication?.bundleIdentifier
    }

    public var runningBundleIDs: Set<String> {
        return Set(NSWorkspace.shared.runningApplications.compactMap(\.bundleIdentifier))
    }

    public func startObserving(_ handler: @escaping @MainActor () -> Void) {
        guard tokens.isEmpty else {
            return
        }
        let center = NSWorkspace.shared.notificationCenter
        let names: [Notification.Name] = [
            NSWorkspace.didActivateApplicationNotification,
            NSWorkspace.didLaunchApplicationNotification,
            NSWorkspace.didTerminateApplicationNotification
        ]
        for name in names {
            let token = center.addObserver(forName: name, object: nil, queue: .main) { _ in
                MainActor.assumeIsolated {
                    handler()
                }
            }
            tokens.append(token)
        }
    }

    public func stopObserving() {
        let center = NSWorkspace.shared.notificationCenter
        for token in tokens {
            center.removeObserver(token)
        }
        tokens.removeAll()
    }
}

@MainActor
public final class NSWorkspaceDisplayAsleepDetector: DisplayAsleepDetector {
    public private(set) var isDisplayAsleep = false
    private var tokens: [NSObjectProtocol] = []

    public init() {}

    public func startObserving(_ handler: @escaping @MainActor () -> Void) {
        guard tokens.isEmpty else {
            return
        }
        let center = NSWorkspace.shared.notificationCenter
        let sleepToken = center.addObserver(
            forName: NSWorkspace.screensDidSleepNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.isDisplayAsleep = true
                handler()
            }
        }
        let wakeToken = center.addObserver(
            forName: NSWorkspace.screensDidWakeNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.isDisplayAsleep = false
                handler()
            }
        }
        tokens.append(sleepToken)
        tokens.append(wakeToken)
    }

    public func stopObserving() {
        let center = NSWorkspace.shared.notificationCenter
        for token in tokens {
            center.removeObserver(token)
        }
        tokens.removeAll()
    }
}
