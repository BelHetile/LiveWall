import AppKit
import AVFoundation

/// A frame of the current wallpaper -> the macOS wallpaper. macOS shows it on the lock screen
/// and wherever LiveWall's windows aren't visible. (macOS doesn't allow third-party live video on the lock screen.)
enum LockScreenSync {
    private static let originalKey = "originalDesktop"

    private static let ownRoot: String = {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return base.appendingPathComponent("LiveWall", isDirectory: true).path
    }()

    static func stillImage(for item: WallpaperItem, source: URL, dir: URL) async -> URL? {
        switch item.kind {
        case .image:
            return source
        case .gif, .video:
            let time = item.kind == .video ? max(0, item.stillTime ?? 0) : 0
            let prefix = item.id.uuidString
            let name = time > 0 ? "\(prefix)-\(Int((time * 1000).rounded())).png" : "\(prefix).png"
            let out = dir.appendingPathComponent(name)
            if FileManager.default.fileExists(atPath: out.path) { return out }

            var cg: CGImage?
            if item.kind == .video {
                let gen = AVAssetImageGenerator(asset: AVURLAsset(url: source))
                gen.appliesPreferredTrackTransform = true
                gen.requestedTimeToleranceBefore = .zero
                gen.requestedTimeToleranceAfter = .zero
                cg = try? await gen.image(at: CMTime(seconds: time, preferredTimescale: 600)).image
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
            // older frames of this wallpaper are no longer needed
            if let files = try? FileManager.default.contentsOfDirectory(atPath: dir.path) {
                for f in files where f.hasPrefix(prefix) && f != name {
                    try? FileManager.default.removeItem(at: dir.appendingPathComponent(f))
                }
            }
            return out
        }
    }

    static func apply(url: URL, mode: FillMode) {
        saveOriginalIfNeeded()
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
            // this frame is already set, so leave the system wallpaper alone
            let wantScaling = Int((opts[.imageScaling] as? UInt) ?? 0)
            let haveScaling = (NSWorkspace.shared.desktopImageOptions(for: screen)?[.imageScaling] as? NSNumber)?.intValue
            if NSWorkspace.shared.desktopImageURL(for: screen)?.standardizedFileURL == url.standardizedFileURL,
               haveScaling == wantScaling {
                continue
            }
            do {
                try NSWorkspace.shared.setDesktopImageURL(url, for: screen, options: opts)
            } catch {
                NSLog("LiveWall: setDesktopImageURL failed: \(error)")
            }
        }
    }

    // MARK: Previous macOS wallpaper

    private static func key(for screen: NSScreen) -> String {
        if let number = screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber,
           let uuid = CGDisplayCreateUUIDFromDisplayID(number.uint32Value)?.takeRetainedValue(),
           let text = CFUUIDCreateString(nil, uuid) as String? {
            return text
        }
        return screen.localizedName
    }

    /// Remembers the wallpaper set before LiveWall (once, until it is restored).
    static func saveOriginalIfNeeded() {
        let defaults = UserDefaults.standard
        guard defaults.dictionary(forKey: originalKey) == nil else { return }
        var saved: [String: [String: Any]] = [:]
        for screen in NSScreen.screens {
            guard let url = NSWorkspace.shared.desktopImageURL(for: screen),
                  !url.path.hasPrefix(ownRoot) else { continue }
            var entry: [String: Any] = ["url": url.absoluteString]
            let opts = NSWorkspace.shared.desktopImageOptions(for: screen) ?? [:]
            if let scaling = opts[.imageScaling] as? NSNumber { entry["scaling"] = scaling.intValue }
            if let clipping = opts[.allowClipping] as? NSNumber { entry["clipping"] = clipping.boolValue }
            if let color = (opts[.fillColor] as? NSColor)?.usingColorSpace(.sRGB) {
                entry["color"] = [color.redComponent, color.greenComponent, color.blueComponent].map { Double($0) }
            }
            saved[key(for: screen)] = entry
        }
        if !saved.isEmpty { defaults.set(saved, forKey: originalKey) }
    }

    static var hasOriginal: Bool {
        UserDefaults.standard.dictionary(forKey: originalKey) != nil
    }

    /// Restores the wallpaper set before LiveWall. false if it wasn't remembered (set before this version).
    @discardableResult
    static func restoreOriginal() -> Bool {
        guard let saved = UserDefaults.standard.dictionary(forKey: originalKey) as? [String: [String: Any]],
              !saved.isEmpty else { return false }
        var restored = false
        for screen in NSScreen.screens {
            guard let entry = saved[key(for: screen)] ?? saved.values.first,
                  let text = entry["url"] as? String, let url = URL(string: text),
                  FileManager.default.fileExists(atPath: url.path) else { continue }
            var opts: [NSWorkspace.DesktopImageOptionKey: Any] = [:]
            if let scaling = entry["scaling"] as? Int { opts[.imageScaling] = scaling }
            if let clipping = entry["clipping"] as? Bool { opts[.allowClipping] = clipping }
            if let c = entry["color"] as? [Double], c.count == 3 {
                opts[.fillColor] = NSColor(srgbRed: c[0], green: c[1], blue: c[2], alpha: 1)
            }
            if (try? NSWorkspace.shared.setDesktopImageURL(url, for: screen, options: opts)) != nil { restored = true }
        }
        UserDefaults.standard.removeObject(forKey: originalKey)
        return restored
    }

    static func openWallpaperSettings() {
        if let url = URL(string: "x-apple.systempreferences:com.apple.Wallpaper-Settings.extension") {
            NSWorkspace.shared.open(url)
        }
    }
}
