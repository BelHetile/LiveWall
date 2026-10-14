import Foundation

@MainActor
public protocol AppNapGuard: AnyObject {
    var isActive: Bool { get }
    func begin()
    func end()
}

@MainActor
public final class ProcessInfoAppNapGuard: AppNapGuard {
    private var token: NSObjectProtocol?

    public init() {}

    public var isActive: Bool {
        return token != nil
    }

    public func begin() {
        guard token == nil else {
            return
        }
        token = ProcessInfo.processInfo.beginActivity(
            options: .userInitiatedAllowingIdleSystemSleep,
            reason: "LiveWall wallpaper playback"
        )
    }

    public func end() {
        guard let token else {
            return
        }
        ProcessInfo.processInfo.endActivity(token)
        self.token = nil
    }

    isolated deinit {
        if let token {
            ProcessInfo.processInfo.endActivity(token)
        }
    }
}
