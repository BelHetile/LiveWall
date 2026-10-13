import SwiftUI
import AppKit
import AVFoundation

enum LibrarySection: String, CaseIterable, Identifiable {
    case wallpapers, settings

    var id: String { rawValue }
}

// MARK: - Root

struct LibraryView: View {
    @ObservedObject var library = Library.shared

    var body: some View {
        ZStack {
            LibraryBackground()

            VStack(spacing: 0) {
                TopBar(library: library)
                Group {
                    switch library.section {
                    case .wallpapers:
                        WallpapersPage(library: library)
                    case .settings:
                        SettingsPage(library: library)
                    }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            }

            if let id = library.detailID, let item = library.items.first(where: { $0.id == id }) {
                WallpaperDetail(item: item, library: library)
                    .transition(.opacity)
            }

            // ⌘V в окне: добавить ссылку или файлы из буфера обмена
            Button("Paste") { library.pasteFromClipboard() }
                .keyboardShortcut("v", modifiers: .command)
                .opacity(0)
                .frame(width: 0, height: 0)
        }
        .animation(.easeInOut(duration: 0.2), value: library.detailID)
        .animation(.easeInOut(duration: 0.2), value: library.section)
        .frame(minWidth: 980, minHeight: 640)
        .ignoresSafeArea(.container, edges: .top)
    }
}

struct LibraryBackground: View {
    var body: some View {
        ZStack {
            VisualEffectBackground(material: .underWindowBackground)
            LinearGradient(colors: [Theme.violet.opacity(0.16), .clear],
                           startPoint: .topLeading, endPoint: .center)
            RadialGradient(colors: [Theme.magenta.opacity(0.12), .clear],
                           center: .topTrailing, startRadius: 0, endRadius: 700)
        }
        .ignoresSafeArea()
    }
}

// MARK: - Top bar

struct TopBar: View {
    @ObservedObject var library: Library

    private var inSettings: Bool { library.section == .settings }

    var body: some View {
        HStack(spacing: 12) {
            if inSettings {
                Button {
                    library.section = .wallpapers
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "chevron.left")
                        Text("Library")
                    }
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(Theme.violet)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                Text("Settings")
                    .font(.system(size: 17, weight: .bold))
                    .padding(.leading, 6)
            } else {
                Image(nsImage: NSApp.applicationIconImage)
                    .resizable()
                    .frame(width: 26, height: 26)
                Text("LiveWall")
                    .font(.system(size: 17, weight: .bold))
                Text("\(library.items.count)")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 7)
                    .padding(.vertical, 2)
                    .background(.quaternary, in: Capsule())
            }

            Spacer()

            StatusPill(library: library)

            if !inSettings {
                Button {
                    library.presentImportPanel()
                } label: {
                    Label("Add", systemImage: "plus")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 7)
                        .background(Theme.accentGradient, in: Capsule())
                }
                .buttonStyle(.plain)
                .help("Add wallpapers")

                Button {
                    library.showLinkPopover = true
                } label: {
                    Image(systemName: "link.badge.plus")
                        .font(.system(size: 14, weight: .semibold))
                        .frame(width: 32, height: 32)
                        .background(Color.primary.opacity(0.08), in: Circle())
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .help("Add from a link (or press ⌘V with a link copied)")
                .popover(isPresented: $library.showLinkPopover, arrowEdge: .bottom) {
                    LinkPopover(library: library)
                }
            }

            Button {
                library.section = inSettings ? .wallpapers : .settings
            } label: {
                Image(systemName: "gearshape.fill")
                    .font(.system(size: 15, weight: .semibold))
                    .frame(width: 32, height: 32)
                    .foregroundStyle(inSettings ? Color.white : Color.primary)
                    .background {
                        Circle().fill(inSettings ? AnyShapeStyle(Theme.accentGradient)
                                                 : AnyShapeStyle(Color.primary.opacity(0.08)))
                    }
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .help("Settings")
        }
        .padding(.leading, 86)   // место под кнопки окна
        .padding(.trailing, 20)
        .frame(height: 52)
    }
}

