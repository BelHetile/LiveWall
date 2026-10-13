import AppKit
import SwiftUI
import Carbon

final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate, NSWindowDelegate {
    private var statusItem: NSStatusItem?
    private var libraryWindow: NSWindow?

    func applicationDidFinishLaunching(_ notification: Notification) {
        // запускали как объект входа — тихо, без окна
        let loginLaunch = AppDelegate.launchedAsLoginItem()

        let lib = Library.shared
        NSApp.setActivationPolicy(lib.settings.showInDock ? .regular : .accessory)
        setupMainMenu()
        lib.start()
        setupStatusItem()

        if !loginLaunch || lib.items.isEmpty {
            showLibrary()
        }
    }

    private static func launchedAsLoginItem() -> Bool {
        guard let event = NSAppleEventManager.shared().currentAppleEvent,
              event.eventID == AEEventID(kAEOpenApplication),
              let prop = event.paramDescriptor(forKeyword: AEKeyword(keyAEPropData)) else { return false }
        return prop.enumCodeValue == OSType(keyAELaunchedAsLogInItem)
    }


    /// Повторный запуск из Spotlight / Finder / клик по иконке в Доке — показываем библиотеку.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        showLibrary()
        return true
    }

    /// Минимальное главное меню: без меню Edit в приложении из строки меню не работают ⌘C/⌘V в полях ввода.
    func setupMainMenu() {
        let main = NSMenu()

        let appItem = NSMenuItem()
        let appMenu = NSMenu(title: "LiveWall")
        appMenu.addItem(withTitle: L("About LiveWall"), action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        appMenu.addItem(.separator())
        let settingsItem = appMenu.addItem(withTitle: L("Settings…"), action: #selector(openSettings), keyEquivalent: ",")
        settingsItem.target = self
        let libraryItem = appMenu.addItem(withTitle: L("Library"), action: #selector(openLibrary), keyEquivalent: "l")
        libraryItem.keyEquivalentModifierMask = [.command, .shift]
        libraryItem.target = self
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: L("Hide LiveWall"), action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        let hideOthers = appMenu.addItem(withTitle: L("Hide Others"), action: #selector(NSApplication.hideOtherApplications(_:)), keyEquivalent: "h")
        hideOthers.keyEquivalentModifierMask = [.command, .option]
        appMenu.addItem(withTitle: L("Show All"), action: #selector(NSApplication.unhideAllApplications(_:)), keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: L("Quit LiveWall"), action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        appItem.submenu = appMenu
        main.addItem(appItem)

        let editItem = NSMenuItem()
        let edit = NSMenu(title: L("Edit"))
        edit.addItem(withTitle: L("Undo"), action: Selector(("undo:")), keyEquivalent: "z")
        let redo = edit.addItem(withTitle: L("Redo"), action: Selector(("redo:")), keyEquivalent: "z")
        redo.keyEquivalentModifierMask = [.command, .shift]
        edit.addItem(.separator())
        edit.addItem(withTitle: L("Cut"), action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        edit.addItem(withTitle: L("Copy"), action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: L("Paste"), action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: L("Select All"), action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        editItem.submenu = edit
        main.addItem(editItem)

        let windowItem = NSMenuItem()
        let windowMenu = NSMenu(title: L("Window"))
        windowMenu.addItem(withTitle: L("Close"), action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        windowMenu.addItem(withTitle: L("Minimize"), action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        windowMenu.addItem(withTitle: L("Zoom"), action: #selector(NSWindow.performZoom(_:)), keyEquivalent: "")
        windowItem.submenu = windowMenu
        main.addItem(windowItem)
        NSApp.windowsMenu = windowMenu

        NSApp.mainMenu = main
    }

    // MARK: Library window

    func showLibrary() {
        if libraryWindow == nil {
            let w = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1040, height: 700),
                             styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                             backing: .buffered,
                             defer: false)
            w.title = "LiveWall"
            w.titleVisibility = .hidden
            w.titlebarAppearsTransparent = true
            w.isMovableByWindowBackground = true
            w.minSize = NSSize(width: 980, height: 640)
            w.isReleasedWhenClosed = false
            w.delegate = self
            w.collectionBehavior.insert(.fullScreenPrimary)
            // библиотека открывается на весь экран (видимую область без Dock и строки меню)
            if let screen = NSScreen.main ?? NSScreen.screens.first {
                w.setFrame(screen.visibleFrame, display: true)
            } else {
                w.center()
            }
            libraryWindow = w
        }
        // интерфейс создаётся при открытии и освобождается при закрытии окна
        if let w = libraryWindow, !(w.contentView is NSHostingView<LibraryView>) {
            w.contentView = NSHostingView(rootView: LibraryView())
        }
        NSApp.activate(ignoringOtherApps: true)
        libraryWindow?.makeKeyAndOrderFront(nil)
    }

    func applicationWillTerminate(_ notification: Notification) {
        Library.shared.flushSave()
    }

    /// Свёрнутое окно не крутит превью.
    func windowDidMiniaturize(_ notification: Notification) {
        guard (notification.object as? NSWindow) === libraryWindow else { return }
        Library.shared.closeDetail()
        Library.shared.tileDisappeared()
    }

    func windowDidDeminiaturize(_ notification: Notification) {
        guard (notification.object as? NSWindow) === libraryWindow else { return }
        Library.shared.tileAppeared()
    }

    /// Окно библиотеки закрыли — закрытые папки снова запираются.
    func windowWillClose(_ notification: Notification) {
        if (notification.object as? NSWindow) === libraryWindow {
            Library.shared.lockAll()
            // освобождаем интерфейс, миниатюры и превью-плеер, пока окно закрыто
            DispatchQueue.main.async { [weak self] in
                guard let w = self?.libraryWindow, !w.isVisible else { return }
                w.contentView = NSView()
                Library.shared.releaseUICaches()
            }
        }
    }

    // MARK: Menu bar

    private func setupStatusItem() {
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        if let button = item.button {
            let img = NSImage(systemSymbolName: "photo.on.rectangle", accessibilityDescription: "LiveWall")
            img?.isTemplate = true
            button.image = img
        }
        let menu = NSMenu()
        menu.delegate = self
        item.menu = menu
        statusItem = item
    }

    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let lib = Library.shared

        // шапка: миниатюра + название + статус
        let header = NSMenuItem()
        let hosting = NSHostingView(rootView: MenuHeaderView(library: lib))
        hosting.frame = NSRect(x: 0, y: 0, width: 290, height: 70)
        header.view = hosting
        menu.addItem(header)
        menu.addItem(.separator())

        menu.addItem(menuItem(L("Next Wallpaper"), #selector(nextWallpaper), symbol: "forward.fill"))
        menu.addItem(menuItem(L("Previous Wallpaper"), #selector(previousWallpaper), symbol: "backward.fill"))
        menu.addItem(menuItem(L("Random Wallpaper"), #selector(randomWallpaper), symbol: "shuffle"))
        // пауза и звук — только у анимированных обоев
        if let kind = lib.current?.kind, kind != .image {
            menu.addItem(menuItem(lib.paused ? L("Resume Video") : L("Pause Video"), #selector(togglePause),
                                  symbol: lib.paused ? "play.fill" : "pause.fill"))
        }

        if !lib.items.isEmpty {
            menu.addItem(.separator())
            let title = NSMenuItem(title: L("Library"), action: nil, keyEquivalent: "")
            title.isEnabled = false
            menu.addItem(title)
            // папки — подменю
            for folder in lib.folders {
                let fi = NSMenuItem(title: folder.name, action: nil, keyEquivalent: "")
                if lib.isLocked(folder.id) {
                    // закрытая папка: содержимое не показываем
                    fi.image = AppDelegate.symbol("lock.fill")
                    fi.isEnabled = false
                    menu.addItem(fi)
                    continue
                }
                let sub = NSMenu()
                for it in lib.items where it.folderID == folder.id {
                    sub.addItem(wallpaperMenuItem(it, lib: lib))
                }
                if case .symbol(let name, _) = folder.icon {
                    fi.image = AppDelegate.symbol(folder.isProtected ? "lock.open.fill" : name)
                } else {
                    fi.image = AppDelegate.symbol(folder.isProtected ? "lock.open.fill" : "folder.fill")
                }
                fi.submenu = sub
                fi.isEnabled = !sub.items.isEmpty
                menu.addItem(fi)
            }
            for it in lib.items.filter({ $0.folderID == nil }).prefix(30) {
                menu.addItem(wallpaperMenuItem(it, lib: lib))
            }
        }

        menu.addItem(.separator())

        let modeParent = NSMenuItem(title: L("Fill Mode"), action: nil, keyEquivalent: "")
        modeParent.image = AppDelegate.symbol("rectangle.expand.vertical")
        let modeMenu = NSMenu()
        for m in FillMode.allCases {
            let mi = menuItem(m.title, #selector(pickMode(_:)), state: lib.current?.mode == m ? .on : .off, symbol: m.symbol)
            mi.representedObject = m.rawValue
            modeMenu.addItem(mi)
        }
        modeParent.submenu = modeMenu
        modeParent.isEnabled = lib.current != nil
        menu.addItem(modeParent)

        if let current = lib.current, current.kind == .video {
            let isMuted = lib.effectiveMuted(current)
            menu.addItem(menuItem(L("Mute Video"), #selector(toggleMute), state: isMuted ? .on : .off,
                                  symbol: isMuted ? "speaker.slash.fill" : "speaker.wave.2.fill"))
        }
        menu.addItem(.separator())
        menu.addItem(menuItem(L("Open LiveWall…"), #selector(openLibrary), symbol: "macwindow"))
        menu.addItem(menuItem(L("Settings…"), #selector(openSettings), symbol: "gearshape"))
        menu.addItem(.separator())
        menu.addItem(menuItem(L("Quit LiveWall"), #selector(quit), symbol: "power"))
    }

    private func wallpaperMenuItem(_ it: WallpaperItem, lib: Library) -> NSMenuItem {
        let m = menuItem(it.name, #selector(pickWallpaper(_:)), state: it.id == lib.currentID ? .on : .off)
        m.representedObject = it.id
        if let thumb = lib.thumbs[it.id] {
            m.image = AppDelegate.roundedThumb(thumb, size: NSSize(width: 40, height: 25))
        } else {
            m.image = AppDelegate.symbol(it.kind.symbol)
            Task { await lib.loadThumbnail(for: it) }
        }
        return m
    }

    static func symbol(_ name: String) -> NSImage? {
        let img = NSImage(systemSymbolName: name, accessibilityDescription: nil)
        img?.isTemplate = true
        return img
    }

    static func roundedThumb(_ image: NSImage, size: NSSize) -> NSImage {
        NSImage(size: size, flipped: false) { rect in
            let path = NSBezierPath(roundedRect: rect, xRadius: 4, yRadius: 4)
            path.addClip()
            // aspect-fill
            let src = image.size
            guard src.width > 0, src.height > 0 else { return true }
            let scale = max(rect.width / src.width, rect.height / src.height)
            let w = src.width * scale, h = src.height * scale
            image.draw(in: NSRect(x: (rect.width - w) / 2, y: (rect.height - h) / 2, width: w, height: h))
            return true
        }
    }

    private func menuItem(_ title: String, _ action: Selector, state: NSControl.StateValue = .off,
                          symbol: String? = nil) -> NSMenuItem {
        let m = NSMenuItem(title: title, action: action, keyEquivalent: "")
        m.target = self
        m.state = state
        if let symbol = symbol { m.image = AppDelegate.symbol(symbol) }
        return m
    }

    @objc private func nextWallpaper() { Library.shared.step(1) }
    @objc private func previousWallpaper() { Library.shared.step(-1) }
    @objc private func randomWallpaper() { Library.shared.random() }
    @objc private func togglePause() { Library.shared.togglePause() }
    @objc private func toggleMute() { Library.shared.toggleMute() }
    @objc private func openLibrary() {
        Library.shared.section = .wallpapers
        showLibrary()
    }
    @objc private func openSettings() {
        Library.shared.section = .settings
        showLibrary()
    }
    @objc private func quit() { NSApp.terminate(nil) }

    @objc private func pickWallpaper(_ sender: NSMenuItem) {
        if let id = sender.representedObject as? UUID { Library.shared.select(id) }
    }

    @objc private func pickMode(_ sender: NSMenuItem) {
        guard let raw = sender.representedObject as? String,
              let mode = FillMode(rawValue: raw),
              let cur = Library.shared.current else { return }
        Library.shared.setMode(mode, for: cur.id)
    }
}

// MARK: - Menu header

struct MenuHeaderView: View {
    @ObservedObject var library: Library

    var body: some View {
        HStack(spacing: 12) {
            ZStack {
                RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Theme.accentGradient)
                if let item = library.current, let thumb = library.thumbs[item.id] {
                    Image(nsImage: thumb)
                        .resizable()
                        .aspectRatio(contentMode: .fill)
                        .blur(radius: isPrivate ? 12 : 0)
                } else {
                    Image(systemName: "photo.on.rectangle.angled")
                        .foregroundStyle(.white)
                }
            }
            .frame(width: 80, height: 50)
            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))

            VStack(alignment: .leading, spacing: 3) {
                Text(isPrivate ? L("Private wallpaper") : (library.current?.name ?? L("No wallpaper")))
                    .font(.system(size: 13, weight: .semibold))
                    .lineLimit(1)
                HStack(spacing: 4) {
                    Circle()
                        .fill(statusColor)
                        .frame(width: 6, height: 6)
                    Text(statusText)
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .frame(width: 290, height: 70, alignment: .leading)
    }

    private var isPrivate: Bool {
        library.current.map { library.isHidden($0) } ?? false
    }

    private var statusText: String {
        guard let item = library.current else { return L("Add wallpapers in LiveWall") }
        if item.kind == .image { return "\(item.kind.title) · \(item.mode.title)" }
        if library.paused { return L("Paused by you") }
        if let r = library.pauseReason { return LF("Paused · %@", r) }
        return item.kind == .video ? LF("Playing · %@", item.mode.title) : "\(item.kind.title) · \(item.mode.title)"
    }

    private var statusColor: Color {
        guard let item = library.current else { return .secondary }
        if item.kind == .image { return .green }
        if library.paused || library.pauseReason != nil { return .orange }
        return .green
    }
}
