import AppKit
import SwiftUI

final class AppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate {
    private var statusItem: NSStatusItem?
    private var libraryWindow: NSWindow?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)

        let lib = Library.shared
        lib.start()
        setupStatusItem()

        if lib.items.isEmpty {
            showLibrary()
        }
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        showLibrary()
        return true
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
            w.collectionBehavior.insert(.fullScreenPrimary)
            w.contentView = NSHostingView(rootView: LibraryView())
            // библиотека открывается на весь экран (видимую область без Dock и строки меню)
            if let screen = NSScreen.main ?? NSScreen.screens.first {
                w.setFrame(screen.visibleFrame, display: true)
            } else {
                w.center()
            }
            libraryWindow = w
        }
        NSApp.activate(ignoringOtherApps: true)
        libraryWindow?.makeKeyAndOrderFront(nil)
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

        menu.addItem(menuItem("Next Wallpaper", #selector(nextWallpaper), symbol: "forward.fill"))
        menu.addItem(menuItem("Previous Wallpaper", #selector(previousWallpaper), symbol: "backward.fill"))
        menu.addItem(menuItem("Random Wallpaper", #selector(randomWallpaper), symbol: "shuffle"))
        menu.addItem(menuItem(lib.paused ? "Resume Video" : "Pause Video", #selector(togglePause),
                              symbol: lib.paused ? "play.fill" : "pause.fill"))

        if !lib.items.isEmpty {
            menu.addItem(.separator())
            let title = NSMenuItem(title: "Library", action: nil, keyEquivalent: "")
            title.isEnabled = false
            menu.addItem(title)
            for it in lib.items.prefix(30) {
                let m = menuItem(it.name, #selector(pickWallpaper(_:)), state: it.id == lib.currentID ? .on : .off)
                m.representedObject = it.id
                if let thumb = lib.thumbs[it.id] {
                    m.image = AppDelegate.roundedThumb(thumb, size: NSSize(width: 40, height: 25))
                } else {
                    m.image = AppDelegate.symbol(it.kind.symbol)
                    Task { await lib.loadThumbnail(for: it) }
                }
                menu.addItem(m)
            }
        }

        menu.addItem(.separator())

        let modeParent = NSMenuItem(title: "Fill Mode", action: nil, keyEquivalent: "")
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

        let isMuted = lib.current.map { lib.effectiveMuted($0) } ?? lib.settings.muted
        menu.addItem(menuItem("Mute Video", #selector(toggleMute), state: isMuted ? .on : .off,
                              symbol: isMuted ? "speaker.slash.fill" : "speaker.wave.2.fill"))
        menu.addItem(.separator())
        menu.addItem(menuItem("Open LiveWall…", #selector(openLibrary), symbol: "macwindow"))
        menu.addItem(menuItem("Settings…", #selector(openSettings), symbol: "gearshape"))
        menu.addItem(.separator())
        menu.addItem(menuItem("Quit LiveWall", #selector(quit), symbol: "power"))
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
                } else {
                    Image(systemName: "photo.on.rectangle.angled")
                        .foregroundStyle(.white)
                }
            }
            .frame(width: 80, height: 50)
            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))

            VStack(alignment: .leading, spacing: 3) {
                Text(library.current?.name ?? "No wallpaper")
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

    private var statusText: String {
        guard let item = library.current else { return "Add wallpapers in LiveWall" }
        if library.paused { return "Paused by you" }
        if let r = library.pauseReason { return "Paused · \(r)" }
        return item.kind == .video ? "Playing · \(item.mode.title)" : "\(item.kind.title) · \(item.mode.title)"
    }

    private var statusColor: Color {
        if library.current == nil { return .secondary }
        if library.paused || library.pauseReason != nil { return .orange }
        return .green
    }
}
