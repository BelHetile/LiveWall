import Foundation
import LiveWallIPC

func printUsage() {
    FileHandle.standardError.write(
        Data("""
        usage: lwctl <pause|resume|mute|unmute|next|previous|random|get-current|get-status|open <path>>
               lwctl download [--no-wait] [--consent-tools] <url>

        """.utf8)
    )
}

func pretty(_ value: JSONValue?) -> String {
    guard let value else {
        return "ok"
    }
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.sortedKeys]
    guard let data = try? encoder.encode(value),
          let object = try? JSONSerialization.jsonObject(with: data),
          let prettyData = try? JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys]),
          let text = String(data: prettyData, encoding: .utf8) else {
        return "ok"
    }
    return text
}

func send(_ request: Request) -> Response {
    let client = IPCClient(socketURL: IPCPaths.socketURL(environment: ProcessInfo.processInfo.environment))
    do {
        return try client.send(request)
    } catch IPCClientError.appNotRunning {
        print("app not running")
        exit(2)
    } catch {
        FileHandle.standardError.write(Data("error: \(error)\n".utf8))
        exit(2)
    }
}

func failResponse(_ response: Response) -> Never {
    let code = response.error?.code ?? "EIPC"
    let message = response.error?.message ?? "request failed"
    FileHandle.standardError.write(Data("error: \(code): \(message)\n".utf8))
    exit(1)
}

func jobID(from response: Response) -> String? {
    guard case let .object(object)? = response.data,
          case let .string(value)? = object["jobID"] else {
        return nil
    }
    return value
}

struct JobStatusView {
    var state: String
    var progress: Double
    var title: String?
    var path: String?
    var error: String?
}

func statusView(from response: Response) -> JobStatusView? {
    guard case let .object(object)? = response.data,
          case let .string(state)? = object["state"] else {
        return nil
    }
    var progress = 0.0
    if case let .number(value)? = object["progress"] {
        progress = value
    }
    var title: String?
    if case let .string(value)? = object["title"] {
        title = value
    }
    var path: String?
    if case let .string(value)? = object["path"] {
        path = value
    }
    var error: String?
    if case let .string(value)? = object["error"] {
        error = value
    }
    return JobStatusView(state: state, progress: progress, title: title, path: path, error: error)
}

func runDownload(arguments: [String]) -> Never {
    let invocation: DownloadInvocation
    do {
        invocation = try DownloadCLI.parse(arguments: arguments)
    } catch {
        printUsage()
        exit(64)
    }
    guard DownloadCLI.isSupportedURL(invocation.url) else {
        FileHandle.standardError.write(Data("error: unsupported url: \(invocation.url)\n".utf8))
        exit(64)
    }

    let response = send(DownloadCLI.makeRequest(invocation))
    guard response.ok else {
        failResponse(response)
    }
    guard let jobID = jobID(from: response) else {
        FileHandle.standardError.write(Data("error: malformed job response\n".utf8))
        exit(1)
    }

    print(jobID)
    if invocation.noWait {
        exit(0)
    }

    var lastProgress = -1.0
    var didEmit = false
    while true {
        let statusResponse = send(DownloadCLI.statusRequest(jobID: jobID))
        guard statusResponse.ok else {
            failResponse(statusResponse)
        }
        guard let status = statusView(from: statusResponse) else {
            FileHandle.standardError.write(Data("error: malformed status response\n".utf8))
            exit(1)
        }

        switch status.state {
        case "running":
            if status.progress > lastProgress {
                lastProgress = status.progress
                didEmit = true
                var line = "\(jobID.prefix(8)) \(Int(status.progress.rounded()))%"
                if let title = status.title {
                    line += " \(title)"
                }
                FileHandle.standardError.write(Data("\r\(line)".utf8))
            }
            Thread.sleep(forTimeInterval: 0.1)
        case "done":
            if didEmit {
                FileHandle.standardError.write(Data("\n".utf8))
            }
            if let path = status.path {
                print(path)
            }
            exit(DownloadCLI.exitCode(forState: status.state))
        case "canceled":
            if didEmit {
                FileHandle.standardError.write(Data("\n".utf8))
            }
            FileHandle.standardError.write(Data("canceled\n".utf8))
            exit(DownloadCLI.exitCode(forState: status.state))
        default:
            if didEmit {
                FileHandle.standardError.write(Data("\n".utf8))
            }
            var message = status.error ?? "download failed"
            if message.lowercased().contains("consent") {
                message += " (rerun with --consent-tools)"
            }
            FileHandle.standardError.write(Data("error: \(message)\n".utf8))
            exit(DownloadCLI.exitCode(forState: status.state))
        }
    }
}

let arguments = Array(CommandLine.arguments.dropFirst())

if arguments.first == Verb.download.rawValue {
    runDownload(arguments: Array(arguments.dropFirst()))
}

let request: Request
do {
    request = try IPCCLI.makeRequest(arguments: arguments)
} catch {
    printUsage()
    exit(64)
}

let response = send(request)

if response.ok {
    print(pretty(response.data))
    exit(0)
}

failResponse(response)
