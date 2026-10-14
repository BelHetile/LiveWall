import AppKit
import LiveWallCore

@MainActor
public protocol MuteControllable: AnyObject {
    var isMuted: Bool { get set }
}

extension VideoNode: MuteControllable {}

@MainActor
public protocol PlaybackNodeFactory: AnyObject {
    func makeNode(for item: WallpaperItem, muted: Bool) async throws -> PlaybackNode
}

@MainActor
public final class DefaultPlaybackNodeFactory: PlaybackNodeFactory {
    public init() {}

    public func makeNode(for item: WallpaperItem, muted: Bool) async throws -> PlaybackNode {
        switch item.kind {
        case .video:
            return try await VideoNode(
                url: item.url,
                muted: muted,
                volume: Float(item.config.volume),
                rate: Float(item.config.speed)
            )
        case .gif:
            return try GifNode(url: item.url)
        case .image:
            return try ImageNode(url: item.url)
        }
    }
}