struct StatusPill: View {
    @ObservedObject var library: Library

    private var text: String {
        guard let item = library.current else { return "No wallpaper" }
        if library.paused { return "Paused by you" }
        if let r = library.pauseReason { return "Paused · \(r)" }
        return item.kind == .video ? "Playing" : "Showing"
    }

    private var color: Color {
        if library.current == nil { return .secondary }
        if library.paused || library.pauseReason != nil { return .orange }
        return .green
    }

    var body: some View {
        HStack(spacing: 6) {
            Circle().fill(color).frame(width: 7, height: 7)
            Text(text)
                .font(.system(size: 12, weight: .medium))
                .lineLimit(1)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(Color.primary.opacity(0.06), in: Capsule())
    }
}

// MARK: - Wallpapers page

struct WallpapersPage: View {
    @ObservedObject var library: Library

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 26) {
                if !library.linkDownloads.isEmpty {
                    DownloadsList(library: library)
                }

                if library.items.isEmpty {
                    EmptyDropZone(library: library)
                        .padding(.top, 40)
                } else {
                    NowSection(library: library)

                    if let msg = library.message {
                        MessageBanner(text: msg) { library.message = nil }
                    }

                    header

                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 240, maximum: 380), spacing: 20)],
                              spacing: 22) {
                        ForEach(Array(library.items.enumerated()), id: \.element.id) { index, item in
                            WallpaperCard(item: item, index: index, library: library)
                        }
                    }
                }
            }
            .padding(.horizontal, 28)
            .padding(.top, 6)
            .padding(.bottom, 28)
        }
        .overlay { DropOverlay(active: library.dropTargeted) }
        .onDrop(of: [.fileURL, .url], isTargeted: $library.dropTargeted) { providers in
            library.handleDrop(providers)
            return true
        }
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text("Library")
                .font(.system(size: 22, weight: .bold))
            if library.busyCount > 0 {
                ProgressView().controlSize(.small).padding(.leading, 6)
                Text("Importing \(library.busyCount)…")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Text("Click a wallpaper to adjust it")
                .font(.callout)
                .foregroundStyle(.secondary)
        }
    }
}

struct MessageBanner: View {
    let text: String
    let onClose: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "exclamationmark.triangle.fill")
            Text(text)
            Spacer()
            Button(action: onClose) {
                Image(systemName: "xmark")
            }
            .buttonStyle(.plain)
        }
        .font(.callout)
        .foregroundStyle(.orange)
        .padding(10)
        .background(Color.orange.opacity(0.12), in: RoundedRectangle(cornerRadius: 10))
    }
}

// MARK: - Now: desktop + lock screen

struct NowSection: View {
    @ObservedObject var library: Library

    var body: some View {
        HStack(spacing: 18) {
            DesktopTile(library: library)
                .frame(maxWidth: .infinity)
            LockTile(library: library)
                .frame(width: 420)
        }
        .frame(height: 300)
    }
}

struct DesktopTile: View {
    @ObservedObject var library: Library

