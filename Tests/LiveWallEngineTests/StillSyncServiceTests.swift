import AppKit
import CoreGraphics
import Foundation
import Testing
@testable import LiveWallCore
@testable import LiveWallEngine

@MainActor
final class FakeDesktopPictureWorkspace: DesktopPictureWorkspace {
    var screenList: [ScreenDescriptor]
    var images: [UInt32: URL]
    private(set) var setCalls: [(displayID: UInt32, url: URL)] = []

    init(screenList: [ScreenDescriptor], images: [UInt32: URL]) {
        self.screenList = screenList
        self.images = images
    }

    var screens: [ScreenDescriptor] {
        return screenList
    }

    func desktopImageURL(for displayID: UInt32) -> URL? {
        return images[displayID]
    }

    func setDesktopImageURL(_ url: URL, for displayID: UInt32) throws {
        guard screenList.contains(where: { $0.displayID == displayID }) else {
            return
        }
        images[displayID] = url
        setCalls.append((displayID, url))
    }
}

private func screenDescriptor(_ id: UInt32) -> ScreenDescriptor {
    return ScreenDescriptor(displayID: id, frame: CGRect(x: 0, y: 0, width: 1920, height: 1080))
}

private func makeRoot() -> URL {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("lwstill-\(UUID().uuidString)")
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

private let imageRequest = StillRequest(url: Fixtures.still, kind: .image)

@Suite @MainActor struct StillSyncServiceTests {
    private func makeService(
        screens: [ScreenDescriptor],
        originals: [UInt32: URL],
        root: URL
    ) -> (StillSyncService, FakeDesktopPictureWorkspace, SwapStateStore) {
        let workspace = FakeDesktopPictureWorkspace(screenList: screens, images: originals)
        let store = SwapStateStore(url: root.appendingPathComponent("swap-state.json"))
        let service = StillSyncService(
            workspace: workspace,
            store: store,
            stillDirectory: root.appendingPathComponent("stills")
        )
        return (service, workspace, store)
    }

    @Test func takeoverCapturesOriginalsAndAppliesStill() async throws {
        let one = URL(fileURLWithPath: "/orig/one.heic")
        let two = URL(fileURLWithPath: "/orig/two.jpg")
        let (service, workspace, store) = makeService(
            screens: [screenDescriptor(1), screenDescriptor(2)],
            originals: [1: one, 2: two],
            root: makeRoot()
        )

        try await service.takeover(imageRequest)

        #expect(service.isOwning)
        let still = try #require(service.currentStillURL)
        #expect(workspace.images[1] == still)
        #expect(workspace.images[2] == still)

        let state = try #require(try await store.load())
        #expect(state.displays.count == 2)
        #expect(state.displays.first(where: { $0.displayID == 1 })?.originalURL == one.absoluteString)
        #expect(state.displays.first(where: { $0.displayID == 2 })?.originalURL == two.absoluteString)
    }

    @Test func takeoverSkipsScreensWithoutOriginals() async throws {
        let one = URL(fileURLWithPath: "/orig/one.heic")
        let (service, workspace, store) = makeService(
            screens: [screenDescriptor(1), screenDescriptor(2)],
            originals: [1: one],
            root: makeRoot()
        )

        try await service.takeover(imageRequest)

        let state = try #require(try await store.load())
        #expect(state.displays.map(\.displayID) == [1])
        #expect(workspace.images[2] == nil)
    }

    @Test func takeoverWhenAlreadyOwningIsNoOp() async throws {
        let one = URL(fileURLWithPath: "/orig/one.heic")
        let (service, workspace, store) = makeService(
            screens: [screenDescriptor(1)],
            originals: [1: one],
            root: makeRoot()
        )

        try await service.takeover(imageRequest)
        let first = try await store.load()
        let calls = workspace.setCalls.count

        workspace.images[1] = one
        try await service.takeover(StillRequest(url: Fixtures.animation, kind: .gif))

        let second = try await store.load()
        #expect(first == second)
        #expect(workspace.images[1] == one)
        #expect(workspace.setCalls.count == calls)
    }

    @Test func restoreWhenNotOwningIsNoOp() async throws {
        let (service, workspace, _) = makeService(
            screens: [screenDescriptor(1)],
            originals: [1: URL(fileURLWithPath: "/orig/one.heic")],
            root: makeRoot()
        )

        try await service.restore()

        #expect(workspace.setCalls.isEmpty)
        #expect(service.isOwning == false)
    }

    @Test func restoreReturnsOriginalsAndClearsState() async throws {
        let one = URL(fileURLWithPath: "/orig/one.heic")
        let two = URL(fileURLWithPath: "/orig/two.jpg")
        let (service, workspace, store) = makeService(
            screens: [screenDescriptor(1), screenDescriptor(2)],
            originals: [1: one, 2: two],
            root: makeRoot()
        )

        try await service.takeover(imageRequest)
        try await service.restore()

        #expect(workspace.images[1] == one)
        #expect(workspace.images[2] == two)
        #expect(service.isOwning == false)
        #expect(service.currentStillURL == nil)
        let state = try await store.load()
        #expect(state == nil)
    }

    @Test func restoreSkipsScreensThatDisappeared() async throws {
        let one = URL(fileURLWithPath: "/orig/one.heic")
        let two = URL(fileURLWithPath: "/orig/two.jpg")
        let (service, workspace, _) = makeService(
            screens: [screenDescriptor(1), screenDescriptor(2)],
            originals: [1: one, 2: two],
            root: makeRoot()
        )

        try await service.takeover(imageRequest)
        let still = try #require(service.currentStillURL)

        workspace.screenList = [screenDescriptor(1)]
        let before = workspace.setCalls.count
        try await service.restore()

        let restored = Array(workspace.setCalls.dropFirst(before))
        #expect(restored.count == 1)
        #expect(restored.first?.displayID == 1)
        #expect(workspace.images[1] == one)
        #expect(workspace.images[2] == still)
    }

    @Test func updateStillWhenNotOwningIsNoOp() async throws {
        let (service, workspace, _) = makeService(
            screens: [screenDescriptor(1)],
            originals: [1: URL(fileURLWithPath: "/orig/one.heic")],
            root: makeRoot()
        )

        try await service.updateStill(imageRequest)

        #expect(workspace.setCalls.isEmpty)
        #expect(service.currentStillURL == nil)
    }

    @Test func updateStillReplacesAppliedStill() async throws {
        let one = URL(fileURLWithPath: "/orig/one.heic")
        let (service, workspace, _) = makeService(
            screens: [screenDescriptor(1)],
            originals: [1: one],
            root: makeRoot()
        )

        try await service.takeover(imageRequest)
        let first = try #require(service.currentStillURL)

        try await service.updateStill(StillRequest(url: Fixtures.still, kind: .image, stillTime: 5))
        let second = try #require(service.currentStillURL)

        #expect(first != second)
        #expect(workspace.images[1] == second)
    }

    @Test func spaceChangeReappliesStill() async throws {
        let one = URL(fileURLWithPath: "/orig/one.heic")
        let (service, workspace, _) = makeService(
            screens: [screenDescriptor(1)],
            originals: [1: one],
            root: makeRoot()
        )

        try await service.takeover(imageRequest)
        let still = try #require(service.currentStillURL)

        workspace.images[1] = one
        service.handleSpaceChange()

        #expect(workspace.images[1] == still)
    }

    @Test func spaceChangeWhileLockedIsSkippedAndUnlockReapplies() async throws {
        let one = URL(fileURLWithPath: "/orig/one.heic")
        let (service, workspace, _) = makeService(
            screens: [screenDescriptor(1)],
            originals: [1: one],
            root: makeRoot()
        )

        try await service.takeover(imageRequest)
        let still = try #require(service.currentStillURL)

        service.handleLockChange(true)
        #expect(service.isLocked)

        workspace.images[1] = one
        service.handleSpaceChange()
        #expect(workspace.images[1] == one)

        service.handleLockChange(false)
        #expect(service.isLocked == false)
        #expect(workspace.images[1] == still)
    }

    @Test func wallpaperChangeIsNoOpWhenStillAlreadyApplied() async throws {
        let one = URL(fileURLWithPath: "/orig/one.heic")
        let (service, workspace, _) = makeService(
            screens: [screenDescriptor(1)],
            originals: [1: one],
            root: makeRoot()
        )

        try await service.takeover(imageRequest)
        let before = workspace.setCalls.count

        service.handleWallpaperChange()

        #expect(workspace.setCalls.count == before)
    }

    @Test func wallpaperChangeReassertsStillWhenOverwritten() async throws {
        let one = URL(fileURLWithPath: "/orig/one.heic")
        let (service, workspace, _) = makeService(
            screens: [screenDescriptor(1)],
            originals: [1: one],
            root: makeRoot()
        )

        try await service.takeover(imageRequest)
        let still = try #require(service.currentStillURL)

        workspace.images[1] = one
        service.handleWallpaperChange()

        #expect(workspace.images[1] == still)
    }

    @Test func screenChangeAdoptsNewScreensAndAppliesStill() async throws {
        let one = URL(fileURLWithPath: "/orig/one.heic")
        let two = URL(fileURLWithPath: "/orig/two.jpg")
        let (service, workspace, store) = makeService(
            screens: [screenDescriptor(1)],
            originals: [1: one],
            root: makeRoot()
        )

        try await service.takeover(imageRequest)
        let still = try #require(service.currentStillURL)

        workspace.screenList = [screenDescriptor(1), screenDescriptor(2)]
        workspace.images[2] = two
        service.handleScreenChange()

        let deadline = ContinuousClock.now.advanced(by: .seconds(2))
        while workspace.images[2] != still && ContinuousClock.now < deadline {
            try await Task.sleep(for: .milliseconds(5))
        }

        #expect(workspace.images[2] == still)
        let state = try #require(try await store.load())
        #expect(state.displays.map(\.displayID).sorted() == [1, 2])
    }

    @Test func recoverIfNeededRestoresPersistedOriginals() async throws {
        let root = makeRoot()
        let store = SwapStateStore(url: root.appendingPathComponent("swap-state.json"))
        let still = URL(fileURLWithPath: "/stills/crashed.png")
        try await store.save(
            SwapState(
                displays: [SwapState.DisplayEntry(displayID: 1, originalURL: "file:///orig/one.heic")],
                takenAt: Date()
            )
        )
        let workspace = FakeDesktopPictureWorkspace(screenList: [screenDescriptor(1)], images: [1: still])
        let service = StillSyncService(
            workspace: workspace,
            store: store,
            stillDirectory: root.appendingPathComponent("stills")
        )

        try await service.recoverIfNeeded()

        #expect(workspace.images[1] == URL(fileURLWithPath: "/orig/one.heic"))
        #expect(service.isOwning == false)
        let state = try await store.load()
        #expect(state == nil)
    }

    @Test func recoverIfNeededWithoutPersistedStateIsNoOp() async throws {
        let (service, workspace, _) = makeService(
            screens: [screenDescriptor(1)],
            originals: [1: URL(fileURLWithPath: "/orig/one.heic")],
            root: makeRoot()
        )

        try await service.recoverIfNeeded()

        #expect(workspace.setCalls.isEmpty)
    }
}

@Suite @MainActor struct StillRendererTests {
    @Test func stillFileNamingIsStablePerItem() {
        let renderer = StillRenderer(directory: URL(fileURLWithPath: "/tmp/lw-stills"))
        let base = renderer.stillFileURL(for: StillRequest(url: Fixtures.still, kind: .image))
        let again = renderer.stillFileURL(for: StillRequest(url: Fixtures.still, kind: .image))
        let otherTime = renderer.stillFileURL(for: StillRequest(url: Fixtures.still, kind: .image, stillTime: 5))
        let otherKind = renderer.stillFileURL(for: StillRequest(url: Fixtures.still, kind: .gif))

        #expect(base == again)
        #expect(base != otherTime)
        #expect(base != otherKind)
        #expect(base.lastPathComponent.hasPrefix("still-"))
        #expect(base.pathExtension == "png")
    }

    @Test func renderCreatesPngAndDoesNotRewriteExisting() async throws {
        let root = makeRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let renderer = StillRenderer(directory: root)
        let request = StillRequest(url: Fixtures.still, kind: .image)

        let file = try await renderer.render(request)
        #expect(FileManager.default.fileExists(atPath: file.path))

        let sentinel = Data("SENTINEL".utf8)
        try sentinel.write(to: file)
        let second = try await renderer.render(request)

        #expect(second == file)
        #expect(try Data(contentsOf: file) == sentinel)
    }
}
