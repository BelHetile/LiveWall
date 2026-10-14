import AppKit

@MainActor
public protocol PlaybackScreenHost: AnyObject {
    var displayIDs: [UInt32] { get }
    func sync() -> [UInt32]
    func install(_ view: NSView, on displayID: UInt32)
    func removeAllViews()
}

@MainActor
public final class ScreenSetHost: PlaybackScreenHost {
    public let screenSet: ScreenSet

    public init(screenSet: ScreenSet = ScreenSet()) {
        self.screenSet = screenSet
    }

    public var displayIDs: [UInt32] {
        return Array(screenSet.windows.keys)
    }

    public func sync() -> [UInt32] {
        _ = screenSet.syncWithScreens()
        return displayIDs
    }

    public func install(_ view: NSView, on displayID: UInt32) {
        screenSet.window(for: displayID)?.installContentView(view)
    }

    public func removeAllViews() {
        for window in screenSet.windows.values {
            window.contentView = nil
        }
    }
}