    var body: some View {
        ZStack(alignment: .bottomLeading) {
            ZStack {
                Theme.accentGradient
                if let item = library.current {
                    if let thumb = library.thumbs[item.id] {
                        Image(nsImage: thumb)
                            .resizable()
                            .aspectRatio(contentMode: .fill)
                    }
                    if item.kind == .video, let player = WallpaperController.shared.currentPlayer {
                        PlayerPreview(player: player)
                    }
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .clipped()

            Theme.heroScrim

            VStack(alignment: .leading, spacing: 8) {
                Chip(text: "DESKTOP", systemImage: "display")
                Text(library.current?.name ?? "No wallpaper")
                    .font(.system(size: 26, weight: .heavy))
                    .foregroundStyle(.white)
                    .lineLimit(1)
                    .shadow(color: .black.opacity(0.4), radius: 6, y: 2)
                HStack(spacing: 12) {
                    CircleButton(systemImage: "backward.fill") { library.step(-1) }
                        .help("Previous")
                    CircleButton(systemImage: library.paused ? "play.fill" : "pause.fill", size: 42, prominent: true) {
                        library.togglePause()
                    }
                    .help(library.paused ? "Resume" : "Pause")
                    CircleButton(systemImage: "forward.fill") { library.step(1) }
                        .help("Next")
                    CircleButton(systemImage: "shuffle") { library.random() }
                        .help("Random")
                    if let item = library.current {
                        CircleButton(systemImage: "slider.horizontal.3") { library.openDetail(item.id) }
                            .help("Adjust")
                    }
                }
            }
            .padding(20)
        }
        .clipShape(RoundedRectangle(cornerRadius: Theme.heroRadius, style: .continuous))
        .shadow(color: Theme.violet.opacity(0.25), radius: 18, y: 8)
        .contentShape(RoundedRectangle(cornerRadius: Theme.heroRadius, style: .continuous))
        .onTapGesture {
            if let item = library.current { library.openDetail(item.id) }
        }
        .task(id: library.currentID) {
            if let item = library.current { await library.loadThumbnail(for: item) }
        }
    }
}

struct LockTile: View {
    @ObservedObject var library: Library

    var body: some View {
        let item = library.lockItem
        ZStack {
            ZStack {
                LinearGradient(colors: [Color(white: 0.15), Color(white: 0.05)],
                               startPoint: .top, endPoint: .bottom)
                if let item = item, let thumb = library.thumbs[item.id] {
                    Image(nsImage: thumb)
                        .resizable()
                        .aspectRatio(contentMode: .fill)
                }
                Color.black.opacity(0.18)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .clipped()

            // часы как на настоящем экране блокировки
            TimelineView(.periodic(from: Date(), by: 30)) { context in
                VStack(spacing: 2) {
                    Text(context.date, format: .dateTime.weekday(.wide).day().month(.wide))
                        .font(.system(size: 13, weight: .semibold))
                    Text(context.date, format: .dateTime.hour().minute())
                        .font(.system(size: 54, weight: .bold, design: .rounded))
                }
                .foregroundStyle(.white.opacity(0.92))
                .shadow(color: .black.opacity(0.35), radius: 8, y: 2)
            }
            .frame(maxHeight: .infinity, alignment: .top)
            .padding(.top, 26)

            VStack(alignment: .leading, spacing: 6) {
                Chip(text: "LOCK SCREEN", systemImage: "lock.fill")
                Text(item?.name ?? "Not set")
                    .font(.system(size: 15, weight: .bold))
                    .foregroundStyle(.white)
                    .lineLimit(1)
                Text(subtitle)
                    .font(.system(size: 11))
                    .foregroundStyle(.white.opacity(0.75))
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading)
            .padding(18)
        }
        .clipShape(RoundedRectangle(cornerRadius: Theme.heroRadius, style: .continuous))
        .shadow(color: .black.opacity(0.25), radius: 14, y: 6)
        .contentShape(RoundedRectangle(cornerRadius: Theme.heroRadius, style: .continuous))
        .onTapGesture {
            if let item = item { library.openDetail(item.id) }
        }
        .task(id: item?.id) {
            if let item = item { await library.loadThumbnail(for: item) }
        }
    }

    private var subtitle: String {
        if library.lockScreenID != nil { return "Custom · still frame" }
        if library.settings.syncSystemWallpaper { return "Same as desktop · still frame" }
        return "Off — enable in Settings or pick a wallpaper"
    }
}

// MARK: - Card

struct WallpaperCard: View {
    let item: WallpaperItem
    let index: Int
    @ObservedObject var library: Library

    private var isDesktop: Bool { library.currentID == item.id }
    private var isLock: Bool { library.lockItem?.id == item.id }
    private var isHovered: Bool { library.hoveredID == item.id }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            thumbnail
            HStack(spacing: 6) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(item.name)
                        .font(.system(size: 13, weight: .semibold))
                        .lineLimit(1)
                        .truncationMode(.middle)
                    HStack(spacing: 4) {
                        Image(systemName: item.kind.symbol)
                        Text("\(item.kind.title) · \(item.mode.title)")
                    }
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
                }
                Spacer()
                if index < 9 {
                    Text("⌃⌥⌘\(index + 1)")
                        .font(.system(size: 10, weight: .medium, design: .rounded))
                        .foregroundStyle(.tertiary)
                }
            }
            .padding(.horizontal, 2)
        }
        .scaleEffect(isHovered ? 1.03 : 1.0)
        .animation(.spring(response: 0.28, dampingFraction: 0.75), value: isHovered)
        .onHover { inside in
            if inside {
                library.hoveredID = item.id
            } else if library.hoveredID == item.id {
                library.hoveredID = nil
            }
        }
        .contextMenu {
            Button("Set on Desktop") { library.setDesktop(item.id) }
            Button("Set on Lock Screen") { library.setLockScreen(item.id) }
            Button("Adjust…") { library.openDetail(item.id) }
            Menu("Fill Mode") {
                ForEach(FillMode.allCases) { m in
                    Button {
                        library.setMode(m, for: item.id)
                    } label: {
                        if m == item.mode {
                            Label(m.title, systemImage: "checkmark")
                        } else {
                            Text(m.title)
                        }
                    }
                }
            }
            Button("Reveal in Finder") { library.revealInFinder(item) }
            Divider()
            Button("Remove", role: .destructive) { library.remove(item.id) }
        }
        .task(id: item.id) {
            await library.loadThumbnail(for: item)
        }
    }

    private var thumbnail: some View {
        ZStack {
            Rectangle().fill(Theme.accentGradient.opacity(0.35))
            if let thumb = library.thumbs[item.id] {
                Image(nsImage: thumb)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
            } else {
                ProgressView().controlSize(.small)
            }

            if isHovered {
                Color.black.opacity(0.35)
                VStack(spacing: 6) {
                    Image(systemName: "slider.horizontal.3")
                        .font(.system(size: 22, weight: .semibold))
                    Text("Adjust")
                        .font(.system(size: 12, weight: .semibold))
                }
                .foregroundStyle(.white)
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                .transition(.scale.combined(with: .opacity))
            }
        }
        .aspectRatio(16.0 / 10.0, contentMode: .fit)
        .clipShape(RoundedRectangle(cornerRadius: Theme.cardRadius, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: Theme.cardRadius, style: .continuous)
                .strokeBorder(isDesktop ? AnyShapeStyle(Theme.accentGradient) : AnyShapeStyle(Color.primary.opacity(0.08)),
                              lineWidth: isDesktop ? 3 : 1)
        }
        .overlay(alignment: .topLeading) {
            HStack(spacing: 4) {
                if isDesktop { Chip(text: "DESKTOP", systemImage: "display") }
                if isLock { Chip(text: "LOCK", systemImage: "lock.fill") }
            }
            .padding(8)
        }
        .overlay(alignment: .topTrailing) {
            Image(systemName: item.kind.symbol)
                .font(.system(size: 10, weight: .bold))
                .foregroundStyle(.white)
                .padding(6)
                .background(.ultraThinMaterial, in: Circle())
                .padding(8)
        }
        .shadow(color: .black.opacity(isHovered ? 0.35 : 0.12), radius: isHovered ? 14 : 5, y: isHovered ? 8 : 2)
        .contentShape(Rectangle())
        .onTapGesture { library.openDetail(item.id) }
    }
}

