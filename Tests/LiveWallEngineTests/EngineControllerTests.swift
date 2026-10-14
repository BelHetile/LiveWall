import AppKit
import Foundation
import Testing
@testable import LiveWallCore
@testable import LiveWallEngine
@testable import LiveWallIPC

@MainActor
final class FakePlaybackNode: PlaybackNode {
    let view = NSView()
    private(set) var isPlaying = false
    var isMuted = false

    func play() {
        isPlaying = true
    }

    func pause() {
        isPlaying = false
    }
}

extension FakePlaybackNode: MuteControllable {}

@MainActor
final class FakeScreenHost: PlaybackScreenHost {
    var displays: [UInt32]
    private(set) var installs: [(displayID: UInt32, view: NSView)] = []

    init(displays: [UInt32]) {
        self.displays = displays
    }

    var displayIDs: [UInt32] {
        return displays
    }

    func sync() -> [UInt32] {
        return displays
    }

    func install(_ view: NSView, on displayID: UInt32) {
        installs.append((displayID, view))
    }

    func removeAllViews() {
        installs.removeAll()
    }
}

@MainActor
final class FakeNodeFactory: PlaybackNodeFactory {
    private(set) var created: [FakePlaybackNode] = []
    var failingItemIDs: Set<String> = []

    func makeNode(for item: WallpaperItem, muted: Bool) async throws -> PlaybackNode {
        if failingItemIDs.contains(item.id) {
            throw LiveWallError.mediaUnreadable(path: item.url.path, reason: "fake")
        }
        let node = FakePlaybackNode()
        node.isMuted = muted
        created.append(node)
        return node
    }
}

@MainActor
final class FakeAppNapGuard: AppNapGuard {
    private(set) var isActive = false
    private(set) var beginCount = 0
    private(set) var endCount = 0

    func begin() {
        guard !isActive else {
            return
        }
        isActive = true
        beginCount += 1
    }

    func end() {
        guard isActive else {
            return
        }
        isActive = false
        endCount += 1
    }
}

private func item(_ name: String, kind: ItemKind) -> WallpaperItem {
    return WallpaperItem(id: name, url: URL(fileURLWithPath: "/tmp/\(name)"), kind: kind)
}

@Suite @MainActor struct EngineControllerTests {
    private func makeController(
        items: [WallpaperItem],
        displays: [UInt32] = [1],
        stillSync: StillSyncService? = nil,
        snapshotService: SystemSnapshotService? = nil,
        power: PowerMonitor? = nil,
        ruleConfiguration: RuleConfig = RuleConfig(),
        randomIndex: @escaping (Range<Int>) -> Int = { Int.random(in: $0) }
    ) -> (EngineController, FakeScreenHost, FakeNodeFactory, FakeAppNapGuard) {
        let host = FakeScreenHost(displays: displays)
        let factory = FakeNodeFactory()
        let appNap = FakeAppNapGuard()
        let controller = EngineController(
            source: StaticWallpaperSource(items: items),
            screenHost: host,
            nodeFactory: factory,
            stillSync: stillSync,
            snapshotService: snapshotService,
            hotkeys: nil,
            power: power,
            appNap: appNap,
            ruleConfiguration: ruleConfiguration,
            randomIndex: randomIndex
        )
        return (controller, host, factory, appNap)
    }

    @Test func startAppliesFirstItemAndPlays() async {
        let items = [item("a", kind: .image), item("b", kind: .image), item("c", kind: .image)]
        let (controller, host, factory, appNap) = makeController(items: items)

        await controller.start()

        #expect(controller.currentIndex == 0)
        #expect(controller.currentItem?.id == "a")
        #expect(controller.isPlaying)
        #expect(factory.created.count == 1)
        #expect(host.installs.count == 1)
        #expect(appNap.isActive)
        #expect(status(controller).path == "/tmp/a")
    }

    @Test func nextWrapsAroundSource() async {
        let items = [item("a", kind: .image), item("b", kind: .image), item("c", kind: .image)]
        let (controller, _, _, _) = makeController(items: items)
        await controller.start()

        await controller.next()
        #expect(controller.currentItem?.id == "b")
        await controller.next()
        #expect(controller.currentItem?.id == "c")
        await controller.next()
        #expect(controller.currentItem?.id == "a")
    }

