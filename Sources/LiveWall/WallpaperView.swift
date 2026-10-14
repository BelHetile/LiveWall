import AppKit
import AVFoundation
import QuartzCore

/// A view that draws one wallpaper (video / image / GIF) with its fill mode
/// and crossfades smoothly when it changes.
final class WallpaperView: NSView {
    private var stack: [CALayer] = []
    private var readyObservation: NSKeyValueObservation?

    var mode: FillMode = .fill {
        didSet { relayout() }
    }
    var videoSize: CGSize = .zero {
        didSet { relayout() }
    }

    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        wantsLayer = true
        layer?.backgroundColor = NSColor.black.cgColor
        layer?.masksToBounds = true
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func layout() {
        super.layout()
        relayout()
    }

    // MARK: Presenting

    func clear() {
        readyObservation = nil
        for l in stack { l.removeFromSuperlayer() }
        stack.removeAll()
    }

    func present(kind: MediaKind, url: URL, player: AVPlayer?, mode: FillMode, videoSize: CGSize, animated: Bool) {
        self.mode = mode
        self.videoSize = videoSize

        let newLayer: CALayer
        switch kind {
        case .video:
            newLayer = AVPlayerLayer(player: player)
        case .image:
            let l = CALayer()
            if let img = NSImage(contentsOf: url),
               let cg = img.cgImage(forProposedRect: nil, context: nil, hints: nil) {
                l.contents = cg
            }
            newLayer = l
        case .gif:
            newLayer = GIFLayer(url: url)
        }

        configure(newLayer)
        newLayer.opacity = animated ? 0 : 1
        layer?.addSublayer(newLayer)
        stack.append(newLayer)
        readyObservation = nil

        let reveal: () -> Void = { [weak self, weak newLayer] in
            guard let self = self, let newLayer = newLayer else { return }
            guard let idx = self.stack.firstIndex(where: { $0 === newLayer }) else { return }
            let stale = Array(self.stack[0..<idx])
            CATransaction.begin()
            CATransaction.setAnimationDuration(animated ? 0.45 : 0)
            CATransaction.setCompletionBlock { [weak self] in
                for l in stale { l.removeFromSuperlayer() }
                self?.stack.removeAll(where: { layer in stale.contains(where: { $0 === layer }) })
            }
            newLayer.opacity = 1
            CATransaction.commit()
        }

        if !animated {
            // no animation: remove everything old right away
            let stale = stack.dropLast()
            for l in stale { l.removeFromSuperlayer() }
            stack = [newLayer]
            return
        }

        if let pl = newLayer as? AVPlayerLayer {
            // wait for the first frame to avoid a black flash
            var fired = false
            readyObservation = pl.observe(\.isReadyForDisplay, options: [.initial, .new]) { layer, _ in
                guard layer.isReadyForDisplay, !fired else { return }
                fired = true
                DispatchQueue.main.async { reveal() }
            }
        } else {
            reveal()
        }
    }

    /// The video is unloaded: its frame replaces the player layer, without animation.
    func showStill(_ image: CGImage, mode: FillMode, videoSize: CGSize) {
        readyObservation = nil
        self.mode = mode
        self.videoSize = videoSize
        let l = CALayer()
        l.contents = image
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        configure(l)
        layer?.addSublayer(l)
        for old in stack { old.removeFromSuperlayer() }
        stack = [l]
        CATransaction.commit()
    }

    /// Pauses GIF animation without losing its position.
    func setAnimationPaused(_ paused: Bool) {
        for case let l as GIFLayer in stack {
            l.setPaused(paused)
        }
    }

    // MARK: Layout

    private func relayout() {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        for l in stack { configure(l) }
        CATransaction.commit()
    }

    private func configure(_ l: CALayer) {
        let b = bounds
        let scale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 2

        if let pl = l as? AVPlayerLayer {
            switch mode {
            case .fill:
                pl.videoGravity = .resizeAspectFill
                pl.frame = b
            case .fit:
                pl.videoGravity = .resizeAspect
                pl.frame = b
            case .stretch:
                pl.videoGravity = .resize
                pl.frame = b
            case .center:
                if videoSize.width > 0, videoSize.height > 0 {
                    let w = videoSize.width / scale
                    let h = videoSize.height / scale
                    pl.videoGravity = .resize
                    pl.frame = CGRect(x: (b.width - w) / 2, y: (b.height - h) / 2, width: w, height: h)
                } else {
                    pl.videoGravity = .resizeAspect
                    pl.frame = b
                }
            }
        } else {
            l.frame = b
            l.contentsScale = scale
            switch mode {
            case .fill: l.contentsGravity = .resizeAspectFill
            case .fit: l.contentsGravity = .resizeAspect
            case .stretch: l.contentsGravity = .resize
            case .center: l.contentsGravity = .center
            }
        }
    }
}