// MARK: - Detail panel

struct WallpaperDetail: View {
    let item: WallpaperItem
    @ObservedObject var library: Library

    private let speeds: [Double] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0]

    private var isDesktop: Bool { library.currentID == item.id }
    private var isLock: Bool { library.lockItem?.id == item.id }

    private var volumeBinding: Binding<Double> {
        Binding(get: { library.effectiveVolume(item) },
                set: { library.setVolume($0, for: item.id) })
    }

    private var speedBinding: Binding<Double> {
        Binding(get: { library.effectiveSpeed(item) },
                set: { library.setSpeed($0, for: item.id) })
    }

    private var modeBinding: Binding<FillMode> {
        Binding(get: { item.mode },
                set: { library.setMode($0, for: item.id) })
    }

    var body: some View {
        ZStack {
            Color.black.opacity(0.5)
                .ignoresSafeArea()
                .contentShape(Rectangle())
                .onTapGesture { library.closeDetail() }

            HStack(spacing: 0) {
                previewPane
                Divider().opacity(0.4)
                controlsPane
                    .frame(width: 350)
            }
            .frame(maxWidth: 1240, maxHeight: 680)
            .background(VisualEffectBackground(material: .hudWindow, blending: .withinWindow))
            .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: 22, style: .continuous)
                    .strokeBorder(Color.white.opacity(0.08))
            }
            .shadow(color: .black.opacity(0.45), radius: 40, y: 18)
            .padding(.horizontal, 60)
            .padding(.top, 70)
            .padding(.bottom, 50)
        }
    }

    // MARK: Превью

    private var previewPane: some View {
        ZStack {
            Color.black

            if item.kind == .video {
                if let thumb = library.thumbs[item.id] {
                    Image(nsImage: thumb)
                        .resizable()
                        .aspectRatio(contentMode: .fit)
                }
                PlayerPreview(player: isDesktop ? WallpaperController.shared.currentPlayer : library.previewPlayer,
                              gravity: .resizeAspect)
            } else if let image = library.previewImage(for: item) {
                Image(nsImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
            }

            HStack {
                CircleButton(systemImage: "chevron.left", size: 36) { library.showAdjacentDetail(-1) }
                    .help("Previous wallpaper")
                Spacer()
                CircleButton(systemImage: "chevron.right", size: 36) { library.showAdjacentDetail(1) }
                    .help("Next wallpaper")
            }
            .padding(16)

            if isDesktop && item.kind == .video {
                CircleButton(systemImage: library.paused ? "play.fill" : "pause.fill", size: 54, prominent: true) {
                    library.togglePause()
                }
                .help(library.paused ? "Resume" : "Pause")
                .frame(maxHeight: .infinity, alignment: .bottom)
                .padding(.bottom, 22)
            }
        }
        .overlay(alignment: .topLeading) {
            HStack(spacing: 6) {
                if isDesktop { Chip(text: "ON DESKTOP", systemImage: "display") }
                if isLock { Chip(text: "ON LOCK SCREEN", systemImage: "lock.fill") }
                if !isDesktop && item.kind == .video { Chip(text: "PREVIEW", systemImage: "eye") }
            }
            .padding(16)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .clipped()
        .task(id: item.id) { await library.loadThumbnail(for: item) }
    }

    // MARK: Управление

    private var controlsPane: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(item.name)
                        .font(.system(size: 20, weight: .bold))
                        .lineLimit(2)
                    Label("\(item.kind.title) · \(item.mode.title)", systemImage: item.kind.symbol)
                        .font(.system(size: 12))
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button {
                    library.closeDetail()
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 12, weight: .bold))
                        .frame(width: 28, height: 28)
                        .background(Color.primary.opacity(0.08), in: Circle())
                        .contentShape(Circle())
                }
                .buttonStyle(.plain)
                .keyboardShortcut(.cancelAction)
                .help("Close")
            }

            VStack(alignment: .leading, spacing: 10) {
                SectionLabel(text: "Use as")
                HStack(spacing: 10) {
                    TargetButton(title: "Desktop", symbol: "display", active: isDesktop) {
                        library.setDesktop(item.id)
                    }
                    TargetButton(title: "Lock Screen", symbol: "lock.fill", active: isLock) {
                        library.setLockScreen(item.id)
                    }
                }
                if library.lockScreenID == item.id {
                    Button("Lock screen: follow the desktop again") {
                        library.lockScreenFollowDesktop()
                    }
                    .buttonStyle(.link)
                    .font(.system(size: 11))
                }
            }

            if item.kind == .video {
                VStack(alignment: .leading, spacing: 10) {
                    SectionLabel(text: "Playback")

                    if isDesktop {
                        Button {
                            library.togglePause()
                        } label: {
                            Label(library.paused ? "Resume" : "Pause",
                                  systemImage: library.paused ? "play.fill" : "pause.fill")
                                .font(.system(size: 13, weight: .semibold))
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 8)
                                .background(Color.primary.opacity(0.08), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        if let reason = library.pauseReason, !library.paused {
                            Label("Auto-paused: \(reason)", systemImage: "pause.circle")
                                .font(.system(size: 11))
                                .foregroundStyle(.orange)
                        }
                    }

                    HStack(spacing: 10) {
                        let muted = library.effectiveMuted(item)
                        Button {
                            library.setMuted(!muted, for: item.id)
                        } label: {
                            Image(systemName: muted ? "speaker.slash.fill" : "speaker.wave.2.fill")
                                .frame(width: 22)
                        }
                        .buttonStyle(.plain)
                        .help(muted ? "Unmute" : "Mute")
                        Slider(value: volumeBinding, in: 0...1)
                            .disabled(muted)
                        Text(muted ? "Muted" : "\(Int((library.effectiveVolume(item) * 100).rounded()))%")
                            .font(.system(size: 11, weight: .medium))
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                            .frame(width: 44, alignment: .trailing)
                    }

                    HStack {
                        Text("Speed")
                            .font(.system(size: 12))
                        Spacer()
                    }
                    Picker("", selection: speedBinding) {
                        ForEach(speeds, id: \.self) { v in
                            Text(WallpaperDetail.speedLabel(v)).tag(v)
                        }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                }
            }

            VStack(alignment: .leading, spacing: 10) {
                SectionLabel(text: "Fill mode")
                Picker("", selection: modeBinding) {
                    ForEach(FillMode.allCases) { m in
                        Text(m.title).tag(m)
                    }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
            }

            Spacer(minLength: 0)

            HStack {
                Button {
                    library.revealInFinder(item)
                } label: {
                    Label("Show in Finder", systemImage: "folder")
                }
                Spacer()
                Button(role: .destructive) {
                    library.remove(item.id)
                } label: {
                    Label("Remove", systemImage: "trash")
                }
                .foregroundStyle(.red)
            }
            .buttonStyle(.borderless)
            .font(.system(size: 12))
        }
        .padding(22)
    }

    static func speedLabel(_ v: Double) -> String {
        let s = v == v.rounded() ? "\(Int(v))" : "\(v)"
        return s + "×"
    }
}

