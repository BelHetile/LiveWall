import AppKit
import AVFoundation
import QuartzCore

/// Вид, который рисует одни обои (видео / картинка / GIF) с нужным режимом заполнения
/// и плавно перекрёстно затухает при смене.
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

    // MARK: Показ

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
            newLayer = GIFLayer.make(url: url)
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
            // без анимации — сразу убираем всё старое
            let stale = stack.dropLast()
            for l in stale { l.removeFromSuperlayer() }
            stack = [newLayer]
            return
        }

        if let pl = newLayer as? AVPlayerLayer {
            // ждём первого кадра, чтобы не мигать чёрным
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

    /// Пауза анимации GIF без потери позиции.
    func setAnimationPaused(_ paused: Bool) {
        for l in stack where !(l is AVPlayerLayer) {
            if paused, l.speed != 0 {
                let t = l.convertTime(CACurrentMediaTime(), from: nil)
                l.speed = 0
                l.timeOffset = t
            } else if !paused, l.speed == 0 {
                let pausedAt = l.timeOffset
                l.speed = 1
                l.timeOffset = 0
                l.beginTime = 0
                l.beginTime = l.convertTime(CACurrentMediaTime(), from: nil) - pausedAt
            }
        }
    }

    // MARK: Раскладка

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

enum GIFLayer {
    static func make(url: URL) -> CALayer {
        let layer = CALayer()
        guard let src = CGImageSourceCreateWithURL(url as CFURL, nil) else { return layer }

        var frames: [CGImage] = []
        var delays: [Double] = []
        let count = CGImageSourceGetCount(src)
        for i in 0..<count {
            guard let img = CGImageSourceCreateImageAtIndex(src, i, nil) else { continue }
            frames.append(img)
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
        guard let first = frames.first else { return layer }
        layer.contents = first

        if frames.count > 1 {
            let total = delays.reduce(0, +)
            var keyTimes: [NSNumber] = []
            var acc = 0.0
            for d in delays {
                keyTimes.append(NSNumber(value: acc / total))
                acc += d
            }
            let anim = CAKeyframeAnimation(keyPath: "contents")
            anim.values = frames
            anim.keyTimes = keyTimes
            anim.duration = total
            anim.calculationMode = .discrete
            anim.repeatCount = .infinity
            anim.isRemovedOnCompletion = false
            layer.add(anim, forKey: "gif")
        }
        return layer
    }
}
