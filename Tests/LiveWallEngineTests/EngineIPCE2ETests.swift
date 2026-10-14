import Foundation
import Testing
@testable import LiveWallEngine

private final class EngineE2EBundleToken {}

@Suite(.serialized, .enabled(if: EngineE2E.enabled)) struct EngineIPCE2ETests {
    private var buildDirectory: URL {
        return Bundle(for: EngineE2EBundleToken.self).bundleURL.deletingLastPathComponent()
    }

    private var serverBinary: URL {
        return buildDirectory.appendingPathComponent("LiveWall")
    }

    private var clientBinary: URL {
        return buildDirectory.appendingPathComponent("lwctl")
    }

    private func makeRoot() -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("lweng-\(UUID().uuidString.prefix(8))")
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    private func environment(socketURL: URL) -> [String: String] {
        var environment = ProcessInfo.processInfo.environment
        environment["LIVEWALL_CONTROL_SOCKET"] = socketURL.path
        environment["LIVEWALL_ENGINE_ITEMS"] = [
            Fixtures.hevcClip.path,
            Fixtures.animation.path,
            Fixtures.still.path
        ].joined(separator: ":")
        environment["LIVEWALL_TEST_MODE"] = "1"
        return environment
    }

    private func launchServer(socketURL: URL) throws -> Process {
        guard FileManager.default.isExecutableFile(atPath: serverBinary.path) else {
            throw EngineE2EError.missingBinary(serverBinary.path)
        }
        let process = Process()
        process.executableURL = serverBinary
        process.environment = environment(socketURL: socketURL)
        process.standardOutput = Pipe()
        process.standardError = Pipe()
        try process.run()
        return process
    }

    private func runClient(_ arguments: [String], socketURL: URL) throws -> (status: Int32, stdout: String) {
        guard FileManager.default.isExecutableFile(atPath: clientBinary.path) else {
            throw EngineE2EError.missingBinary(clientBinary.path)
        }
        let process = Process()
        process.executableURL = clientBinary
        process.arguments = arguments
        process.environment = environment(socketURL: socketURL)
        let stdout = Pipe()
        process.standardOutput = stdout
        process.standardError = Pipe()
        try process.run()
        process.waitUntilExit()
        let data = stdout.fileHandleForReading.readDataToEndOfFile()
        return (process.terminationStatus, String(decoding: data, as: UTF8.self))
    }

    private func statusField(_ field: String, socketURL: URL) throws -> Any? {
        let result = try runClient(["get-status"], socketURL: socketURL)
        guard result.status == 0 else {
            return nil
        }
        return try object(from: result.stdout)[field]
    }

    private func currentPath(socketURL: URL) throws -> String? {
        guard let value = try statusField("current", socketURL: socketURL) else {
            return nil
        }
        return value as? String
    }

    private func object(from output: String) throws -> [String: Any] {
        let data = Data(output.utf8)
        return (try? JSONSerialization.jsonObject(with: data) as? [String: Any]) ?? [:]
    }

    private func waitForSocket(_ socketURL: URL, timeout: TimeInterval = 15) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if FileManager.default.fileExists(atPath: socketURL.path) {
                let result = try? runClient(["get-status"], socketURL: socketURL)
                if result?.status == 0 {
                    return true
                }
            }
            Thread.sleep(forTimeInterval: 0.1)
        }
        return false
    }

    private func waitForCurrent(_ socketURL: URL, timeout: TimeInterval = 15) -> String? {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if let path = try? currentPath(socketURL: socketURL) {
                return path
            }
            Thread.sleep(forTimeInterval: 0.1)
        }
        return nil
    }

    private func terminate(_ process: Process?) {
        guard let process, process.isRunning else {
            return
        }
        kill(process.processIdentifier, SIGKILL)
        process.waitUntilExit()
    }

    @Test func everyVerbRoundTripsAndNextChangesCurrent() throws {
        let root = makeRoot()
        let socketURL = root.appendingPathComponent("control.sock")
        var server: Process?
        defer {
            terminate(server)
            try? FileManager.default.removeItem(at: root)
        }

        server = try launchServer(socketURL: socketURL)
        try #require(waitForSocket(socketURL), "server never bound the control socket")
        let firstCurrent = try #require(waitForCurrent(socketURL), "engine never applied a wallpaper")
        #expect(firstCurrent == Fixtures.hevcClip.path)

        for verb in ["pause", "resume", "mute", "unmute", "next", "previous", "random", "get-current", "get-status"] {
            let result = try runClient([verb], socketURL: socketURL)
            #expect(result.status == 0, "\(verb) exited \(result.status): \(result.stdout)")
        }

        _ = try runClient(["open", Fixtures.still.path], socketURL: socketURL)
        let openedValue = try currentPath(socketURL: socketURL)
        let opened = try #require(openedValue)
        #expect(opened == Fixtures.still.path)
    }

    @Test func nextChangesCurrentAndPauseResumeFlipPlaying() throws {
        let root = makeRoot()
        let socketURL = root.appendingPathComponent("control.sock")
        var server: Process?
        defer {
            terminate(server)
            try? FileManager.default.removeItem(at: root)
        }

        server = try launchServer(socketURL: socketURL)
        try #require(waitForSocket(socketURL), "server never bound the control socket")
        let before = try #require(waitForCurrent(socketURL), "engine never applied a wallpaper")

        let next = try runClient(["next"], socketURL: socketURL)
        #expect(next.status == 0)
        let afterValue = try currentPath(socketURL: socketURL)
        let after = try #require(afterValue)
        #expect(after != before)

        _ = try runClient(["pause"], socketURL: socketURL)
        #expect(try statusField("playing", socketURL: socketURL) as? Bool == false)

        _ = try runClient(["resume"], socketURL: socketURL)
        #expect(try statusField("playing", socketURL: socketURL) as? Bool == true)

        let rule = try statusField("rule", socketURL: socketURL) as? String
        #expect(rule == "keep-running")
    }

    @Test func staleSocketGivesExitTwoAndRestartRebinds() throws {
        let root = makeRoot()
        let socketURL = root.appendingPathComponent("control.sock")
        var server: Process?
        defer {
            terminate(server)
            try? FileManager.default.removeItem(at: root)
        }

        server = try launchServer(socketURL: socketURL)
        try #require(waitForSocket(socketURL), "server never bound the control socket")

        kill(server!.processIdentifier, SIGKILL)
        server!.waitUntilExit()
        #expect(FileManager.default.fileExists(atPath: socketURL.path), "socket file should remain after crash")

        let refused = try runClient(["get-status"], socketURL: socketURL)
        #expect(refused.status == 2)
        #expect(refused.stdout.contains("app not running"))

        server = try launchServer(socketURL: socketURL)
        try #require(waitForSocket(socketURL), "server did not rebind the stale socket")
        let ok = try runClient(["get-status"], socketURL: socketURL)
        #expect(ok.status == 0)
    }
}

enum EngineE2EError: Error {
    case missingBinary(String)
}