struct SectionLabel: View {
    let text: String

    var body: some View {
        Text(text.uppercased())
            .font(.system(size: 11, weight: .bold))
            .tracking(0.6)
            .foregroundStyle(.secondary)
    }
}

struct TargetButton: View {
    let title: String
    let symbol: String
    let active: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 6) {
                Image(systemName: active ? "checkmark.circle.fill" : symbol)
                    .font(.system(size: 18, weight: .semibold))
                Text(title)
                    .font(.system(size: 12, weight: .semibold))
                Text(active ? "Active" : "Set")
                    .font(.system(size: 10))
                    .opacity(0.8)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 12)
            .foregroundStyle(active ? Color.white : Color.primary)
            .background {
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .fill(active ? AnyShapeStyle(Theme.accentGradient) : AnyShapeStyle(Color.primary.opacity(0.07)))
            }
            .contentShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        }
        .buttonStyle(.plain)
    }
}

// MARK: - Drop

struct EmptyDropZone: View {
    @ObservedObject var library: Library

    var body: some View {
        VStack(spacing: 14) {
            ZStack {
                Circle().fill(Theme.accentGradient).frame(width: 84, height: 84)
                Image(systemName: "square.and.arrow.down.fill")
                    .font(.system(size: 32, weight: .semibold))
                    .foregroundStyle(.white)
            }
            Text("Drop videos, images, GIFs or links here")
                .font(.system(size: 20, weight: .semibold))
            Text("mp4 · mov · m4v · png · jpg · heic · gif — folders work too")
                .foregroundStyle(.secondary)
            HStack(spacing: 10) {
                Button("Choose Files…") { library.presentImportPanel() }
                Button("Add from Link…") { library.showLinkPopover = true }
            }
            .controlSize(.large)
            Text("Tip: copy a link to a video or image and press ⌘V here")
                .font(.system(size: 12))
                .foregroundStyle(.tertiary)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 90)
        .background {
            RoundedRectangle(cornerRadius: 22, style: .continuous)
                .strokeBorder(style: StrokeStyle(lineWidth: 2, dash: [8, 6]))
                .foregroundStyle(Theme.violet.opacity(0.5))
        }
    }
}

