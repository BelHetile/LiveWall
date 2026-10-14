import AppKit
import CoreGraphics
import Foundation

@MainActor
public protocol DesktopPictureWorkspace: AnyObject {
    var screens: [ScreenDescriptor] { get }
    func desktopImageURL(for displayID: UInt32) -> URL?
    func setDesktopImageURL(_ url: URL, for displayID: UInt32) throws
}

@MainActor
public final class NSWorkspaceDesktopPicture: DesktopPictureWorkspace {
    private let workspace: NSWorkspace

    public init(workspace: NSWorkspace = .shared) {
        self.workspace = workspace
    }

    public var screens: [ScreenDescriptor] {
        return NSScreen.screens.compactMap { screen in
            guard let id = ScreenDescriptor.displayID(of: screen) else {
                return nil
            }
            return ScreenDescriptor(displayID: id, frame: screen.frame)
        }
    }

    public func desktopImageURL(for displayID: UInt32) -> URL? {
        guard let screen = screen(for: displayID) else {
            return nil
        }
        return workspace.desktopImageURL(for: screen)
    }

    public func setDesktopImageURL(_ url: URL, for displayID: UInt32) throws {
        guard let screen = screen(for: displayID) else {
            return
        }
        try workspace.setDesktopImageURL(url, for: screen, options: [:])
    }

    private func screen(for displayID: UInt32) -> NSScreen? {
        return NSScreen.screens.first { ScreenDescriptor.displayID(of: $0) == displayID }
    }
}
