import Darwin
import Foundation
import Testing
@testable import LiveWallIPC

private func makeSocketURL() -> URL {
    let directory = FileManager.default.temporaryDirectory
        .appendingPathComponent("lwipcs-\(UUID().uuidString.prefix(8))")
    try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    return directory.appendingPathComponent("c.sock")
}

private func rawExchange(socketURL: URL, line: String) -> String {
    let fd = socket(AF_UNIX, SOCK_STREAM, 0)
    defer { close(fd) }
    guard let address = UnixSocket.makeAddress(socketURL.path) else {
        return ""
    }
    var mutableAddress = address
    let result = withUnsafePointer(to: &mutableAddress) { pointer in
        pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { sockaddrPointer in
            connect(fd, sockaddrPointer, socklen_t(MemoryLayout<sockaddr_un>.size))
        }
    }
    guard result == 0 else {
        return ""
    }
    UnixSocket.write(Data((line + "\n").utf8), to: fd)
    return String(decoding: UnixSocket.readLine(fd), as: UTF8.self)
}

@Suite struct IPCServerTests {
    private func makeServer(_ socketURL: URL) -> IPCServer {
        return IPCServer(socketURL: socketURL) { request in
            if request.cmd == .getStatus {
                return .success(.object(["current": .string("/a.mp4"), "playing": .bool(true)]))
            }
            return .success(.object(["echo": .string(request.cmd.rawValue)]))
        }
    }

    @Test func startBindsAndStopRemovesSocket() throws {
        let url = makeSocketURL()
        defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        let server = makeServer(url)

        try server.start()
        #expect(FileManager.default.fileExists(atPath: url.path))

        let attributes = try FileManager.default.attributesOfItem(atPath: url.path)
        #expect((attributes[.posixPermissions] as? NSNumber)?.intValue == 0o600)

        server.stop()
        #expect(!FileManager.default.fileExists(atPath: url.path))
    }

    @Test func roundTripsRequestThroughClient() throws {
        let url = makeSocketURL()
        defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        let server = makeServer(url)
        try server.start()
        defer { server.stop() }

        let response = try IPCClient(socketURL: url).send(Request(cmd: .getStatus))
        #expect(response.ok)
        #expect(response.data == .object(["current": .string("/a.mp4"), "playing": .bool(true)]))
    }

    @Test func malformedLineGetsErrorFrame() throws {
        let url = makeSocketURL()
        defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        let server = makeServer(url)
        try server.start()
        defer { server.stop() }

        let raw = rawExchange(socketURL: url, line: "not json")
        let response = try IPCCodec.decodeResponse(from: raw)
        #expect(!response.ok)
        #expect(response.error?.code == "EMALFORMED")
    }

    @Test func versionMismatchGetsErrorFrame() throws {
        let url = makeSocketURL()
        defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        let server = makeServer(url)
        try server.start()
        defer { server.stop() }

        let raw = rawExchange(socketURL: url, line: #"{"v":99,"cmd":"pause"}"#)
        let response = try IPCCodec.decodeResponse(from: raw)
        #expect(!response.ok)
        #expect(response.error?.code == "EVERSION")
    }

    @Test func staleSocketFileIsReplaced() throws {
        let url = makeSocketURL()
        defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        try Data("stale".utf8).write(to: url)

        let server = makeServer(url)
        try server.start()
        defer { server.stop() }

        let response = try IPCClient(socketURL: url).send(Request(cmd: .pause))
        #expect(response.ok)
    }

    @Test func restartRebindsAfterStop() throws {
        let url = makeSocketURL()
        defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        let server = makeServer(url)

        try server.start()
        server.stop()
        try server.start()
        defer { server.stop() }

        let response = try IPCClient(socketURL: url).send(Request(cmd: .pause))
        #expect(response.ok)
    }
}
