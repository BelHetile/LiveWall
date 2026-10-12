import Foundation
import AppKit
import AVFoundation
import UniformTypeIdentifiers
import ServiceManagement
import Carbon.HIToolbox

enum LoginItem {
    static func set(_ on: Bool) {
        do {
            if on {
                try SMAppService.mainApp.register()
            } else {
                try SMAppService.mainApp.unregister()
            }
        } catch {
            NSLog("LiveWall: login item error: \(error)")
        }
    }
}

/// Библиотека обоев, настройки и вся логика переключения.
final class Library: ObservableObject {
    static let shared = Library()

    @Published private(set) var items: [WallpaperItem] = []
    @Published private(set) var currentID: UUID?
    @Published private(set) var paused = false
    @Published var message: String?
    @Published var dropTargeted = false
    @Published private(set) var thumbs: [UUID: NSImage] = [:]
    @Published private(set) var recordingAction: HotkeyAction?
    @Published private(set) var busyCount = 0
    @Published private(set) var pauseReason: String?
    @Published var settings = Settings() {
        didSet { settingsChanged(old: oldValue) }
    }

    private let root: URL
    private let filesDir: URL
    private let thumbsDir: URL
    private let stillsDir: URL
    private let indexURL: URL
    private var loading = true
    private var rotateTimer: Timer?
    private var recordMonitor: Any?
    private var spaceObserver: NSObjectProtocol?
    private var lockSyncGeneration = 0

    private struct Index: Codable {
        var items: [WallpaperItem]
        var currentID: UUID?
    }

    var current: WallpaperItem? {
        items.first(where: { $0.id == currentID })
    }

    private init() {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        root = base.appendingPathComponent("LiveWall", isDirectory: true)
        filesDir = root.appendingPathComponent("Wallpapers", isDirectory: true)
        thumbsDir = root.appendingPathComponent("Thumbs", isDirectory: true)
        stillsDir = root.appendingPathComponent("Stills", isDirectory: true)
        indexURL = root.appendingPathComponent("library.json")
        try? FileManager.default.createDirectory(at: filesDir, withIntermediateDirectories: true)
        try? FileManager.default.createDirectory(at: thumbsDir, withIntermediateDirectories: true)
        try? FileManager.default.createDirectory(at: stillsDir, withIntermediateDirectories: true)
        load()
        loading = false
    }

    // MARK: Хранилище

    func url(for item: WallpaperItem) -> URL {
        filesDir.appendingPathComponent(item.file)
    }

    private func load() {
        if let data = try? Data(contentsOf: indexURL),
           let idx = try? JSONDecoder().decode(Index.self, from: data) {
            // отбрасываем записи, чей файл пропал
            items = idx.items.filter { FileManager.default.fileExists(atPath: filesDir.appendingPathComponent($0.file).path) }
            currentID = items.contains(where: { $0.id == idx.currentID }) ? idx.currentID : items.first?.id
        }
        if let data = UserDefaults.standard.data(forKey: "settings"),
           let s = try? JSONDecoder().decode(Settings.self, from: data) {
            settings = s
        }
    }

    private func save() {
        let idx = Index(items: items, currentID: currentID)
        if let data = try? JSONEncoder().encode(idx) {
            try? data.write(to: indexURL, options: .atomic)
        }
    }

    private func saveSettings() {
        if let data = try? JSONEncoder().encode(settings) {
            UserDefaults.standard.set(data, forKey: "settings")
        }
    }

    // MARK: Запуск

