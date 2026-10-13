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
            LibraryBackground(library: library)

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
            Button(L("Paste")) { library.pasteFromClipboard() }
                .keyboardShortcut("v", modifiers: .command)
                .opacity(0)
                .frame(width: 0, height: 0)
        }
        .animation(.easeInOut(duration: 0.2), value: library.detailID)
        .animation(.easeInOut(duration: 0.2), value: library.section)
        .sheet(isPresented: Binding(get: { library.iconPickerFolderID != nil },
                                    set: { if !$0 { library.iconPickerFolderID = nil } })) {
            FolderIconPicker(library: library)
        }
        .frame(minWidth: 980, minHeight: 640)
        .ignoresSafeArea(.container, edges: .top)
        // смена языка перестраивает весь интерфейс
        .id(library.settings.language)
    }
}

struct LibraryBackground: View {
    @ObservedObject var library: Library

    var body: some View {
        ZStack {
            Color(nsColor: .windowBackgroundColor)
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
                        Text(L("Library"))
                    }
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(Theme.violet)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                Text(L("Settings"))
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
                    Label(L("Add"), systemImage: "plus")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 14)
                        .padding(.vertical, 7)
                        .background(Theme.accentGradient, in: Capsule())
                }
                .buttonStyle(.plain)
                .help(L("Add wallpapers"))

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
                .help(L("Add from a link (or press ⌘V with a link copied)"))
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
            .help(L("Settings"))
        }
        .padding(.leading, 86)   // место под кнопки окна
        .padding(.trailing, 20)
        .frame(height: 52)
    }
}

struct StatusPill: View {
    @ObservedObject var library: Library

    private var text: String {
        guard let item = library.current else { return L("No wallpaper") }
        if library.paused { return L("Paused by you") }
        if let r = library.pauseReason { return LF("Paused · %@", r) }
        return item.kind == .video ? L("Playing") : L("Showing")
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

                    FoldersStrip(library: library)

                    if let msg = library.message {
                        MessageBanner(text: msg) { library.message = nil }
                    }

                    header

                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 240, maximum: 380), spacing: 20)],
                              spacing: 22) {
                        ForEach(library.visibleItems) { item in
                            WallpaperCard(item: item,
                                          index: library.items.firstIndex(where: { $0.id == item.id }) ?? 0,
                                          library: library)
                        }
                    }
                    if library.visibleItems.isEmpty {
                        Text(L("This folder is empty. Drag wallpapers onto the folder or use “Move to Folder” in a wallpaper’s menu."))
                            .font(.callout)
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 40)
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
            if let folder = library.selectedFolder {
                Text(folder.name)
                    .font(.system(size: 22, weight: .bold))
                    .onTapGesture(count: 2) { library.renameFolder(folder.id) }
                Text("\(library.visibleItems.count)")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(.secondary)
                Button {
                    library.renameFolder(folder.id)
                } label: {
                    Image(systemName: "pencil")
                }
                .buttonStyle(.borderless)
                .help(L("Rename folder"))
                Button {
                    library.openIconPicker(folder.id)
                } label: {
                    Image(systemName: "paintbrush")
                }
                .buttonStyle(.borderless)
                .help(L("Change folder icon"))
                if folder.isProtected {
                    Button {
                        library.lockFolder(folder.id)
                    } label: {
                        Label(L("Lock"), systemImage: "lock.fill")
                    }
                    .buttonStyle(.borderless)
                    .help(L("Lock the folder and hide its wallpapers"))
                }
            } else {
                Text(L("All Wallpapers"))
                    .font(.system(size: 22, weight: .bold))
            }
            if library.busyCount > 0 {
                ProgressView().controlSize(.small).padding(.leading, 6)
                Text(LF("Importing %ld…", library.busyCount))
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Text(L("Click a wallpaper to adjust it"))
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
        HStack(spacing: 16) {
            DesktopTile(library: library)
                .frame(width: 368, height: 230)
            LockTile(library: library)
                .frame(width: 368, height: 230)
            Spacer(minLength: 0)
        }
    }
}

