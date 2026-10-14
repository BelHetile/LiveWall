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

public struct DownloadInvocation: Sendable, Equatable {
    public let url: String
    public let noWait: Bool
    public let consentTools: Bool

    public init(url: String, noWait: Bool, consentTools: Bool) {
        self.url = url
        self.noWait = noWait
        self.consentTools = consentTools
    }
}

public enum DownloadCLIError: Error, Equatable {
    case missingURL
    case multipleURLs
    case unknownFlag(String)
}

public enum DownloadCLI {
    public static let noWaitFlag = "--no-wait"
    public static let consentToolsFlag = "--consent-tools"

    public static func parse(arguments: [String]) throws -> DownloadInvocation {
        var noWait = false
        var consentTools = false
        var url: String?
        for argument in arguments {
            switch argument {
            case noWaitFlag:
                noWait = true
            case consentToolsFlag:
                consentTools = true
            default:
                if argument.hasPrefix("--") {
                    throw DownloadCLIError.unknownFlag(argument)
                }
                if url != nil {
                    throw DownloadCLIError.multipleURLs
                }
                url = argument
            }
        }
        guard let url else {
            throw DownloadCLIError.missingURL
        }
        return DownloadInvocation(url: url, noWait: noWait, consentTools: consentTools)
    }

    public static func isSupportedURL(_ raw: String) -> Bool {
        guard let url = URL(string: raw),
              let scheme = url.scheme?.lowercased(),
              scheme == "http" || scheme == "https",
              let host = url.host,
              !host.isEmpty else {
            return false
        }
        return true
    }

    public static func makeRequest(_ invocation: DownloadInvocation) -> Request {
        return Request(cmd: .download, url: invocation.url, consentTools: invocation.consentTools)
    }

    public static func statusRequest(jobID: String) -> Request {
        return Request(cmd: .downloadStatus, jobID: jobID)
    }

    public static func cancelRequest(jobID: String) -> Request {
        return Request(cmd: .downloadCancel, jobID: jobID)
    }

    public static func exitCode(forState state: String) -> Int32 {
        return state == "done" ? 0 : 1
    }
}
