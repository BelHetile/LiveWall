import Darwin
import Foundation

public enum IPCPaths {
    public static func defaultDataDirectory() -> URL {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? FileManager.default.homeDirectoryForCurrentUser
                .appendingPathComponent("Library/Application Support", isDirectory: true)
        return base.appendingPathComponent("LiveWall", isDirectory: true)
    }

    public static func defaultSocketURL() -> URL {
        return defaultDataDirectory().appendingPathComponent(IPCProtocol.socketFileName)
    }

    public static func socketURL(environment: [String: String]) -> URL {
        if let override = environment["LIVEWALL_CONTROL_SOCKET"], !override.isEmpty {
            return URL(fileURLWithPath: override)
        }
        return defaultSocketURL()
    }
}

public enum IPCCLIError: Error, Equatable {
    case missingCommand
    case unknownCommand(String)
    case missingPath
}

public enum IPCCLI {
    public static func makeRequest(arguments: [String]) throws -> Request {
        guard let command = arguments.first else {
            throw IPCCLIError.missingCommand
        }
        guard let verb = Verb(rawValue: command) else {
            throw IPCCLIError.unknownCommand(command)
        }
        var path: String?
        if verb == .open {
            guard arguments.count >= 2 else {
                throw IPCCLIError.missingPath
            }
            path = arguments[1]
        }
        return Request(cmd: verb, path: path)
    }
}
