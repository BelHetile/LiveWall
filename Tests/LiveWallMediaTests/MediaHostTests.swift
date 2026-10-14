import Foundation
import Testing
import LiveWallCore
import LiveWallIPC
import LiveWallMedia
import LiveWallTools

private func makeTempDir(_ label: String) -> URL {
    let token = String(UUID().uuidString.prefix(8))
    let url = FileManager.default.temporaryDirectory
        .appendingPathComponent("lw-\(label.prefix(4))-\(token)", isDirectory: true)
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

@discardableResult
private func writeExecutable(_ script: String, named name: String, in directory: URL, executable: Bool = true) -> URL {
    let url = directory.appendingPathComponent(name)
    try? script.write(to: url, atomically: true, encoding: .utf8)
    try? FileManager.default.setAttributes([.posixPermissions: executable ? 0o755 : 0o644], ofItemAtPath: url.path)
    return url
}

private func singleRouteStub(title: String) -> String {
    return """
    #!/bin/sh
    fmt=""
    out=""
    while [ $# -gt 0 ]; do
      case "$1" in
        -f) fmt="$2"; shift 2 ;;
        -o) out="$2"; shift 2 ;;
        *) shift ;;
      esac
    done
    printf 'LWTITLE:\(title)\\n'
    printf 'LWPROG: 20.0%%:5: 1.0MiB/s:%s\\n' "$fmt"
    printf 'LWPROG: 60.0%%:2: 1.0MiB/s:%s\\n' "$fmt"
    case "$fmt" in
      bv*) printf 'ERROR: Requested format is not available\\n' >&2; exit 1 ;;
    esac
    dir=$(dirname "$out")
    mkdir -p "$dir"
    sleep 0.2
    : > "$dir/out.mp4"
    printf 'LWPROG:100.0%%:0: 1.0MiB/s:%s\\n' "$fmt"
    exit 0
    """
}

private func cancelStub() -> String {
    return """
    #!/bin/sh
    out=""
    while [ $# -gt 0 ]; do
      case "$1" in
        -o) out="$2"; shift 2 ;;
        *) shift ;;
      esac
    done
    dir=$(dirname "$out")
    mkdir -p "$dir"
    : > "$dir/started"
    exec sleep 30
    """
}

private func brewStub(chmodTarget: URL) -> String {
    return """
    #!/bin/sh
    chmod 755 "\(chmodTarget.path)"
    exit 0
    """
}

private func send(_ client: IPCClient, _ request: Request) async throws -> Response {
    return try await Task.detached { try client.send(request) }.value
}

private func downloadJobID(_ client: IPCClient, url: String, consentTools: Bool) async throws -> String {
    let response = try await send(
        client,
        Request(cmd: .download, url: url, consentTools: consentTools)
    )
    #expect(response.ok)
    guard case let .object(object)? = response.data,
          case let .string(jobID)? = object["jobID"] else {
        Issue.record("missing jobID in response")
        return ""
    }
    return jobID
}

private struct StatusView {
    var state: String
    var progress: Double
    var path: String?
    var error: String?
}

private func statusView(_ client: IPCClient, jobID: String) async throws -> StatusView? {
    let response = try await send(client, Request(cmd: .downloadStatus, jobID: jobID))
    guard response.ok, case let .object(object)? = response.data,
          case let .string(state)? = object["state"] else {
        return nil
    }
    var progress = 0.0
    if case let .number(value)? = object["progress"] {
        progress = value
    }
    var path: String?
    if case let .string(value)? = object["path"] {
        path = value
    }
    var error: String?
    if case let .string(value)? = object["error"] {
        error = value
    }
    return StatusView(state: state, progress: progress, path: path, error: error)
}

private func pollTerminal(
    _ client: IPCClient,
    jobID: String,
    timeout: TimeInterval = 15
) async throws -> (status: StatusView, runningProgress: [Double]) {
    let deadline = Date().addingTimeInterval(timeout)
    var latest = StatusView(state: "running", progress: 0, path: nil, error: nil)
    var observed: [Double] = []
    while Date() < deadline {
        if let status = try await statusView(client, jobID: jobID) {
            if status.state != "running" {
                return (status, observed)
            }
            latest = status
            observed.append(status.progress)
        }
        try await Task.sleep(for: .milliseconds(20))
    }
    return (latest, observed)
}

private func startHost(
    root: URL,
    incoming: URL,
    downloaderConsent: Bool = false
) throws -> (server: IPCServer, client: IPCClient, host: MediaHost, socketURL: URL) {
    let resolver = ToolLocator(searchDirectories: [root])
    let toolManager = ToolManager(resolver: resolver, consent: StaticToolConsent(consented: true))
    let downloads = DownloadService(
        resolver: resolver,
        incomingDirectory: incoming,
        installer: toolManager,
        consent: StaticToolConsent(consented: downloaderConsent)
    )
    let optimization = OptimizationCoordinator(
        resolver: resolver,
        workDirectory: root.appendingPathComponent("cache", isDirectory: true)
    )
    let host = MediaHost(downloads: downloads, optimization: optimization, maxHeight: { 1080 })
    let socketURL = root.appendingPathComponent("control.sock")
    let server = IPCServer(socketURL: socketURL) { request in
        return await host.handle(request)
    }
    try server.start()
    return (server, IPCClient(socketURL: socketURL), host, socketURL)
}

@Suite(.serialized) struct MediaHostDownloadTests {
    @Test func downloadReachesDoneWithStagedPath() async throws {
        let root = makeTempDir("done")
        let incoming = makeTempDir("done-incoming")
        writeExecutable(singleRouteStub(title: "Host Clip"), named: "yt-dlp", in: root)
        let (server, client, _, _) = try startHost(root: root, incoming: incoming)
        defer { server.stop() }

        let jobID = try await downloadJobID(client, url: "https://example.com/v", consentTools: false)

        let (status, observed) = try await pollTerminal(client, jobID: jobID)
        let monotonic = zip(observed, observed.dropFirst()).allSatisfy { $0 <= $1 }
        #expect(monotonic)

        #expect(status.state == "done")
        #expect(status.progress == 100)
        let path = try #require(status.path)
        #expect(path.hasSuffix("Host Clip.mp4"))
        #expect(FileManager.default.fileExists(atPath: path))
    }

    @Test func cancelDiscardsPartialJob() async throws {
        let root = makeTempDir("cancel")
        let incoming = makeTempDir("cancel-incoming")
        writeExecutable(cancelStub(), named: "yt-dlp", in: root)
        let (server, client, _, _) = try startHost(root: root, incoming: incoming)
        defer { server.stop() }

        let jobID = try await downloadJobID(client, url: "https://example.com/v", consentTools: false)
        let marker = incoming.appendingPathComponent(jobID, isDirectory: true)
            .appendingPathComponent("video", isDirectory: true)
            .appendingPathComponent("started")
        try await waitForFile(marker)

        let cancel = try await send(client, Request(cmd: .downloadCancel, jobID: jobID))
        #expect(cancel.ok)

        let status = try await pollTerminal(client, jobID: jobID).status
        #expect(status.state == "canceled")
        #expect(!FileManager.default.fileExists(atPath: incoming.appendingPathComponent(jobID).path))
    }

    @Test func missingToolWithConsentInstallsThenRetries() async throws {
        let root = makeTempDir("consent")
        let incoming = makeTempDir("consent-incoming")
        let ytDlp = writeExecutable(singleRouteStub(title: "Installed Clip"), named: "yt-dlp", in: root, executable: false)
        writeExecutable(brewStub(chmodTarget: ytDlp), named: "brew", in: root)
        let (server, client, _, _) = try startHost(root: root, incoming: incoming)
        defer { server.stop() }

        let jobID = try await downloadJobID(client, url: "https://example.com/v", consentTools: true)
        let status = try await pollTerminal(client, jobID: jobID).status

        #expect(status.state == "done")
        let path = try #require(status.path)
        #expect(path.hasSuffix("Installed Clip.mp4"))
        #expect(FileManager.default.fileExists(atPath: path))
    }

    @Test func missingToolWithoutConsentFailsWithReason() async throws {
        let root = makeTempDir("noconsent")
        let incoming = makeTempDir("noconsent-incoming")
        let ytDlp = writeExecutable(singleRouteStub(title: "Never Clip"), named: "yt-dlp", in: root, executable: false)
        writeExecutable(brewStub(chmodTarget: ytDlp), named: "brew", in: root)
        let (server, client, _, _) = try startHost(root: root, incoming: incoming)
        defer { server.stop() }

        let jobID = try await downloadJobID(client, url: "https://example.com/v", consentTools: false)
        let status = try await pollTerminal(client, jobID: jobID).status

        #expect(status.state == "failed")
        #expect(status.error?.contains("consent") == true)
        #expect(status.path == nil)
    }

    @Test func unknownJobIDGetsErrorFrame() async throws {
        let root = makeTempDir("unknown")
        let incoming = makeTempDir("unknown-incoming")
        let (server, client, _, _) = try startHost(root: root, incoming: incoming)
        defer { server.stop() }

        let response = try await send(client, Request(cmd: .downloadStatus, jobID: UUID().uuidString))
        #expect(!response.ok)
        #expect(response.error?.code == "ENOJOB")
    }

    @Test func invalidURLGetsErrorFrame() async throws {
        let root = makeTempDir("invalid")
        let incoming = makeTempDir("invalid-incoming")
        let (server, client, _, _) = try startHost(root: root, incoming: incoming)
        defer { server.stop() }

        let response = try await send(client, Request(cmd: .download, url: "ftp://example.com/v"))
        #expect(!response.ok)
        #expect(response.error?.code == "invalidURL")
    }

    @Test func cancelUnknownJobGetsErrorFrame() async throws {
        let root = makeTempDir("cancelunknown")
        let incoming = makeTempDir("cancelunknown-incoming")
        let (server, client, _, _) = try startHost(root: root, incoming: incoming)
        defer { server.stop() }

        let response = try await send(client, Request(cmd: .downloadCancel, jobID: UUID().uuidString))
        #expect(!response.ok)
        #expect(response.error?.code == "ENOJOB")
    }
}

@Suite struct MediaHostLaunchTests {
    @Test func prepareForLaunchWipesIncoming() async throws {
        let root = makeTempDir("wipe")
        let incoming = makeTempDir("wipe-incoming")
        let stale = incoming.appendingPathComponent("stale", isDirectory: true)
        try FileManager.default.createDirectory(at: stale, withIntermediateDirectories: true)
        try Data("partial".utf8).write(to: stale.appendingPathComponent("out.part"))

        let resolver = ToolLocator(searchDirectories: [root])
        let downloads = DownloadService(resolver: resolver, incomingDirectory: incoming)
        let optimization = OptimizationCoordinator(
            resolver: resolver,
            workDirectory: root.appendingPathComponent("cache", isDirectory: true)
        )
        let host = MediaHost(downloads: downloads, optimization: optimization, maxHeight: { 1080 })

        try await host.prepareForLaunch()

        let contents = try FileManager.default.contentsOfDirectory(atPath: incoming.path)
        #expect(contents.isEmpty)
        #expect(host.handles(.download))
        #expect(host.handles(.downloadStatus))
        #expect(host.handles(.downloadCancel))
        #expect(!host.handles(.pause))
    }
}

private func waitForFile(_ url: URL, timeout: TimeInterval = 5) async throws {
    let deadline = Date().addingTimeInterval(timeout)
    while Date() < deadline {
        if FileManager.default.fileExists(atPath: url.path) {
            return
        }
        try await Task.sleep(for: .milliseconds(20))
    }
    Issue.record("file never appeared: \(url.path)")
}
