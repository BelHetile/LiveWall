import Foundation

@MainActor
public protocol PowerStateProviding: AnyObject {
    var isLowPowerModeEnabled: Bool { get }
}

@MainActor
public final class ProcessInfoPowerState: PowerStateProviding {
    public init() {}

    public var isLowPowerModeEnabled: Bool {
        return ProcessInfo.processInfo.isLowPowerModeEnabled
    }
}

@MainActor
public final class PowerMonitor {
    public private(set) var isLowPowerModeEnabled: Bool
    public var onChange: ((Bool) -> Void)?

    private let provider: PowerStateProviding
    private let center: NotificationCenter
    private var observer: NSObjectProtocol?
    private var started = false

    public init(
        provider: PowerStateProviding = ProcessInfoPowerState(),
        center: NotificationCenter = .default
    ) {
        self.provider = provider
        self.center = center
        self.isLowPowerModeEnabled = provider.isLowPowerModeEnabled
    }

    public func start() {
        guard !started else {
            return
        }
        started = true
        observer = center.addObserver(
            forName: Notification.Name.NSProcessInfoPowerStateDidChange,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                _ = self?.refresh()
            }
        }
        refresh()
    }

    public func stop() {
        guard started else {
            return
        }
        started = false
        if let observer {
            center.removeObserver(observer)
        }
        observer = nil
    }

    @discardableResult
    public func refresh() -> Bool {
        let next = provider.isLowPowerModeEnabled
        guard next != isLowPowerModeEnabled else {
            return false
        }
        isLowPowerModeEnabled = next
        onChange?(next)
        return true
    }

    isolated deinit {
        if let observer {
            center.removeObserver(observer)
        }
    }
}
