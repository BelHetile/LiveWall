import AppKit
import CoreGraphics
import ImageIO
import LiveWallCore

@MainActor
public final class GifNode: PlaybackNode {
    public let timeline: GifTimeline
    public let view: NSView

    private let source: CGImageSource
    private let cache: GifFrameCache
    private var index = 0
    private var timer: Timer?

    public init(url: URL, maxCacheFrames: Int = 120, maxCacheBytes: Int = 64 * 1024 * 1024) throws {
        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else {
            throw LiveWallError.mediaUnreadable(path: url.path, reason: "unreadable gif")
        }
        let count = CGImageSourceGetCount(source)
        guard count > 0 else {
            throw LiveWallError.mediaUnreadable(path: url.path, reason: "no frames")
        }

        let view = NSView(frame: .zero)
        view.wantsLayer = true
        view.layer?.contentsGravity = .resizeAspectFill

        self.source = source
        self.timeline = GifTimeline.read(from: source)
        self.cache = GifFrameCache(maxFrames: maxCacheFrames, maxBytes: maxCacheBytes) { index in
            guard index >= 0, index < count else {
                return nil
            }
            return CGImageSourceCreateImageAtIndex(source, index, nil)
        }
        self.view = view

        showFrame(0)
    }

    public var isPlaying: Bool {
        return timer != nil
    }

    public var currentFrameIndex: Int {
        return index
    }

    public func play() {
        guard timer == nil, timeline.frameCount > 0 else {
            return
        }
        scheduleNext()
    }

    public func pause() {
        timer?.invalidate()
        timer = nil
    }

    private func showFrame(_ frame: Int) {
        index = frame
        guard let image = cache.image(at: frame) else {
            return
        }
        view.layer?.contents = image
    }

    private func scheduleNext() {
        let interval = max(timeline.delay(at: index), 0.02)
        let timer = Timer(timeInterval: interval, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.advance()
            }
        }
        RunLoop.main.add(timer, forMode: .common)
        self.timer = timer
    }

    private func advance() {
        timer = nil
        guard timeline.frameCount > 0 else {
            return
        }
        showFrame((index + 1) % timeline.frameCount)
        scheduleNext()
    }

    isolated deinit {
        timer?.invalidate()
    }
}
