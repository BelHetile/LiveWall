import SwiftUI
import AppKit

enum LibrarySection: String, CaseIterable, Identifiable {
    case wallpapers, settings

    var id: String { rawValue }

    var title: String {
        switch self {
        case .wallpapers: return "Wallpapers"
        case .settings: return "Settings"
        }
    }

    var symbol: String {
        switch self {
        case .wallpapers: return "photo.on.rectangle.angled"
        case .settings: return "gearshape.fill"
        }
    }
}

// MARK: - Root

struct LibraryView: View {
    @ObservedObject var library = Library.shared

    var body: some View {
        HStack(spacing: 0) {
            Sidebar(library: library)
                .frame(width: 210)
            Divider()
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
        .frame(minWidth: 900, minHeight: 600)
        .ignoresSafeArea(.container, edges: .top)
    }
}

// MARK: - Sidebar

struct Sidebar: View {
    @ObservedObject var library: Library

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 10) {
                Image(nsImage: NSApp.applicationIconImage)
                    .resizable()
                    .frame(width: 34, height: 34)
                VStack(alignment: .leading, spacing: 0) {
                    Text("LiveWall")
                        .font(.system(size: 15, weight: .bold))
                    Text("\(library.items.count) wallpapers")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .padding(.top, 44)
            .padding(.bottom, 18)
            .padding(.horizontal, 6)

            ForEach(LibrarySection.allCases) { section in
                SidebarRow(section: section, selected: library.section == section) {
                    library.section = section
                }
            }

            Spacer()

            NowPlayingMini(library: library)
        }
        .padding(.horizontal, 10)
        .padding(.bottom, 12)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(VisualEffectBackground(material: .sidebar))
    }
}

struct SidebarRow: View {
    let section: LibrarySection
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Image(systemName: section.symbol)
                    .font(.system(size: 13, weight: .semibold))
                    .frame(width: 20)
                    .foregroundStyle(selected ? Color.white : Theme.violet)
                Text(section.title)
                    .font(.system(size: 13, weight: selected ? .semibold : .regular))
                    .foregroundStyle(selected ? Color.white : Color.primary)
                Spacer()
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 7)
            .background {
                if selected {
                    RoundedRectangle(cornerRadius: 8).fill(Theme.accentGradient)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

struct NowPlayingMini: View {
    @ObservedObject var library: Library

    var body: some View {
        if let item = library.current {
            HStack(spacing: 8) {
                ZStack {
                    RoundedRectangle(cornerRadius: 6).fill(Color.black.opacity(0.2))
                    if let thumb = library.thumbs[item.id] {
                        Image(nsImage: thumb)
                            .resizable()
                            .aspectRatio(contentMode: .fill)
                    }
                }
                .frame(width: 44, height: 28)
                .clipShape(RoundedRectangle(cornerRadius: 6))

                VStack(alignment: .leading, spacing: 1) {
                    Text(item.name)
                        .font(.system(size: 11, weight: .semibold))
                        .lineLimit(1)
                    Text(statusText)
                        .font(.system(size: 10))
                        .foregroundStyle(statusColor)
                        .lineLimit(1)
                }
                Spacer(minLength: 0)
                Button {
                    library.togglePause()
                } label: {
                    Image(systemName: library.paused ? "play.fill" : "pause.fill")
                        .font(.system(size: 11, weight: .bold))
                }
                .buttonStyle(.plain)
                .help(library.paused ? "Resume" : "Pause")
            }
            .padding(8)
            .background(.quaternary.opacity(0.6), in: RoundedRectangle(cornerRadius: 10))
            .task(id: item.id) { await library.loadThumbnail(for: item) }
        }
    }

    private var statusText: String {
        if library.paused { return "Paused by you" }
        if let r = library.pauseReason { return "Paused · \(r)" }
        return library.current?.kind == .video ? "Playing" : "Showing"
    }

    private var statusColor: Color {
        if library.paused || library.pauseReason != nil { return .orange }
        return .green
    }
}

// MARK: - Wallpapers page

struct WallpapersPage: View {
    @ObservedObject var library: Library

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                if let item = library.current {
                    HeroView(item: item, library: library)
                }

                if let msg = library.message {
                    HStack(spacing: 8) {
                        Image(systemName: "exclamationmark.triangle.fill")
                        Text(msg)
                        Spacer()
                        Button {
                            library.message = nil
                        } label: {
                            Image(systemName: "xmark")
                        }
                        .buttonStyle(.plain)
                    }
                    .font(.callout)
                    .foregroundStyle(.orange)
                    .padding(10)
                    .background(Color.orange.opacity(0.12), in: RoundedRectangle(cornerRadius: 10))
                }

                header

                if library.items.isEmpty {
                    EmptyDropZone(library: library)
                } else {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 210, maximum: 320), spacing: 18)],
                              spacing: 18) {
                        ForEach(Array(library.items.enumerated()), id: \.element.id) { index, item in
                            WallpaperCard(item: item, index: index, library: library)
                        }
                    }
                }
            }
            .padding(.horizontal, 26)
            .padding(.top, 40)
            .padding(.bottom, 26)
        }
        .overlay { DropOverlay(active: library.dropTargeted) }
        .onDrop(of: [.fileURL], isTargeted: $library.dropTargeted) { providers in
            library.handleDrop(providers)
            return true
        }
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline) {
            Text("Library")
                .font(.system(size: 22, weight: .bold))
            Text("\(library.items.count)")
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(.secondary)
            if library.busyCount > 0 {
                ProgressView().controlSize(.small).padding(.leading, 6)
                Text("Importing \(library.busyCount)…")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Button {
                library.presentImportPanel()
            } label: {
                Label("Add Wallpapers", systemImage: "plus")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 7)
                    .background(Theme.accentGradient, in: Capsule())
            }
            .buttonStyle(.plain)
        }
    }
}

