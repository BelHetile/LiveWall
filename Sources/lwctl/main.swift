import Foundation
import LiveWallIPC

func printUsage() {
    FileHandle.standardError.write(
        Data("usage: lwctl <pause|resume|mute|unmute|next|previous|random|get-current|get-status|open <path>>\n".utf8)
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

let request: Request
do {
    request = try IPCCLI.makeRequest(arguments: Array(CommandLine.arguments.dropFirst()))
} catch {
    printUsage()
    exit(64)
}

let client = IPCClient(socketURL: IPCPaths.socketURL(environment: ProcessInfo.processInfo.environment))

let response: Response
do {
    response = try client.send(request)
} catch IPCClientError.appNotRunning {
    print("app not running")
    exit(2)
} catch {
    FileHandle.standardError.write(Data("error: \(error)\n".utf8))
    exit(2)
}

if response.ok {
    print(pretty(response.data))
    exit(0)
}

let code = response.error?.code ?? "EIPC"
let message = response.error?.message ?? "request failed"
FileHandle.standardError.write(Data("error: \(code): \(message)\n".utf8))
exit(1)