struct DesktopTile: View {
    @ObservedObject var library: Library

    private var isPrivate: Bool {
        library.current.map { library.isHidden($0) } ?? false
    }

    /// Анимированные обои (видео, GIF): у них есть пауза.
    private var isAnimated: Bool {
        library.current.map { $0.kind != .image } ?? false
    }

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
                    if item.kind == .video, let player = library.tilePlayer {
                        PlayerPreview(player: player)
                    }
                    if isAnimated && library.paused {
                        Color.black.opacity(0.25)
                    }
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .blur(radius: isPrivate ? 40 : 0)
            .overlay {
                if isPrivate {
                    Image(systemName: "lock.fill")
                        .font(.system(size: 40, weight: .semibold))
                        .foregroundStyle(.white.opacity(0.85))
                }
            }
            .clipped()

            Theme.heroScrim

            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 6) {
                    Chip(text: L("DESKTOP"), systemImage: "display")
                    if isAnimated && library.paused {
                        Chip(text: L("PAUSED"), systemImage: "pause.fill")
                    }
                }
                Text(isPrivate ? L("Private wallpaper") : (library.current?.name ?? L("No wallpaper")))
                    .font(.system(size: 18, weight: .heavy))
                    .foregroundStyle(.white)
                    .lineLimit(1)
                    .shadow(color: .black.opacity(0.4), radius: 6, y: 2)
                HStack(spacing: 12) {
                    CircleButton(systemImage: "backward.fill") { library.step(-1) }
                        .help(L("Previous"))
                    if isAnimated {
                        CircleButton(systemImage: library.paused ? "play.fill" : "pause.fill", size: 36, prominent: true) {
                            library.togglePause()
                        }
                        .help(library.paused ? L("Resume") : L("Pause"))
                    }
                    CircleButton(systemImage: "forward.fill") { library.step(1) }
                        .help(L("Next"))
                    CircleButton(systemImage: "shuffle") { library.random() }
                        .help(L("Random"))
                    if let item = library.current {
                        CircleButton(systemImage: "slider.horizontal.3") { library.openDetail(item.id) }
                            .help(L("Adjust"))
                    }
                }
            }
            .padding(16)
        }
        .clipShape(RoundedRectangle(cornerRadius: Theme.heroRadius, style: .continuous))
        .shadow(color: Theme.violet.opacity(0.25), radius: 18, y: 8)
        .contentShape(RoundedRectangle(cornerRadius: Theme.heroRadius, style: .continuous))
        .onTapGesture {
            if let item = library.current { library.openDetail(item.id) }
        }
        .task(id: library.currentID) {
            library.tileAppeared()
            if let item = library.current { await library.loadThumbnail(for: item) }
        }
        .onDisappear { library.tileDisappeared() }
    }
}

struct LockTile: View {
    @ObservedObject var library: Library

