import AppKit
import Foundation
import LiveWallCore
import LiveWallIPC

@MainActor
public final class EngineController {
    public private(set) var currentItem: WallpaperItem?
    public private(set) var currentIndex: Int = -1
    public private(set) var isPaused = false
    public private(set) var isMuted = true
    public private(set) var isLowPowerMode = false
    public private(set) var ruleAction: RuleAction = .keepRunning
    public private(set) var lastError: Error?

    public var ruleConfiguration: RuleConfig

    private let source: WallpaperSource
    private let screenHost: PlaybackScreenHost
    private let nodeFactory: PlaybackNodeFactory
    private let stillSync: StillSyncService?
    private let snapshotService: SystemSnapshotService?
    private let hotkeys: HotkeyManager?
    private let power: PowerMonitor?
    private let appNap: AppNapGuard
    private let randomIndex: (Range<Int>) -> Int

    private var items: [WallpaperItem] = []
    private var nodes: [UInt32: PlaybackNode] = [:]
    private var stillTakenOver = false
    private var started = false
    private var screenObserver: NSObjectProtocol?

    public init(
        source: WallpaperSource,
        screenHost: PlaybackScreenHost,
        nodeFactory: PlaybackNodeFactory,
        stillSync: StillSyncService? = nil,
        snapshotService: SystemSnapshotService? = nil,
        hotkeys: HotkeyManager? = nil,
        power: PowerMonitor? = nil,
        appNap: AppNapGuard,
        ruleConfiguration: RuleConfig = RuleConfig(),
        randomIndex: @escaping (Range<Int>) -> Int = { Int.random(in: $0) }
    ) {
        self.source = source
        self.screenHost = screenHost
        self.nodeFactory = nodeFactory
        self.stillSync = stillSync
        self.snapshotService = snapshotService
        self.hotkeys = hotkeys
        self.power = power
        self.appNap = appNap
        self.ruleConfiguration = ruleConfiguration
        self.randomIndex = randomIndex
    }

    public var itemCount: Int {
        return items.count
    }

    public var isPlaying: Bool {
        return shouldPlay
    }

    public var effectiveMuted: Bool {
        return isMuted || (currentItem?.config.muted ?? false)
    }

    public func start() async {
        guard !started else {
            return
        }
        started = true

        items = source.orderedItems()

        wireHotkeys()
        wirePower()
        wireRules()
        observeScreenChanges()
        snapshotService?.start()
        power?.start()

        guard !items.isEmpty else {
            return
        }
        currentIndex = 0
        await apply(items[0])
    }

    public func shutdown() async {
        screenObserver.map { NotificationCenter.default.removeObserver($0) }
        screenObserver = nil
        snapshotService?.stop()
        power?.stop()
        hotkeys?.onAction = nil
        appNap.end()

        for node in nodes.values {
            node.pause()
        }
        nodes.removeAll()
        screenHost.removeAllViews()

        if let stillSync, stillTakenOver {
            try? await stillSync.restore()
            stillTakenOver = false
        }

        started = false
    }

    public func apply(_ item: WallpaperItem) async {
        currentItem = item
        lastError = nil

        let displays = screenHost.sync()
        screenHost.removeAllViews()
        nodes.removeAll()

        for displayID in displays {
            do {
                let node = try await nodeFactory.makeNode(for: item, muted: effectiveMuted)
                nodes[displayID] = node
                screenHost.install(node.view, on: displayID)
            } catch {
                lastError = error
            }
        }

        await updateStill(for: item)
        reconcile()
    }

    public func applyByPath(_ path: String) async throws {
        let url = URL(fileURLWithPath: path)
        guard FileManager.default.fileExists(atPath: url.path) else {
            throw LiveWallError.mediaUnreadable(path: path, reason: "missing file")
        }
        if let index = items.firstIndex(where: { $0.url.standardizedFileURL == url.standardizedFileURL }) {
            currentIndex = index
            await apply(items[index])
            return
        }
        currentIndex = -1
        await apply(WallpaperItem(url: url, kind: WallpaperItem.kind(for: url)))
    }

    public func next() async {
        guard !items.isEmpty else {
            return
        }
        let nextIndex = currentIndex < 0 ? 0 : (currentIndex + 1) % items.count
        currentIndex = nextIndex
        await apply(items[nextIndex])
    }

    public func previous() async {
        guard !items.isEmpty else {
            return
        }
        let previousIndex = currentIndex <= 0 ? items.count - 1 : currentIndex - 1
        currentIndex = previousIndex
        await apply(items[previousIndex])
    }

    public func random() async {
        guard !items.isEmpty else {
            return
        }
        if items.count == 1 {
            currentIndex = 0
        } else {
            var candidate = randomIndex(0..<items.count)
            if candidate == currentIndex {
                candidate = (candidate + 1) % items.count
            }
            currentIndex = candidate
        }
        await apply(items[currentIndex])
    }

    public func pause() {
        isPaused = true
        reconcile()
    }

    public func resume() {
        isPaused = false
        reconcile()
    }

    public func togglePause() {
        isPaused.toggle()
        reconcile()
    }

    public func mute() {
        isMuted = true
        applyMuteToNodes()
    }

    public func unmute() {
        isMuted = false
        applyMuteToNodes()
    }

    public func toggleMute() {
        isMuted.toggle()
        applyMuteToNodes()
    }

