import Foundation
import LiveWallCore

public protocol ToolExecutableResolving: Sendable {
    func executableURL(for tool: MediaTool) -> URL?
    func brewExecutableURL() -> URL?
}

public struct ToolLocator: ToolExecutableResolving, Sendable {
    public static let brewExecutableName = "brew"

    public let searchDirectories: [URL]

    public init(searchDirectories: [URL]) {
        self.searchDirectories = searchDirectories
    }

    public static var system: ToolLocator {
        return ToolLocator(searchDirectories: defaultSearchDirectories())
    }

    public static func defaultSearchDirectories(
        appToolsDirectory: URL = LiveWallPaths.toolsDirectory()
    ) -> [URL] {
        return [
            appToolsDirectory,
            URL(fileURLWithPath: "/opt/homebrew/bin", isDirectory: true),
            URL(fileURLWithPath: "/usr/local/bin", isDirectory: true)
        ]
    }

    public func executableURL(for tool: MediaTool) -> URL? {
        return locate(named: tool.executableName)
    }

    public func brewExecutableURL() -> URL? {
        return locate(named: ToolLocator.brewExecutableName)
    }

    public func locate(named name: String) -> URL? {
        let fileManager = FileManager.default
        for directory in searchDirectories {
            let candidate = directory.appendingPathComponent(name)
            if fileManager.isExecutableFile(atPath: candidate.path) {
                return candidate
            }
        }
        return nil
    }
}
