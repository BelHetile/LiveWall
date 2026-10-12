import AppKit
import AVFoundation

/// Статичный кадр текущих обоев -> системные обои. Их же macOS показывает на экране блокировки.
/// (Живое видео на экране блокировки macOS сторонним приложениям не позволяет.)
enum LockScreenSync {
    static func stillImage(for item: WallpaperItem, source: URL, dir: URL) async -> URL? {
        switch item.kind {
        case .image:
            return source
        case .gif, .video:
            let out = dir.appendingPathComponent("\(item.id.uuidString).png")
            if FileManager.default.fileExists(atPath: out.path) { return out }

            var cg: CGImage?
            if item.kind == .video {
                let gen = AVAssetImageGenerator(asset: AVURLAsset(url: source))
                gen.appliesPreferredTrackTransform = true
                gen.requestedTimeToleranceBefore = .zero
                gen.requestedTimeToleranceAfter = .zero
                cg = try? await gen.image(at: .zero).image
            } else if let src = CGImageSourceCreateWithURL(source as CFURL, nil) {
                cg = CGImageSourceCreateImageAtIndex(src, 0, nil)
            }
            guard let image = cg else { return nil }

            let rep = NSBitmapImageRep(cgImage: image)
            guard let data = rep.representation(using: .png, properties: [:]) else { return nil }
            do {
                try data.write(to: out, options: .atomic)
            } catch {
                return nil
            }
            return out
        }
    }

    static func apply(url: URL, mode: FillMode) {
        var opts: [NSWorkspace.DesktopImageOptionKey: Any] = [:]
        switch mode {
        case .fill:
            opts[.imageScaling] = NSImageScaling.scaleProportionallyUpOrDown.rawValue
            opts[.allowClipping] = true
        case .fit:
            opts[.imageScaling] = NSImageScaling.scaleProportionallyUpOrDown.rawValue
            opts[.allowClipping] = false
            opts[.fillColor] = NSColor.black
        case .stretch:
            opts[.imageScaling] = NSImageScaling.scaleAxesIndependently.rawValue
            opts[.allowClipping] = true
        case .center:
            opts[.imageScaling] = NSImageScaling.scaleNone.rawValue
            opts[.allowClipping] = true
            opts[.fillColor] = NSColor.black
        }
        for screen in NSScreen.screens {
            do {
                try NSWorkspace.shared.setDesktopImageURL(url, for: screen, options: opts)
            } catch {
                NSLog("LiveWall: setDesktopImageURL failed: \(error)")
            }
        }
    }
}
