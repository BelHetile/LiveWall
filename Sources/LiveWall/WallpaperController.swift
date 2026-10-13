import AppKit
import AVFoundation

/// Окно без рамки на уровне рабочего стола: лежит над системными обоями, но под иконками.
final class DesktopWindow: NSWindow {
    let wallpaperView: WallpaperView

    init(screen: NSScreen) {
        wallpaperView = WallpaperView(frame: NSRect(origin: .zero, size: screen.frame.size))
        super.init(contentRect: screen.frame,
                   styleMask: [.borderless],
                   backing: .buffered,
                   defer: false)
        isReleasedWhenClosed = false
        isOpaque = true
        backgroundColor = .black
        hasShadow = false
        ignoresMouseEvents = true
        canHide = false
        animationBehavior = .none
        isExcludedFromWindowsMenu = true
        level = NSWindow.Level(rawValue: Int(CGWindowLevelForKey(.desktopWindow)) + 1)
        collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle, .fullScreenAuxiliary]
        wallpaperView.autoresizingMask = [.width, .height]
        contentView = wallpaperView
        setFrame(screen.frame, display: true)
    }

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

final class WallpaperController: NSObject {
    static let shared = WallpaperController()

    private var windows: [NSNumber: DesktopWindow] = [:]
    private var player: AVQueuePlayer?
    private var looper: AVPlayerLooper?

    private var currentItem: WallpaperItem?
    private var currentURL: URL?
    private var currentVideoSize: CGSize = .zero

    var userPaused = false {
        didSet { updatePlayback() }
    }
    var autoPaused = false {
        didSet { if autoPaused != oldValue { updatePlayback() } }
    }
    var muted = true {
        didSet { player?.isMuted = muted }
    }
    var volume: Float = 0.5 {
        didSet { player?.volume = volume }
    }
    var speed: Float = 1.0 {
        didSet { if speed != oldValue { updatePlayback() } }
    }

    var currentPlayer: AVPlayer? { player }

    var isPlaying: Bool {
        player != nil && !userPaused && !autoPaused
    }

    // MARK: Запуск

    func start() {
        syncWindows()
        NotificationCenter.default.addObserver(self, selector: #selector(screensChanged),
                                               name: NSApplication.didChangeScreenParametersNotification, object: nil)
    }

    @objc private func screensChanged() { syncWindows() }

    // MARK: Окна по экранам

    private static func key(for screen: NSScreen) -> NSNumber? {
        screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber
    }

    func syncWindows() {
        var seen = Set<NSNumber>()
        for s in NSScreen.screens {
            guard let key = WallpaperController.key(for: s) else { continue }
            seen.insert(key)
            if let w = windows[key] {
                w.setFrame(s.frame, display: true)
            } else {
                let w = DesktopWindow(screen: s)
                windows[key] = w
                if let item = currentItem, let url = currentURL {
                    present(item: item, url: url, in: w, animated: false)
                    w.orderFront(nil)
                }
            }
        }
        let stale = windows.keys.filter { !seen.contains($0) }
        for k in stale {
            windows[k]?.orderOut(nil)
            windows[k] = nil
        }
    }

    // MARK: Применение обоев

    func apply(item: WallpaperItem?, url: URL?) {
        guard let item = item, let url = url else {
            currentItem = nil
            currentURL = nil
            PauseMonitor.shared.needsMonitoring = false
            player?.pause()
            player = nil
            looper = nil
            for w in windows.values {
                w.wallpaperView.clear()
                w.orderOut(nil)
            }
            return
        }

        let oldPlayer = player
        let oldLooper = looper

        currentItem = item
        currentURL = url
        currentVideoSize = .zero
        PauseMonitor.shared.needsMonitoring = (item.kind != .image)

        if item.kind == .video {
            let p = AVQueuePlayer()
            p.isMuted = muted
            p.volume = volume
            p.preventsDisplaySleepDuringVideoPlayback = false
            // локальный файл: не ждём буферизации и не держим лишний запас кадров
            p.automaticallyWaitsToMinimizeStalling = false
            let template = AVPlayerItem(url: url)
            template.preferredForwardBufferDuration = 2
            looper = AVPlayerLooper(player: p, templateItem: template)
            player = p

            Task { @MainActor [weak self] in
                let size = await WallpaperController.naturalSize(of: url)
                guard let self = self, self.currentItem?.id == item.id else { return }
                self.currentVideoSize = size
                for w in self.windows.values { w.wallpaperView.videoSize = size }
            }
        } else {
            player = nil
            looper = nil
        }

        for w in windows.values {
            present(item: item, url: url, in: w, animated: true)
            w.orderFront(nil)
        }
        updatePlayback()

        // старый плеер держим до конца перекрёстного затухания
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
            oldPlayer?.pause()
            _ = oldLooper
        }
    }

    func updateMode(_ mode: FillMode) {
        for w in windows.values { w.wallpaperView.mode = mode }
    }

    private func present(item: WallpaperItem, url: URL, in window: DesktopWindow, animated: Bool) {
        window.wallpaperView.present(kind: item.kind,
                                     url: url,
                                     player: player,
                                     mode: item.mode,
                                     videoSize: currentVideoSize,
                                     animated: animated)
    }

    private func updatePlayback() {
        let paused = userPaused || autoPaused
        // GIF тоже замирают, когда их не видно
        if currentItem?.kind == .gif {
            for w in windows.values { w.wallpaperView.setAnimationPaused(paused) }
        }
        guard let p = player else { return }
        if paused {
            p.pause()
        } else {
            p.defaultRate = speed
            p.play()
        }
    }

    private static func naturalSize(of url: URL) async -> CGSize {
        let asset = AVURLAsset(url: url)
        guard let track = try? await asset.loadTracks(withMediaType: .video).first else { return .zero }
        guard let size = try? await track.load(.naturalSize) else { return .zero }
        let transform = (try? await track.load(.preferredTransform)) ?? .identity
        let r = CGRect(origin: .zero, size: size).applying(transform)
        return CGSize(width: abs(r.width), height: abs(r.height))
    }
}