// MARK: - GIF

/// An animated GIF. While paused the animation is removed entirely (the render server redraws nothing),
/// the current frame stays on the layer, and on resume the animation continues from the same spot.
final class GIFLayer: CALayer {
    private var frames: [CGImage] = []
    private var keyTimes: [NSNumber] = []
    private var total: Double = 0
    private var startedAt: CFTimeInterval = 0   // layer-local time at which the phase was 0
    private var pausedPhase: Double?
    private var wantsPaused = false
    private var animationActive = false
    private var cacheKey: GIFCache.Key?

    private static let maxPixelSize: CGFloat = {
        let screens = NSScreen.screens
        let longest = screens.map { max($0.frame.width, $0.frame.height) }.max() ?? 2560
        let scale = screens.map { $0.backingScaleFactor }.max() ?? 2
        return longest * scale
    }()

    init(url: URL) {
        super.init()
        guard let src = CGImageSourceCreateWithURL(url as CFURL, nil),
              let first = CGImageSourceCreateImageAtIndex(src, 0, [kCGImageSourceShouldCacheImmediately: true] as CFDictionary) else { return }
        contents = first
        cacheKey = GIFCache.shared.load(url: url, maxPixelSize: GIFLayer.maxPixelSize) { [weak self] decoded in
            self?.apply(decoded)
        }
    }

    override init(layer: Any) {
        super.init(layer: layer)
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    deinit {
        GIFCache.shared.release(cacheKey)
    }

    private var isAnimated: Bool { frames.count > 1 && total > 0 }

    private func apply(_ decoded: GIFDecoded?) {
        guard let decoded = decoded, !decoded.frames.isEmpty else { return }
        frames = decoded.frames
        keyTimes = decoded.keyTimes
        total = decoded.total
        if let first = frames.first { contents = first }
        if !wantsPaused { startAnimation(phase: 0) }
    }

    private func startAnimation(phase: Double) {
        guard isAnimated else { return }
        let anim = CAKeyframeAnimation(keyPath: "contents")
        anim.values = frames
        anim.keyTimes = keyTimes
        anim.duration = total
        anim.calculationMode = .discrete
        anim.repeatCount = .infinity
        anim.isRemovedOnCompletion = false
        let now = convertTime(CACurrentMediaTime(), from: nil)
        anim.beginTime = now - phase
        startedAt = now - phase
        add(anim, forKey: "gif")
        animationActive = true
    }

    /// Frame index for an animation phase (seconds from the start of the loop).
    private func frameIndex(at phase: Double) -> Int {
        let fraction = phase / total
        var index = 0
        for (i, k) in keyTimes.enumerated() where k.doubleValue <= fraction { index = i }
        return min(index, frames.count - 1)
    }

    func setPaused(_ paused: Bool) {
        if paused {
            guard !wantsPaused else { return }
            wantsPaused = true
            guard isAnimated, animationActive else { return }
            let now = convertTime(CACurrentMediaTime(), from: nil)
            let phase = (now - startedAt).truncatingRemainder(dividingBy: total)
            pausedPhase = max(0, phase)
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            removeAnimation(forKey: "gif")
            animationActive = false
            contents = frames[frameIndex(at: max(0, phase))]
            CATransaction.commit()
        } else {
            guard wantsPaused else { return }
            wantsPaused = false
            guard isAnimated else { return }
            if let phase = pausedPhase {
                pausedPhase = nil
                startAnimation(phase: phase)
            } else if !animationActive {
                startAnimation(phase: 0)
            }
        }
    }
}

final class GIFDecoded {
    let frames: [CGImage]
    let keyTimes: [NSNumber]
    let total: Double
    let bytes: Int

