import AppKit
import CoreGraphics
import Foundation
import LiveWallCore

@MainActor
public final class StillSyncService {
    public private(set) var isOwning = false
    public private(set) var isLocked = false
    public private(set) var currentStillURL: URL?

    private let workspace: DesktopPictureWorkspace
    private let store: SwapStateStore
    private let renderer: StillRenderer

    private var captured: SwapState?
    private var persistedIDs: Set<UInt32> = []
    private var request: StillRequest?
    private var tokens: [(center: NotificationCenter, token: NSObjectProtocol)] = []
    private var lockObserver: LockStateObserver?

    public init(workspace: DesktopPictureWorkspace, store: SwapStateStore, stillDirectory: URL) {
        self.workspace = workspace
        self.store = store
        self.renderer = StillRenderer(directory: stillDirectory)
    }

    public func recoverIfNeeded() async throws {
        guard let persisted = try await store.load() else {
            return
        }
        applyOriginal(persisted)
        try await store.clear()
    }

    public func takeover(_ request: StillRequest) async throws {
        guard !isOwning else {
            return
        }
        let state = capture(excluding: [])
        try await store.save(state)
        captured = state
        persistedIDs = Set(state.displays.map(\.displayID))
        self.request = request
        isOwning = true
        try await applyStill()
    }

    public func updateStill(_ request: StillRequest) async throws {
        guard isOwning else {
            return
        }
        self.request = request
        try await applyStill()
    }

    public func restore() async throws {
        guard isOwning else {
            return
        }
        defer { resetOwnership() }
        if let captured {
            applyOriginal(captured)
        }
        try await store.clear()
    }

    private func resetOwnership() {
        captured = nil
        persistedIDs = []
        request = nil
        currentStillURL = nil
        isOwning = false
    }

    public func startObserving() {
        guard tokens.isEmpty, lockObserver == nil else {
            return
        }

        let workspaceCenter = NSWorkspace.shared.notificationCenter
        let spaceToken = workspaceCenter.addObserver(
            forName: NSWorkspace.activeSpaceDidChangeNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.handleSpaceChange()
            }
        }
        tokens.append((workspaceCenter, spaceToken))

        let defaultCenter = NotificationCenter.default
        let screenToken = defaultCenter.addObserver(
            forName: NSApplication.didChangeScreenParametersNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.handleScreenChange()
            }
        }
        tokens.append((defaultCenter, screenToken))

        let distributedCenter = DistributedNotificationCenter.default()
        let wallpaperToken = distributedCenter.addObserver(
            forName: Notification.Name("com.apple.desktop"),
            object: "BackgroundChanged",
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.handleWallpaperChange()
            }
        }
        tokens.append((distributedCenter, wallpaperToken))

        lockObserver = LockStateObserver { [weak self] locked in
            self?.handleLockChange(locked)
        }
    }

    public func stopObserving() {
        for entry in tokens {
            entry.center.removeObserver(entry.token)
        }
        tokens.removeAll()
        lockObserver?.invalidate()
        lockObserver = nil
    }

    func handleSpaceChange() {
        reapplyStill()
    }

    func handleScreenChange() {
        guard isOwning else {
            return
        }
        adoptNewScreens()
        reapplyStill()
    }

    func handleWallpaperChange() {
        guard isOwning, !isLocked, let still = currentStillURL else {
            return
        }
        let ownedIDs = captured?.displays.map(\.displayID) ?? []
        let allOurs = !ownedIDs.isEmpty && ownedIDs.allSatisfy { displayID in
            workspace.desktopImageURL(for: displayID)?.standardizedFileURL == still.standardizedFileURL
        }
        guard !allOurs else {
            return
        }
        reapplyStill()
    }

    func handleLockChange(_ locked: Bool) {
        let wasLocked = isLocked
        isLocked = locked
        if wasLocked && !locked {
            reapplyStill()
        }
    }

    private func capture(excluding known: Set<UInt32>) -> SwapState {
        var state = SwapState(displays: [], takenAt: Date())
        for descriptor in workspace.screens where !known.contains(descriptor.displayID) {
            guard let original = workspace.desktopImageURL(for: descriptor.displayID) else {
                continue
            }
            state.displays.append(
                SwapState.DisplayEntry(
                    displayID: descriptor.displayID,
                    originalURL: original.absoluteString
                )
            )
        }
        return state
    }

    private func adoptNewScreens() {
        guard let state = captured else {
            return
        }
        let known = Set(state.displays.map(\.displayID))
        let fresh = workspace.screens.filter { !known.contains($0.displayID) }
        guard !fresh.isEmpty else {
            return
        }
        let addition = capture(excluding: known)
        guard !addition.displays.isEmpty else {
            return
        }
        var merged = state
        merged.displays.append(contentsOf: addition.displays)
        captured = merged

        let toSave = merged
        let still = currentStillURL
        let newIDs = addition.displays.map(\.displayID)
        Task { @MainActor [weak self] in
            guard let self else {
                return
            }
            try? await self.store.save(toSave)
            self.persistedIDs.formUnion(newIDs)
            guard let still, !self.isLocked else {
                return
            }
            for displayID in newIDs {
                guard self.workspace.screens.contains(where: { $0.displayID == displayID }) else {
                    continue
                }
                try? self.workspace.setDesktopImageURL(still, for: displayID)
            }
        }
    }

    private func reapplyStill() {
        guard isOwning, !isLocked, let still = currentStillURL else {
            return
        }
        for entry in captured?.displays ?? [] where persistedIDs.contains(entry.displayID) {
            guard workspace.screens.contains(where: { $0.displayID == entry.displayID }) else {
                continue
            }
            try? workspace.setDesktopImageURL(still, for: entry.displayID)
        }
    }

    private func applyStill() async throws {
        guard let request else {
            return
        }
        let still = try await renderer.render(request)
        currentStillURL = still
        for entry in captured?.displays ?? [] where persistedIDs.contains(entry.displayID) {
            guard workspace.screens.contains(where: { $0.displayID == entry.displayID }) else {
                continue
            }
            try workspace.setDesktopImageURL(still, for: entry.displayID)
        }
    }

    private func applyOriginal(_ state: SwapState) {
        for entry in state.displays {
            guard workspace.screens.contains(where: { $0.displayID == entry.displayID }) else {
                continue
            }
            guard let url = URL(string: entry.originalURL) else {
                continue
            }
            try? workspace.setDesktopImageURL(url, for: entry.displayID)
        }
    }

    isolated deinit {
        for entry in tokens {
            entry.center.removeObserver(entry.token)
        }
        lockObserver?.invalidate()
    }
}
