import SwiftUI
import AppKit

// MARK: - Settings

struct SettingsPage: View {
    @ObservedObject var library: Library

    var body: some View {
        SettingsTab(library: library)
            .frame(maxWidth: 820)
            .frame(maxWidth: .infinity)
    }
}

struct SettingsTab: View {
    @ObservedObject var library: Library

    private let speeds: [Double] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0]

    var body: some View {
        Form {
            AppearanceSection(library: library)

            Section {
                Toggle("Mute video", isOn: $library.settings.muted)
                HStack {
                    Text("Volume")
                    Slider(value: $library.settings.volume, in: 0...1)
                }
                .disabled(library.settings.muted)
                Picker("Speed", selection: $library.settings.speed) {
                    ForEach(speeds, id: \.self) { v in
                        Text(WallpaperDetail.speedLabel(v)).tag(v)
                    }
                }
            } header: {
                Text("Default playback")
            } footer: {
                Text("Used by wallpapers you haven't adjusted. Click a wallpaper in the library to set its own volume and speed.")
                    .foregroundStyle(.secondary)
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
                Toggle("Put a still frame of the desktop wallpaper on the lock screen", isOn: $library.settings.syncSystemWallpaper)
                if let lock = library.lockItem, library.lockScreenID != nil {
                    HStack {
                        Text("Custom lock screen: \(lock.name)")
                            .lineLimit(1)
                        Spacer()
                        Button("Follow desktop") { library.lockScreenFollowDesktop() }
                    }
                }
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
                Text("Hotkeys for switching to a specific wallpaper are set in that wallpaper's panel — none are assigned until you set them. They also work for wallpapers in locked folders, without the password.")
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                HStack {
                    Spacer()
                    Button("Reset hotkeys to defaults") {
                        library.settings.hotkeys = Settings.defaultHotkeys
                    }
                }
            }
        }
        .formStyle(.grouped)
        .scrollContentBackground(.hidden)
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

// MARK: - Appearance

struct AppearanceSection: View {
    @ObservedObject var library: Library

    var body: some View {
        Group {
            Section {
                // раскладка с переносом: не прокручивается и не прыгает при добавлении стиля
                FlowLayout(spacing: 12) {
                    ForEach(library.allStyles) { style in
                        StyleSwatch(style: style, selected: library.settings.colorStyleID == style.id)
                            .onTapGesture { library.selectStyle(style.id) }
                            .contextMenu {
                                Button("Export…") { library.exportStyle(style.id) }
                                if !style.isBuiltIn {
                                    Button("Rename…") { library.renameStyle(style.id) }
                                    Button("Delete", role: .destructive) { library.deleteStyle(style.id) }
                                }
                            }
                    }
                }
                .padding(.vertical, 6)
                .frame(maxWidth: .infinity, alignment: .leading)

                ForEach(library.settings.customStyles) { style in
                    HStack(spacing: 10) {
                        Circle()
                            .fill(Theme.gradient(for: style))
                            .frame(width: 18, height: 18)
                        Text(style.name)
                            .lineLimit(1)
                        Spacer()
                        ColorPicker("Main", selection: colorBinding(style.id, primary: true), supportsOpacity: false)
                            .fixedSize()
                        ColorPicker("Second", selection: colorBinding(style.id, primary: false), supportsOpacity: false)
                            .fixedSize()
                        Button {
                            library.deleteStyle(style.id)
                        } label: {
                            Image(systemName: "trash")
                        }
                        .buttonStyle(.borderless)
                        .help("Delete style")
                    }
                }

                HStack {
                    Button {
                        library.createStyle()
                    } label: {
                        Label("New Style…", systemImage: "plus")
                    }
                    Button {
                        library.importStyles()
                    } label: {
                        Label("Import…", systemImage: "square.and.arrow.down")
                    }
                    Button {
                        library.exportStyle(library.settings.colorStyleID)
                    } label: {
                        Label("Export Current…", systemImage: "square.and.arrow.up")
                    }
                    Spacer()
                }
            } header: {
                Text("Color style")
            } footer: {
                Text("Styles are shared as small .json files — import ones you got from friends, or export yours.")
                    .foregroundStyle(.secondary)
            }

            Section {
                if library.userIcons.isEmpty {
                    Text("No icons yet. Import PNG, JPG, HEIC or other images — they become available for every folder.")
                        .foregroundStyle(.secondary)
                } else {
                    FlowLayout(spacing: 10) {
                        ForEach(library.userIcons, id: \.self) { name in
                            UserIconThumb(name: name, size: 46, library: library)
                                .contextMenu {
                                    Button("Remove", role: .destructive) { library.removeUserIcon(name) }
                                }
                        }
                    }
                    .padding(.vertical, 4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                HStack {
                    Button {
                        library.importUserIcons()
                    } label: {
                        Label("Import Icons…", systemImage: "square.and.arrow.down")
                    }
                    Spacer()
                    Text("\(library.userIcons.count) icon(s) · right-click to remove")
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                }
            } header: {
                Text("My icons")
            } footer: {
                Text("Use them from a folder's icon picker (right-click a folder → Change Icon…).")
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func colorBinding(_ id: String, primary: Bool) -> Binding<Color> {
        Binding(get: {
            let style = library.settings.customStyles.first(where: { $0.id == id })
            return Color(hex: (primary ? style?.primary : style?.secondary) ?? "#808080")
        }, set: { color in
            if primary {
                library.updateStyle(id, primary: color.hexString)
            } else {
                library.updateStyle(id, secondary: color.hexString)
            }
        })
    }
}

struct StyleSwatch: View {
    let style: ColorStyle
    let selected: Bool

    var body: some View {
        VStack(spacing: 6) {
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(Theme.gradient(for: style))
                .frame(width: 64, height: 42)
                .overlay {
                    RoundedRectangle(cornerRadius: 15, style: .continuous)
                        .strokeBorder(Color.primary, lineWidth: selected ? 2.5 : 0)
                        .padding(-4)
                }
                .overlay(alignment: .bottomTrailing) {
                    if selected {
                        Image(systemName: "checkmark.circle.fill")
                            .font(.system(size: 14, weight: .bold))
                            .foregroundStyle(.white)
                            .padding(4)
                    }
                }
            Text(style.name)
                .font(.system(size: 11, weight: selected ? .semibold : .regular))
                .lineLimit(1)
                .frame(width: 72)
        }
        .padding(4)
        .contentShape(Rectangle())
    }
}

/// Простая раскладка «в строку с переносом» — стабильна внутри Form.
struct FlowLayout: Layout {
    var spacing: CGFloat = 12

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let maxWidth = proposal.width ?? 640
        var x: CGFloat = 0
        var y: CGFloat = 0
        var rowHeight: CGFloat = 0
        var widest: CGFloat = 0
        for view in subviews {
            let size = view.sizeThatFits(.unspecified)
            if x > 0 && x + size.width > maxWidth {
                x = 0
                y += rowHeight + spacing
                rowHeight = 0
            }
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
            widest = max(widest, x - spacing)
        }
        return CGSize(width: proposal.width ?? widest, height: y + rowHeight)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX
        var y = bounds.minY
        var rowHeight: CGFloat = 0
        for view in subviews {
            let size = view.sizeThatFits(.unspecified)
            if x > bounds.minX && x + size.width > bounds.maxX {
                x = bounds.minX
                y += rowHeight + spacing
                rowHeight = 0
            }
            view.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
        }
    }
}
