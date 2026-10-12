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
            w.minSize = NSSize(width: 900, height: 600)
            w.isReleasedWhenClosed = false
            w.contentView = NSHostingView(rootView: LibraryView())
            w.center()
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

        let header = NSMenuItem(title: lib.current?.name ?? "No wallpaper", action: nil, keyEquivalent: "")
        header.isEnabled = false
        menu.addItem(header)
        if let reason = lib.pauseReason {
            let r = NSMenuItem(title: "Paused: \(reason)", action: nil, keyEquivalent: "")
            r.isEnabled = false
            menu.addItem(r)
        }
        menu.addItem(.separator())

        menu.addItem(menuItem("Next", #selector(nextWallpaper)))
        menu.addItem(menuItem("Previous", #selector(previousWallpaper)))
        menu.addItem(menuItem("Random", #selector(randomWallpaper)))
        menu.addItem(menuItem(lib.paused ? "Resume video" : "Pause video", #selector(togglePause)))

        if !lib.items.isEmpty {
            menu.addItem(.separator())
            for it in lib.items.prefix(30) {
                let m = menuItem(it.name, #selector(pickWallpaper(_:)), state: it.id == lib.currentID ? .on : .off)
                m.representedObject = it.id
                menu.addItem(m)
            }
        }

        menu.addItem(.separator())

        let modeParent = NSMenuItem(title: "Fill mode", action: nil, keyEquivalent: "")
        let modeMenu = NSMenu()
        for m in FillMode.allCases {
            let mi = menuItem(m.title, #selector(pickMode(_:)), state: lib.current?.mode == m ? .on : .off)
            mi.representedObject = m.rawValue
            modeMenu.addItem(mi)
        }
        modeParent.submenu = modeMenu
        modeParent.isEnabled = lib.current != nil
        menu.addItem(modeParent)

        menu.addItem(menuItem("Mute video", #selector(toggleMute), state: lib.settings.muted ? .on : .off))
        menu.addItem(.separator())
        menu.addItem(menuItem("Open Library…", #selector(openLibrary)))
        menu.addItem(menuItem("Quit LiveWall", #selector(quit)))
    }

    private func menuItem(_ title: String, _ action: Selector, state: NSControl.StateValue = .off) -> NSMenuItem {
        let m = NSMenuItem(title: title, action: action, keyEquivalent: "")
        m.target = self
        m.state = state
        return m
    }

    @objc private func nextWallpaper() { Library.shared.step(1) }
    @objc private func previousWallpaper() { Library.shared.step(-1) }
    @objc private func randomWallpaper() { Library.shared.random() }
    @objc private func togglePause() { Library.shared.togglePause() }
    @objc private func toggleMute() { Library.shared.toggleMute() }
    @objc private func openLibrary() { showLibrary() }
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
