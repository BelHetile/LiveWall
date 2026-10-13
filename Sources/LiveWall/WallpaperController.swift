import AppKit
import AVFoundation

/// A borderless window at desktop level: above the system wallpaper, below the desktop icons.
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

    /// Seconds of pause after which the video is unloaded from memory (0 = never).
    var stopAfter: TimeInterval = 30 {
        didSet { if stopAfter != oldValue { scheduleStop() } }
    }
    /// The video is unloaded: its frame is on screen and there is no player.
    private(set) var isStopped = false
    private var stopWork: DispatchWorkItem?
    private var stoppedTime: CMTime = .zero
    private var stillImage: CGImage?
    private var stopGeneration = 0

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

    /// Playback position of the wallpaper video (also while it is unloaded).
    var playbackTime: CMTime? {
        if isStopped { return stoppedTime }
        return player?.currentTime()
    }

    // MARK: Start

    func start() {
        syncWindows()
        NotificationCenter.default.addObserver(self, selector: #selector(screensChanged),
                                               name: NSApplication.didChangeScreenParametersNotification, object: nil)
    }

    @objc private func screensChanged() { syncWindows() }

    // MARK: Windows per screen

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
                    if isStopped, let still = stillImage {
                        w.wallpaperView.showStill(still, mode: item.mode, videoSize: currentVideoSize)
                    } else {
                        present(item: item, url: url, in: w, animated: false)
                    }
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

    // MARK: Applying wallpapers

    func apply(item: WallpaperItem?, url: URL?) {
        resetStop()
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
            makePlayer(url: url)

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

        // keep the old player until the crossfade ends
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0) {
            oldPlayer?.pause()
            _ = oldLooper
        }
    }

    func updateMode(_ mode: FillMode) {
        for w in windows.values { w.wallpaperView.mode = mode }
    }

    private func makePlayer(url: URL) {
        let p = AVQueuePlayer()
        p.isMuted = muted
        p.volume = volume
        p.preventsDisplaySleepDuringVideoPlayback = false
        // local file: no waiting for buffering and no extra frames buffered
        p.automaticallyWaitsToMinimizeStalling = false
        let template = AVPlayerItem(url: url)
        template.preferredForwardBufferDuration = 2
        looper = AVPlayerLooper(player: p, templateItem: template)
        player = p
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
        // GIFs freeze too when they can't be seen
        if currentItem?.kind == .gif {
            for w in windows.values { w.wallpaperView.setAnimationPaused(paused) }
        }
        if !paused && isStopped {
            resume()
            return
        }
        scheduleStop()
        guard let p = player else { return }
        if paused {
            p.pause()
        } else {
            p.defaultRate = speed
            p.play()
        }
    }

    // MARK: Stop instead of pause

    private func resetStop() {
        stopWork?.cancel()
        stopWork = nil
        stopGeneration += 1
        isStopped = false
        stillImage = nil
    }

    /// The video has been paused longer than `stopAfter`: unload the player, keep the frame.
    private func scheduleStop() {
        stopWork?.cancel()
        stopWork = nil
        let paused = userPaused || autoPaused
        guard paused, !isStopped, stopAfter > 0, player != nil, currentItem?.kind == .video else { return }
        let work = DispatchWorkItem { [weak self] in self?.stop() }
        stopWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + stopAfter, execute: work)
    }

    private func stop() {
        stopWork = nil
        guard userPaused || autoPaused, !isStopped, let p = player, let url = currentURL,
              let item = currentItem, item.kind == .video else { return }
        let time = p.currentTime()
        stopGeneration += 1
        let generation = stopGeneration
        Task { @MainActor [weak self] in
            guard let image = await WallpaperController.frame(of: url, at: time) else { return }
            guard let self = self, generation == self.stopGeneration, self.currentItem?.id == item.id,
                  self.userPaused || self.autoPaused, !self.isStopped, let p = self.player else { return }
            self.isStopped = true
            self.stoppedTime = time
            self.stillImage = image
            for w in self.windows.values {
                w.wallpaperView.showStill(image, mode: self.currentItem?.mode ?? item.mode, videoSize: self.currentVideoSize)
            }
            p.pause()
            self.looper?.disableLooping()
            p.removeAllItems()
            self.looper = nil
            self.player = nil
        }
    }

    /// Loads the video again from the same spot; the frame goes away once the video is ready.
    private func resume() {
        guard isStopped, let item = currentItem, let url = currentURL else { return }
        isStopped = false
        stopGeneration += 1
        let generation = stopGeneration
        makePlayer(url: url)
        guard let p = player else { return }
        let start = stoppedTime
        let begin: () -> Void = { [weak self] in
            guard let self = self, generation == self.stopGeneration, self.player === p else { return }
            self.stillImage = nil
            for w in self.windows.values {
                self.present(item: self.currentItem ?? item, url: url, in: w, animated: true)
            }
            self.updatePlayback()
        }
        if CMTimeGetSeconds(start) > 0.05 {
            p.seek(to: start, toleranceBefore: .zero, toleranceAfter: .zero) { _ in
                DispatchQueue.main.async { begin() }
            }
        } else {
            begin()
        }
    }

    /// Moves the wallpaper video to a time (also while it is unloaded: the shown frame is updated).
    func seek(to time: CMTime) {
        if isStopped {
            guard let url = currentURL, let item = currentItem else { return }
            stoppedTime = time
            stopGeneration += 1
            let generation = stopGeneration
            Task { @MainActor [weak self] in
                guard let image = await WallpaperController.frame(of: url, at: time) else { return }
                guard let self = self, generation == self.stopGeneration, self.isStopped,
                      self.currentItem?.id == item.id else { return }
                self.stillImage = image
                for w in self.windows.values {
                    w.wallpaperView.showStill(image, mode: self.currentItem?.mode ?? item.mode, videoSize: self.currentVideoSize)
                }
            }
        } else if let p = player {
            p.seek(to: time, toleranceBefore: .zero, toleranceAfter: .zero)
        }
    }

    private static func frame(of url: URL, at time: CMTime) async -> CGImage? {
        let gen = AVAssetImageGenerator(asset: AVURLAsset(url: url))
        gen.appliesPreferredTrackTransform = true
        gen.requestedTimeToleranceBefore = .zero
        gen.requestedTimeToleranceAfter = .zero
        return try? await gen.image(at: time).image
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
