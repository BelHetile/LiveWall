import SwiftUI
import AppKit
import AVFoundation

// MARK: - Palette

enum Theme {
    /// The current color style (built-in or custom).
    static var style: ColorStyle {
        let s = Library.shared.settings
        return (ColorStyle.builtIns + s.customStyles).first(where: { $0.id == s.colorStyleID }) ?? ColorStyle.builtIns[0]
    }

    // historical names: "violet" is the style's main color, "magenta" the second one
    static var violet: Color { Color(hex: style.primary) }
    static var magenta: Color { Color(hex: style.secondary) }
    static let sunset = Color(red: 1.00, green: 0.66, blue: 0.47)

    static func gradient(for style: ColorStyle) -> LinearGradient {
        LinearGradient(colors: [Color(hex: style.primary), Color(hex: style.secondary)],
                       startPoint: .topLeading, endPoint: .bottomTrailing)
    }

    static var accentGradient: LinearGradient { LinearGradient(colors: [violet, magenta],
                                               startPoint: .topLeading,
                                               endPoint: .bottomTrailing) }

    static let heroScrim = LinearGradient(colors: [.clear, .black.opacity(0.25), .black.opacity(0.78)],
                                          startPoint: .top,
                                          endPoint: .bottom)

    /// Color themes for folder icons.
    static let folderPalettes: [[Color]] = [
        [Color(red: 0.49, green: 0.30, blue: 0.98), Color(red: 0.93, green: 0.33, blue: 0.62)],
        [Color(red: 0.20, green: 0.48, blue: 1.00), Color(red: 0.20, green: 0.82, blue: 0.95)],
        [Color(red: 0.16, green: 0.72, blue: 0.47), Color(red: 0.10, green: 0.60, blue: 0.70)],
        [Color(red: 1.00, green: 0.62, blue: 0.25), Color(red: 0.98, green: 0.36, blue: 0.52)],
        [Color(red: 0.95, green: 0.27, blue: 0.27), Color(red: 1.00, green: 0.55, blue: 0.20)],
        [Color(red: 0.98, green: 0.80, blue: 0.20), Color(red: 1.00, green: 0.56, blue: 0.16)],
        [Color(red: 0.36, green: 0.30, blue: 0.85), Color(red: 0.62, green: 0.30, blue: 0.86)],
        [Color(red: 0.38, green: 0.40, blue: 0.46), Color(red: 0.12, green: 0.13, blue: 0.16)],
    ]

    static func folderGradient(_ index: Int) -> LinearGradient {
        let colors = folderPalettes[((index % folderPalettes.count) + folderPalettes.count) % folderPalettes.count]
        return LinearGradient(colors: colors, startPoint: .topLeading, endPoint: .bottomTrailing)
    }

    static let cardRadius: CGFloat = 12
    static let heroRadius: CGFloat = 26
}

// MARK: - AppKit bridges

/// A real macOS vibrancy background (like system app sidebars).
struct VisualEffectBackground: NSViewRepresentable {
    var material: NSVisualEffectView.Material = .sidebar
    var blending: NSVisualEffectView.BlendingMode = .behindWindow

    func makeNSView(context: Context) -> NSVisualEffectView {
        let v = NSVisualEffectView()
        v.material = material
        v.blendingMode = blending
        v.state = .followsWindowActiveState
        return v
    }

    func updateNSView(_ nsView: NSVisualEffectView, context: Context) {
        nsView.material = material
        nsView.blendingMode = blending
    }
}

/// The value of the slider being dragged right now. While dragging, only the sliders redraw; the library gets
/// the value once the drag ends, so a heavy interface can't make the knob jump back.
final class SliderDraft: ObservableObject {
    static let shared = SliderDraft()
    @Published var key: String?
    @Published var value: Double = 0
    var pending: DispatchWorkItem?
}

struct SmoothSlider: View {
    let key: String
    let value: Double
    var range: ClosedRange<Double> = 0...1
    var live: (Double) -> Void = { _ in }
    let commit: (Double) -> Void
    @ObservedObject var draft = SliderDraft.shared

    var body: some View {
        Slider(value: Binding(get: { draft.key == key ? draft.value : value },
                              set: { v in
                                  if draft.key != key { draft.key = key }
                                  draft.value = v
                                  live(v)
                                  // keyboard changes have no "editing ended": commit after a short pause
                                  draft.pending?.cancel()
                                  let work = DispatchWorkItem { finish() }
                                  draft.pending = work
                                  DispatchQueue.main.asyncAfter(deadline: .now() + 0.6, execute: work)
                              }),
               in: range,
               onEditingChanged: { editing in
                   if !editing { finish() }
               })
    }

    private func finish() {
        guard draft.key == key else { return }
        draft.pending?.cancel()
        draft.pending = nil
        let v = draft.value
        draft.key = nil
        commit(v)
    }
}

/// An animated GIF preview in the library (the same layer as on the desktop).
struct GIFPreview: NSViewRepresentable {
    let url: URL
    var paused: Bool
    var fill = true

    final class HostView: NSView {
        var gif: GIFLayer?
        var url: URL?

        override init(frame frameRect: NSRect) {
            super.init(frame: frameRect)
            wantsLayer = true
        }

        required init?(coder: NSCoder) {
            fatalError("init(coder:) has not been implemented")
        }

        func load(_ newURL: URL, fill: Bool) {
            guard newURL != url else { return }
            gif?.removeFromSuperlayer()
            let l = GIFLayer(url: newURL)
            l.contentsGravity = fill ? .resizeAspectFill : .resizeAspect
            l.masksToBounds = true
            layer?.addSublayer(l)
            gif = l
            url = newURL
            needsLayout = true
        }