struct DropOverlay: View {
    let active: Bool

    var body: some View {
        ZStack {
            if active {
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .fill(Theme.violet.opacity(0.14))
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .strokeBorder(Theme.accentGradient, lineWidth: 3)
                VStack(spacing: 8) {
                    Image(systemName: "plus.circle.fill")
                        .font(.system(size: 44))
                        .foregroundStyle(Theme.accentGradient)
                    Text("Drop to add to your library")
                        .font(.system(size: 16, weight: .semibold))
                }
            }
        }
        .padding(10)
        .allowsHitTesting(false)
        .animation(.easeInOut(duration: 0.15), value: active)
    }
}

// MARK: - Links

struct LinkPopover: View {
    @ObservedObject var library: Library

    private var isEmpty: Bool {
        library.linkText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Add from link")
                .font(.system(size: 15, weight: .bold))
            Text("Paste a direct link to a video or image, or a web page that contains one.")
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            TextField("https://example.com/wallpaper.mp4", text: $library.linkText)
                .textFieldStyle(.roundedBorder)
                .onSubmit { if !isEmpty { library.submitLink() } }
            HStack {
                Button {
                    if let s = NSPasteboard.general.string(forType: .string) {
                        library.linkText = s.trimmingCharacters(in: .whitespacesAndNewlines)
                    }
                } label: {
                    Label("Paste", systemImage: "doc.on.clipboard")
                }
                Spacer()
                Button("Cancel") { library.showLinkPopover = false }
                    .keyboardShortcut(.cancelAction)
                Button("Add") { library.submitLink() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(isEmpty)
            }
            Text("mp4 · mov · m4v · jpg · png · heic · webp · gif")
                .font(.system(size: 10))
                .foregroundStyle(.tertiary)
        }
        .padding(16)
        .frame(width: 420)
    }
}

