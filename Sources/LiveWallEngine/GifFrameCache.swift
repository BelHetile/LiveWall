import CoreGraphics

@MainActor
final class GifFrameCache {
    typealias Decoder = (Int) -> CGImage?
    typealias ByteCounter = (CGImage) -> Int

    let maxFrames: Int
    let maxBytes: Int

    private let decoder: Decoder
    private let byteCounter: ByteCounter
    private var frames: [Int: CGImage] = [:]
    private var totalBytes = 0
    private(set) var currentIndex = 0

    init(maxFrames: Int, maxBytes: Int, decoder: @escaping Decoder, byteCounter: @escaping ByteCounter = GifFrameCache.decodedByteSize) {
        self.maxFrames = max(1, maxFrames)
        self.maxBytes = max(1, maxBytes)
        self.decoder = decoder
        self.byteCounter = byteCounter
    }

    nonisolated static func decodedByteSize(_ image: CGImage) -> Int {
        return image.bytesPerRow * image.height
    }

    var cachedFrameCount: Int {
        return frames.count
    }

    var cachedBytes: Int {
        return totalBytes
    }

    func image(at index: Int) -> CGImage? {
        currentIndex = index
        if let cached = frames[index] {
            evictFarthest()
            return cached
        }

        guard let image = decoder(index) else {
            evictFarthest()
            return nil
        }

        let size = byteCounter(image)
        if size <= maxBytes {
            frames[index] = image
            totalBytes += size
        }
        evictFarthest()
        return image
    }

    func setCurrent(_ index: Int) {
        currentIndex = index
        evictFarthest()
    }

    private func evictFarthest() {
        while frames.count > maxFrames || totalBytes > maxBytes {
            guard frames.count > 1 else {
                break
            }
            let victim = frames.keys
                .filter { $0 != currentIndex }
                .max { abs($0 - currentIndex) < abs($1 - currentIndex) }
            guard let victim, let removed = frames.removeValue(forKey: victim) else {
                break
            }
            totalBytes -= byteCounter(removed)
        }
    }
}