    public func applyLowPower(_ lowPower: Bool) {
        isLowPowerMode = lowPower
        reconcile()
    }

    public func applyRuleAction(_ action: RuleAction) {
        ruleAction = action
        reconcile()
    }

    public func status() -> EngineStatus {
        return EngineStatus(
            id: currentItem?.id,
            path: currentItem?.url.path,
            playing: shouldPlay,
            paused: isPaused,
            muted: effectiveMuted,
            lowPower: isLowPowerMode,
            ruleAction: ruleAction,
            index: currentIndex,
            itemCount: items.count
        )
    }

    public func handle(_ request: Request) async -> Response {
        switch request.cmd {
        case .pause:
            pause()
        case .resume:
            resume()
        case .mute:
            mute()
        case .unmute:
            unmute()
        case .next:
            await next()
        case .previous:
            await previous()
        case .random:
            await random()
        case .open:
            guard let path = request.path, !path.isEmpty else {
                return .failure(code: "EINVAL", message: "open requires a path")
            }
            do {
                try await applyByPath(path)
            } catch {
                return .failure(code: "EMEDIA", message: Self.describe(error))
            }
        case .getCurrent, .getStatus:
            break
        }
        return .success(statusValue())
    }

    private var shouldPlay: Bool {
        guard currentItem != nil, !isPaused, !isLowPowerMode, ruleAction == .keepRunning else {
            return false
        }
        return true
    }

    private func reconcile() {
        let playing = shouldPlay
        for node in nodes.values {
            if playing {
                node.play()
            } else {
                node.pause()
            }
        }
        if playing {
            appNap.begin()
        } else {
            appNap.end()
        }
    }

    private func applyMuteToNodes() {
        let muted = effectiveMuted
        for node in nodes.values {
            (node as? MuteControllable)?.isMuted = muted
        }
    }

    private func updateStill(for item: WallpaperItem) async {
        guard let stillSync else {
            return
        }
        let request = StillRequest(url: item.url, kind: item.kind, stillTime: item.config.stillTime)
        do {
            if stillTakenOver {
                try await stillSync.updateStill(request)
            } else {
                try await stillSync.takeover(request)
                stillTakenOver = true
            }
        } catch {
            lastError = error
        }
    }

    private func statusValue() -> JSONValue {
        let snapshot = status()
        var object: [String: JSONValue] = [
            "playing": .bool(snapshot.playing),
            "paused": .bool(snapshot.paused),
            "muted": .bool(snapshot.muted),
            "lowPower": .bool(snapshot.lowPower),
            "rule": .string(snapshot.ruleAction.ipcName),
            "index": .number(Double(snapshot.index)),
            "count": .number(Double(snapshot.itemCount))
        ]
        if let id = snapshot.id {
            object["id"] = .string(id)
        }
        if let path = snapshot.path {
            object["current"] = .string(path)
        }
        return .object(object)
    }

    private func wireHotkeys() {
        hotkeys?.onAction = { [weak self] action in
            guard let self else {
                return
            }
            switch action {
            case .next:
                Task { @MainActor in await self.next() }
            case .previous:
                Task { @MainActor in await self.previous() }
            case .random:
                Task { @MainActor in await self.random() }
            case .pauseResume:
                self.togglePause()
            case .toggleMute:
                self.toggleMute()
            }
        }
    }

    private func wirePower() {
        guard let power else {
            return
        }
        isLowPowerMode = power.isLowPowerModeEnabled
        power.onChange = { [weak self] low in
            MainActor.assumeIsolated {
                self?.applyLowPower(low)
            }
        }
    }

    private func wireRules() {
        guard let snapshotService else {
            return
        }
        snapshotService.onChange = { [weak self] snapshot in
            MainActor.assumeIsolated {
                guard let self else {
                    return
                }
                let action = RuleEvaluator.evaluate(snapshot, config: self.ruleConfiguration)
                self.applyRuleAction(action)
            }
        }
    }

    private func observeScreenChanges() {
        guard screenObserver == nil else {
            return
        }
        screenObserver = NotificationCenter.default.addObserver(
            forName: NSApplication.didChangeScreenParametersNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, let item = self.currentItem else {
                    return
                }
                Task { @MainActor in await self.apply(item) }
            }
        }
    }

    private static func describe(_ error: Error) -> String {
        switch error {
        case let liveWallError as LiveWallError:
            switch liveWallError {
            case let .mediaUnreadable(path, reason):
                return "unreadable media: \(path) (\(reason))"
            case let .codecUnsupported(path):
                return "unsupported codec: \(path)"
            case let .libraryCorrupt(path):
                return "corrupt library: \(path)"
            case let .swapStateCorrupt(path):
                return "corrupt swap state: \(path)"
            case let .stillUnavailable(path, reason):
                return "still unavailable: \(path) (\(reason))"
            case let .ipc(code, message):
                return "\(code): \(message)"
            case let .downloadFailed(jobID, stderr):
                return "download failed: \(jobID) (\(stderr))"
            case let .toolMissing(tool):
                return "missing tool: \(tool)"
            case let .toolAcquisitionFailed(tools, stderr):
                return "tool install failed: \(tools.joined(separator: ", ")) (\(stderr))"
            case .consentRequired:
                return "tool install consent required"
            }
        default:
            return String(describing: error)
        }
    }

    isolated deinit {
        if let screenObserver {
            NotificationCenter.default.removeObserver(screenObserver)
        }
    }
}
