import Foundation
import Testing
@testable import LiveWallIPC

@Suite struct IPCClientTests {
    @Test func connectRefusedReportsAppNotRunning() {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("lwipcc-\(UUID().uuidString.prefix(8))")
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }

        let client = IPCClient(socketURL: directory.appendingPathComponent("c.sock"))
        #expect(throws: IPCClientError.appNotRunning) {
            _ = try client.send(Request(cmd: .getStatus))
        }
    }

    @Test func decodesSuccessAndFailureResponses() throws {
        let success = try IPCCodec.decodeResponse(from: #"{"ok":true,"data":{"playing":true}}"#)
        #expect(success.ok)
        #expect(success.data == .object(["playing": .bool(true)]))

        let failure = try IPCCodec.decodeResponse(from: #"{"ok":false,"error":{"code":"EMALFORMED","message":"bad"}}"#)
        #expect(!failure.ok)
        #expect(failure.error?.code == "EMALFORMED")
    }

    @Test func realSocketRoundTrip() throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("lwipcc-\(UUID().uuidString.prefix(8))")
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("c.sock")

        let server = IPCServer(socketURL: url) { request in
            return .success(.object(["cmd": .string(request.cmd.rawValue)]))
        }
        try server.start()
        defer { server.stop() }

        let response = try IPCClient(socketURL: url).send(Request(cmd: .next))
        #expect(response.data == .object(["cmd": .string("next")]))
    }
}

@Suite struct IPCCLITests {
    @Test func parsesEveryBareVerb() throws {
        for verb in Verb.allCases where verb != .open {
            let request = try IPCCLI.makeRequest(arguments: [verb.rawValue])
            #expect(request.cmd == verb)
            #expect(request.path == nil)
        }
    }

    @Test func parsesOpenWithPath() throws {
        let request = try IPCCLI.makeRequest(arguments: ["open", "/tmp/wall.mp4"])
        #expect(request.cmd == .open)
        #expect(request.path == "/tmp/wall.mp4")
    }

    @Test func rejectsMissingCommand() {
        #expect(throws: IPCCLIError.missingCommand) {
            _ = try IPCCLI.makeRequest(arguments: [])
        }
    }

    @Test func rejectsUnknownCommand() {
        #expect(throws: IPCCLIError.unknownCommand("bogus")) {
            _ = try IPCCLI.makeRequest(arguments: ["bogus"])
        }
    }

    @Test func rejectsOpenWithoutPath() {
        #expect(throws: IPCCLIError.missingPath) {
            _ = try IPCCLI.makeRequest(arguments: ["open"])
        }
    }
}
