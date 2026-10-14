import AppKit
import Testing
@testable import LiveWallEngine

@Suite(.enabled(if: EngineE2E.enabled)) struct ScreenSetE2ETests {
    @Test @MainActor func windowsExistAtExpectedLevelOnAllScreens() throws {
        _ = NSApplication.shared
        let screens = NSScreen.screens
        try #require(!screens.isEmpty)

        let set = ScreenSet()
        defer { set.removeAll() }
        _ = set.syncWithScreens()

        #expect(set.windows.count == screens.count)
        for screen in screens {
            let id = try #require(ScreenDescriptor.displayID(of: screen))
            let window = try #require(set.window(for: id))

            #expect(window.level == WallpaperWindow.wallpaperLevel)
            #expect(window.frame == screen.frame)
            #expect(window.collectionBehavior.contains(.canJoinAllSpaces))
            #expect(window.collectionBehavior.contains(.stationary))
            #expect(window.collectionBehavior.contains(.ignoresCycle))
            #expect(window.ignoresMouseEvents)
            #expect(window.hasShadow == false)
            #expect(window.canBecomeKey == false)
            #expect(window.canBecomeMain == false)
            #expect(window.isVisible)
        }
    }

    @Test @MainActor func displayConfigChangeResyncsWithoutLeakingWindows() throws {
        _ = NSApplication.shared
        let screens = NSScreen.screens
        try #require(!screens.isEmpty)

        let set = ScreenSet()
        defer { set.removeAll() }

        _ = set.sync([])
        #expect(set.windows.isEmpty)

        set.startObserving()
        defer { set.stopObserving() }
        NotificationCenter.default.post(
            name: NSApplication.didChangeScreenParametersNotification,
            object: nil
        )
        RunLoop.main.run(until: Date().addingTimeInterval(0.2))

        #expect(set.windows.count == screens.count)
        set.removeAll()
        #expect(set.windows.isEmpty)
    }
}
