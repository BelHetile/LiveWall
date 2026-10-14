import CoreGraphics
import ImageIO

public struct GifTimeline: Equatable, Sendable {
    public static let defaultFrameDelay: Double = 0.1

    public let frameDelays: [Double]
    public let loopCount: Int

    public init(frameDelays: [Double], loopCount: Int) {
        self.frameDelays = frameDelays
        self.loopCount = loopCount
    }

    public var frameCount: Int {
        return frameDelays.count
    }

    public var totalDuration: Double {
        return frameDelays.reduce(0, +)
    }

    public func delay(at index: Int) -> Double {
        guard index >= 0, index < frameDelays.count else {
            return Self.defaultFrameDelay
        }
        return frameDelays[index]
    }

    public static func read(from source: CGImageSource) -> GifTimeline {
        let count = CGImageSourceGetCount(source)
        var delays: [Double] = []
        delays.reserveCapacity(count)
        for index in 0..<count {
            delays.append(frameDelay(source: source, index: index))
        }
        return GifTimeline(frameDelays: delays, loopCount: loopCount(source: source))
    }

    public static func frameDelay(source: CGImageSource, index: Int) -> Double {
        guard let dictionary = gifDictionary(source: source, index: index) else {
            return defaultFrameDelay
        }
        if let unclamped = dictionary[kCGImagePropertyGIFUnclampedDelayTime] as? Double, unclamped > 0 {
            return unclamped
        }
        if let clamped = dictionary[kCGImagePropertyGIFDelayTime] as? Double, clamped > 0 {
            return clamped
        }
        return defaultFrameDelay
    }

    public static func loopCount(source: CGImageSource) -> Int {
        guard let dictionary = gifDictionary(source: source, index: 0),
              let count = dictionary[kCGImagePropertyGIFLoopCount] as? Int else {
            return 0
        }
        return count
    }

    private static func gifDictionary(source: CGImageSource, index: Int) -> [CFString: Any]? {
        guard let properties = CGImageSourceCopyPropertiesAtIndex(source, index, nil) as? [CFString: Any] else {
            return nil
        }
        return properties[kCGImagePropertyGIFDictionary] as? [CFString: Any]
    }
}
