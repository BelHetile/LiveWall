import AppKit
import Foundation
import LiveWallCore
import LiveWallEngine
import LiveWallIPC

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var controller: EngineController?
    private var server: IPCServer?
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

        let server = IPCServer(socketURL: socketURL) { request in
            await controller.handle(request)
        }
        do {
            try server.start()
        } catch {
            FileHandle.standardError.write(Data("LiveWall: ipc server failed to start: \(error)\n".utf8))
        }
        self.server = server

        await controller.start()
    }

    private static func makeSource(environment: [String: String]) -> WallpaperSource {
        guard let raw = environment["LIVEWALL_ENGINE_ITEMS"], !raw.isEmpty else {
            return StaticWallpaperSource(items: [])
        }
        let paths = raw.split(separator: ":").map(String.init).filter { !$0.isEmpty }
        return StaticWallpaperSource(paths: paths)
    }
}
