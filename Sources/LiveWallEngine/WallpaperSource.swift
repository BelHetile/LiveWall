import Foundation
import LiveWallCore

public struct WallpaperItem: Sendable, Equatable, Identifiable {
    public var id: String
    public var url: URL
    public var kind: ItemKind
    public var config: ItemConfig

    public init(
        id: String = UUID().uuidString,
        url: URL,
        kind: ItemKind,
        config: ItemConfig = ItemConfig()
    ) {
        self.id = id
        self.url = url
        self.kind = kind
        self.config = config
    }

    public static func kind(for url: URL) -> ItemKind {
        switch url.pathExtension.lowercased() {
        case "mp4", "mov", "m4v", "webm", "mkv":
            return .video
        case "gif":
            return .gif
        default:
            return .image
        }
    }
}

public protocol WallpaperSource: Sendable {
    func orderedItems() -> [WallpaperItem]
}

public struct StaticWallpaperSource: WallpaperSource {
    public let items: [WallpaperItem]

    public init(items: [WallpaperItem]) {
        self.items = items
    }

    public init(paths: [String]) {
        self.items = paths.map { path in
            let url = URL(fileURLWithPath: path)
            return WallpaperItem(url: url, kind: WallpaperItem.kind(for: url))
        }
    }

    public func orderedItems() -> [WallpaperItem] {
        return items
    }
}
