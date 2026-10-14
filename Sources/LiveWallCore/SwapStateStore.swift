import Foundation

public actor SwapStateStore {
    public let url: URL

    public init(url: URL) {
        self.url = url
    }

    public static func defaultURL() -> URL {
        let base = FileManager.default
            .urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first ?? URL(fileURLWithPath: NSHomeDirectory())
            .appendingPathComponent("Library/Application Support")
        return base
            .appendingPathComponent("LiveWall", isDirectory: true)
            .appendingPathComponent("swap-state.json")
    }

    public var exists: Bool {
        return FileManager.default.fileExists(atPath: url.path)
    }

    public func load() throws -> SwapState? {
        guard FileManager.default.fileExists(atPath: url.path) else {
            return nil
        }
        do {
            return try AtomicJSON.read(SwapState.self, from: url)
        } catch {
            throw LiveWallError.swapStateCorrupt(path: url.path)
        }
    }

    public func save(_ state: SwapState) throws {
        try AtomicJSON.write(state, to: url)
    }

    public func clear() throws {
        guard FileManager.default.fileExists(atPath: url.path) else {
            return
        }
        try FileManager.default.removeItem(at: url)
    }
}
