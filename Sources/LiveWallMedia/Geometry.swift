import CoreGraphics

public enum MediaGeometry {
    public static func aspectRatio(_ size: CGSize) -> CGFloat {
        guard size.height > 0 else {
            return 0
        }
        return size.width / size.height
    }

    public static func aspectFitRect(source: CGSize, in bounds: CGRect) -> CGRect {
        guard isUsable(source) else {
            return bounds
        }
        let scale = min(bounds.width / source.width, bounds.height / source.height)
        return centeredRect(of: CGSize(width: source.width * scale, height: source.height * scale), in: bounds)
    }

    public static func aspectFillRect(source: CGSize, in bounds: CGRect) -> CGRect {
        guard isUsable(source) else {
            return bounds
        }
        let scale = max(bounds.width / source.width, bounds.height / source.height)
        return centeredRect(of: CGSize(width: source.width * scale, height: source.height * scale), in: bounds)
    }

    public static func stretchRect(source: CGSize, in bounds: CGRect) -> CGRect {
        return bounds
    }

    public static func centerRect(source: CGSize, in bounds: CGRect) -> CGRect {
        return centeredRect(of: source, in: bounds)
    }

    public static func rect(for mode: FillMode, source: CGSize, in bounds: CGRect) -> CGRect {
        switch mode {
        case .fill:
            return aspectFillRect(source: source, in: bounds)
        case .fit:
            return aspectFitRect(source: source, in: bounds)
        case .stretch:
            return stretchRect(source: source, in: bounds)
        case .center:
            return centerRect(source: source, in: bounds)
        }
    }

    private static func isUsable(_ size: CGSize) -> Bool {
        return size.width > 0 && size.height > 0
    }

    private static func centeredRect(of size: CGSize, in bounds: CGRect) -> CGRect {
        let x = bounds.minX + (bounds.width - size.width) / 2
        let y = bounds.minY + (bounds.height - size.height) / 2
        return CGRect(x: x, y: y, width: size.width, height: size.height)
    }
}
