import SwiftUI
import AppKit
import AVFoundation

// MARK: - Palette

enum Theme {
    static let violet = Color(red: 0.49, green: 0.30, blue: 0.98)
    static let magenta = Color(red: 0.93, green: 0.33, blue: 0.62)
    static let sunset = Color(red: 1.00, green: 0.66, blue: 0.47)

    static let accentGradient = LinearGradient(colors: [violet, magenta],
                                               startPoint: .topLeading,
                                               endPoint: .bottomTrailing)

    static let heroScrim = LinearGradient(colors: [.clear, .black.opacity(0.25), .black.opacity(0.78)],
                                          startPoint: .top,
                                          endPoint: .bottom)

    /// Цветовые темы для иконок папок.
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
    static let heroRadius: CGFloat = 18
}

// MARK: - AppKit bridges

/// Настоящий «вибрантный» фон macOS (как у сайдбаров системных приложений).
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

/// Живое превью: тот же AVPlayer, что крутит обои на рабочем столе.
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
        case .video: return "Video"
        case .image: return "Image"
        case .gif: return "GIF"
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
                    if let img = Library.shared.folderImage(file: file) {
                        Image(nsImage: img)
                            .resizable()
                            .aspectRatio(contentMode: .fill)
                    }
                }
                .clipShape(RoundedRectangle(cornerRadius: size * 0.26, style: .continuous))
            }
        }
        .frame(width: size, height: size)
    }
}
