import Foundation
import Testing
@testable import LiveWallIPC

@Suite struct RequestCodecTests {
    @Test func roundtripsEveryVerb() throws {
        for verb in Verb.allCases {
            let request = Request(cmd: verb)
            let data = try IPCCodec.encode(request)
            let decoded = try IPCCodec.decodeRequest(from: data)
            #expect(decoded == request)
            #expect(decoded.v == IPCProtocol.version)
        }
    }

    @Test func openCarriesPath() throws {
        let request = Request(cmd: .open, path: "/Users/me/wall.mp4")
        let data = try IPCCodec.encode(request)
        let decoded = try IPCCodec.decodeRequest(from: data)
        #expect(decoded == request)
        #expect(decoded.path == "/Users/me/wall.mp4")
    }

    @Test func bareVerbOmitsPathKey() throws {
        let data = try IPCCodec.encode(Request(cmd: .pause))
        let object = try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
        #expect(object["v"] as? Int == 1)
        #expect(object["cmd"] as? String == "pause")
        #expect(object["path"] == nil)
    }

    @Test func downloadCarriesURLAndConsent() throws {
        let request = Request(cmd: .download, url: "https://example.com/v", consentTools: true)
        let decoded = try IPCCodec.decodeRequest(from: try IPCCodec.encode(request))
        #expect(decoded == request)
        #expect(decoded.cmd == .download)
        #expect(decoded.url == "https://example.com/v")
        #expect(decoded.consentTools == true)
    }

    @Test func downloadStatusCarriesJobID() throws {
        let request = Request(cmd: .downloadStatus, jobID: "A1B2")
        let decoded = try IPCCodec.decodeRequest(from: try IPCCodec.encode(request))
        #expect(decoded == request)
        #expect(decoded.cmd == .downloadStatus)
        #expect(decoded.jobID == "A1B2")
    }

    @Test func downloadCancelCarriesJobID() throws {
        let request = Request(cmd: .downloadCancel, jobID: "C3D4")
        let decoded = try IPCCodec.decodeRequest(from: try IPCCodec.encode(request))
        #expect(decoded == request)
        #expect(decoded.cmd == .downloadCancel)
        #expect(decoded.jobID == "C3D4")
    }

    @Test func downloadVerbWireShape() throws {
        let data = try IPCCodec.encode(Request(cmd: .download, url: "https://example.com/v"))
        let object = try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
        #expect(object["cmd"] as? String == "download")
        #expect(object["url"] as? String == "https://example.com/v")
        #expect(object["consentTools"] == nil)
        #expect(object["path"] == nil)
    }

    @Test func newlineDelimitedLineEncodes() throws {
        let data = try IPCCodec.line(Request(cmd: .getStatus))
        #expect(data.last == 0x0A)
        let body = data.dropLast()
        #expect(try IPCCodec.decodeRequest(from: Data(body)).cmd == .getStatus)
    }
}

@Suite struct ResponseCodecTests {
    @Test func successRoundtrip() throws {
        let response = Response.success(.object(["current": .string("/a.mp4"), "playing": .bool(true)]))
        let decoded = try IPCCodec.decodeResponse(from: try IPCCodec.encode(response))
        #expect(decoded == response)
        #expect(decoded.ok == true)
        #expect(decoded.error == nil)
    }

    @Test func nullDataSuccessRoundtrip() throws {
        let response = Response.success()
        let decoded = try IPCCodec.decodeResponse(from: try IPCCodec.encode(response))
        #expect(decoded == response)
        #expect(decoded.data == nil)
    }

    @Test func failureRoundtrip() throws {
        let response = Response.failure(code: "ECONNREFUSED", message: "app not running")
        let decoded = try IPCCodec.decodeResponse(from: try IPCCodec.encode(response))
        #expect(decoded == response)
        #expect(decoded.ok == false)
        #expect(decoded.error?.code == "ECONNREFUSED")
    }

    @Test func failureWireShape() throws {
        let data = try IPCCodec.encode(Response.failure(code: "bad", message: "nope"))
        let object = try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
        #expect(object["ok"] as? Bool == false)
        let error = try #require(object["error"] as? [String: Any])
        #expect(error["code"] as? String == "bad")
        #expect(error["message"] as? String == "nope")
    }
}

@Suite struct MalformedLineTests {
    @Test func truncatedJSONIsMalformed() {
        #expect(throws: IPCCodecError.malformedLine) {
            try IPCCodec.decodeRequest(from: #"{"v":1,"cmd":"pause""#)
        }
    }

    @Test func nonJSONIsMalformed() {
        #expect(throws: IPCCodecError.malformedLine) {
            try IPCCodec.decodeRequest(from: "hello world")
        }
    }

    @Test func unknownCommandIsMalformed() {
        #expect(throws: IPCCodecError.malformedLine) {
            try IPCCodec.decodeRequest(from: #"{"v":1,"cmd":"bogus"}"#)
        }
    }

    @Test func emptyLineIsReported() {
        #expect(throws: IPCCodecError.emptyLine) {
            try IPCCodec.decodeRequest(from: "")
        }
    }

    @Test func versionMismatchIsReported() {
        #expect(throws: IPCCodecError.unsupportedVersion(99)) {
            try IPCCodec.decodeRequest(from: #"{"v":99,"cmd":"pause"}"#)
        }
    }
}
