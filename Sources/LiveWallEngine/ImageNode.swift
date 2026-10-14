import AppKit
import CoreGraphics
import ImageIO
import LiveWallCore

@MainActor
public final class ImageNode: PlaybackNode {
    public let image: CGImage
    public let view: NSView

    public init(url: URL) throws {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
              CGImageSourceGetCount(source) > 0,
              let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
            throw LiveWallError.mediaUnreadable(path: url.path, reason: "unreadable image")
        }

        let view = NSView(frame: .zero)
        view.wantsLayer = true
        view.layer?.contents = image
        view.layer?.contentsGravity = .resizeAspectFill

        self.image = image
        self.view = view
    }

    public var isPlaying: Bool {
        return false
    }

    public func play() {}

    public func pause() {}
}