    @Test func previousWrapsBackwards() async {
        let items = [item("a", kind: .image), item("b", kind: .image), item("c", kind: .image)]
        let (controller, _, _, _) = makeController(items: items)
        await controller.start()

        await controller.previous()
        #expect(controller.currentItem?.id == "c")
        await controller.previous()
        #expect(controller.currentItem?.id == "b")
    }

    @Test func randomMovesWithinBoundsAndAvoidsCurrent() async {
        let items = [item("a", kind: .image), item("b", kind: .image), item("c", kind: .image)]
        let (controller, _, _, _) = makeController(items: items, randomIndex: { _ in 0 })
        await controller.start()

        await controller.random()
        #expect(controller.currentItem?.id != "a")
        #expect(controller.currentIndex >= 0 && controller.currentIndex < items.count)
    }

    @Test func randomSingleItemStaysPut() async {
        let (controller, _, _, _) = makeController(items: [item("a", kind: .image)])
        await controller.start()

        await controller.random()
        #expect(controller.currentItem?.id == "a")
    }

    @Test func nextWithoutItemsIsNoOp() async {
        let (controller, _, _, _) = makeController(items: [])
        await controller.start()

        await controller.next()
        #expect(controller.currentItem == nil)
        #expect(controller.currentIndex == -1)
    }

    @Test func pauseResumeTogglesPlayingAndAppNap() async {
        let (controller, _, factory, appNap) = makeController(items: [item("a", kind: .image)])
        await controller.start()
        let node = factory.created[0]
        #expect(node.isPlaying)

        controller.pause()
        #expect(!controller.isPlaying)
        #expect(!node.isPlaying)
        #expect(!appNap.isActive)

        controller.resume()
        #expect(controller.isPlaying)
        #expect(node.isPlaying)
        #expect(appNap.isActive)
    }

    @Test func muteUnmuteUpdatesNodesAndStatus() async {
        let (controller, _, factory, _) = makeController(items: [item("a", kind: .image)])
        await controller.start()
        let node = factory.created[0]
        #expect(controller.isMuted)
        #expect(node.isMuted)

        controller.unmute()
        #expect(!controller.isMuted)
        #expect(!node.isMuted)
        #expect(!status(controller).muted)

        controller.mute()
        #expect(controller.isMuted)
        #expect(node.isMuted)
    }

    @Test func lowPowerPausesAndReverts() async {
        let (controller, _, factory, _) = makeController(items: [item("a", kind: .image)])
        await controller.start()
        let node = factory.created[0]

        controller.applyLowPower(true)
        #expect(controller.isLowPowerMode)
        #expect(!controller.isPlaying)
        #expect(!node.isPlaying)

        controller.applyLowPower(false)
        #expect(!controller.isLowPowerMode)
        #expect(controller.isPlaying)
        #expect(node.isPlaying)
    }

    @Test func ruleActionPausesAndReverts() async {
        let (controller, _, factory, _) = makeController(items: [item("a", kind: .image)])
        await controller.start()
        let node = factory.created[0]

        controller.applyRuleAction(.pause)
        #expect(!controller.isPlaying)
        #expect(!node.isPlaying)

        controller.applyRuleAction(.keepRunning)
        #expect(controller.isPlaying)
        #expect(node.isPlaying)
    }

    @Test func ruleEvaluationSubscriptionTriggersPause() async {
        let fullscreen = FakeFullscreenDetector()
        let snapshotService = SystemSnapshotService(
            fullscreen: fullscreen,
            coverage: FakeCoverageDetector(),
            audio: FakeAudioPlayingDetector(),
            apps: FakeFrontmostDetector(),
            display: FakeDisplayAsleepDetector(),
            pollInterval: 3600,
            selfBundleID: "io.github.belhetile.livewall"
        )
        let rules = RuleConfig(displayAsleepAction: .keepRunning, fullscreenAction: .pause, coverageAction: .keepRunning)
        let (controller, _, factory, _) = makeController(items: [item("a", kind: .image)], snapshotService: snapshotService, ruleConfiguration: rules)
        await controller.start()
        let node = factory.created[0]
        #expect(controller.isPlaying)

        fullscreen.value = true
        snapshotService.refresh()
        #expect(controller.ruleAction == .pause)
        #expect(!controller.isPlaying)
        #expect(!node.isPlaying)

        fullscreen.value = false
        snapshotService.refresh()
        #expect(controller.ruleAction == .keepRunning)
        #expect(controller.isPlaying)
        #expect(node.isPlaying)
    }

