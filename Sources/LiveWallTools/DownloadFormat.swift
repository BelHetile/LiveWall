import Foundation
#if canImport(VideoToolbox)
import VideoToolbox
#endif

public enum AV1HardwareSupport {
    public static let av1CodecType: UInt32 = 0x61763031

    public static func isSupported() -> Bool {
        #if canImport(VideoToolbox)
        return VTIsHardwareDecodeSupported(av1CodecType)
        #else
        return false
        #endif
    }
}

public enum DownloadFormatSelector {
    public static let minimumHeight = 720
    public static let maximumHeight = 2160
    public static let h264Ceiling = 1080

    public static func clampedHeight(_ maxHeight: Int) -> Int {
        return min(maximumHeight, max(minimumHeight, maxHeight))
    }

    public static func preferredCodec(maxHeight: Int, hwAV1: Bool) -> String {
        let cap = clampedHeight(maxHeight)
        if cap <= h264Ceiling {
            return "avc1"
        }
        return hwAV1 ? "av01" : "vp9"
    }

    public static func formatExpression(maxHeight: Int, hwAV1: Bool) -> String {
        let cap = clampedHeight(maxHeight)
        let codec = preferredCodec(maxHeight: maxHeight, hwAV1: hwAV1)
        return "bv[height<=\(cap)][vcodec^=\(codec)]+ba/bv[height<=\(cap)]+ba/b[height<=\(cap)]/b"
    }

    public static func videoExpression(maxHeight: Int, hwAV1: Bool) -> String {
        let cap = clampedHeight(maxHeight)
        let codec = preferredCodec(maxHeight: maxHeight, hwAV1: hwAV1)
        return "bv[height<=\(cap)][vcodec^=\(codec)]/bv[height<=\(cap)]"
    }

    public static func audioExpression() -> String {
        return "ba"
    }

    public static func singleExpression(maxHeight: Int) -> String {
        let cap = clampedHeight(maxHeight)
        return "b[height<=\(cap)]/b"
    }
}
