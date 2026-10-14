import AppKit
import Foundation
import LiveWallCore
import LiveWallEngine
import LiveWallIPC
import LiveWallMedia
import LiveWallTools

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var controller: EngineController?
    private var server: IPCServer?
    private var mediaHost: MediaHost?
    private var isShuttingDown = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        Task { await self.startEngine() }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        return false
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard !isShuttingDown else {
            return .terminateNow
        }
        isShuttingDown = true
        Task { @MainActor in
            await self.mediaHost?.cancelAll()
            await self.controller?.shutdown()
            self.server?.stop()
            NSApp.reply(toApplicationShouldTerminate: true)
        }
        return .terminateLater
    }

    private func startEngine() async {
        let environment = ProcessInfo.processInfo.environment
        let dataDirectory = IPCPaths.defaultDataDirectory()
        let socketURL = IPCPaths.socketURL(environment: environment)
        let source = Self.makeSource(environment: environment)
        let testMode = environment["LIVEWALL_TEST_MODE"] == "1"

        let screenHost = ScreenSetHost()

        let stillSync: StillSyncService?
        if testMode {
            stillSync = nil
        } else {
            let service = StillSyncService(
                workspace: NSWorkspaceDesktopPicture(),
                store: SwapStateStore(url: dataDirectory.appendingPathComponent("swap-state.json")),
                stillDirectory: dataDirectory.appendingPathComponent("Stills", isDirectory: true)
            )
            try? await service.recoverIfNeeded()
            stillSync = service
        }

        let snapshotService: SystemSnapshotService?
        if testMode {
            snapshotService = nil
        } else {
            snapshotService = SystemSnapshotService(
                fullscreen: CGWindowFullscreenDetector(),
                coverage: CGWindowCoverageDetector(),
                audio: CoreAudioPlayingDetector(),
                apps: NSWorkspaceAppStateDetector(),
                display: NSWorkspaceDisplayAsleepDetector()
            )
        }

        let controller = EngineController(
            source: source,
            screenHost: screenHost,
            nodeFactory: DefaultPlaybackNodeFactory(),
            stillSync: stillSync,
            snapshotService: snapshotService,
            hotkeys: HotkeyManager(),
            power: PowerMonitor(),
            appNap: ProcessInfoAppNapGuard()
        )
        self.controller = controller

        let mediaHost = await Self.makeMediaHost()
        self.mediaHost = mediaHost

        let server = IPCServer(socketURL: socketURL) { request in
            if mediaHost.handles(request.cmd) {
                return await mediaHost.handle(request)
            }
            return await controller.handle(request)
        }
        do {
            try server.start()
        } catch {
            FileHandle.standardError.write(Data("LiveWall: ipc server failed to start: \(error)\n".utf8))
        }
        self.server = server

        await controller.start()
    }

    private static func makeMediaHost() async -> MediaHost {
        let toolManager = ToolManager()
        let downloads = DownloadService(installer: toolManager)
        let optimization = OptimizationCoordinator()
        let targetHeight = targetScreenHeight()
        let host = MediaHost(
            downloads: downloads,
            optimization: optimization,
            maxHeight: { targetHeight }
        )
        try? await host.prepareForLaunch()
        return host
    }

    private static func targetScreenHeight() -> Int {
        guard let screen = NSScreen.main else {
            return DownloadFormatSelector.maximumHeight
        }
        let backing = screen.convertRectToBacking(screen.frame)
        return Int(backing.height.rounded())
    }

    private static func makeSource(environment: [String: String]) -> WallpaperSource {
        guard let raw = environment["LIVEWALL_ENGINE_ITEMS"], !raw.isEmpty else {
            return StaticWallpaperSource(items: [])
        }
        let paths = raw.split(separator: ":").map(String.init).filter { !$0.isEmpty }
        return StaticWallpaperSource(paths: paths)
    }
}