        override func layout() {
            super.layout()
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            gif?.frame = bounds
            gif?.contentsScale = window?.backingScaleFactor ?? 2
            CATransaction.commit()
        }
    }

    func makeNSView(context: Context) -> HostView {
        let v = HostView()
        v.load(url, fill: fill)
        v.gif?.setPaused(paused)
        return v
    }

    func updateNSView(_ nsView: HostView, context: Context) {
        nsView.load(url, fill: fill)
        nsView.gif?.contentsGravity = fill ? .resizeAspectFill : .resizeAspect
        nsView.gif?.setPaused(paused)
    }
}

/// A live video preview for an AVPlayer.
struct PlayerPreview: NSViewRepresentable {
    let player: AVPlayer?
    var gravity: AVLayerVideoGravity = .resizeAspectFill

    final class PlayerHostView: NSView {
        let playerLayer = AVPlayerLayer()

        override init(frame frameRect: NSRect) {
            super.init(frame: frameRect)
            wantsLayer = true
            layer?.addSublayer(playerLayer)
            playerLayer.videoGravity = .resizeAspectFill
        }

        required init?(coder: NSCoder) {
            fatalError("init(coder:) has not been implemented")
        }

        override func layout() {
            super.layout()
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            playerLayer.frame = bounds
            CATransaction.commit()
        }
    }

    func makeNSView(context: Context) -> PlayerHostView {
        let v = PlayerHostView()
        v.playerLayer.player = player
        v.playerLayer.videoGravity = gravity
        return v
    }

    func updateNSView(_ nsView: PlayerHostView, context: Context) {
        if nsView.playerLayer.player !== player {
            nsView.playerLayer.player = player
        }
        if nsView.playerLayer.videoGravity != gravity {
            nsView.playerLayer.videoGravity = gravity
        }
    }
}

// MARK: - Small components

struct Chip: View {
    let text: String
    var systemImage: String?
    var tint: Color = .white

    var body: some View {
        HStack(spacing: 4) {
            if let systemImage = systemImage {
                Image(systemName: systemImage)
            }
            Text(text)
        }
        .font(.system(size: 11, weight: .semibold))
        .foregroundStyle(tint)
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .background(.ultraThinMaterial, in: Capsule())
    }
}

struct CircleButton: View {
    @ObservedObject var themeSource = Library.shared
    let systemImage: String
    var size: CGFloat = 30
    var prominent = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .font(.system(size: size * 0.42, weight: .bold))
                .frame(width: size, height: size)
                .foregroundStyle(.white)
                .background {
                    if prominent {
                        Circle().fill(Theme.accentGradient)
                    } else {
                        Circle().fill(.white.opacity(0.16))
                    }
                }
                .contentShape(Circle())
        }
        .buttonStyle(.plain)
    }
}

extension MediaKind {
    var title: String {
        switch self {
        case .video: return L("Video")
        case .image: return L("Image")
        case .gif: return L("GIF")
        }
    }

    var symbol: String {
        switch self {
        case .video: return "play.rectangle.fill"
        case .image: return "photo.fill"
        case .gif: return "livephoto"
        }
    }
}

extension FillMode {
    var symbol: String {
        switch self {
        case .fill: return "arrow.up.left.and.arrow.down.right"
        case .fit: return "rectangle.inset.filled"
        case .stretch: return "arrow.left.and.right"
        case .center: return "dot.square"
        }
    }
}

// MARK: - Folder icon

struct FolderIconView: View {
    let icon: FolderIcon
    var size: CGFloat = 44
    @ObservedObject var images = Library.shared.images

    var body: some View {
        Group {
            switch icon {
            case .symbol(let name, let palette):
                ZStack {
                    RoundedRectangle(cornerRadius: size * 0.26, style: .continuous)
                        .fill(Theme.folderGradient(palette))
                    Image(systemName: name)
                        .font(.system(size: size * 0.46, weight: .semibold))
                        .foregroundStyle(.white)
                        .shadow(color: .black.opacity(0.18), radius: size * 0.05, y: size * 0.02)
                }
            case .image(let file):
                ZStack {
                    Color.gray.opacity(0.25)
                    if let img = images.folderIcons[file] {
                        Image(nsImage: img)
                            .resizable()
                            .aspectRatio(contentMode: .fill)
                    }
                }
                .clipShape(RoundedRectangle(cornerRadius: size * 0.26, style: .continuous))
                .task(id: file) { await Library.shared.loadFolderImage(file: file) }
            }
        }
        .frame(width: size, height: size)
    }
}

// MARK: - Hex colors

extension Color {
    init(hex: String) {
        var s = hex.trimmingCharacters(in: .whitespacesAndNewlines)
        if s.hasPrefix("#") { s.removeFirst() }
        var value: UInt64 = 0
        Scanner(string: s).scanHexInt64(&value)
        let r, g, b: Double
        if s.count == 6 {
            r = Double((value >> 16) & 0xFF) / 255
            g = Double((value >> 8) & 0xFF) / 255
            b = Double(value & 0xFF) / 255
        } else {
            r = 0.5; g = 0.5; b = 0.5
        }
        self.init(red: r, green: g, blue: b)
    }

    var hexString: String {
        let ns = NSColor(self).usingColorSpace(.sRGB) ?? NSColor.gray
        let r = Int((ns.redComponent * 255).rounded())
        let g = Int((ns.greenComponent * 255).rounded())
        let b = Int((ns.blueComponent * 255).rounded())
        return String(format: "#%02X%02X%02X", max(0, min(255, r)), max(0, min(255, g)), max(0, min(255, b)))
    }
}