    init(frames: [CGImage], keyTimes: [NSNumber], total: Double, bytes: Int) {
        self.frames = frames
        self.keyTimes = keyTimes
        self.total = total
        self.bytes = bytes
    }
}

enum GIFDecoder {
    static func decode(url: URL, maxPixelSize: CGFloat) -> GIFDecoded? {
        guard let src = CGImageSourceCreateWithURL(url as CFURL, nil) else { return nil }
        let count = CGImageSourceGetCount(src)
        guard count > 0 else { return nil }

        var options: [CFString: Any] = [kCGImageSourceShouldCacheImmediately: true]
        let bytes = estimatedBytes(src: src, count: count)
        var downsampled = false
        if bytes > GIFCache.memoryLimit, maxPixelSize > 0 {
            options[kCGImageSourceCreateThumbnailFromImageAlways] = true
            options[kCGImageSourceThumbnailMaxPixelSize] = Int(maxPixelSize.rounded())
            downsampled = true
        }

        var frames: [CGImage] = []
        var delays: [Double] = []
        frames.reserveCapacity(count)
        delays.reserveCapacity(count)
        for i in 0..<count {
            let image: CGImage?
            if downsampled {
                image = CGImageSourceCreateThumbnailAtIndex(src, i, options as CFDictionary)
            } else {
                image = CGImageSourceCreateImageAtIndex(src, i, options as CFDictionary)
            }
            guard let image = image else { continue }
            frames.append(image)
            var d = 0.1
            if let props = CGImageSourceCopyPropertiesAtIndex(src, i, nil) as? [CFString: Any],
               let g = props[kCGImagePropertyGIFDictionary] as? [CFString: Any] {
                d = (g[kCGImagePropertyGIFUnclampedDelayTime] as? Double)
                    ?? (g[kCGImagePropertyGIFDelayTime] as? Double)
                    ?? 0.1
                if d < 0.02 { d = 0.1 }
            }
            delays.append(d)
        }
        guard !frames.isEmpty else { return nil }

        let total = delays.reduce(0, +)
        var keyTimes: [NSNumber] = []
        keyTimes.reserveCapacity(delays.count + 1)
        var acc = 0.0
        for d in delays {
            keyTimes.append(NSNumber(value: acc / total))
            acc += d
        }
        keyTimes.append(1)

        return GIFDecoded(frames: frames, keyTimes: keyTimes, total: total, bytes: bytes)
    }

    private static func estimatedBytes(src: CGImageSource, count: Int) -> Int {
        guard let props = CGImageSourceCopyPropertiesAtIndex(src, 0, nil) as? [CFString: Any] else { return 0 }
        let w = (props[kCGImagePropertyPixelWidth] as? NSNumber)?.intValue ?? 0
        let h = (props[kCGImagePropertyPixelHeight] as? NSNumber)?.intValue ?? 0
        return w * h * 4 * count
    }
}

final class GIFCache {
    static let shared = GIFCache()
    static let memoryLimit = 512 * 1024 * 1024

    struct Key: Hashable {
        let path: String
        let modified: TimeInterval
        let size: Int
    }

    private final class Entry {
        var decoded: GIFDecoded?
        var done = false
        var waiters: [(GIFDecoded?) -> Void] = []
        var refs = 0
    }

    private let lock = NSLock()
    private let queue = DispatchQueue(label: "app.livewall.gif.decode", qos: .userInitiated)
    private var entries: [Key: Entry] = [:]

    private init() {}

    func load(url: URL, maxPixelSize: CGFloat, completion: @escaping (GIFDecoded?) -> Void) -> Key {
        let key = GIFCache.key(for: url)
        var ready: GIFDecoded?
        var finished = false
        var start = false
        lock.lock()
        if let entry = entries[key] {
            entry.refs += 1
            if entry.done {
                ready = entry.decoded
                finished = true
            } else {
                entry.waiters.append(completion)
            }
        } else {
            let entry = Entry()
            entry.refs = 1
            entry.waiters.append(completion)
            entries[key] = entry
            start = true
        }
        lock.unlock()

        if finished {
            DispatchQueue.main.async { completion(ready) }
        } else if start {
            queue.async { [weak self] in
                let decoded = GIFDecoder.decode(url: url, maxPixelSize: maxPixelSize)
                self?.finish(key: key, decoded: decoded)
            }
        }
        return key
    }

    func release(_ key: Key?) {
        guard let key = key else { return }
        lock.lock()
        if let entry = entries[key] {
            entry.refs -= 1
            if entry.refs <= 0 { entries[key] = nil }
        }
        lock.unlock()
    }

    private func finish(key: Key, decoded: GIFDecoded?) {
        lock.lock()
        guard let entry = entries[key] else {
            lock.unlock()
            return
        }
        entry.decoded = decoded
        entry.done = true
        let waiters = entry.waiters
        entry.waiters = []
        lock.unlock()
        DispatchQueue.main.async {
            for waiter in waiters { waiter(decoded) }
        }
    }

    private static func key(for url: URL) -> Key {
        let attributes = try? FileManager.default.attributesOfItem(atPath: url.path)
        let modified = (attributes?[.modificationDate] as? Date)?.timeIntervalSince1970 ?? 0
        let size = (attributes?[.size] as? NSNumber)?.intValue ?? 0
        return Key(path: url.path, modified: modified, size: size)
    }
}