struct DownloadsList: View {
    @ObservedObject var library: Library

    var body: some View {
        VStack(spacing: 8) {
            ForEach(library.linkDownloads) { d in
                HStack(spacing: 12) {
                    Image(systemName: d.error == nil ? "arrow.down.circle.fill" : "exclamationmark.triangle.fill")
                        .font(.system(size: 18))
                        .foregroundStyle(d.error == nil ? AnyShapeStyle(Theme.accentGradient) : AnyShapeStyle(Color.orange))
                    VStack(alignment: .leading, spacing: 4) {
                        Text(d.title)
                            .font(.system(size: 12, weight: .semibold))
                            .lineLimit(1)
                            .truncationMode(.middle)
                        if let error = d.error {
                            Text(error)
                                .font(.system(size: 11))
                                .foregroundStyle(.orange)
                                .lineLimit(2)
                        } else {
                            ProgressView(value: min(max(d.progress, 0), 1))
                                .progressViewStyle(.linear)
                                .tint(Theme.violet)
                        }
                    }
                    if d.error == nil {
                        Text("\(Int((d.progress * 100).rounded()))%")
                            .font(.system(size: 11, weight: .medium))
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                            .frame(width: 38, alignment: .trailing)
                    }
                    Button {
                        if d.error == nil {
                            library.cancelDownload(d.id)
                        } else {
                            library.dismissDownload(d.id)
                        }
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .foregroundStyle(.secondary)
                    }
                    .buttonStyle(.plain)
                    .help(d.error == nil ? "Cancel download" : "Dismiss")
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
        }
    }
}