// MARK: - Hero

struct HeroView: View {
    let item: WallpaperItem
    @ObservedObject var library: Library

    private var modeBinding: Binding<FillMode> {
        Binding(get: { item.mode }, set: { library.setMode($0, for: item.id) })
    }

    var body: some View {
        ZStack(alignment: .bottomLeading) {
            preview
                .frame(maxWidth: .infinity)
                .frame(height: 260)
                .clipped()

            Theme.heroScrim

            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 6) {
                    Chip(text: "NOW PLAYING", systemImage: "sparkles")
                    Chip(text: item.kind.title, systemImage: item.kind.symbol)
                    Chip(text: item.mode.title, systemImage: item.mode.symbol)
                    if library.paused {
                        Chip(text: "Paused", systemImage: "pause.fill", tint: .orange)
                    } else if let r = library.pauseReason {
                        Chip(text: r, systemImage: "pause.circle.fill", tint: .orange)
                    }
                }

                Text(item.name)
                    .font(.system(size: 28, weight: .heavy))
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
                    CircleButton(systemImage: library.settings.muted ? "speaker.slash.fill" : "speaker.wave.2.fill") {
                        library.toggleMute()
                    }
                    .help(library.settings.muted ? "Unmute" : "Mute")

                    Spacer()

                    Picker("", selection: modeBinding) {
                        ForEach(FillMode.allCases) { m in
                            Text(m.title).tag(m)
                        }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .frame(width: 250)
                    .environment(\.colorScheme, .dark)
                }
            }
            .padding(20)
        }
        .clipShape(RoundedRectangle(cornerRadius: Theme.heroRadius, style: .continuous))
        .shadow(color: Theme.violet.opacity(0.25), radius: 18, y: 8)
        .task(id: item.id) { await library.loadThumbnail(for: item) }
    }

    @ViewBuilder
    private var preview: some View {
        ZStack {
            Theme.accentGradient
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
}

// MARK: - Card

struct WallpaperCard: View {
    let item: WallpaperItem
    let index: Int
    @ObservedObject var library: Library

    private var isCurrent: Bool { library.currentID == item.id }
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
        .animation(.easeInOut(duration: 0.25), value: isCurrent)
        .onHover { inside in
            if inside {
                library.hoveredID = item.id
            } else if library.hoveredID == item.id {
                library.hoveredID = nil
            }
        }
        .contextMenu {
            Button("Set as Wallpaper") { library.select(item.id) }
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

            if isHovered && !isCurrent {
                Color.black.opacity(0.35)
                Image(systemName: "play.circle.fill")
                    .font(.system(size: 40))
                    .foregroundStyle(.white)
                    .shadow(radius: 8)
                    .transition(.scale.combined(with: .opacity))
            }
        }
        .aspectRatio(16.0 / 10.0, contentMode: .fit)
        .clipShape(RoundedRectangle(cornerRadius: Theme.cardRadius, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: Theme.cardRadius, style: .continuous)
                .strokeBorder(isCurrent ? AnyShapeStyle(Theme.accentGradient) : AnyShapeStyle(Color.primary.opacity(0.08)),
                              lineWidth: isCurrent ? 3 : 1)
        }
        .overlay(alignment: .topLeading) {
            if isCurrent {
                Chip(text: "ACTIVE", systemImage: "checkmark.circle.fill")
                    .padding(8)
            }
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
        .onTapGesture { library.select(item.id) }
    }
}

// MARK: - Drop

struct EmptyDropZone: View {
    @ObservedObject var library: Library

    var body: some View {
        VStack(spacing: 14) {
            ZStack {
                Circle().fill(Theme.accentGradient).frame(width: 72, height: 72)
                Image(systemName: "square.and.arrow.down.fill")
                    .font(.system(size: 28, weight: .semibold))
                    .foregroundStyle(.white)
            }
            Text("Drop videos, images or GIFs here")
                .font(.system(size: 18, weight: .semibold))
            Text("mp4 · mov · m4v · png · jpg · heic · gif — folders work too")
                .foregroundStyle(.secondary)
            Button("Choose Files…") { library.presentImportPanel() }
                .controlSize(.large)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 60)
        .background {
            RoundedRectangle(cornerRadius: 18, style: .continuous)
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
