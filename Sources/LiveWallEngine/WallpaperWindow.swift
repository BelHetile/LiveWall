import AppKit
import CoreGraphics

@MainActor
public final class WallpaperWindow: NSWindow {
    public static var wallpaperLevel: NSWindow.Level {
        NSWindow.Level(rawValue: Int(CGWindowLevelForKey(.desktopIconWindow)) - 1)
    }

    public let displayID: UInt32

    public init(displayID: UInt32, frame: CGRect) {
        self.displayID = displayID
        super.init(contentRect: frame, styleMask: .borderless, backing: .buffered, defer: false)
        level = Self.wallpaperLevel
        collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle]
        hasShadow = false
        ignoresMouseEvents = true
        isReleasedWhenClosed = false
        isOpaque = true
        backgroundColor = .black
        animationBehavior = .none
    }

    public override var canBecomeKey: Bool {
        return false
    }

    public override var canBecomeMain: Bool {
        return false
    }

    public func installContentView(_ view: NSView) {
        view.frame = contentRect(forFrameRect: frame)
        view.autoresizingMask = [.width, .height]
        contentView = view
    }
}
