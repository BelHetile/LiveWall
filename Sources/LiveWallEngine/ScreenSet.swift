import AppKit
import CoreGraphics

public struct ScreenDescriptor: Equatable, Hashable, Sendable {
    public let displayID: UInt32
    public let frame: CGRect

    public init(displayID: UInt32, frame: CGRect) {
        self.displayID = displayID
        self.frame = frame
    }

    @MainActor
    public static func displayID(of screen: NSScreen) -> UInt32? {
        let key = NSDeviceDescriptionKey("NSScreenNumber")
        return (screen.deviceDescription[key] as? NSNumber).map { $0.uint32Value }
    }
}

public struct ScreenSetChange: Equatable, Sendable {
    public let added: [UInt32]
    public let removed: [UInt32]
    public let moved: [UInt32]
    public let unchanged: [UInt32]

    public init(added: [UInt32], removed: [UInt32], moved: [UInt32], unchanged: [UInt32]) {
        self.added = added
        self.removed = removed
        self.moved = moved
        self.unchanged = unchanged
    }
}

public enum ScreenSetPlanner {
    public static func change(from current: [ScreenDescriptor], to next: [ScreenDescriptor]) -> ScreenSetChange {
        let currentByID = Dictionary(current.map { ($0.displayID, $0) }, uniquingKeysWith: { first, _ in first })
        let nextByID = Dictionary(next.map { ($0.displayID, $0) }, uniquingKeysWith: { first, _ in first })

        let added = nextByID.keys.filter { currentByID[$0] == nil }.sorted()
        let removed = currentByID.keys.filter { nextByID[$0] == nil }.sorted()

        var moved: [UInt32] = []
        var unchanged: [UInt32] = []
        for (id, descriptor) in nextByID {
            guard let previous = currentByID[id] else {
                continue
            }
            if previous.frame == descriptor.frame {
                unchanged.append(id)
            } else {
                moved.append(id)
            }
        }

        return ScreenSetChange(
            added: added.sorted(),
            removed: removed.sorted(),
            moved: moved.sorted(),
            unchanged: unchanged.sorted()
        )
    }
}

@MainActor
public final class ScreenSet {
    public private(set) var windows: [UInt32: WallpaperWindow] = [:]

    private var lastDescriptors: [ScreenDescriptor] = []
    private var observer: NSObjectProtocol?

    public init() {}

    public static func currentDescriptors() -> [ScreenDescriptor] {
        return NSScreen.screens.compactMap { screen in
            guard let id = ScreenDescriptor.displayID(of: screen) else {
                return nil
            }
            return ScreenDescriptor(displayID: id, frame: screen.frame)
        }
    }

    @discardableResult
    public func sync(_ descriptors: [ScreenDescriptor]) -> ScreenSetChange {
        let change = ScreenSetPlanner.change(from: lastDescriptors, to: descriptors)

        for id in change.removed {
            if let window = windows.removeValue(forKey: id) {
                window.orderOut(nil)
                window.close()
            }
        }

        let byID = Dictionary(descriptors.map { ($0.displayID, $0) }, uniquingKeysWith: { first, _ in first })

        for id in change.added {
            guard let descriptor = byID[id] else {
                continue
            }
            let window = WallpaperWindow(displayID: id, frame: descriptor.frame)
            windows[id] = window
            window.orderFrontRegardless()
        }

        for id in change.moved {
            guard let descriptor = byID[id], let window = windows[id] else {
                continue
            }
            window.setFrame(descriptor.frame, display: true)
        }

        lastDescriptors = descriptors
        return change
    }

    @discardableResult
    public func syncWithScreens() -> ScreenSetChange {
        return sync(Self.currentDescriptors())
    }

    public func window(for displayID: UInt32) -> WallpaperWindow? {
        return windows[displayID]
    }

    public func startObserving() {
        guard observer == nil else {
            return
        }
        observer = NotificationCenter.default.addObserver(
            forName: NSApplication.didChangeScreenParametersNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                _ = self?.syncWithScreens()
            }
        }
    }

    public func stopObserving() {
        guard let observer else {
            return
        }
        NotificationCenter.default.removeObserver(observer)
        self.observer = nil
    }

    public func removeAll() {
        for (_, window) in windows {
            window.orderOut(nil)
            window.close()
        }
        windows.removeAll()
        lastDescriptors.removeAll()
    }

    isolated deinit {
        if let observer {
            NotificationCenter.default.removeObserver(observer)
        }
    }
}