    var body: some View {
        let item = library.lockItem
        let isPrivate = item.map { library.isHidden($0) } ?? false
        ZStack {
            ZStack {
                LinearGradient(colors: [Color(white: 0.15), Color(white: 0.05)],
                               startPoint: .top, endPoint: .bottom)
                if let item = item, let thumb = library.thumbs[item.id] {
                    Image(nsImage: thumb)
                        .resizable()
                        .aspectRatio(contentMode: .fill)
                        .blur(radius: isPrivate ? 30 : 0)
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
                        .font(.system(size: 44, weight: .bold, design: .rounded))
                }
                .foregroundStyle(.white.opacity(0.92))
                .shadow(color: .black.opacity(0.35), radius: 8, y: 2)
            }
            .frame(maxHeight: .infinity, alignment: .top)
            .padding(.top, 18)

            VStack(alignment: .leading, spacing: 6) {
                Chip(text: L("LOCK SCREEN"), systemImage: "lock.fill")
                Text(isPrivate ? L("Private wallpaper") : (item?.name ?? L("Not set")))
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
        if library.lockScreenID != nil { return L("Custom · still frame") }
        if library.settings.syncSystemWallpaper { return L("Same as desktop · still frame") }
        return L("Off — enable in Settings or pick a wallpaper")
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
                        .onTapGesture(count: 2) { library.renameItem(item.id) }
                        .help(L("Double-click to rename"))
                    HStack(spacing: 4) {
                        Image(systemName: item.kind.symbol)
                        Text("\(item.kind.title) · \(item.mode.title)")
                    }
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
                }
                Spacer()
                if let hk = item.hotkey {
                    Text(HotkeyFormat.string(hk))
                        .font(.system(size: 10, weight: .semibold, design: .rounded))
                        .foregroundStyle(Theme.violet)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Theme.violet.opacity(0.12), in: Capsule())
                        .help(L("Press to switch to this wallpaper, press again to go back"))
                }
            }
            .padding(.horizontal, 2)
        }
        .onDrag { NSItemProvider(object: item.id.uuidString as NSString) }
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
            Button(L("Set on Desktop")) { library.setDesktop(item.id) }
            Button(L("Set on Lock Screen")) { library.setLockScreen(item.id) }
            Button(L("Adjust…")) { library.openDetail(item.id) }
            Button(L("Rename…")) { library.renameItem(item.id) }
            Menu(L("Move to Folder")) {
                Button {
                    library.moveItem(item.id, to: nil)
                } label: {
                    if item.folderID == nil { Label(L("No Folder"), systemImage: "checkmark") } else { Text(L("No Folder")) }
                }
                if !library.folders.isEmpty { Divider() }
                ForEach(library.folders) { folder in
                    Button {
                        library.moveItem(item.id, to: folder.id)
                    } label: {
                        if item.folderID == folder.id { Label(folder.name, systemImage: "checkmark") } else { Text(folder.name) }
                    }
                }
                Divider()
                Button(L("New Folder…")) { library.createFolder(moving: [item.id]) }
            }
            Menu(L("Fill Mode")) {
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
            if item.kind == .video {
                if library.optimizing[item.id] != nil {
                    Button(L("Cancel Optimization")) { library.cancelOptimize(item.id) }
                } else {
                    Button(L("Optimize for This Mac")) { library.optimize(item.id, manual: true) }
                }
            }
            Button(L("Reveal in Finder")) { library.revealInFinder(item) }
            Divider()
            Button(L("Remove"), role: .destructive) { library.remove(item.id) }
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
                    Text(L("Adjust"))
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
                if isDesktop { Chip(text: L("DESKTOP"), systemImage: "display") }
                if isLock { Chip(text: L("LOCK"), systemImage: "lock.fill") }
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
        .overlay(alignment: .bottom) {
            if let progress = library.optimizing[item.id] {
                OptimizeBadge(progress: progress)
                    .padding(8)
            }
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
                PlayerPreview(player: library.previewPlayer, gravity: .resizeAspect)
                if library.previewPaused {
                    Color.black.opacity(0.2)
                }
            } else if let image = library.previewImage(for: item) {
                Image(nsImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
            }

            HStack {
                CircleButton(systemImage: "chevron.left", size: 36) { library.showAdjacentDetail(-1) }
                    .help(L("Previous wallpaper"))
                Spacer()
                CircleButton(systemImage: "chevron.right", size: 36) { library.showAdjacentDetail(1) }
                    .help(L("Next wallpaper"))
            }
            .padding(14)

            if item.kind == .video {
                CircleButton(systemImage: library.previewPaused ? "play.fill" : "pause.fill", size: 54, prominent: true) {
                    library.togglePreviewPause()
                }
                .help(library.previewPaused ? L("Resume") : L("Pause"))
                .frame(maxHeight: .infinity, alignment: .bottom)
                .padding(.bottom, 20)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .strokeBorder(Color.white.opacity(0.06))
        }
        .overlay(alignment: .topLeading) {
            HStack(spacing: 6) {
                if isDesktop { Chip(text: L("ON DESKTOP"), systemImage: "display") }
                if isLock { Chip(text: L("ON LOCK SCREEN"), systemImage: "lock.fill") }
                if !isDesktop && item.kind == .video { Chip(text: L("PREVIEW"), systemImage: "eye") }
                if item.kind == .video && library.previewPaused { Chip(text: L("PAUSED"), systemImage: "pause.fill") }
            }
            .padding(14)
        }
        .padding(20)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .task(id: item.id) { await library.loadThumbnail(for: item) }
    }

    // MARK: Управление

    private var controlsPane: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 4) {
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Text(item.name)
                            .font(.system(size: 20, weight: .bold))
                            .lineLimit(2)
                            .onTapGesture(count: 2) { library.renameItem(item.id) }
                        Button {
                            library.renameItem(item.id)
                        } label: {
                            Image(systemName: "pencil")
                                .font(.system(size: 12, weight: .semibold))
                        }
                        .buttonStyle(.borderless)
                        .help(L("Rename"))
                    }
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
                .help(L("Close"))
            }

            ScrollView(.vertical, showsIndicators: false) {
                VStack(alignment: .leading, spacing: 18) {
                    VStack(alignment: .leading, spacing: 10) {
                        SectionLabel(text: L("Use as"))
                        HStack(spacing: 10) {
                            TargetButton(title: L("Desktop"), symbol: "display", active: isDesktop) {
                                library.setDesktop(item.id)
                            }
                            TargetButton(title: L("Lock Screen"), symbol: "lock.fill", active: isLock) {
                                library.setLockScreen(item.id)
                            }
                        }
                        if library.lockScreenID == item.id {
                            Button(L("Lock screen: follow the desktop again")) {
                                library.lockScreenFollowDesktop()
                            }
                            .buttonStyle(.link)
                            .font(.system(size: 11))
                        }
                    }

                    if item.kind == .video {
                        VStack(alignment: .leading, spacing: 10) {
                            SectionLabel(text: L("Playback"))

                            if isDesktop {
                                Button {
                                    library.togglePause()
                                } label: {
                                    Label(library.paused ? L("Resume") : L("Pause"),
                                          systemImage: library.paused ? "play.fill" : "pause.fill")
                                        .font(.system(size: 13, weight: .semibold))
                                        .frame(maxWidth: .infinity)
                                        .padding(.vertical, 8)
                                        .background(Color.primary.opacity(0.08), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                                        .contentShape(Rectangle())
                                }
                                .buttonStyle(.plain)
                                if let reason = library.pauseReason, !library.paused {
                                    Label(LF("Auto-paused: %@", reason), systemImage: "pause.circle")
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
                                .help(muted ? L("Unmute") : L("Mute"))
                                Slider(value: volumeBinding, in: 0...1)
                                    .disabled(muted)
                                Text(muted ? L("Muted") : "\(Int((library.effectiveVolume(item) * 100).rounded()))%")
                                    .font(.system(size: 11, weight: .medium))
                                    .monospacedDigit()
                                    .foregroundStyle(.secondary)
                                    .frame(width: 44, alignment: .trailing)
                            }

                            HStack {
                                Text(L("Speed"))
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

                    HStack(alignment: .center, spacing: 10) {
                        VStack(alignment: .leading, spacing: 8) {
                            SectionLabel(text: L("Folder"))
                            Menu {
                                Button(L("No Folder")) { library.moveItem(item.id, to: nil) }
                                if !library.folders.isEmpty { Divider() }
                                ForEach(library.folders) { folder in
                                    Button(folder.name) { library.moveItem(item.id, to: folder.id) }
                                }
                                Divider()
                                Button(L("New Folder…")) { library.createFolder(moving: [item.id]) }
                            } label: {
                                Text(library.folders.first(where: { $0.id == item.folderID })?.name ?? L("No Folder"))
                                    .lineLimit(1)
                            }
                            .fixedSize()
                        }
                        Spacer()
                        VStack(alignment: .trailing, spacing: 8) {
                            SectionLabel(text: L("Hotkey"))
                            WallpaperHotkeyRecorder(item: item, library: library)
                        }
                    }

                    if item.hotkey != nil {
                        VStack(alignment: .leading, spacing: 6) {
                            Text(L("Hotkey switches"))
                                .font(.system(size: 12))
                                .foregroundStyle(.secondary)
                            Picker("", selection: Binding(get: { item.hotkeyScope ?? .both },
                                                          set: { library.setHotkeyScope($0, for: item.id) })) {
                                ForEach(HotkeyScope.allCases) { scope in
                                    Text(scope.title).tag(scope)
                                }
                            }
                            .pickerStyle(.segmented)
                            .labelsHidden()
                        }
                    }

                    VStack(alignment: .leading, spacing: 10) {
                        SectionLabel(text: L("Fill mode"))
                        Picker("", selection: modeBinding) {
                            ForEach(FillMode.allCases) { m in
                                Text(m.title).tag(m)
                            }
                        }
                        .pickerStyle(.segmented)
                        .labelsHidden()
                    }

                    if item.kind == .video {
                        PerformanceSection(item: item, library: library)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }

            HStack {
                Button {
                    library.revealInFinder(item)
                } label: {
                    Label(L("Show in Finder"), systemImage: "folder")
                }
                Spacer()
                Button(role: .destructive) {
                    library.remove(item.id)
                } label: {
                    Label(L("Remove"), systemImage: "trash")
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

/// Оптимизация видео в панели обоев.
struct PerformanceSection: View {
    let item: WallpaperItem
    @ObservedObject var library: Library

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionLabel(text: L("Performance"))
            if let info = library.videoInfo[item.id] {
                Label(info.summary, systemImage: "film")
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
            } else {
                Text(L("Reading video info…"))
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
            }

            if let progress = library.optimizing[item.id] {
                HStack(spacing: 10) {
                    ProgressView(value: progress)
                    Text("\(Int((progress * 100).rounded()))%")
                        .font(.system(size: 11, weight: .medium))
                        .monospacedDigit()
                        .foregroundStyle(.secondary)
                        .frame(width: 36, alignment: .trailing)
                    Button {
                        library.cancelOptimize(item.id)
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                    }
                    .buttonStyle(.borderless)
                    .help(L("Cancel Optimization"))
                }
                Text(progress > 0 ? L("Optimizing… You can keep using the Mac.") : L("Waiting in line…"))
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
            } else if library.unplayable.contains(item.id) {
                Label(L("macOS can't play this video's format, so it shows up black."), systemImage: "exclamationmark.triangle.fill")
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(.orange)
                    .fixedSize(horizontal: false, vertical: true)
                Button {
                    library.optimize(item.id, manual: true)
                } label: {
                    Label(L("Convert to a Playable Video"), systemImage: "arrow.triangle.2.circlepath")
                        .font(.system(size: 13, weight: .semibold))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                        .background(Color.primary.opacity(0.08), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            } else if library.canOptimize(item.id) == false {
                Label(L("Already optimal for this Mac"), systemImage: "checkmark.seal.fill")
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(.green)
            } else if library.canOptimize(item.id) == true {
                Button {
                    library.optimize(item.id, manual: true)
                } label: {
                    Label(L("Optimize for This Mac"), systemImage: "bolt.fill")
                        .font(.system(size: 13, weight: .semibold))
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 8)
                        .background(Color.primary.opacity(0.08), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                Text(LF("Re-encodes to HEVC at your screen size and up to %ld fps, so it uses less energy. The original is replaced.",
                        Int(library.optimizeFpsLimit)))
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

/// Полоска прогресса оптимизации на карточке.
struct OptimizeBadge: View {
    let progress: Double

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "bolt.fill")
                .font(.system(size: 9, weight: .bold))
            ProgressView(value: progress)
                .progressViewStyle(.linear)
                .tint(.white)
            Text("\(Int((progress * 100).rounded()))%")
                .font(.system(size: 10, weight: .semibold))
                .monospacedDigit()
        }
        .foregroundStyle(.white)
        .padding(.horizontal, 8)
        .padding(.vertical, 5)
        .background(.ultraThinMaterial, in: Capsule())
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
    @ObservedObject var themeSource = Library.shared
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
                Text(active ? L("Active") : L("Set"))
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
            Text(L("Drop videos, images, GIFs or links here"))
                .font(.system(size: 20, weight: .semibold))
            Text(L("mp4 · mov · m4v · png · jpg · heic · gif — folders work too"))
                .foregroundStyle(.secondary)
            HStack(spacing: 10) {
                Button(L("Choose Files…")) { library.presentImportPanel() }
                Button(L("Add from Link…")) { library.showLinkPopover = true }
            }
            .controlSize(.large)
            Text(L("Tip: copy a link to a video or image and press ⌘V here"))
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
    @ObservedObject var themeSource = Library.shared
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
                    Text(L("Drop to add to your library"))
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
            Text(L("Add from link"))
                .font(.system(size: 15, weight: .bold))
            Text(L("Paste a direct link to a video or image, or a web page that contains one."))
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            TextField(L("https://example.com/wallpaper.mp4"), text: $library.linkText)
                .textFieldStyle(.roundedBorder)
                .onSubmit { if !isEmpty { library.submitLink() } }
            HStack {
                Button {
                    if let s = NSPasteboard.general.string(forType: .string) {
                        library.linkText = s.trimmingCharacters(in: .whitespacesAndNewlines)
                    }
                } label: {
                    Label(L("Paste"), systemImage: "doc.on.clipboard")
                }
                Spacer()
                Button(L("Cancel")) { library.showLinkPopover = false }
                    .keyboardShortcut(.cancelAction)
                Button(L("Add")) { library.submitLink() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(isEmpty)
            }
            Text(L("mp4 · mov · m4v · jpg · png · heic · webp · gif"))
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
                    .help(d.error == nil ? L("Cancel download") : L("Dismiss"))
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
        }
    }
}

// MARK: - Folders

struct FoldersStrip: View {
    @ObservedObject var library: Library

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text(L("Folders"))
                    .font(.system(size: 15, weight: .bold))
                Spacer()
            }
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 12) {
                    FolderTile(title: L("All Wallpapers"),
                               count: library.browsableItems.count,
                               icon: .symbol(name: "square.grid.2x2.fill", palette: 7),
                               selected: library.selectedFolderID == nil,
                               dropTargeted: library.allTileDropTargeted,
                               isProtected: false,
                               isLocked: false)
                        .onTapGesture { library.selectedFolderID = nil }
                        .onDrop(of: [.plainText], isTargeted: $library.allTileDropTargeted) { providers in
                            library.handleFolderDrop(providers, folderID: nil)
                        }

                    ForEach(library.folders) { folder in
                        FolderTile(title: folder.name,
                                   count: library.count(in: folder.id),
                                   icon: folder.icon,
                                   selected: library.selectedFolderID == folder.id,
                                   dropTargeted: library.folderDropTarget == folder.id,
                                   isProtected: folder.isProtected,
                                   isLocked: library.isLocked(folder.id))
                            .onTapGesture { library.openFolder(folder.id) }
                            .onDrop(of: [.plainText], isTargeted: dropBinding(folder.id)) { providers in
                                library.handleFolderDrop(providers, folderID: folder.id)
                            }
                            .contextMenu {
                                Button(L("Rename…")) { library.renameFolder(folder.id) }
                                Button(L("Change Icon…")) { library.openIconPicker(folder.id) }
                                Divider()
                                if folder.isProtected {
                                    if library.isLocked(folder.id) {
                                        Button(L("Unlock…")) { library.openFolder(folder.id) }
                                    } else {
                                        Button(L("Lock Now")) { library.lockFolder(folder.id) }
                                    }
                                    Button(L("Change Password…")) { library.setPassword(folder.id) }
                                    Button(L("Remove Password…")) { library.removePassword(folder.id) }
                                } else {
                                    Button(L("Set Password…")) { library.setPassword(folder.id) }
                                }
                                Divider()
                                Button(L("Delete Folder"), role: .destructive) { library.deleteFolder(folder.id) }
                            }
                    }

                    Button {
                        if let id = library.createFolder() { library.selectedFolderID = id }
                    } label: {
                        VStack(spacing: 8) {
                            Image(systemName: "plus")
                                .font(.system(size: 20, weight: .semibold))
                                .frame(width: 44, height: 44)
                            Text(L("New Folder"))
                                .font(.system(size: 12, weight: .semibold))
                        }
                        .foregroundStyle(.secondary)
                        .frame(width: 130, height: 104)
                        .background {
                            RoundedRectangle(cornerRadius: 14, style: .continuous)
                                .strokeBorder(style: StrokeStyle(lineWidth: 1.5, dash: [6, 5]))
                                .foregroundStyle(Color.primary.opacity(0.25))
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
                .padding(.vertical, 4)
                .padding(.horizontal, 2)
            }
        }
    }

    private func dropBinding(_ id: UUID) -> Binding<Bool> {
        Binding(get: { library.folderDropTarget == id },
                set: { inside in
                    if inside {
                        library.folderDropTarget = id
                    } else if library.folderDropTarget == id {
                        library.folderDropTarget = nil
                    }
                })
    }
}

struct FolderTile: View {
    @ObservedObject var themeSource = Library.shared
    let title: String
    let count: Int
    let icon: FolderIcon
    let selected: Bool
    let dropTargeted: Bool
    let isProtected: Bool
    let isLocked: Bool

    var body: some View {
        VStack(spacing: 8) {
            FolderIconView(icon: icon, size: 44)
                .overlay(alignment: .bottomTrailing) {
                    if isProtected {
                        Image(systemName: isLocked ? "lock.fill" : "lock.open.fill")
                            .font(.system(size: 10, weight: .bold))
                            .foregroundStyle(.white)
                            .padding(4)
                            .background(Circle().fill(isLocked ? Color.black.opacity(0.75) : Color.green.opacity(0.85)))
                            .offset(x: 6, y: 6)
                    }
                }
            VStack(spacing: 1) {
                Text(title)
                    .font(.system(size: 12, weight: .semibold))
                    .lineLimit(1)
                    .truncationMode(.tail)
                Text(isLocked ? L("Locked") : "\(count)")
                    .font(.system(size: 10))
                    .foregroundStyle(.secondary)
            }
        }
        .padding(.horizontal, 8)
        .frame(width: 130, height: 104)
        .background {
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(selected ? Theme.violet.opacity(0.16) : Color.primary.opacity(0.05))
        }
        .overlay {
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .strokeBorder(dropTargeted || selected ? AnyShapeStyle(Theme.accentGradient) : AnyShapeStyle(Color.clear),
                              lineWidth: dropTargeted ? 3 : 2)
        }
        .scaleEffect(dropTargeted ? 1.06 : 1)
        .animation(.spring(response: 0.25, dampingFraction: 0.7), value: dropTargeted)
        .contentShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
    }
}

struct FolderIconPicker: View {
    @ObservedObject var library: Library

    private var folder: WallpaperFolder? {
        library.folders.first(where: { $0.id == library.iconPickerFolderID })
    }

    private var currentPalette: Int {
        if case .symbol(_, let p)? = folder?.icon { return p }
        return 0
    }

    private var currentSymbol: String? {
        if case .symbol(let n, _)? = folder?.icon { return n }
        return nil
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            if let folder = folder {
                HStack(spacing: 14) {
                    FolderIconView(icon: folder.icon, size: 60)
                    VStack(alignment: .leading, spacing: 3) {
                        Text(L("Folder Icon"))
                            .font(.system(size: 16, weight: .bold))
                        Text(folder.name)
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button(L("Done")) { library.iconPickerFolderID = nil }
                        .keyboardShortcut(.defaultAction)
                }

                SectionLabel(text: L("Color"))
                HStack(spacing: 10) {
                    ForEach(0..<Theme.folderPalettes.count, id: \.self) { i in
                        Circle()
                            .fill(Theme.folderGradient(i))
                            .frame(width: 28, height: 28)
                            .overlay {
                                Circle()
                                    .strokeBorder(Color.primary, lineWidth: currentPalette == i && currentSymbol != nil ? 2.5 : 0)
                                    .padding(-4)
                            }
                            .contentShape(Circle())
                            .onTapGesture { library.setFolderPalette(i, for: folder.id) }
                    }
                }
                .padding(.leading, 4)

                SectionLabel(text: L("Icon"))
                ScrollView {
                    LazyVGrid(columns: Array(repeating: GridItem(.fixed(50), spacing: 10), count: 9), spacing: 10) {
                        ForEach(FolderIcon.basePack, id: \.self) { name in
                            FolderIconView(icon: .symbol(name: name, palette: currentPalette), size: 50)
                                .overlay {
                                    RoundedRectangle(cornerRadius: 50 * 0.26 + 3, style: .continuous)
                                        .strokeBorder(Color.primary, lineWidth: currentSymbol == name ? 2.5 : 0)
                                        .padding(-3)
                                }
                                .contentShape(Rectangle())
                                .onTapGesture { library.setFolderSymbol(name, for: folder.id) }
                                .help(name)
                        }
                    }
                    .padding(6)
                }
                .frame(height: 200)

                HStack {
                    SectionLabel(text: L("My Icons"))
                    Spacer()
                    Button {
                        library.importUserIcons()
                    } label: {
                        Label(L("Import Icons…"), systemImage: "square.and.arrow.down")
                    }
                    .buttonStyle(.borderless)
                }
                if library.userIcons.isEmpty {
                    Text(L("Import your own images once and use them for any folder."))
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                } else {
                    ScrollView {
                        LazyVGrid(columns: Array(repeating: GridItem(.fixed(50), spacing: 10), count: 9), spacing: 10) {
                            ForEach(library.userIcons, id: \.self) { name in
                                UserIconThumb(name: name, size: 50, library: library)
                                    .onTapGesture { library.setFolderUserIcon(name, for: folder.id) }
                                    .contextMenu {
                                        Button(L("Remove from My Icons"), role: .destructive) { library.removeUserIcon(name) }
                                    }
                            }
                        }
                        .padding(4)
                    }
                    .frame(maxHeight: 120)
                }

                HStack {
                    Button {
                        library.chooseFolderImage(for: folder.id)
                    } label: {
                        Label(L("Choose Image…"), systemImage: "photo")
                    }
                    if case .image = folder.icon {
                        Text(L("Using a custom image"))
                            .font(.system(size: 12))
                            .foregroundStyle(.secondary)
                    }
                    Spacer()
                }
            } else {
                Text(L("Folder not found"))
                Button(L("Close")) { library.iconPickerFolderID = nil }
            }
        }
        .padding(22)
        .frame(width: 600)
    }
}

struct WallpaperHotkeyRecorder: View {
    let item: WallpaperItem
    @ObservedObject var library: Library

    private var recording: Bool { library.recordingTarget == .wallpaper(item.id) }

    var body: some View {
        HStack(spacing: 6) {
            Button {
                if recording {
                    library.endRecording()
                } else {
                    library.beginRecording(target: .wallpaper(item.id))
                }
            } label: {
                Text(recording ? L("Press shortcut…") : (item.hotkey.map { HotkeyFormat.string($0) } ?? L("Set Hotkey")))
                    .frame(minWidth: 96)
            }
            .help(L("Press once to switch to this wallpaper, press again to return to the previous one"))
            if item.hotkey != nil && !recording {
                Button {
                    library.clearWallpaperHotkey(item.id)
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .help(L("Remove hotkey"))
            }
        }
    }
}

struct UserIconThumb: View {
    let name: String
    var size: CGFloat = 50
    @ObservedObject var library: Library

    var body: some View {
        ZStack {
            Color.gray.opacity(0.2)
            if let img = library.userIconImage(name) {
                Image(nsImage: img)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
            }
        }
        .frame(width: size, height: size)
        .clipShape(RoundedRectangle(cornerRadius: size * 0.26, style: .continuous))
        .contentShape(Rectangle())
    }
}
