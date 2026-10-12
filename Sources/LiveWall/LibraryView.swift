import SwiftUI
import AppKit

struct LibraryView: View {
    @ObservedObject var library = Library.shared

    var body: some View {
        TabView {
            WallpapersTab(library: library)
                .tabItem { Label("Wallpapers", systemImage: "photo.on.rectangle") }
            SettingsTab(library: library)
                .tabItem { Label("Settings", systemImage: "gearshape") }
        }
        .frame(minWidth: 780, minHeight: 540)
    }
}

// MARK: - Wallpapers

struct WallpapersTab: View {
    @ObservedObject var library: Library

    var body: some View {
        VStack(spacing: 0) {
            toolbar.padding(12)
            Divider()

            if let msg = library.message {
                Text(msg)
                    .font(.callout)
                    .foregroundStyle(.orange)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 14)
                    .padding(.top, 8)
            }

            if library.items.isEmpty {
                VStack(spacing: 10) {
                    Image(systemName: "arrow.down.doc")
                        .font(.system(size: 42))
                        .foregroundStyle(.secondary)
                    Text("Drop videos, images or GIFs here")
                        .font(.title3)
                    Text("mp4 / mov / m4v · png / jpg / heic · gif")
                        .foregroundStyle(.secondary)
                    Button("Add…") { library.presentImportPanel() }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 200), spacing: 16)], spacing: 16) {
                        ForEach(library.items) { item in
                            WallpaperCard(item: item, library: library)
                        }
                    }
                    .padding(16)
                }
            }
        }
        .overlay {
            if library.dropTargeted {
                RoundedRectangle(cornerRadius: 12)
                    .strokeBorder(Color.accentColor, lineWidth: 3)
                    .padding(6)
                    .allowsHitTesting(false)
            }
        }
        .onDrop(of: [.fileURL], isTargeted: $library.dropTargeted) { providers in
            library.handleDrop(providers)
            return true
        }
    }

    private var toolbar: some View {
        HStack(spacing: 10) {
            Button { library.presentImportPanel() } label: {
                Label("Add…", systemImage: "plus")
            }

            if let reason = library.pauseReason {
                Label("Paused: \(reason)", systemImage: "pause.circle")
                    .foregroundStyle(.orange)
                    .lineLimit(1)
            }

            if library.busyCount > 0 {
                ProgressView().controlSize(.small)
                Text("Importing \(library.busyCount)…").foregroundStyle(.secondary)
            }

            Spacer()

            Button { library.step(-1) } label: { Image(systemName: "backward.fill") }
                .help("Previous")
            Button { library.togglePause() } label: {
                Image(systemName: library.paused ? "play.fill" : "pause.fill")
            }
            .help(library.paused ? "Resume video" : "Pause video")
            Button { library.step(1) } label: { Image(systemName: "forward.fill") }
                .help("Next")
            Button { library.random() } label: { Image(systemName: "shuffle") }
                .help("Random")
        }
    }
}

struct WallpaperCard: View {
    let item: WallpaperItem
    @ObservedObject var library: Library

    private var isCurrent: Bool { library.currentID == item.id }

    private var modeBinding: Binding<FillMode> {
        Binding(get: { item.mode },
                set: { library.setMode($0, for: item.id) })
    }

    private var kindIcon: String {
        switch item.kind {
        case .video: return "play.rectangle.fill"
        case .gif: return "livephoto"
        case .image: return "photo"
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ZStack {
                Rectangle().fill(Color.black.opacity(0.18))
                if let thumb = library.thumbs[item.id] {
                    Image(nsImage: thumb)
                        .resizable()
                        .aspectRatio(contentMode: .fill)
                } else {
                    ProgressView().controlSize(.small)
                }
            }
            .frame(height: 112)
            .clipped()
            .clipShape(RoundedRectangle(cornerRadius: 8))
            .overlay(
                RoundedRectangle(cornerRadius: 8)
                    .strokeBorder(isCurrent ? Color.accentColor : Color.clear, lineWidth: 3)
            )
            .overlay(alignment: .topTrailing) {
                Image(systemName: kindIcon)
                    .font(.caption)
                    .padding(5)
                    .background(.ultraThinMaterial, in: Circle())
                    .padding(6)
            }
            .contentShape(Rectangle())
            .onTapGesture { library.select(item.id) }

            HStack {
                Text(item.name)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer()
                if isCurrent {
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.accentColor)
                }
            }

            Picker("", selection: modeBinding) {
                ForEach(FillMode.allCases) { m in
                    Text(m.title).tag(m)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
        }
        .contextMenu {
            Button("Set as wallpaper") { library.select(item.id) }
            Button("Reveal in Finder") { library.revealInFinder(item) }
            Divider()
            Button("Remove", role: .destructive) { library.remove(item.id) }
        }
        .task(id: item.id) {
            await library.loadThumbnail(for: item)
        }
    }
}

// MARK: - Settings

struct SettingsTab: View {
    @ObservedObject var library: Library

    private let speeds: [Double] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0]

