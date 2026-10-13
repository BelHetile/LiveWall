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
    @Published private(set) var recordingTarget: HotkeyTarget?
    @Published private(set) var folders: [WallpaperFolder] = []
    @Published var selectedFolderID: UUID?
    @Published var iconPickerFolderID: UUID?
    @Published var folderDropTarget: UUID?
    @Published var allTileDropTargeted = false
    @Published private(set) var unlockedFolderIDs: Set<UUID> = []
    @Published private(set) var userIcons: [String] = []
    @Published private(set) var busyCount = 0
    @Published private(set) var pauseReason: String?
    @Published var section: LibrarySection = .wallpapers
    @Published var hoveredID: UUID?
    @Published private(set) var lockScreenID: UUID?
    @Published private(set) var detailID: UUID?
    @Published private(set) var previewPlayer: AVQueuePlayer?
    @Published private(set) var linkDownloads: [LinkDownload] = []
    @Published var showLinkPopover = false
    @Published var linkText = ""
    /// Прогресс оптимизации видео (0…1); в очереди — с прогрессом 0.
    @Published private(set) var optimizing: [UUID: Double] = [:]
    @Published private(set) var videoInfo: [UUID: VideoInfo] = [:]
    @Published var settings = Settings() {
        didSet { settingsChanged(old: oldValue) }
    }

    private let root: URL
    private let filesDir: URL
    private let thumbsDir: URL
    private let stillsDir: URL
    private let indexURL: URL
    private let incomingDir: URL
    private let iconsDir: URL
    private let userIconsDir: URL
    private var previousID: UUID?
    /// Экран блокировки до последнего нажатия хоткея (.some(nil) — «повторял рабочий стол»).
    private var previousLockState: UUID??
    private var folderImageCache: [String: NSImage] = [:]
    private var loading = true
    private var rotateTimer: Timer?
    private var recordMonitor: Any?
    private var spaceObserver: NSObjectProtocol?
    private var lockSyncGeneration = 0
    private var previewLooper: AVPlayerLooper?
    private var imageCache: [UUID: NSImage] = [:]
    private var downloadTasks: [UUID: URLSessionDownloadTask] = [:]
    private var downloadObservations: [UUID: NSKeyValueObservation] = [:]
    private var optimizeQueue: [(id: UUID, manual: Bool)] = []
    private var optimizeActive: UUID?
    private var optimizeToken: CancelToken?

    private struct Index: Codable {
        var items: [WallpaperItem]
        var currentID: UUID?
        var lockScreenID: UUID?
        var folders: [WallpaperFolder]?
        var previousID: UUID?
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
        incomingDir = root.appendingPathComponent("Incoming", isDirectory: true)
        iconsDir = root.appendingPathComponent("Icons", isDirectory: true)
        userIconsDir = root.appendingPathComponent("MyIcons", isDirectory: true)
        try? FileManager.default.createDirectory(at: userIconsDir, withIntermediateDirectories: true)
        try? FileManager.default.createDirectory(at: iconsDir, withIntermediateDirectories: true)
        try? FileManager.default.createDirectory(at: incomingDir, withIntermediateDirectories: true)
        try? FileManager.default.createDirectory(at: filesDir, withIntermediateDirectories: true)
        try? FileManager.default.createDirectory(at: thumbsDir, withIntermediateDirectories: true)
        try? FileManager.default.createDirectory(at: stillsDir, withIntermediateDirectories: true)
        load()
        Lang.current = settings.language
        reloadUserIcons()
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
            lockScreenID = items.contains(where: { $0.id == idx.lockScreenID }) ? idx.lockScreenID : nil
            folders = idx.folders ?? []
            previousID = items.contains(where: { $0.id == idx.previousID }) ? idx.previousID : nil
            // обои из удалённых папок — в «Все»
            let folderIDs = Set(folders.map { $0.id })
            for i in items.indices where items[i].folderID != nil && !folderIDs.contains(items[i].folderID!) {
                items[i].folderID = nil
            }
        }
        var settingsData = UserDefaults.standard.data(forKey: "settings")
        if settingsData == nil,
           let old = UserDefaults(suiteName: "com.local.livewall")?.data(forKey: "settings") {
            // перенос настроек со старого идентификатора приложения (до 1.0)
            settingsData = old
            UserDefaults.standard.set(old, forKey: "settings")
        }
        if let data = settingsData,
           let s = try? JSONDecoder().decode(Settings.self, from: data) {
            settings = s
        }
    }

    private var saveWork: DispatchWorkItem?

    /// Сохранение откладывается на долю секунды: ползунки и серии изменений пишут файл один раз.
    private func save() {
        saveWork?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.writeIndex() }
        saveWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4, execute: work)
    }

    /// Немедленно записать отложенные изменения (при выходе из приложения).
    func flushSave() {
        guard let work = saveWork else { return }
        work.cancel()
        saveWork = nil
        writeIndex()
    }

    private func writeIndex() {
        saveWork = nil
        let idx = Index(items: items, currentID: currentID, lockScreenID: lockScreenID,
                        folders: folders, previousID: previousID)
        if let data = try? JSONEncoder().encode(idx) {
            try? data.write(to: indexURL, options: .atomic)
        }
    }

    /// Окно библиотеки закрыли: отпускаем миниатюры, картинки и превью — при открытии они подгрузятся снова.
    func releaseUICaches() {
        closeDetail()
        thumbs.removeAll()
        imageCache.removeAll()
        folderImageCache.removeAll()
        hoveredID = nil
    }

    private func saveSettings() {
        if let data = try? JSONEncoder().encode(settings) {
            UserDefaults.standard.set(data, forKey: "settings")
        }
    }

    // MARK: Запуск

    func start() {
        // объект входа привязан к идентификатору приложения — обновляем регистрацию
        if settings.launchAtLogin { LoginItem.set(true) }
        applyControllerSettings()
        WallpaperController.shared.onPlayerChange = { [weak self] in self?.objectWillChange.send() }
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

        // закрытые папки запираются при блокировке экрана и сне
        DistributedNotificationCenter.default().addObserver(
            forName: NSNotification.Name("com.apple.screenIsLocked"), object: nil, queue: .main
        ) { [weak self] _ in
            self?.lockAll()
        }
        NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.willSleepNotification, object: nil, queue: .main
        ) { [weak self] _ in
            self?.lockAll()
        }
    }

    private func applyControllerSettings() {
        let c = WallpaperController.shared
        applyPlayback(of: current)
        c.userPaused = paused
        c.stopAfter = TimeInterval(max(0, settings.stopAfterSeconds))
        PauseMonitor.shared.settings = settings
    }

    // MARK: Экран блокировки / системные обои

    /// Обои для экрана блокировки: выбранные явно, иначе — те же, что на рабочем столе.
    var lockItem: WallpaperItem? {
        if let id = lockScreenID, let item = items.first(where: { $0.id == id }) { return item }
        return settings.syncSystemWallpaper ? current : nil
    }

    func syncLockScreen() {
        guard settings.syncSystemWallpaper || lockScreenID != nil, let item = lockItem else { return }
        let src = url(for: item)
        let dir = stillsDir
        lockSyncGeneration += 1
        let generation = lockSyncGeneration
        Task { @MainActor [weak self] in
            guard let still = await LockScreenSync.stillImage(for: item, source: src, dir: dir) else { return }
            guard let self = self, generation == self.lockSyncGeneration else { return }
            let mode = self.lockItem?.mode ?? item.mode
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
        p.message = L("Choose apps (e.g. games) that should pause the wallpaper.")
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
        if old.language != settings.language {
            // язык меняем первым — дальше тексты (причины паузы, меню) берутся уже на новом
            Lang.current = settings.language
            (NSApp.delegate as? AppDelegate)?.setupMainMenu()
        }
        saveSettings()
        applyControllerSettings()
        if old.hotkeys != settings.hotkeys {
            registerHotkeys()
        }
        if old.autoRotate != settings.autoRotate || old.rotateMinutes != settings.rotateMinutes {
            rescheduleRotation()
        }
        if old.launchAtLogin != settings.launchAtLogin {
            LoginItem.set(settings.launchAtLogin)
        }
        if old.showInDock != settings.showInDock {
            NSApp.setActivationPolicy(settings.showInDock ? .regular : .accessory)
            NSApp.activate(ignoringOtherApps: true)
        }
        if settings.syncSystemWallpaper && !old.syncSystemWallpaper {
            syncLockScreen()
        }
    }

    // MARK: Импорт

    static func kind(for url: URL) -> MediaKind? {
        kind(forExtension: url.pathExtension)
    }

    static func kind(forExtension ext: String) -> MediaKind? {
        let e = ext.lowercased()
        // AVFoundation их не проигрывает
        if ["webm", "mkv", "avi", "flv", "wmv", "ogv"].contains(e) { return nil }
        guard !e.isEmpty, let t = UTType(filenameExtension: e) else { return nil }
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
        p.message = L("Choose videos (mp4/mov/m4v), images or GIFs. Folders are scanned recursively.")
        if p.runModal() == .OK {
            importFiles(p.urls)
        }
    }

    func handleDrop(_ providers: [NSItemProvider]) {
        for provider in providers {
            _ = provider.loadObject(ofClass: URL.self) { url, _ in
                guard let url = url else { return }
                DispatchQueue.main.async {
                    if url.isFileURL {
                        self.importFiles([url])
                    } else {
                        self.addFromLink(url.absoluteString)
                    }
                }
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
            message = LF("%ld file(s) skipped: unsupported format. Use mp4/mov/m4v, png/jpg/heic or gif.", skipped)
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
                        self?.message = LF("Could not copy %@: %@", src.lastPathComponent, error.localizedDescription)
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
                    if item.kind == .video && self.settings.optimizeOnImport { self.optimize(item.id, manual: false) }
                }
            }
        }
    }

    // MARK: Импорт по ссылке

    static let userAgent = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15"

    /// Отправка ссылки из поля ввода.
    func submitLink() {
        let text = linkText
        linkText = ""
        showLinkPopover = false
        addFromLink(text)
    }

    /// ⌘V в окне библиотеки: файлы или ссылки из буфера обмена.
    func pasteFromClipboard() {
        let pb = NSPasteboard.general
        if let urls = pb.readObjects(forClasses: [NSURL.self], options: nil) as? [URL], !urls.isEmpty {
            let files = urls.filter { $0.isFileURL }
            if !files.isEmpty { importFiles(files) }
            for u in urls where !u.isFileURL {
                addFromLink(u.absoluteString)
            }
            return
        }
        if let s = pb.string(forType: .string), !s.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            addFromLink(s)
        } else {
            message = L("Clipboard has no link or file to add.")
        }
    }

    static func normalizeLink(_ text: String) -> URL? {
        var s = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if let first = s.split(whereSeparator: { $0 == "\n" || $0 == " " || $0 == "\t" }).first {
            s = String(first)
        }
        guard !s.isEmpty else { return nil }
        let lower = s.lowercased()
        if !lower.hasPrefix("http://") && !lower.hasPrefix("https://") && !lower.hasPrefix("file://") {
            s = "https://" + s
        }
        let url = URL(string: s)
            ?? s.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed).flatMap { URL(string: $0) }
        guard let u = url else { return nil }
        if u.isFileURL { return u }
        guard let host = u.host, host.contains(".") || host == "localhost" else { return nil }
        return u
    }

    static func displayName(for url: URL) -> String {
        let last = (url.deletingPathExtension().lastPathComponent.removingPercentEncoding
            ?? url.deletingPathExtension().lastPathComponent)
        if !last.isEmpty && last != "/" { return last }
        return url.host ?? "Wallpaper"
    }

    func addFromLink(_ text: String) {
        guard let url = Library.normalizeLink(text) else {
            message = L("That doesn't look like a link.")
            return
        }
        if url.isFileURL {
            importFiles([url])
            return
        }
        message = nil
        let id = UUID()
        linkDownloads.append(LinkDownload(id: id, title: Library.displayName(for: url), progress: 0, error: nil))
        startDownload(id: id, url: url, depth: 0)
    }

    func cancelDownload(_ id: UUID) {
        if let task = downloadTasks[id] {
            task.cancel()
        }
        removeDownload(id)
    }

    func dismissDownload(_ id: UUID) {
        removeDownload(id)
    }

    private func removeDownload(_ id: UUID) {
        downloadTasks[id] = nil
        downloadObservations[id] = nil
        linkDownloads.removeAll { $0.id == id }
    }

    private func failDownload(_ id: UUID, _ text: String) {
        downloadTasks[id] = nil
        downloadObservations[id] = nil
        if let i = linkDownloads.firstIndex(where: { $0.id == id }) {
            linkDownloads[i].error = text
        }
    }

    private func setDownloadProgress(_ id: UUID, _ value: Double) {
        guard let i = linkDownloads.firstIndex(where: { $0.id == id }) else { return }
        if abs(linkDownloads[i].progress - value) >= 0.01 || value >= 1 {
            linkDownloads[i].progress = value
        }
    }

    private func startDownload(id: UUID, url: URL, depth: Int) {
        var request = URLRequest(url: url)
        request.setValue(Library.userAgent, forHTTPHeaderField: "User-Agent")
        let dir = incomingDir
        let task = URLSession.shared.downloadTask(with: request) { [weak self] tmp, response, error in
            // временный файл удаляется сразу после выхода из обработчика — переносим его к себе
            var saved: URL?
            if let tmp = tmp {
                let dest = dir.appendingPathComponent(UUID().uuidString)
                if (try? FileManager.default.moveItem(at: tmp, to: dest)) != nil {
                    saved = dest
                }
            }
            DispatchQueue.main.async {
                self?.finishDownload(id: id, source: url, file: saved, response: response, error: error, depth: depth)
            }
        }
        downloadObservations[id] = task.progress.observe(\.fractionCompleted, options: [.new]) { [weak self] progress, _ in
            let value = progress.fractionCompleted
            DispatchQueue.main.async { self?.setDownloadProgress(id, value) }
        }
        downloadTasks[id] = task
        task.resume()
    }

    private func finishDownload(id: UUID, source: URL, file: URL?, response: URLResponse?, error: Error?, depth: Int) {
        // загрузку уже отменили
        guard linkDownloads.contains(where: { $0.id == id }) else {
            if let file = file { try? FileManager.default.removeItem(at: file) }
            return
        }
        downloadTasks[id] = nil
        downloadObservations[id] = nil

        if let nsError = error as NSError?, nsError.code == NSURLErrorCancelled {
            removeDownload(id)
            return
        }
        guard let file = file, error == nil else {
            failDownload(id, error?.localizedDescription ?? L("Download failed."))
            return
        }
        if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
            try? FileManager.default.removeItem(at: file)
            failDownload(id, LF("The server answered %ld.", http.statusCode))
            return
        }

        let mime = response?.mimeType?.lowercased() ?? ""
        let finalURL = response?.url ?? source

        // страница, а не файл: ищем на ней видео или картинку
        if mime.hasPrefix("text/html") || mime == "application/xhtml+xml" {
            let media = depth == 0 ? Library.findMediaURL(inHTMLAt: file, base: finalURL) : nil
            try? FileManager.default.removeItem(at: file)
            guard let mediaURL = media else {
                failDownload(id, L("No video or image file found on this page. Paste a direct link to the file."))
                return
            }
            if let i = linkDownloads.firstIndex(where: { $0.id == id }) {
                linkDownloads[i].title = Library.displayName(for: mediaURL)
                linkDownloads[i].progress = 0
            }
            startDownload(id: id, url: mediaURL, depth: depth + 1)
            return
        }

        guard let ext = Library.fileExtension(url: finalURL, mime: mime, suggested: response?.suggestedFilename) else {
            try? FileManager.default.removeItem(at: file)
            let type = mime.isEmpty ? "" : " (\(mime))"
            failDownload(id, LF("Unsupported file type%@. Use mp4/mov/m4v, png/jpg/heic/webp or gif.", type))
            return
        }

        var name = Library.displayName(for: finalURL)
        if let suggested = response?.suggestedFilename, !suggested.isEmpty,
           Library.kind(forExtension: (suggested as NSString).pathExtension) != nil {
            name = (suggested as NSString).deletingPathExtension
        }
        removeDownload(id)
        addDownloadedFile(file, name: name, ext: ext)
    }

    static func fileExtension(url: URL, mime: String, suggested: String?) -> String? {
        let candidates = [suggested.map { ($0 as NSString).pathExtension } ?? "", url.pathExtension]
        for c in candidates {
            let e = c.lowercased()
            if kind(forExtension: e) != nil { return e == "jpeg" ? "jpg" : e }
        }
        let byMime: [String: String] = [
            "video/mp4": "mp4", "video/quicktime": "mov", "video/x-m4v": "m4v",
            "image/jpeg": "jpg", "image/jpg": "jpg", "image/png": "png", "image/gif": "gif",
            "image/heic": "heic", "image/heif": "heic", "image/webp": "webp",
            "image/tiff": "tiff", "image/bmp": "bmp",
        ]
        return byMime[mime]
    }

    /// Ищет на HTML-странице ссылку на видео или картинку: og:video, <video>/<source>, прямые .mp4, затем og:image.
    static func findMediaURL(inHTMLAt file: URL, base: URL) -> URL? {
        guard let handle = try? FileHandle(forReadingFrom: file) else { return nil }
        let data = handle.readData(ofLength: 4 * 1024 * 1024)
        try? handle.close()
        let html = String(decoding: data, as: UTF8.self)

        func matches(_ pattern: String, in text: String) -> [[String]] {
            guard let re = try? NSRegularExpression(pattern: pattern, options: [.caseInsensitive]) else { return [] }
            let ns = text as NSString
            return re.matches(in: text, range: NSRange(location: 0, length: ns.length)).map { m in
                (0..<m.numberOfRanges).map { i in
                    let r = m.range(at: i)
                    return r.location == NSNotFound ? "" : ns.substring(with: r)
                }
            }
        }

        func attribute(_ name: String, in tag: String) -> String? {
            let found = matches("\\b\(name)\\s*=\\s*(\"([^\"]*)\"|'([^']*)'|([^\\s>]+))", in: tag)
            guard let m = found.first else { return nil }
            let value = [m[2], m[3], m[4]].first(where: { !$0.isEmpty }) ?? ""
            return value.isEmpty ? nil : value
        }

        func resolve(_ raw: String) -> URL? {
            let cleaned = raw
                .replacingOccurrences(of: "&amp;", with: "&")
                .replacingOccurrences(of: "\\/", with: "/")
                .trimmingCharacters(in: .whitespacesAndNewlines)
            guard !cleaned.isEmpty, !cleaned.hasPrefix("data:"), !cleaned.hasPrefix("blob:") else { return nil }
            guard let u = URL(string: cleaned, relativeTo: base)?.absoluteURL,
                  let scheme = u.scheme?.lowercased(), scheme == "http" || scheme == "https" else { return nil }
            return u
        }

        func isVideoFile(_ u: URL) -> Bool {
            ["mp4", "mov", "m4v"].contains(u.pathExtension.lowercased())
        }

        var videos: [URL] = []
        var images: [URL] = []

        for m in matches("<meta\\b[^>]*>", in: html) {
            let tag = m[0]
            let key = (attribute("property", in: tag) ?? attribute("name", in: tag) ?? "").lowercased()
            guard let content = attribute("content", in: tag), let u = resolve(content) else { continue }
            switch key {
            case "og:video:secure_url", "og:video:url", "og:video", "twitter:player:stream":
                videos.append(u)
            case "og:image:secure_url", "og:image", "og:image:url", "twitter:image":
                images.append(u)
            default:
                break
            }
        }
        for m in matches("<(?:video|source)\\b[^>]*>", in: html) {
            if let src = attribute("src", in: m[0]), let u = resolve(src) { videos.append(u) }
        }
        for m in matches("https?:\\\\?/\\\\?/[^\"'\\s<>]+?\\.(?:mp4|mov|m4v)(?:\\?[^\"'\\s<>]*)?", in: html) {
            if let u = resolve(m[0]) { videos.append(u) }
        }

        // сначала то, что точно похоже на видеофайл
        if let v = videos.first(where: isVideoFile) { return v }
        if let v = videos.first { return v }
        return images.first
    }

    private func addDownloadedFile(_ file: URL, name: String, ext: String) {
        guard let kind = Library.kind(forExtension: ext) else {
            try? FileManager.default.removeItem(at: file)
            return
        }
        let id = UUID()
        let dest = filesDir.appendingPathComponent("\(id.uuidString).\(ext)")
        do {
            try FileManager.default.moveItem(at: file, to: dest)
        } catch {
            try? FileManager.default.removeItem(at: file)
            message = LF("Could not save %@: %@", name, error.localizedDescription)
            return
        }
        let item = WallpaperItem(id: id, name: name, file: dest.lastPathComponent, kind: kind, mode: .fill)
        items.append(item)
        save()
        if currentID == nil { select(id) }
        if kind == .video && settings.optimizeOnImport { optimize(id, manual: false) }
    }

    func remove(_ id: UUID) {
        guard let idx = items.firstIndex(where: { $0.id == id }) else { return }
        let item = items[idx]
        try? FileManager.default.removeItem(at: url(for: item))
        try? FileManager.default.removeItem(at: thumbsDir.appendingPathComponent("\(id.uuidString).png"))
        try? FileManager.default.removeItem(at: stillsDir.appendingPathComponent("\(id.uuidString).png"))
        thumbs[id] = nil
        imageCache[id] = nil
        videoInfo[id] = nil
        cancelOptimize(id)
        if detailID == id { closeDetail() }
        if lockScreenID == id {
            lockScreenID = nil
            syncLockScreen()
        }
        let wasCurrent = (id == currentID)
        let hadHotkey = item.hotkey != nil
        items.remove(at: idx)
        if hadHotkey { registerHotkeys() }
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

    // MARK: Оптимизация видео

    var optimizeFpsLimit: Double { Double(min(max(settings.optimizeFpsLimit, 15), 120)) }

    func loadVideoInfo(_ id: UUID, force: Bool = false) {
        guard let item = items.first(where: { $0.id == id }), item.kind == .video else { return }
        guard force || videoInfo[id] == nil else { return }
        let src = url(for: item)
        Task { @MainActor [weak self] in
            guard let info = await VideoOptimizer.info(for: src) else { return }
            self?.videoInfo[id] = info
        }
    }

    /// Можно ли сделать видео легче для этого Mac (nil — сведения ещё не загружены).
    func canOptimize(_ id: UUID) -> Bool? {
        guard let info = videoInfo[id] else { return nil }
        return VideoOptimizer.needsOptimization(info, fpsLimit: optimizeFpsLimit,
                                                target: VideoOptimizer.targetScreenPixels())
    }

    /// Ставит видео в очередь. `manual` — пользователь нажал кнопку (сообщаем об итоге).
    func optimize(_ id: UUID, manual: Bool) {
        guard let item = items.first(where: { $0.id == id }), item.kind == .video else { return }
        guard optimizing[id] == nil else { return }
        optimizing[id] = 0
        optimizeQueue.append((id, manual))
        runNextOptimization()
    }

    /// Все видео, которые ещё не оптимизированы.
    func optimizeAll() {
        let ids = items.filter { $0.kind == .video && $0.optimized != true }.map { $0.id }
        if ids.isEmpty {
            message = L("All videos are already optimized.")
            return
        }
        for id in ids { optimize(id, manual: false) }
    }

    var pendingOptimizationCount: Int {
        items.filter { $0.kind == .video && $0.optimized != true }.count
    }

    func cancelOptimize(_ id: UUID) {
        if optimizeActive == id {
            optimizeToken?.cancel()
        } else {
            optimizeQueue.removeAll { $0.id == id }
            optimizing[id] = nil
        }
    }

    private func runNextOptimization() {
        guard optimizeActive == nil, !optimizeQueue.isEmpty else { return }
        let job = optimizeQueue.removeFirst()
        guard let item = items.first(where: { $0.id == job.id }) else {
            optimizing[job.id] = nil
            runNextOptimization()
            return
        }
        let id = item.id
        let src = url(for: item)
        let fpsLimit = optimizeFpsLimit
        let target = VideoOptimizer.targetScreenPixels()
        let temp = incomingDir.appendingPathComponent("\(id.uuidString)-optimized.mov")
        let token = CancelToken()
        optimizeActive = id
        optimizeToken = token

        Task.detached(priority: .utility) {
            var outcome: Result<Bool, Error>
            if let info = await VideoOptimizer.info(for: src),
               !VideoOptimizer.needsOptimization(info, fpsLimit: fpsLimit, target: target) {
                outcome = .success(false)
            } else {
                var last = 0.0
                do {
                    try await VideoOptimizer.transcode(source: src, dest: temp, fpsLimit: fpsLimit, target: target,
                                                       token: token) { value in
                        guard value - last >= 0.01 || value >= 1 else { return }
                        last = value
                        DispatchQueue.main.async { self.optimizing[id] = value }
                    }
                    outcome = .success(true)
                } catch {
                    outcome = .failure(error)
                }
            }
            let result = outcome
            await MainActor.run {
                self.finishOptimization(id: id, temp: temp, manual: job.manual, result: result)
            }
        }
    }

    private func finishOptimization(id: UUID, temp: URL, manual: Bool, result: Result<Bool, Error>) {
        optimizeActive = nil
        optimizeToken = nil
        optimizing[id] = nil
        defer { runNextOptimization() }
        guard let idx = items.firstIndex(where: { $0.id == id }) else {
            try? FileManager.default.removeItem(at: temp)
            return
        }
        let name = items[idx].name
        switch result {
        case .failure(let error):
            try? FileManager.default.removeItem(at: temp)
            if let e = error as? OptimizeError, case .cancelled = e { return }
            message = LF("Could not optimize %@: %@", name, error.localizedDescription)
        case .success(false):
            items[idx].optimized = true
            save()
            if manual { message = LF("%@ is already light enough for this Mac.", name) }
        case .success(true):
            let old = url(for: items[idx])
            let oldSize = (try? old.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
            let newSize = (try? temp.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
            let newName = "\(id.uuidString).mov"
            let dest = filesDir.appendingPathComponent(newName)
            do {
                if dest.path == old.path {
                    _ = try FileManager.default.replaceItemAt(dest, withItemAt: temp)
                } else {
                    try? FileManager.default.removeItem(at: dest)
                    try FileManager.default.moveItem(at: temp, to: dest)
                    try? FileManager.default.removeItem(at: old)
                }
            } catch {
                try? FileManager.default.removeItem(at: temp)
                message = LF("Could not optimize %@: %@", name, error.localizedDescription)
                return
            }
            items[idx].file = newName
            items[idx].optimized = true
            save()
            let item = items[idx]
            if currentID == id {
                WallpaperController.shared.apply(item: item, url: dest)
            }
            if detailID == id {
                openDetail(id)
            }
            loadVideoInfo(id, force: true)
            if oldSize > 0 && newSize > 0 {
                let fmt = ByteCountFormatter()
                message = LF("%@ optimized: %@ → %@.", name, fmt.string(fromByteCount: Int64(oldSize)),
                             fmt.string(fromByteCount: Int64(newSize)))
            }
        }
    }

    func revealInFinder(_ item: WallpaperItem) {
        NSWorkspace.shared.activateFileViewerSelecting([url(for: item)])
    }

    // MARK: Переключение

    func select(_ id: UUID) {
        guard let item = items.first(where: { $0.id == id }) else { return }
        if let old = currentID, old != id { previousID = old }
        currentID = id
        save()
        applyPlayback(of: item)
        WallpaperController.shared.apply(item: item, url: url(for: item))
        if detailID == id { releasePreview() }
        syncLockScreen()
    }

    // MARK: Рабочий стол / экран блокировки

    func setDesktop(_ id: UUID) {
        select(id)
    }

    func setLockScreen(_ id: UUID) {
        guard items.contains(where: { $0.id == id }) else { return }
        lockScreenID = id
        save()
        syncLockScreen()
    }

    /// Экран блокировки снова повторяет рабочий стол.
    func lockScreenFollowDesktop() {
        lockScreenID = nil
        save()
        syncLockScreen()
    }

    // MARK: Воспроизведение у каждых обоев

    func effectiveVolume(_ item: WallpaperItem) -> Double { item.volume ?? settings.volume }
    func effectiveSpeed(_ item: WallpaperItem) -> Double { item.speed ?? settings.speed }
    func effectiveMuted(_ item: WallpaperItem) -> Bool { item.muted ?? settings.muted }

    private func applyPlayback(of item: WallpaperItem?) {
        let c = WallpaperController.shared
        if let item = item {
            c.muted = effectiveMuted(item)
            c.volume = Float(effectiveVolume(item))
            c.speed = Float(effectiveSpeed(item))
        } else {
            c.muted = settings.muted
            c.volume = Float(settings.volume)
            c.speed = Float(settings.speed)
        }
    }

    private func updateItem(_ id: UUID, _ change: (inout WallpaperItem) -> Void) {
        guard let idx = items.firstIndex(where: { $0.id == id }) else { return }
        change(&items[idx])
        save()
        if id == currentID { applyPlayback(of: items[idx]) }
        if id == detailID, let p = previewPlayer {
            p.defaultRate = Float(effectiveSpeed(items[idx]))
            if p.rate != 0 { p.play() }
        }
    }

    func setVolume(_ v: Double, for id: UUID) {
        updateItem(id) { $0.volume = min(max(v, 0), 1); if v > 0 { $0.muted = false } }
    }

    func setSpeed(_ v: Double, for id: UUID) {
        updateItem(id) { $0.speed = v }
    }

    func setMuted(_ m: Bool, for id: UUID) {
        updateItem(id) { $0.muted = m }
    }

    // MARK: Панель обоев

    func openDetail(_ id: UUID) {
        guard let item = items.first(where: { $0.id == id }) else { return }
        // обои из закрытой папки — только после пароля
        if isHidden(item), let folderID = item.folderID, !requestUnlock(folderID) { return }
        releasePreview()
        detailID = id
        if item.kind == .video { loadVideoInfo(id) }
        // для не активного видео — своё беззвучное превью
        if item.kind == .video && id != currentID {
            let p = AVQueuePlayer()
            p.isMuted = true
            p.preventsDisplaySleepDuringVideoPlayback = false
            previewLooper = AVPlayerLooper(player: p, templateItem: AVPlayerItem(url: url(for: item)))
            p.defaultRate = Float(effectiveSpeed(item))
            p.play()
            previewPlayer = p
        }
    }

    func closeDetail() {
        releasePreview()
        detailID = nil
    }

    func showAdjacentDetail(_ delta: Int) {
        let pool = visibleItems
        guard let id = detailID, let i = pool.firstIndex(where: { $0.id == id }), !pool.isEmpty else { return }
        let n = pool.count
        openDetail(pool[((i + delta) % n + n) % n].id)
    }

    /// Полноразмерная картинка для панели (картинки и первый кадр GIF), с кэшем.
    func previewImage(for item: WallpaperItem) -> NSImage? {
        if let img = imageCache[item.id] { return img }
        guard item.kind != .video, let img = NSImage(contentsOf: url(for: item)) else { return nil }
        imageCache[item.id] = img
        return img
    }

    private func releasePreview() {
        previewPlayer?.pause()
        previewPlayer = nil
        previewLooper = nil
    }

    /// Обои, по которым ходят Next / Previous / Random / автосмена: без закрытых папок.
    var browsableItems: [WallpaperItem] {
        items.filter { !isHidden($0) }
    }

    func step(_ delta: Int) {
        let pool = browsableItems
        guard !pool.isEmpty else { return }
        let n = pool.count
        let i = pool.firstIndex(where: { $0.id == currentID }) ?? (delta > 0 ? -1 : 0)
        let j = ((i + delta) % n + n) % n
        select(pool[j].id)
    }

    func random() {
        let all = browsableItems
        guard !all.isEmpty else { return }
        if all.count == 1 {
            select(all[0].id)
            return
        }
        let pool = all.filter { $0.id != currentID }
        if let pick = pool.randomElement() { select(pick.id) }
    }

    func setMode(_ mode: FillMode, for id: UUID) {
        guard let idx = items.firstIndex(where: { $0.id == id }) else { return }
        items[idx].mode = mode
        save()
        if id == currentID {
            WallpaperController.shared.updateMode(mode)
        }
        if id == currentID || id == lockItem?.id {
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
        if let c = current {
            setMuted(!effectiveMuted(c), for: c.id)
        } else {
            settings.muted.toggle()
        }
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
        // свои хоткеи у обоев: поставить / вернуть прежние
        for (i, item) in items.enumerated() {
            guard let b = item.hotkey else { continue }
            let id = item.id
            hk.register(id: UInt32(1000 + i), binding: b) { [weak self] in
                self?.toggleWallpaperHotkey(id)
            }
        }
    }

    // MARK: Запись хоткея

    var recordingAction: HotkeyAction? {
        if case .action(let a)? = recordingTarget { return a }
        return nil
    }

    func beginRecording(_ action: HotkeyAction) {
        beginRecording(target: .action(action))
    }

    func beginRecording(target: HotkeyTarget) {
        endRecording()
        recordingTarget = target
        // пока записываем — снимаем глобальные хоткеи, чтобы они не перехватывали нажатие
        HotkeyCenter.shared.unregisterAll()
        recordMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self = self, let target = self.recordingTarget else { return event }
            if event.keyCode == 53 { // Esc
                self.endRecording()
                return nil
            }
            let mods = HotkeyFormat.carbonMods(event.modifierFlags)
            if mods == 0 { return nil } // нужен хотя бы один модификатор
            let binding = HotkeyBinding(keyCode: UInt32(event.keyCode), modifiers: mods)
            self.assign(binding, to: target)
            self.endRecording()
            return nil
        }
    }

    func endRecording() {
        if let m = recordMonitor {
            NSEvent.removeMonitor(m)
            recordMonitor = nil
        }
        if recordingTarget != nil {
            recordingTarget = nil
            registerHotkeys()
        }
    }

    /// Назначает сочетание; если оно уже занято другим действием или обоями — забирает его оттуда.
    private func assign(_ binding: HotkeyBinding, to target: HotkeyTarget) {
        var movedFrom: String?
        for (key, b) in settings.hotkeys where b == binding {
            if case .action(let a) = target, a.rawValue == key { continue }
            settings.hotkeys[key] = nil
            movedFrom = HotkeyAction(rawValue: key)?.title ?? key
        }
        for i in items.indices where items[i].hotkey == binding {
            if case .wallpaper(let id) = target, items[i].id == id { continue }
            items[i].hotkey = nil
            movedFrom = items[i].name
        }
        switch target {
        case .action(let a):
            settings.hotkeys[a.rawValue] = binding
        case .wallpaper(let id):
            if let i = items.firstIndex(where: { $0.id == id }) { items[i].hotkey = binding }
        }
        save()
        if let from = movedFrom {
            message = LF("%@ was moved from “%@”.", HotkeyFormat.string(binding), from)
        }
    }

    func clearWallpaperHotkey(_ id: UUID) {
        guard let i = items.firstIndex(where: { $0.id == id }) else { return }
        items[i].hotkey = nil
        save()
        registerHotkeys()
    }

    /// Хоткей обоев: первое нажатие ставит их, повторное — возвращает те, что стояли до этого.
    /// Работает и для обоев из закрытых папок — пароль не нужен.
    func toggleWallpaperHotkey(_ id: UUID) {
        guard let item = items.first(where: { $0.id == id }) else { return }
        switch item.hotkeyScope ?? .both {
        case .desktop:
            toggleDesktop(id)
        case .lockScreen:
            if lockScreenID == id {
                revertLockScreen(from: id)
            } else {
                applyLockScreen(id)
            }
        case .both:
            // направление решает рабочий стол: стоят эти обои — возвращаем оба экрана
            let goBack = (currentID == id)
            toggleDesktop(id)
            if goBack {
                revertLockScreen(from: id)
            } else {
                applyLockScreen(id)
            }
        }
    }

    private func toggleDesktop(_ id: UUID) {
        if currentID == id {
            if let prev = previousID, prev != id, items.contains(where: { $0.id == prev }) {
                select(prev)
            }
        } else {
            select(id)
        }
    }

    private func applyLockScreen(_ id: UUID) {
        if lockScreenID != id { previousLockState = .some(lockScreenID) }
        setLockScreen(id)
    }

    private func revertLockScreen(from id: UUID) {
        guard lockScreenID == id else { return }
        if case .some(let prev) = previousLockState,
           prev == nil || items.contains(where: { $0.id == prev }) {
            lockScreenID = prev
        } else {
            lockScreenID = nil
        }
        previousLockState = .some(id)
        save()
        syncLockScreen()
    }

    func setHotkeyScope(_ scope: HotkeyScope, for id: UUID) {
        guard let i = items.firstIndex(where: { $0.id == id }) else { return }
        items[i].hotkeyScope = scope
        save()
    }

    // MARK: Папки

    var visibleItems: [WallpaperItem] {
        guard let folder = selectedFolderID else { return items.filter { !isHidden($0) } }
        if isLocked(folder) { return [] }
        return items.filter { $0.folderID == folder }
    }

    // MARK: Закрытые папки

    func folder(_ id: UUID?) -> WallpaperFolder? {
        guard let id = id else { return nil }
        return folders.first(where: { $0.id == id })
    }

    /// Папка с паролем, которую ещё не открыли.
    func isLocked(_ folderID: UUID) -> Bool {
        guard let f = folder(folderID), f.isProtected else { return false }
        return !unlockedFolderIDs.contains(folderID)
    }

    /// Обои лежат в закрытой (не открытой) папке.
    func isHidden(_ item: WallpaperItem) -> Bool {
        guard let fid = item.folderID else { return false }
        return isLocked(fid)
    }

    /// Открывает папку: спрашивает пароль, если нужно. true — доступ есть.
    @discardableResult
    func requestUnlock(_ folderID: UUID) -> Bool {
        guard let f = folder(folderID) else { return false }
        guard f.isProtected, !unlockedFolderIDs.contains(folderID) else { return true }
        guard let password = PasswordPrompt.ask(title: LF("“%@” is locked", f.name),
                                                message: L("Enter the folder password."),
                                                button: L("Unlock"), confirm: false) else { return false }
        guard let salt = f.passwordSalt, let hash = f.passwordHash,
              PasswordHasher.verify(password, saltB64: salt, hashB64: hash) else {
            message = LF("Wrong password for “%@”.", f.name)
            return false
        }
        unlockedFolderIDs.insert(folderID)
        message = nil
        return true
    }

    func openFolder(_ folderID: UUID) {
        if requestUnlock(folderID) { selectedFolderID = folderID }
    }

    func setPassword(_ folderID: UUID) {
        guard let i = folders.firstIndex(where: { $0.id == folderID }) else { return }
        if folders[i].isProtected && !requestUnlock(folderID) { return }
        let title = folders[i].isProtected ? L("Change Password") : LF("Lock “%@” with a Password", folders[i].name)
        guard let password = PasswordPrompt.ask(
            title: title,
            message: L("Wallpapers in this folder will be hidden until you enter the password. Their hotkeys keep working. There is no way to recover a forgotten password."),
            button: L("Set Password"), confirm: true) else { return }
        let salt = PasswordHasher.makeSalt()
        folders[i].passwordSalt = salt.base64EncodedString()
        folders[i].passwordHash = PasswordHasher.hash(password, salt: salt).base64EncodedString()
        unlockedFolderIDs.insert(folderID)
        save()
    }

    func removePassword(_ folderID: UUID) {
        guard let i = folders.firstIndex(where: { $0.id == folderID }), folders[i].isProtected else { return }
        guard requestUnlock(folderID) else { return }
        folders[i].passwordSalt = nil
        folders[i].passwordHash = nil
        unlockedFolderIDs.remove(folderID)
        save()
    }

    func lockFolder(_ folderID: UUID) {
        unlockedFolderIDs.remove(folderID)
        hideLockedContent()
    }

    /// Закрывает все папки (окно закрыто, экран заблокирован, сон).
    func lockAll() {
        guard !unlockedFolderIDs.isEmpty else { return }
        unlockedFolderIDs.removeAll()
        hideLockedContent()
    }

    private func hideLockedContent() {
        if let sel = selectedFolderID, isLocked(sel) { selectedFolderID = nil }
        if let id = detailID, let item = items.first(where: { $0.id == id }), isHidden(item) { closeDetail() }
    }

    var selectedFolder: WallpaperFolder? {
        folders.first(where: { $0.id == selectedFolderID })
    }

    func count(in folderID: UUID) -> Int {
        if isLocked(folderID) { return 0 }
        return items.filter { $0.folderID == folderID }.count
    }

    private func rawCount(in folderID: UUID) -> Int {
        items.filter { $0.folderID == folderID }.count
    }

    @discardableResult
    func createFolder(moving itemIDs: [UUID] = []) -> UUID? {
        guard let name = Library.promptForName(title: L("New Folder"), message: L("Name the folder."),
                                               initial: L("New Folder"), confirm: L("Create")) else { return nil }
        let folder = WallpaperFolder(id: UUID(), name: name,
                                     icon: .symbol(name: "folder.fill", palette: folders.count % Theme.folderPalettes.count))
        folders.append(folder)
        for id in itemIDs { moveItem(id, to: folder.id) }
        save()
        return folder.id
    }

    func renameFolder(_ id: UUID) {
        guard requestUnlock(id), let i = folders.firstIndex(where: { $0.id == id }) else { return }
        guard let name = Library.promptForName(title: L("Rename Folder"), message: nil,
                                               initial: folders[i].name, confirm: L("Rename")) else { return }
        folders[i].name = name
        save()
    }

    func deleteFolder(_ id: UUID) {
        guard requestUnlock(id), let i = folders.firstIndex(where: { $0.id == id }) else { return }
        if folders[i].isProtected {
            let alert = NSAlert()
            alert.messageText = LF("Delete “%@”?", folders[i].name)
            alert.informativeText = LF("%ld wallpaper(s) will move out of the folder and become visible to everyone.", rawCount(in: id))
            alert.addButton(withTitle: L("Delete Folder"))
            alert.addButton(withTitle: L("Cancel"))
            guard alert.runModal() == .alertFirstButtonReturn else { return }
        }
        unlockedFolderIDs.remove(id)
        if case .image(let file) = folders[i].icon { removeIconFile(file) }
        folders.remove(at: i)
        for j in items.indices where items[j].folderID == id { items[j].folderID = nil }
        if selectedFolderID == id { selectedFolderID = nil }
        save()
    }

    func moveItem(_ itemID: UUID, to folderID: UUID?) {
        guard let i = items.firstIndex(where: { $0.id == itemID }) else { return }
        items[i].folderID = folderID
        save()
    }

    func handleFolderDrop(_ providers: [NSItemProvider], folderID: UUID?) -> Bool {
        var handled = false
        for provider in providers where provider.canLoadObject(ofClass: NSString.self) {
            handled = true
            _ = provider.loadObject(ofClass: NSString.self) { obj, _ in
                guard let s = obj as? String, let id = UUID(uuidString: s) else { return }
                DispatchQueue.main.async { self.moveItem(id, to: folderID) }
            }
        }
        return handled
    }

    // MARK: Иконки папок

    // MARK: Мои иконки (своя библиотека для папок)

    private func reloadUserIcons() {
        let files = (try? FileManager.default.contentsOfDirectory(at: userIconsDir,
                                                                  includingPropertiesForKeys: [.creationDateKey],
                                                                  options: [.skipsHiddenFiles])) ?? []
        let dated = files.map { url -> (String, Date) in
            let d = (try? url.resourceValues(forKeys: [.creationDateKey]).creationDate) ?? .distantPast
            return (url.lastPathComponent, d)
        }
        userIcons = dated.sorted { $0.1 < $1.1 }.map { $0.0 }
    }

    /// Квадратный PNG 512×512 (заполнение с обрезкой).
    static func squarePNG(from image: NSImage, side: CGFloat = 512) -> Data? {
        let scaled = NSImage(size: NSSize(width: side, height: side), flipped: false) { rect in
            let s = image.size
            guard s.width > 0, s.height > 0 else { return true }
            let k = max(rect.width / s.width, rect.height / s.height)
            let w = s.width * k, h = s.height * k
            image.draw(in: NSRect(x: (rect.width - w) / 2, y: (rect.height - h) / 2, width: w, height: h))
            return true
        }
        guard let tiff = scaled.tiffRepresentation, let rep = NSBitmapImageRep(data: tiff) else { return nil }
        return rep.representation(using: .png, properties: [:])
    }

    func importUserIcons() {
        let p = NSOpenPanel()
        p.allowsMultipleSelection = true
        p.canChooseDirectories = false
        p.allowedContentTypes = [.image]
        p.message = L("Choose images to add to My Icons.")
        guard p.runModal() == .OK else { return }
        var failed = 0
        for url in p.urls {
            guard let img = NSImage(contentsOf: url), let png = Library.squarePNG(from: img) else {
                failed += 1
                continue
            }
            let name = "\(UUID().uuidString).png"
            do {
                try png.write(to: userIconsDir.appendingPathComponent(name), options: .atomic)
            } catch {
                failed += 1
            }
        }
        reloadUserIcons()
        if failed > 0 { message = LF("%ld image(s) could not be added to My Icons.", failed) }
    }

    func removeUserIcon(_ name: String) {
        folderImageCache["my:" + name] = nil
        try? FileManager.default.removeItem(at: userIconsDir.appendingPathComponent(name))
        reloadUserIcons()
    }

    func userIconImage(_ name: String) -> NSImage? {
        let key = "my:" + name
        if let img = folderImageCache[key] { return img }
        guard let img = NSImage(contentsOf: userIconsDir.appendingPathComponent(name)) else { return nil }
        folderImageCache[key] = img
        return img
    }

    /// Ставит иконку из «Моих иконок» на папку (копия — папка не зависит от библиотеки иконок).
    func setFolderUserIcon(_ name: String, for folderID: UUID) {
        guard let i = folders.firstIndex(where: { $0.id == folderID }) else { return }
        let file = "\(folderID.uuidString)-\(UUID().uuidString.prefix(8)).png"
        do {
            try FileManager.default.copyItem(at: userIconsDir.appendingPathComponent(name),
                                             to: iconsDir.appendingPathComponent(file))
        } catch {
            message = LF("Could not set the icon: %@", error.localizedDescription)
            return
        }
        if case .image(let old) = folders[i].icon { removeIconFile(old) }
        folders[i].icon = .image(file: file)
        save()
    }

    // MARK: Цветовые стили

    var allStyles: [ColorStyle] { ColorStyle.builtIns + settings.customStyles }

    func selectStyle(_ id: String) {
        settings.colorStyleID = id
    }

    func createStyle() {
        let base = Theme.style
        guard let name = Library.promptForName(title: L("New Color Style"), message: L("Pick the colors after creating it."),
                                               initial: L("My Style"), confirm: L("Create")) else { return }
        let style = ColorStyle(id: UUID().uuidString, name: name, primary: base.primary, secondary: base.secondary)
        settings.customStyles.append(style)
        settings.colorStyleID = style.id
    }

    func updateStyle(_ id: String, primary: String? = nil, secondary: String? = nil) {
        guard let i = settings.customStyles.firstIndex(where: { $0.id == id }) else { return }
        if let p = primary { settings.customStyles[i].primary = p }
        if let s = secondary { settings.customStyles[i].secondary = s }
    }

    func renameStyle(_ id: String) {
        guard let i = settings.customStyles.firstIndex(where: { $0.id == id }) else { return }
        guard let name = Library.promptForName(title: L("Rename Style"), message: nil,
                                               initial: settings.customStyles[i].name, confirm: L("Rename")) else { return }
        settings.customStyles[i].name = name
    }

    func deleteStyle(_ id: String) {
        settings.customStyles.removeAll { $0.id == id }
        if settings.colorStyleID == id { settings.colorStyleID = ColorStyle.builtIns[0].id }
    }

    /// Импорт стилей из .json (один стиль или массив).
    func importStyles() {
        let p = NSOpenPanel()
        p.allowsMultipleSelection = true
        p.canChooseDirectories = false
        p.allowedContentTypes = [.json]
        p.message = L("Choose LiveWall color style files (.json).")
        guard p.runModal() == .OK else { return }
        var added: [ColorStyle] = []
        for url in p.urls {
            guard let data = try? Data(contentsOf: url) else { continue }
            var styles: [ColorStyle] = []
            if let many = try? JSONDecoder().decode([ColorStyle].self, from: data) {
                styles = many
            } else if let one = try? JSONDecoder().decode(ColorStyle.self, from: data) {
                styles = [one]
            }
            for var s in styles {
                // новый id, чтобы не пересечься со встроенными и уже добавленными
                s.id = UUID().uuidString
                added.append(s)
            }
        }
        guard !added.isEmpty else {
            message = L("No color styles found in the selected file(s).")
            return
        }
        settings.customStyles.append(contentsOf: added)
        settings.colorStyleID = added[0].id
    }

    func exportStyle(_ id: String) {
        guard let style = allStyles.first(where: { $0.id == id }) else { return }
        let p = NSSavePanel()
        p.allowedContentTypes = [.json]
        p.nameFieldStringValue = "\(style.name).livewall-style.json"
        guard p.runModal() == .OK, let url = p.url else { return }
        let enc = JSONEncoder()
        enc.outputFormatting = [.prettyPrinted, .sortedKeys]
        do {
            try enc.encode(style).write(to: url, options: .atomic)
        } catch {
            message = LF("Could not export the style: %@", error.localizedDescription)
        }
    }

    func openIconPicker(_ folderID: UUID) {
        if requestUnlock(folderID) { iconPickerFolderID = folderID }
    }

    func setFolderSymbol(_ name: String, for id: UUID) {
        guard let i = folders.firstIndex(where: { $0.id == id }) else { return }
        var palette = 0
        switch folders[i].icon {
        case .symbol(_, let p): palette = p
        case .image(let file): removeIconFile(file)
        }
        folders[i].icon = .symbol(name: name, palette: palette)
        save()
    }

    func setFolderPalette(_ palette: Int, for id: UUID) {
        guard let i = folders.firstIndex(where: { $0.id == id }) else { return }
        var name = "folder.fill"
        switch folders[i].icon {
        case .symbol(let n, _): name = n
        case .image(let file): removeIconFile(file)
        }
        folders[i].icon = .symbol(name: name, palette: palette)
        save()
    }

    func chooseFolderImage(for id: UUID) {
        guard let i = folders.firstIndex(where: { $0.id == id }) else { return }
        let p = NSOpenPanel()
        p.allowsMultipleSelection = false
        p.canChooseDirectories = false
        p.allowedContentTypes = [.image]
        p.message = L("Choose an image for the folder icon.")
        guard p.runModal() == .OK, let src = p.url else { return }
        guard let image = NSImage(contentsOf: src) else {
            message = LF("Could not read %@.", src.lastPathComponent)
            return
        }
        // уменьшаем до 512 px и сохраняем как PNG
        let side: CGFloat = 512
        let scaled = NSImage(size: NSSize(width: side, height: side), flipped: false) { rect in
            let s = image.size
            guard s.width > 0, s.height > 0 else { return true }
            let k = max(rect.width / s.width, rect.height / s.height)
            let w = s.width * k, h = s.height * k
            image.draw(in: NSRect(x: (rect.width - w) / 2, y: (rect.height - h) / 2, width: w, height: h))
            return true
        }
        guard let tiff = scaled.tiffRepresentation,
              let rep = NSBitmapImageRep(data: tiff),
              let png = rep.representation(using: .png, properties: [:]) else { return }
        let file = "\(id.uuidString)-\(UUID().uuidString.prefix(8)).png"
        do {
            try png.write(to: iconsDir.appendingPathComponent(file), options: .atomic)
        } catch {
            message = LF("Could not save the icon: %@", error.localizedDescription)
            return
        }
        if case .image(let old) = folders[i].icon { removeIconFile(old) }
        folders[i].icon = .image(file: file)
        save()
    }

    func folderImage(file: String) -> NSImage? {
        if let img = folderImageCache[file] { return img }
        guard let img = NSImage(contentsOf: iconsDir.appendingPathComponent(file)) else { return nil }
        folderImageCache[file] = img
        return img
    }

    private func removeIconFile(_ file: String) {
        folderImageCache[file] = nil
        try? FileManager.default.removeItem(at: iconsDir.appendingPathComponent(file))
    }

    // MARK: Переименование

    func renameItem(_ id: UUID) {
        guard let i = items.firstIndex(where: { $0.id == id }) else { return }
        guard let name = Library.promptForName(title: L("Rename Wallpaper"), message: nil,
                                               initial: items[i].name, confirm: L("Rename")) else { return }
        items[i].name = name
        save()
    }

    static func promptForName(title: String, message: String?, initial: String, confirm: String) -> String? {
        let alert = NSAlert()
        alert.messageText = title
        if let message = message { alert.informativeText = message }
        alert.addButton(withTitle: confirm)
        alert.addButton(withTitle: L("Cancel"))
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 300, height: 24))
        field.stringValue = initial
        alert.accessoryView = field
        alert.window.initialFirstResponder = field
        NSApp.activate(ignoringOtherApps: true)
        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        let value = field.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        return value.isEmpty ? nil : value
    }

    // MARK: Миниатюры

    func loadThumbnail(for item: WallpaperItem) async {
        if thumbs[item.id] != nil { return }
        // не больше трёх миниатюр одновременно — без рывков при первом открытии большой библиотеки
        await ThumbnailGate.shared.enter()
        let img = await thumbnail(for: item)
        await ThumbnailGate.shared.leave()
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

/// Ограничивает число одновременно создаваемых миниатюр.
actor ThumbnailGate {
    static let shared = ThumbnailGate()
    private let limit = 3
    private var running = 0
    private var waiters: [CheckedContinuation<Void, Never>] = []

    func enter() async {
        if running < limit {
            running += 1
            return
        }
        await withCheckedContinuation { waiters.append($0) }
    }

    func leave() {
        if waiters.isEmpty {
            running = max(0, running - 1)
        } else {
            // место переходит следующему ожидающему
            waiters.removeFirst().resume()
        }
    }
}
