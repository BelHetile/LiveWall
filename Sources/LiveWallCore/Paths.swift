import Foundation

public enum LiveWallPaths {
    public static func dataDirectory() -> URL {
        let base = FileManager.default
            .urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first ?? URL(fileURLWithPath: NSHomeDirectory())
            .appendingPathComponent("Library/Application Support")
        return base.appendingPathComponent("LiveWall", isDirectory: true)
    }

    public static func toolsDirectory() -> URL {
        return dataDirectory().appendingPathComponent("Tools", isDirectory: true)
    }

    public static func incomingDirectory() -> URL {
        return dataDirectory().appendingPathComponent("Incoming", isDirectory: true)
    }

    public static func thumbsDirectory() -> URL {
        return dataDirectory().appendingPathComponent("Thumbs", isDirectory: true)
    }
}
