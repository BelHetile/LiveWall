import AppKit
import CoreGraphics
import Testing
@testable import LiveWallEngine

@Suite struct ScreenSetPlannerTests {
    private let primary = ScreenDescriptor(displayID: 1, frame: CGRect(x: 0, y: 0, width: 1920, height: 1080))
    private let secondary = ScreenDescriptor(displayID: 2, frame: CGRect(x: 1920, y: 0, width: 1280, height: 800))

    @Test func initialSyncAddsEverything() {
        let change = ScreenSetPlanner.change(from: [], to: [primary, secondary])
        #expect(change.added == [1, 2])
        #expect(change.removed.isEmpty)
        #expect(change.moved.isEmpty)
        #expect(change.unchanged.isEmpty)
    }

    @Test func detectsAddedRemovedMovedUnchanged() {
        let moved = ScreenDescriptor(displayID: 2, frame: CGRect(x: 1920, y: 0, width: 2560, height: 1440))
        let added = ScreenDescriptor(displayID: 3, frame: CGRect(x: 0, y: 1080, width: 800, height: 600))

        let change = ScreenSetPlanner.change(from: [primary, secondary], to: [primary, moved, added])

        #expect(change.added == [3])
        #expect(change.removed.isEmpty)
        #expect(change.moved == [2])
        #expect(change.unchanged == [1])
    }

    @Test func detectsRemoval() {
        let change = ScreenSetPlanner.change(from: [primary, secondary], to: [primary])
        #expect(change.added.isEmpty)
        #expect(change.removed == [2])
        #expect(change.moved.isEmpty)
        #expect(change.unchanged == [1])
    }

    @Test func identicalTopologyIsAllUnchanged() {
        let change = ScreenSetPlanner.change(from: [primary, secondary], to: [secondary, primary])
        #expect(change.added.isEmpty)
        #expect(change.removed.isEmpty)
        #expect(change.moved.isEmpty)
        #expect(change.unchanged == [1, 2])
    }

    @Test func frameShiftCountsAsMoved() {
        let shifted = ScreenDescriptor(displayID: 1, frame: CGRect(x: 0, y: 0, width: 2560, height: 1440))
        let change = ScreenSetPlanner.change(from: [primary], to: [shifted])
        #expect(change.moved == [1])
        #expect(change.unchanged.isEmpty)
    }
}

@Suite struct WallpaperWindowLevelTests {
    @Test @MainActor func levelMatchesDesktopIconWindowMinusOne() {
        let expected = Int(CGWindowLevelForKey(.desktopIconWindow)) - 1
        #expect(WallpaperWindow.wallpaperLevel.rawValue == expected)
    }

    @Test @MainActor func levelMatchesPhaseOneMeasurement() {
        #expect(WallpaperWindow.wallpaperLevel.rawValue == -2147483604)
    }
}