    var body: some View {
        Form {
            Section("Playback") {
                Toggle("Mute video", isOn: $library.settings.muted)
                HStack {
                    Text("Volume")
                    Slider(value: $library.settings.volume, in: 0...1)
                }
                .disabled(library.settings.muted)
                Picker("Speed", selection: $library.settings.speed) {
                    ForEach(speeds, id: \.self) { v in
                        Text(v == v.rounded() ? "\(Int(v))x" : "\(v)x").tag(v)
                    }
                }
            }

            Section {
                Toggle("Another app is focused", isOn: $library.settings.pauseWhenAppFocused)
                Toggle("Windows cover the desktop (maximized apps)", isOn: $library.settings.pauseWhenMaximized)
                Toggle("An app is in fullscreen", isOn: $library.settings.pauseWhenFullscreen)
                Toggle("Running on battery", isOn: $library.settings.pauseOnBattery)
                Toggle("Low Power Mode is on", isOn: $library.settings.pauseInLowPower)
                Toggle("Mac is running hot", isOn: $library.settings.pauseWhenHot)
                HStack {
                    Text("Status")
                    Spacer()
                    if let reason = library.pauseReason {
                        Label("Paused: \(reason)", systemImage: "pause.circle.fill")
                            .foregroundStyle(.orange)
                    } else if library.paused {
                        Label("Paused by you", systemImage: "pause.circle.fill")
                            .foregroundStyle(.secondary)
                    } else {
                        Label("Playing", systemImage: "play.circle.fill")
                            .foregroundStyle(.green)
                    }
                }
            } header: {
                Text("Pause video when…")
            } footer: {
                Text("Video is also always paused while the display sleeps or the screen is locked.")
                    .foregroundStyle(.secondary)
            }

            Section {
                if library.settings.appRules.isEmpty {
                    Text("No app rules. Add games or heavy apps here.")
                        .foregroundStyle(.secondary)
                }
                ForEach(library.settings.appRules) { rule in
                    AppRuleRow(rule: rule, library: library)
                }
                HStack {
                    Menu("Add running app") {
                        ForEach(Library.runningRegularApps(), id: \.processIdentifier) { app in
                            Button(app.localizedName ?? (app.bundleIdentifier ?? "App")) {
                                if let id = app.bundleIdentifier {
                                    library.addAppRule(bundleID: id, name: app.localizedName ?? id)
                                }
                            }
                        }
                    }
                    .fixedSize()
                    Button("Choose app…") { library.chooseAppForRule() }
                    Spacer()
                }
            } header: {
                Text("App rules — pause while these apps are…")
            }

            Section {
                Toggle("Set a still frame as system & lock screen wallpaper", isOn: $library.settings.syncSystemWallpaper)
            } header: {
                Text("Lock screen")
            } footer: {
                Text("macOS does not allow live video on the lock screen, so LiveWall puts a still frame of the current wallpaper there (Wallpaper Engine on Windows does the same).")
                    .foregroundStyle(.secondary)
            }

            Section("Automatic rotation") {
                Toggle("Change wallpaper automatically", isOn: $library.settings.autoRotate)
                Stepper("Every \(library.settings.rotateMinutes) min",
                        value: $library.settings.rotateMinutes, in: 1...720)
                    .disabled(!library.settings.autoRotate)
                Toggle("Random order", isOn: $library.settings.shuffle)
                    .disabled(!library.settings.autoRotate)
            }

            Section("System") {
                Toggle("Launch at login", isOn: $library.settings.launchAtLogin)
            }

            Section("Hotkeys") {
                ForEach(HotkeyAction.allCases) { action in
                    HStack {
                        Text(action.title)
                        Spacer()
                        HotkeyRecorder(action: action, library: library)
                    }
                }
                Toggle("⌃⌥⌘ 1–9 jumps to wallpaper number N", isOn: $library.settings.numberHotkeys)
                HStack {
                    Spacer()
                    Button("Reset hotkeys to defaults") {
                        library.settings.hotkeys = Settings.defaultHotkeys
                    }
                }
            }
        }
        .formStyle(.grouped)
        .padding(.vertical, 4)
    }
}

struct AppRuleRow: View {
    let rule: AppRule
    @ObservedObject var library: Library

    private var whenBinding: Binding<Bool> {
        Binding(get: { rule.frontmostOnly },
                set: { library.setRuleFrontmostOnly(rule.bundleID, $0) })
    }

    var body: some View {
        HStack {
            Text(rule.name)
                .lineLimit(1)
            Spacer()
            Picker("", selection: whenBinding) {
                Text("Running").tag(false)
                Text("In front").tag(true)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 170)
            Button {
                library.removeAppRule(rule.bundleID)
            } label: {
                Image(systemName: "minus.circle.fill")
            }
            .buttonStyle(.plain)
            .help("Remove rule")
        }
    }
}

struct HotkeyRecorder: View {
    let action: HotkeyAction
    @ObservedObject var library: Library

    private var recording: Bool { library.recordingAction == action }
    private var binding: HotkeyBinding? { library.settings.hotkeys[action.rawValue] }

    private var label: String {
        if recording { return "Press shortcut…" }
        if let b = binding { return HotkeyFormat.string(b) }
        return "Not set"
    }

    var body: some View {
        HStack(spacing: 6) {
            Button {
                if recording {
                    library.endRecording()
                } else {
                    library.beginRecording(action)
                }
            } label: {
                Text(label)
                    .frame(minWidth: 130)
            }
            if binding != nil && !recording {
                Button {
                    library.settings.hotkeys[action.rawValue] = nil
                } label: {
                    Image(systemName: "xmark.circle.fill")
                }
                .buttonStyle(.plain)
                .help("Clear")
            }
        }
    }
}