    func start() {
        applyControllerSettings()
        WallpaperController.shared.start()

        let monitor = PauseMonitor.shared
        monitor.onChange = { [weak self] reason in
            self?.pauseReason = reason
            WallpaperController.shared.autoPaused = (reason != nil)
        }
        monitor.settings = settings
        monitor.start()

        registerHotkeys()
        rescheduleRotation()
        if let c = current {
            WallpaperController.shared.apply(item: c, url: url(for: c))
        }

        // setDesktopImageURL действует только на текущий Space — переприменяем при переключении
        spaceObserver = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.activeSpaceDidChangeNotification, object: nil, queue: .main
        ) { [weak self] _ in
            self?.syncLockScreen()
        }
        syncLockScreen()
    }

    private func applyControllerSettings() {
        let c = WallpaperController.shared
        c.muted = settings.muted
        c.volume = Float(settings.volume)
        c.speed = Float(settings.speed)
        c.userPaused = paused
        PauseMonitor.shared.settings = settings
    }

    // MARK: Экран блокировки / системные обои

    func syncLockScreen() {
        guard settings.syncSystemWallpaper, let item = current else { return }
        let src = url(for: item)
        let dir = stillsDir
        lockSyncGeneration += 1
        let generation = lockSyncGeneration
        Task { @MainActor [weak self] in
            guard let still = await LockScreenSync.stillImage(for: item, source: src, dir: dir) else { return }
            guard let self = self, generation == self.lockSyncGeneration else { return }
            let mode = self.current?.mode ?? item.mode
            LockScreenSync.apply(url: still, mode: mode)
        }
    }

    // MARK: Правила для приложений

    func addAppRule(bundleID: String, name: String) {
        guard !settings.appRules.contains(where: { $0.bundleID == bundleID }) else { return }
        settings.appRules.append(AppRule(bundleID: bundleID, name: name, frontmostOnly: false))
    }

    func removeAppRule(_ bundleID: String) {
        settings.appRules.removeAll { $0.bundleID == bundleID }
    }

    func setRuleFrontmostOnly(_ bundleID: String, _ value: Bool) {
        guard let i = settings.appRules.firstIndex(where: { $0.bundleID == bundleID }) else { return }
        settings.appRules[i].frontmostOnly = value
    }

    func chooseAppForRule() {
        let p = NSOpenPanel()
        p.allowsMultipleSelection = true
        p.canChooseDirectories = false
        p.allowedContentTypes = [.application]
        p.directoryURL = URL(fileURLWithPath: "/Applications")
        p.message = "Choose apps (e.g. games) that should pause the wallpaper."
        guard p.runModal() == .OK else { return }
        for u in p.urls {
            guard let bundle = Bundle(url: u), let id = bundle.bundleIdentifier else { continue }
            let name = (bundle.object(forInfoDictionaryKey: "CFBundleDisplayName") as? String)
                ?? (bundle.object(forInfoDictionaryKey: "CFBundleName") as? String)
                ?? u.deletingPathExtension().lastPathComponent
            addAppRule(bundleID: id, name: name)
        }
    }

    static func runningRegularApps() -> [NSRunningApplication] {
        let me = getpid()
        return NSWorkspace.shared.runningApplications
            .filter { $0.activationPolicy == .regular && $0.bundleIdentifier != nil && $0.processIdentifier != me }
            .sorted { ($0.localizedName ?? "") < ($1.localizedName ?? "") }
    }

    private func settingsChanged(old: Settings) {
        guard !loading else { return }
        saveSettings()
        applyControllerSettings()
        if old.hotkeys != settings.hotkeys || old.numberHotkeys != settings.numberHotkeys {
            registerHotkeys()
        }
        if old.autoRotate != settings.autoRotate || old.rotateMinutes != settings.rotateMinutes {
            rescheduleRotation()
        }
        if old.launchAtLogin != settings.launchAtLogin {
            LoginItem.set(settings.launchAtLogin)
        }
        if settings.syncSystemWallpaper && !old.syncSystemWallpaper {
            syncLockScreen()
        }
    }

    // MARK: Импорт

    static func kind(for url: URL) -> MediaKind? {
        guard let t = UTType(filenameExtension: url.pathExtension.lowercased()) else { return nil }
        if t.conforms(to: .gif) { return .gif }
        if t.conforms(to: .movie) || t.conforms(to: .video) { return .video }
        if t.conforms(to: .image) { return .image }
        return nil
    }

    func presentImportPanel() {
        let p = NSOpenPanel()
        p.allowsMultipleSelection = true
        p.canChooseDirectories = true
        p.canChooseFiles = true
        p.allowedContentTypes = [.movie, .image]
        p.message = "Choose videos (mp4/mov/m4v), images or GIFs. Folders are scanned recursively."
        if p.runModal() == .OK {
            importFiles(p.urls)
        }
    }

    func handleDrop(_ providers: [NSItemProvider]) {
        for provider in providers {
            _ = provider.loadObject(ofClass: URL.self) { url, _ in
                guard let url = url else { return }
                DispatchQueue.main.async { self.importFiles([url]) }
            }
        }
    }

    func importFiles(_ urls: [URL]) {
        var files: [URL] = []
        for u in urls {
            var isDir: ObjCBool = false
            if FileManager.default.fileExists(atPath: u.path, isDirectory: &isDir), isDir.boolValue {
                if let en = FileManager.default.enumerator(at: u, includingPropertiesForKeys: nil, options: [.skipsHiddenFiles]) {
                    for case let f as URL in en { files.append(f) }
                }
            } else {
                files.append(u)
            }
        }
        let supported = files.filter { Library.kind(for: $0) != nil }
        let skipped = files.count - supported.count
        if skipped > 0 {
            message = "\(skipped) file(s) skipped: unsupported format. Use mp4/mov/m4v, png/jpg/heic or gif."
        } else {
            message = nil
        }
        guard !supported.isEmpty else { return }

        busyCount += supported.count
        let dir = filesDir
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            for src in supported {
                let kind = Library.kind(for: src) ?? .image
                let id = UUID()
                let ext = src.pathExtension.lowercased()
                let dest = dir.appendingPathComponent("\(id.uuidString).\(ext)")
                do {
                    try FileManager.default.copyItem(at: src, to: dest)
                } catch {
                    DispatchQueue.main.async {
                        self?.busyCount -= 1
                        self?.message = "Could not copy \(src.lastPathComponent): \(error.localizedDescription)"
                    }
                    continue
                }
                let item = WallpaperItem(id: id,
                                         name: src.deletingPathExtension().lastPathComponent,
                                         file: dest.lastPathComponent,
                                         kind: kind,
                                         mode: .fill)
                DispatchQueue.main.async {
                    guard let self = self else { return }
                    self.items.append(item)
                    self.busyCount -= 1
                    self.save()
                    if self.currentID == nil { self.select(item.id) }
                }
            }
        }
    }

    func remove(_ id: UUID) {
        guard let idx = items.firstIndex(where: { $0.id == id }) else { return }
        let item = items[idx]
        try? FileManager.default.removeItem(at: url(for: item))
        try? FileManager.default.removeItem(at: thumbsDir.appendingPathComponent("\(id.uuidString).png"))
        try? FileManager.default.removeItem(at: stillsDir.appendingPathComponent("\(id.uuidString).png"))
        thumbs[id] = nil
        let wasCurrent = (id == currentID)
        items.remove(at: idx)
        if wasCurrent {
            if items.isEmpty {
                currentID = nil
                save()
                WallpaperController.shared.apply(item: nil, url: nil)
            } else {
                select(items[min(idx, items.count - 1)].id)
            }
        } else {
            save()
        }
    }

    func revealInFinder(_ item: WallpaperItem) {
        NSWorkspace.shared.activateFileViewerSelecting([url(for: item)])
    }

    // MARK: Переключение

    func select(_ id: UUID) {
        guard let item = items.first(where: { $0.id == id }) else { return }
        currentID = id
        save()
        WallpaperController.shared.apply(item: item, url: url(for: item))
        syncLockScreen()
    }

    func step(_ delta: Int) {
        guard !items.isEmpty else { return }
        let n = items.count
        let i = items.firstIndex(where: { $0.id == currentID }) ?? (delta > 0 ? -1 : 0)
        let j = ((i + delta) % n + n) % n
        select(items[j].id)
    }

    func random() {
        guard !items.isEmpty else { return }
        if items.count == 1 {
            select(items[0].id)
            return
        }
        let pool = items.filter { $0.id != currentID }
        if let pick = pool.randomElement() { select(pick.id) }
    }

    func jump(to index: Int) {
        guard index >= 0, index < items.count else { return }
        select(items[index].id)
    }

    func setMode(_ mode: FillMode, for id: UUID) {
        guard let idx = items.firstIndex(where: { $0.id == id }) else { return }
        items[idx].mode = mode
        save()
        if id == currentID {
            WallpaperController.shared.updateMode(mode)
            syncLockScreen()
        }
    }

    func cycleMode() {
        guard let c = current else { return }
        setMode(c.mode.next, for: c.id)
    }

    func togglePause() {
        paused.toggle()
        WallpaperController.shared.userPaused = paused
    }

    func toggleMute() {
        settings.muted.toggle()
    }

    // MARK: Автосмена

    func rescheduleRotation() {
        rotateTimer?.invalidate()
        rotateTimer = nil
        guard settings.autoRotate, settings.rotateMinutes > 0 else { return }
        rotateTimer = Timer.scheduledTimer(withTimeInterval: Double(settings.rotateMinutes) * 60,
                                           repeats: true) { [weak self] _ in
            guard let self = self else { return }
            if self.settings.shuffle {
                self.random()
            } else {
                self.step(1)
            }
        }
    }

    // MARK: Хоткеи

    func perform(_ action: HotkeyAction) {
        switch action {
        case .next: step(1)
        case .previous: step(-1)
        case .random: random()
        case .togglePause: togglePause()
        case .cycleMode: cycleMode()
        case .toggleMute: toggleMute()
        case .openLibrary: (NSApp.delegate as? AppDelegate)?.showLibrary()
        }
    }

    func registerHotkeys() {
        let hk = HotkeyCenter.shared
        hk.unregisterAll()
        for action in HotkeyAction.allCases {
            guard let b = settings.hotkeys[action.rawValue] else { continue }
            hk.register(id: action.hotkeyID, binding: b) { [weak self] in
                self?.perform(action)
            }
        }
        if settings.numberHotkeys {
            let codes: [Int] = [kVK_ANSI_1, kVK_ANSI_2, kVK_ANSI_3, kVK_ANSI_4, kVK_ANSI_5,
                                kVK_ANSI_6, kVK_ANSI_7, kVK_ANSI_8, kVK_ANSI_9]
            let mods = UInt32(controlKey | optionKey | cmdKey)
            for (i, c) in codes.enumerated() {
                hk.register(id: UInt32(100 + i),
                            binding: HotkeyBinding(keyCode: UInt32(c), modifiers: mods)) { [weak self] in
                    self?.jump(to: i)
                }
            }
        }
    }

    // MARK: Запись хоткея

    func beginRecording(_ action: HotkeyAction) {
        endRecording()
        recordingAction = action
        // пока записываем — снимаем глобальные хоткеи, чтобы они не перехватывали нажатие
        HotkeyCenter.shared.unregisterAll()
        recordMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self = self, let action = self.recordingAction else { return event }
            if event.keyCode == 53 { // Esc
                self.endRecording()
                return nil
            }
            let mods = HotkeyFormat.carbonMods(event.modifierFlags)
            if mods == 0 { return nil } // нужен хотя бы один модификатор
            self.settings.hotkeys[action.rawValue] = HotkeyBinding(keyCode: UInt32(event.keyCode), modifiers: mods)
            self.endRecording()
            return nil
        }
    }

    func endRecording() {
        if let m = recordMonitor {
            NSEvent.removeMonitor(m)
            recordMonitor = nil
        }
        if recordingAction != nil {
            recordingAction = nil
            registerHotkeys()
        }
    }

    // MARK: Миниатюры

    func loadThumbnail(for item: WallpaperItem) async {
        if thumbs[item.id] != nil { return }
        let img = await thumbnail(for: item)
        await MainActor.run {
            if let img = img { self.thumbs[item.id] = img }
        }
    }

    func thumbnail(for item: WallpaperItem) async -> NSImage? {
        let cache = thumbsDir.appendingPathComponent("\(item.id.uuidString).png")
        if let img = NSImage(contentsOf: cache) { return img }

        let src = url(for: item)
        var cg: CGImage?
        switch item.kind {
        case .video:
            let gen = AVAssetImageGenerator(asset: AVURLAsset(url: src))
            gen.appliesPreferredTrackTransform = true
            gen.maximumSize = CGSize(width: 480, height: 480)
            cg = try? await gen.image(at: CMTime(seconds: 1, preferredTimescale: 600)).image
            if cg == nil {
                cg = try? await gen.image(at: .zero).image
            }
        case .image, .gif:
            if let s = CGImageSourceCreateWithURL(src as CFURL, nil) {
                let opts: [CFString: Any] = [
                    kCGImageSourceCreateThumbnailFromImageAlways: true,
                    kCGImageSourceCreateThumbnailWithTransform: true,
                    kCGImageSourceThumbnailMaxPixelSize: 480,
                ]
                cg = CGImageSourceCreateThumbnailAtIndex(s, 0, opts as CFDictionary)
            }
        }
        guard let cg = cg else { return nil }
        let rep = NSBitmapImageRep(cgImage: cg)
        if let data = rep.representation(using: .png, properties: [:]) {
            try? data.write(to: cache)
        }
        return NSImage(cgImage: cg, size: NSSize(width: cg.width, height: cg.height))
    }
}
