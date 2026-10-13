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
            Section {
                Picker(L("Language"), selection: $library.settings.language) {
                    ForEach(AppLanguage.allCases) { lang in
                        Text(lang.nativeName).tag(lang)
                    }
                }
                .pickerStyle(.segmented)
            } header: {
                Text(L("Language"))
            }

            AppearanceSection(library: library)

            Section {
                Toggle(L("Mute video"), isOn: $library.settings.muted)
                HStack {
                    Text(L("Volume"))
                    SmoothSlider(key: "default-volume", value: library.settings.volume,
                                 live: { v in
                                     if let c = library.current, c.volume == nil {
                                         WallpaperController.shared.volume = Float(v)
                                     }
                                 },
                                 commit: { library.settings.volume = $0 })
                }
                .disabled(library.settings.muted)
                Picker(L("Speed"), selection: $library.settings.speed) {
                    ForEach(speeds, id: \.self) { v in
                        Text(WallpaperDetail.speedLabel(v)).tag(v)
                    }
                }
            } header: {
                Text(L("Default playback"))
            } footer: {
                Text(L("Used by wallpapers you haven't adjusted. Click a wallpaper in the library to set its own volume and speed."))
                    .foregroundStyle(.secondary)
            }

            Section {
                Toggle(L("Another app is focused"), isOn: $library.settings.pauseWhenAppFocused)
                Toggle(L("Windows cover the desktop (maximized apps)"), isOn: $library.settings.pauseWhenMaximized)
                Toggle(L("An app is in fullscreen"), isOn: $library.settings.pauseWhenFullscreen)
                Toggle(L("Running on battery"), isOn: $library.settings.pauseOnBattery)
                Toggle(L("Low Power Mode is on"), isOn: $library.settings.pauseInLowPower)
                Toggle(L("Mac is running hot"), isOn: $library.settings.pauseWhenHot)
                HStack {
                    Text(L("Status"))
                    Spacer()
                    if let reason = library.pauseReason {
                        Label(LF("Paused: %@", reason), systemImage: "pause.circle.fill")
                            .foregroundStyle(.orange)
                    } else if library.paused {
                        Label(L("Paused by you"), systemImage: "pause.circle.fill")
                            .foregroundStyle(.secondary)
                    } else {
                        Label(L("Playing"), systemImage: "play.circle.fill")
                            .foregroundStyle(.green)
                    }
                }
            } header: {
                Text(L("Pause video when…"))
            } footer: {
                Text(L("Video is also always paused while the display sleeps or the screen is locked."))
                    .foregroundStyle(.secondary)
            }

            Section {
                if library.settings.appRules.isEmpty {
                    Text(L("No app rules. Add games or heavy apps here."))
                        .foregroundStyle(.secondary)
                }
                ForEach(library.settings.appRules) { rule in
                    AppRuleRow(rule: rule, library: library)
                }
                HStack {
                    Menu(L("Add running app")) {
                        ForEach(Library.runningRegularApps(), id: \.processIdentifier) { app in
                            Button(app.localizedName ?? (app.bundleIdentifier ?? "App")) {
                                if let id = app.bundleIdentifier {
                                    library.addAppRule(bundleID: id, name: app.localizedName ?? id)
                                }
                            }
                        }
                    }
                    .fixedSize()
                    Button(L("Choose app…")) { library.chooseAppForRule() }
                    Spacer()
                }
            } header: {
                Text(L("App rules — pause while these apps are…"))
            }

            Section {
                Picker(L("Unload paused video after"), selection: $library.settings.stopAfterSeconds) {
                    Text(L("10 seconds")).tag(10)
                    Text(L("30 seconds")).tag(30)
                    Text(L("1 minute")).tag(60)
                    Text(L("5 minutes")).tag(300)
                    Text(L("Never")).tag(0)
                }
                Toggle(L("Optimize new videos automatically"), isOn: $library.settings.optimizeOnImport)
                Picker(L("Frame rate limit"), selection: $library.settings.optimizeFpsLimit) {
                    Text("24 fps").tag(24)
                    Text("30 fps").tag(30)
                    Text("60 fps").tag(60)
                }
                HStack {
                    if library.optimizing.isEmpty {
                        Text(LF("Videos not optimized yet: %ld", library.pendingOptimizationCount))
                            .foregroundStyle(.secondary)
                    } else {
                        ProgressView().controlSize(.small)
                        Text(LF("Optimizing videos: %ld left", library.optimizing.count))
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button(L("Optimize All Videos")) { library.optimizeAll() }
                        .disabled(!library.optimizing.isEmpty || library.pendingOptimizationCount == 0)
                }
            } header: {
                Text(L("Performance"))
            } footer: {
                Text(L("A paused video still holds its decoded frames in memory. After the delay LiveWall unloads it and shows the exact paused frame, then loads it back when playback resumes. Optimizing re-encodes a video to HEVC at your screen size with the frame rate limit, which cuts decoding work; videos that are already light are left as they are."))
                    .foregroundStyle(.secondary)
            }

            Section {
                Toggle(L("Show the wallpaper on the lock screen"), isOn: $library.settings.syncSystemWallpaper)
                if let lock = library.lockItem, library.lockScreenID != nil {
                    HStack {
                        Text(LF("Separate lock screen wallpaper: %@", lock.name))
                            .lineLimit(1)
                        Spacer()
                        Button(L("Follow desktop")) { library.lockScreenFollowDesktop() }
                    }
                }
                if !library.settings.syncSystemWallpaper && library.restoreFailed {
                    HStack {
                        Text(L("LiveWall doesn't know which macOS wallpaper you had before, so the last frame is still set."))
                            .foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                        Spacer()
                        Button(L("Choose in System Settings…")) { LockScreenSync.openWallpaperSettings() }
                    }
                }
            } header: {
                Text(L("Lock screen"))
            } footer: {
                Text(L("macOS doesn't let apps show live video on the lock screen. So LiveWall sets one frame of the wallpaper as the regular macOS wallpaper: you see it on the lock screen and for a moment when switching Spaces. Pick the frame in a video's panel (by default it's the first one). Turn this off to get back the macOS wallpaper you had before."))
                    .foregroundStyle(.secondary)
            }

            Section(L("Automatic rotation")) {
                Toggle(L("Change wallpaper automatically"), isOn: $library.settings.autoRotate)
                Stepper(LF("Every %ld min", library.settings.rotateMinutes),
                        value: $library.settings.rotateMinutes, in: 1...720)
                    .disabled(!library.settings.autoRotate)
                Toggle(L("Random order"), isOn: $library.settings.shuffle)
                    .disabled(!library.settings.autoRotate)
            }

            Section {
                HStack {
                    if MediaTools.installed {
                        Label(L("yt-dlp and Deno are installed"), systemImage: "checkmark.circle.fill")
                            .foregroundStyle(.green)
                    } else {
                        Label(L("Not installed — they will be downloaded when you first add such a link"), systemImage: "arrow.down.circle")
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    if MediaTools.installed && !MediaTools.ownCopy {
                        Text(L("from Homebrew"))
                            .foregroundStyle(.secondary)
                    } else {
                        Button(MediaTools.installed ? L("Update") : L("Install")) { library.installTools() }
                            .disabled(library.toolsInstalling)
                    }
                }
            } header: {
                Text(L("YouTube and other sites"))
            } footer: {
                Text(L("Links to YouTube, Vimeo, TikTok and hundreds of other sites are downloaded with yt-dlp (it needs Deno for YouTube). If a site stops working, update the tools. Formats macOS can't play (WebM, MKV, VP9, AV1) are converted to HEVC automatically."))
                    .foregroundStyle(.secondary)
            }

            Section {
                Toggle(L("Launch at login"), isOn: $library.settings.launchAtLogin)
                Toggle(L("Open the library when LiveWall starts"), isOn: $library.settings.openLibraryAtLaunch)
                Picker(L("Library window"), selection: $library.settings.libraryWindowMode) {
                    ForEach(LibraryWindowMode.allCases) { mode in
                        Text(mode.title).tag(mode)
                    }
                }
                Toggle(L("Show in Dock"), isOn: $library.settings.showInDock)
                Toggle(L("Show in the menu bar"), isOn: $library.settings.showInMenuBar)
            } header: {
                Text(L("System"))
            } footer: {
                if !library.settings.showInDock && !library.settings.showInMenuBar {
                    Text(L("With no Dock or menu bar icon, open LiveWall again from Spotlight or Applications to show the library."))
                        .foregroundStyle(.orange)
                } else if !library.settings.showInDock {
                    Text(L("The Dock icon still appears while the library window is open, so macOS can show LiveWall's menu bar."))
                        .foregroundStyle(.secondary)
                } else {
                    Text(L("When started at login, LiveWall stays in the background either way."))
                        .foregroundStyle(.secondary)
                }
            }

            Section(L("Hotkeys")) {
                ForEach(HotkeyAction.allCases) { action in
                    HStack {
                        Text(action.title)
                        Spacer()
                        HotkeyRecorder(action: action, library: library)
                    }
                }
                if library.recordingAction != nil, let warning = library.hotkeyWarning {
                    Label(warning, systemImage: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                }
                Text(L("Hotkeys for switching to a specific wallpaper are set in that wallpaper's panel — none are assigned until you set them. They also work for wallpapers in locked folders, without the password."))
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                HStack {
                    Spacer()
                    Button(L("Reset hotkeys to defaults")) {
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
                Text(L("Running")).tag(false)
                Text(L("In front")).tag(true)
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
            .help(L("Remove rule"))
        }
    }
}

struct HotkeyRecorder: View {
    let action: HotkeyAction
    @ObservedObject var library: Library

    private var recording: Bool { library.recordingAction == action }
    private var binding: HotkeyBinding? { library.settings.hotkeys[action.rawValue] }

    private var label: String {
        if recording { return L("Press shortcut…") }
        if let b = binding { return HotkeyFormat.string(b) }
        return L("Not set")
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
                .help(L("Clear"))
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
                // wrapping layout: doesn't scroll or jump when a style is added
                FlowLayout(spacing: 12) {
                    ForEach(library.allStyles) { style in
                        StyleSwatch(style: style, selected: library.settings.colorStyleID == style.id)
                            .onTapGesture { library.selectStyle(style.id) }
                            .contextMenu {
                                Button(L("Export…")) { library.exportStyle(style.id) }
                                if !style.isBuiltIn {
                                    Button(L("Rename…")) { library.renameStyle(style.id) }
                                    Button(L("Delete"), role: .destructive) { library.deleteStyle(style.id) }
                                }
                            }
                    }
                }
                .padding(.vertical, 6)
                .frame(maxWidth: .infinity, alignment: .leading)

                ForEach(library.settings.customStyles) { style in
                    CustomStyleRow(style: style, library: library,
                                   primary: colorBinding(style.id, primary: true),
                                   secondary: colorBinding(style.id, primary: false))
                }

                HStack {
                    Button {
                        library.createStyle()
                    } label: {
                        Label(L("New Style…"), systemImage: "plus")
                    }
                    Button {
                        library.importStyles()
                    } label: {
                        Label(L("Import…"), systemImage: "square.and.arrow.down")
                    }
                    Button {
                        library.exportStyle(library.settings.colorStyleID)
                    } label: {
                        Label(L("Export Current…"), systemImage: "square.and.arrow.up")
                    }
                    Spacer()
                }
            } header: {
                Text(L("Color style"))
            } footer: {
                Text(L("Styles are shared as small .json files — import ones you got from friends, or export yours."))
                    .foregroundStyle(.secondary)
            }

            Section {
                if library.userIcons.isEmpty {
                    Text(L("No icons yet. Import PNG, JPG, HEIC or other images — they become available for every folder."))
                        .foregroundStyle(.secondary)
                } else {
                    FlowLayout(spacing: 10) {
                        ForEach(library.userIcons, id: \.self) { name in
                            UserIconThumb(name: name, size: 46, library: library)
                                .contextMenu {
                                    Button(L("Remove"), role: .destructive) { library.removeUserIcon(name) }
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
                        Label(L("Import Icons…"), systemImage: "square.and.arrow.down")
                    }
                    Spacer()
                    Text(LF("%ld icon(s) · right-click to remove", library.userIcons.count))
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                }
            } header: {
                Text(L("My icons"))
            } footer: {
                Text(L("Use them from a folder's icon picker (right-click a folder → Change Icon…)."))
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
            Text(style.isBuiltIn ? L(style.name) : style.name)
                .font(.system(size: 11, weight: selected ? .semibold : .regular))
                .lineLimit(1)
                .frame(width: 72)
        }
        .padding(4)
        .contentShape(Rectangle())
    }
}

/// A simple wrapping row layout that stays stable inside a Form.
struct FlowLayout: Layout {
    var spacing: CGFloat = 12

    /// Wrap width: the proposed one, or a sensible limit when there is none (an ideal-size query),
    /// so the layout doesn't stretch into one long row and widen the form.
    private func wrapWidth(_ proposal: ProposedViewSize) -> CGFloat {
        if let w = proposal.width, w.isFinite, w > 0 { return w }
        return 560
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let maxWidth = wrapWidth(proposal)
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
        return CGSize(width: min(widest, maxWidth), height: y + rowHeight)
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

/// A custom style row: swatch, name and two unlabeled color pickers, no wider than the form.
struct CustomStyleRow: View {
    let style: ColorStyle
    @ObservedObject var library: Library
    let primary: Binding<Color>
    let secondary: Binding<Color>

    var body: some View {
        HStack(spacing: 10) {
            RoundedRectangle(cornerRadius: 6, style: .continuous)
                .fill(Theme.gradient(for: style))
                .frame(width: 28, height: 18)
            Text(style.name)
                .lineLimit(1)
                .truncationMode(.tail)
                .layoutPriority(1)
            Spacer(minLength: 8)
            Text(L("Main"))
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
            ColorPicker("", selection: primary, supportsOpacity: false)
                .labelsHidden()
                .frame(width: 44)
            Text(L("Second"))
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
            ColorPicker("", selection: secondary, supportsOpacity: false)
                .labelsHidden()
                .frame(width: 44)
            Button {
                library.renameStyle(style.id)
            } label: {
                Image(systemName: "pencil")
            }
            .buttonStyle(.borderless)
            .help(L("Rename…"))
            Button {
                library.deleteStyle(style.id)
            } label: {
                Image(systemName: "trash")
            }
            .buttonStyle(.borderless)
            .help(L("Delete style"))
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}
