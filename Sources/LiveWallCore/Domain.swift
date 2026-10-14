import Foundation

public enum ItemKind: String, Codable, Sendable, CaseIterable {
    case video
    case gif
    case image
}

public struct ItemConfig: Codable, Sendable, Equatable {
    public var mode: String
    public var volume: Double
    public var speed: Double
    public var muted: Bool
    public var optimized: Bool
    public var stillTime: Double

    public init(
        mode: String = "fill",
        volume: Double = 1.0,
        speed: Double = 1.0,
        muted: Bool = false,
        optimized: Bool = false,
        stillTime: Double = 0
    ) {
        self.mode = mode
        self.volume = volume
        self.speed = speed
        self.muted = muted
        self.optimized = optimized
        self.stillTime = stillTime
    }
}

public struct LibraryItem: Codable, Sendable, Identifiable, Equatable {
    public var id: UUID
    public var kind: ItemKind
    public var path: String
    public var checksum: String?
    public var addedAt: Date
    public var folderID: UUID?
    public var config: ItemConfig
    public var isPlaceholder: Bool

    public init(
        id: UUID = UUID(),
        kind: ItemKind,
        path: String,
        checksum: String? = nil,
        addedAt: Date = Date(),
        folderID: UUID? = nil,
        config: ItemConfig = ItemConfig(),
        isPlaceholder: Bool = false
    ) {
        self.id = id
        self.kind = kind
        self.path = path
        self.checksum = checksum
        self.addedAt = addedAt
        self.folderID = folderID
        self.config = config
        self.isPlaceholder = isPlaceholder
    }
}

public struct Folder: Codable, Sendable, Identifiable, Equatable {
    public var id: UUID
    public var name: String
    public var icon: String?

    public init(id: UUID = UUID(), name: String, icon: String? = nil) {
        self.id = id
        self.name = name
        self.icon = icon
    }
}

public struct Library: Codable, Sendable, Equatable {
    public static let currentVersion = 2

    public var v: Int
    public var items: [LibraryItem]
    public var folders: [Folder]
    public var current: UUID?
    public var previous: UUID?
    public var lockScreen: UUID?

    public init(
        v: Int = Library.currentVersion,
        items: [LibraryItem] = [],
        folders: [Folder] = [],
        current: UUID? = nil,
        previous: UUID? = nil,
        lockScreen: UUID? = nil
    ) {
        self.v = v
        self.items = items
        self.folders = folders
        self.current = current
        self.previous = previous
        self.lockScreen = lockScreen
    }
}

public struct SwapState: Codable, Sendable, Equatable {
    public struct DisplayEntry: Codable, Sendable, Equatable {
        public var displayID: UInt32
        public var originalURL: String

        public init(displayID: UInt32, originalURL: String) {
            self.displayID = displayID
            self.originalURL = originalURL
        }
    }

    public var v: Int
    public var displays: [DisplayEntry]
    public var takenAt: Date

    public init(v: Int = 2, displays: [DisplayEntry] = [], takenAt: Date = Date()) {
        self.v = v
        self.displays = displays
        self.takenAt = takenAt
    }
}
