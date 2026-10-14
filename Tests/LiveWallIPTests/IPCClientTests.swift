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

@Suite struct DownloadCLITests {
    @Test func parsesURLOnly() throws {
        let invocation = try DownloadCLI.parse(arguments: ["https://example.com/v"])
        #expect(invocation.url == "https://example.com/v")
        #expect(invocation.noWait == false)
        #expect(invocation.consentTools == false)
    }

    @Test func parsesFlagsInAnyOrder() throws {
        let first = try DownloadCLI.parse(arguments: ["--consent-tools", "https://example.com/v", "--no-wait"])
        #expect(first.url == "https://example.com/v")
        #expect(first.noWait)
        #expect(first.consentTools)

        let second = try DownloadCLI.parse(arguments: ["--no-wait", "--consent-tools", "https://example.com/a"])
        #expect(second == DownloadInvocation(url: "https://example.com/a", noWait: true, consentTools: true))
    }

    @Test func rejectsMissingURL() {
        #expect(throws: DownloadCLIError.missingURL) {
            _ = try DownloadCLI.parse(arguments: ["--no-wait"])
        }
    }

    @Test func rejectsUnknownFlag() {
        #expect(throws: DownloadCLIError.unknownFlag("--bogus")) {
            _ = try DownloadCLI.parse(arguments: ["--bogus", "https://example.com/v"])
        }
    }

    @Test func rejectsMultipleURLs() {
        #expect(throws: DownloadCLIError.multipleURLs) {
            _ = try DownloadCLI.parse(arguments: ["https://example.com/a", "https://example.com/b"])
        }
    }

    @Test func validatesSupportedSchemes() {
        #expect(DownloadCLI.isSupportedURL("https://example.com/v"))
        #expect(DownloadCLI.isSupportedURL("http://example.com/v"))
        #expect(!DownloadCLI.isSupportedURL("ftp://example.com/v"))
        #expect(!DownloadCLI.isSupportedURL("not a url"))
        #expect(!DownloadCLI.isSupportedURL("https://"))
    }

    @Test func buildsDownloadRequests() {
        let request = DownloadCLI.makeRequest(
            DownloadInvocation(url: "https://example.com/v", noWait: true, consentTools: true)
        )
        #expect(request.cmd == .download)
        #expect(request.url == "https://example.com/v")
        #expect(request.consentTools == true)

        let status = DownloadCLI.statusRequest(jobID: "J1")
        #expect(status.cmd == .downloadStatus)
        #expect(status.jobID == "J1")

        let cancel = DownloadCLI.cancelRequest(jobID: "J2")
        #expect(cancel.cmd == .downloadCancel)
        #expect(cancel.jobID == "J2")
    }

    @Test func mapsTerminalStateToExitCode() {
        #expect(DownloadCLI.exitCode(forState: "done") == 0)
        #expect(DownloadCLI.exitCode(forState: "failed") == 1)
        #expect(DownloadCLI.exitCode(forState: "canceled") == 1)
    }
}