    @Test func lowPowerNotificationPathPauses() async {
        let provider = FakePowerStateProvider(false)
        let center = NotificationCenter()
        let power = PowerMonitor(provider: provider, center: center)
        let (controller, _, factory, _) = makeController(items: [item("a", kind: .image)], power: power)
        await controller.start()
        let node = factory.created[0]
        #expect(controller.isPlaying)

        provider.isLowPowerModeEnabled = true
        center.post(name: Notification.Name.NSProcessInfoPowerStateDidChange, object: nil)
        pumpRunLoop(0.1)
        #expect(controller.isLowPowerMode)
        #expect(!node.isPlaying)

        provider.isLowPowerModeEnabled = false
        center.post(name: Notification.Name.NSProcessInfoPowerStateDidChange, object: nil)
        pumpRunLoop(0.1)
        #expect(!controller.isLowPowerMode)
        #expect(node.isPlaying)
    }

    @Test func applyByPathSelectsKnownItem() async throws {
        let items = [
            WallpaperItem(id: "still", url: Fixtures.still, kind: .image),
            WallpaperItem(id: "gif", url: Fixtures.animation, kind: .gif)
        ]
        let (controller, _, _, _) = makeController(items: items)
        await controller.start()

        try await controller.applyByPath(Fixtures.animation.path)
        #expect(controller.currentItem?.id == "gif")
        #expect(controller.currentIndex == 1)
    }

    @Test func applyByPathRejectsMissingFile() async {
        let (controller, _, _, _) = makeController(items: [item("a", kind: .image)])
        await controller.start()

        await #expect(throws: LiveWallError.mediaUnreadable(path: "/tmp/does-not-exist.mp4", reason: "missing file")) {
            try await controller.applyByPath("/tmp/does-not-exist.mp4")
        }
    }

    @Test func handleMapsVerbsToStatusData() async throws {
        let (controller, _, _, _) = makeController(items: [item("a", kind: .image), item("b", kind: .image)])
        await controller.start()

        let pause = await controller.handle(Request(cmd: .pause))
        #expect(pause.ok)
        #expect(pause.data == .object([
            "playing": .bool(false),
            "paused": .bool(true),
            "muted": .bool(true),
            "lowPower": .bool(false),
            "rule": .string("keep-running"),
            "index": .number(0),
            "count": .number(2),
            "id": .string("a"),
            "current": .string("/tmp/a")
        ]))

        _ = await controller.handle(Request(cmd: .resume))
        _ = await controller.handle(Request(cmd: .next))
        let current = await controller.handle(Request(cmd: .getCurrent))
        guard case let .object(object)? = current.data else {
            Issue.record("expected object data")
            return
        }
        #expect(object["current"] == .string("/tmp/b"))

        let open = await controller.handle(Request(cmd: .open))
        #expect(!open.ok)
        #expect(open.error?.code == "EINVAL")
    }

    @Test func shutdownRestoresStillAndEndsActivity() async {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("lwctrl-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }

        let workspace = FakeDesktopPictureWorkspace(
            screenList: [ScreenDescriptor(displayID: 1, frame: CGRect(x: 0, y: 0, width: 100, height: 100))],
            images: [1: URL(fileURLWithPath: "/orig/one.heic")]
        )
        let stillSync = StillSyncService(
            workspace: workspace,
            store: SwapStateStore(url: root.appendingPathComponent("swap-state.json")),
            stillDirectory: root.appendingPathComponent("stills")
        )
        let (controller, _, _, appNap) = makeController(
            items: [WallpaperItem(id: "still", url: Fixtures.still, kind: .image)],
            stillSync: stillSync
        )
        await controller.start()
        #expect(stillSync.isOwning)
        #expect(workspace.images[1] != URL(fileURLWithPath: "/orig/one.heic"))

        await controller.shutdown()
        #expect(!stillSync.isOwning)
        #expect(workspace.images[1] == URL(fileURLWithPath: "/orig/one.heic"))
        #expect(!appNap.isActive)
    }

    private func status(_ controller: EngineController) -> EngineStatus {
        return controller.status()
    }
}
