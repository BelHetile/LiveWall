import Foundation
import Testing
import LiveWallCore
@testable import LiveWallTools

private func makeTempDir() -> URL {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("lwtools-\(UUID().uuidString)")
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

@discardableResult
private func makeDirectory(named name: String, in parent: URL) -> URL {
    let url = parent.appendingPathComponent(name, isDirectory: true)
    try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

@discardableResult
private func writeStub(_ script: String, named name: String, in directory: URL, executable: Bool = true) -> URL {
    let url = directory.appendingPathComponent(name)
    try? script.write(to: url, atomically: true, encoding: .utf8)
    let permissions = executable ? 0o755 : 0o644
    try? FileManager.default.setAttributes([.posixPermissions: permissions], ofItemAtPath: url.path)
    return url
}

private func ffmpegStub(version: String) -> String {
    return "#!/bin/sh\necho 'ffmpeg version \(version) Copyright (c) 2000-2026 the FFmpeg developers'\n"
}

private func brewStub(recordingTo directory: URL) -> String {
    return """
    #!/bin/sh
    printf '%s\\n' "$@" > "\(directory.path)/brew-args.txt"
    env > "\(directory.path)/brew-env.txt"
    exit 0
    """
}

private func recordedArguments(in directory: URL) throws -> [String] {
    let url = directory.appendingPathComponent("brew-args.txt")
    let contents = try String(contentsOf: url, encoding: .utf8)
    return contents.split(separator: "\n").map(String.init)
}

@Suite struct ToolVersionTests {
    @Test func parsesLeadingNumericToken() {
        #expect(ToolVersion(raw: "9.0.2").components == [9, 0, 2])
        #expect(ToolVersion(raw: "9.0.2_1").components == [9, 0, 2])
        #expect(ToolVersion(raw: "n7.1").components == [7, 1])
    }

    @Test func parsesVersionFromToolOutput() throws {
        let ffmpeg = try #require(ToolVersion.parseOutput("ffmpeg version 9.0.2 Copyright (c) 2000-2026 the FFmpeg developers"))
        #expect(ffmpeg.raw == "9.0.2")
        let deno = try #require(ToolVersion.parseOutput("deno 2.9.7 (stable, release, aarch64-apple-darwin)"))
        #expect(deno.raw == "2.9.7")
        let ytdlp = try #require(ToolVersion.parseOutput("2026.08.19\n"))
        #expect(ytdlp.raw == "2026.08.19")
    }

    @Test func rejectsOutputWithoutDigits() {
        #expect(ToolVersion.parseOutput("no version here") == nil)
        #expect(ToolVersion.parseOutput("") == nil)
        #expect(ToolVersion(raw: "unparsable").components.isEmpty)
    }

    @Test func comparisonIsComponentWiseAndZeroPadded() {
        #expect(ToolVersion(raw: "4.4.4") < ToolVersion(raw: "9.0.2"))
        #expect(ToolVersion(raw: "2.9.7") == ToolVersion(raw: "2.9.7.0"))
        #expect(!(ToolVersion(raw: "9.0.2") < ToolVersion(raw: "9.0.2")))
        #expect(ToolVersion(raw: "2.9.10") > ToolVersion(raw: "2.9.7"))
    }

    @Test func toolsDeclareVersionArguments() {
        #expect(MediaTool.ffmpeg.versionArguments == ["-version"])
        #expect(MediaTool.ytDlp.versionArguments == ["--version"])
        #expect(MediaTool.deno.versionArguments == ["--version"])
        #expect(MediaTool.ytDlp.executableName == "yt-dlp")
        #expect(MediaTool.allCases.count == 3)
    }
}

@Suite struct ToolLocatorTests {
    @Test func defaultSearchOrderPrioritizesAppToolsDirectory() {
        let appTools = URL(fileURLWithPath: "/tmp/lw-app-tools", isDirectory: true)
        let directories = ToolLocator.defaultSearchDirectories(appToolsDirectory: appTools)
        #expect(directories == [
            appTools,
            URL(fileURLWithPath: "/opt/homebrew/bin", isDirectory: true),
            URL(fileURLWithPath: "/usr/local/bin", isDirectory: true)
        ])
    }

    @Test func earlierDirectoryWins() {
        let root = makeTempDir()
        let first = makeDirectory(named: "first", in: root)
        let second = makeDirectory(named: "second", in: root)
        writeStub("#!/bin/sh\necho first\n", named: "ffmpeg", in: first)
        writeStub("#!/bin/sh\necho second\n", named: "ffmpeg", in: second)

        let locator = ToolLocator(searchDirectories: [first, second])
        let resolved = locator.executableURL(for: .ffmpeg)
        #expect(resolved?.deletingLastPathComponent().lastPathComponent == "first")
    }

    @Test func skipsNonExecutableAndFallsThrough() {
        let root = makeTempDir()
        let first = makeDirectory(named: "first", in: root)
        let second = makeDirectory(named: "second", in: root)
        writeStub("#!/bin/sh\necho nope\n", named: "ffmpeg", in: first, executable: false)
        writeStub("#!/bin/sh\necho yes\n", named: "ffmpeg", in: second)

        let locator = ToolLocator(searchDirectories: [first, second])
        let resolved = locator.executableURL(for: .ffmpeg)
        #expect(resolved?.deletingLastPathComponent().lastPathComponent == "second")
    }

    @Test func brewIsDiscoveredFromSearchDirectories() {
        let root = makeTempDir()
        writeStub("#!/bin/sh\nexit 0\n", named: "brew", in: root)
        let locator = ToolLocator(searchDirectories: [root])
        #expect(locator.brewExecutableURL()?.lastPathComponent == "brew")
        #expect(locator.executableURL(for: .deno) == nil)
    }
}

@Suite struct ToolStatusTests {
    @Test func probesInstalledVersion() async {
        let root = makeTempDir()
        writeStub(ffmpegStub(version: "9.0.2"), named: "ffmpeg", in: root)
        let manager = ToolManager(
            resolver: ToolLocator(searchDirectories: [root]),
            consent: StaticToolConsent(consented: false)
        )
        #expect(await manager.status(of: .ffmpeg) == .installed(version: ToolVersion(raw: "9.0.2")))
    }

    @Test func probesYtDlpAndDenoVersions() async {
        let root = makeTempDir()
        writeStub("#!/bin/sh\necho 2026.08.19\n", named: "yt-dlp", in: root)
        writeStub("#!/bin/sh\necho 'deno 2.9.7 (stable, release, aarch64-apple-darwin)'\n", named: "deno", in: root)
        let manager = ToolManager(
            resolver: ToolLocator(searchDirectories: [root]),
            consent: StaticToolConsent(consented: false)
        )
        #expect(await manager.status(of: .ytDlp) == .installed(version: ToolVersion(raw: "2026.08.19")))
        #expect(await manager.status(of: .deno) == .installed(version: ToolVersion(raw: "2.9.7")))
    }

    @Test func reportsMissingTool() async {
        let root = makeTempDir()
        let manager = ToolManager(
            resolver: ToolLocator(searchDirectories: [root]),
            consent: StaticToolConsent(consented: false)
        )
        #expect(await manager.status(of: .ffmpeg) == .missing)
        #expect(manager.missingTools() == [.ffmpeg, .ytDlp, .deno])
    }

    @Test func reportsStaleVersionWithoutUpgrading() async {
        let root = makeTempDir()
        writeStub(ffmpegStub(version: "4.4.4"), named: "ffmpeg", in: root)
        let manager = ToolManager(
            resolver: ToolLocator(searchDirectories: [root]),
            consent: StaticToolConsent(consented: false)
        )
        #expect(await manager.status(of: .ffmpeg) == .stale(
            installed: ToolVersion(raw: "4.4.4"),
            minimum: MediaTool.ffmpeg.knownGoodVersion
        ))
    }

    @Test func reportsPreviousToolAsMissingRelativeToInjectedMinimum() async {
        let root = makeTempDir()
        writeStub(ffmpegStub(version: "9.0.2"), named: "ffmpeg", in: root)
        let manager = ToolManager(
            resolver: ToolLocator(searchDirectories: [root]),
            consent: StaticToolConsent(consented: false),
            minimumVersions: [.ffmpeg: ToolVersion(raw: "10.0.0")]
        )
        #expect(await manager.status(of: .ffmpeg) == .stale(
            installed: ToolVersion(raw: "9.0.2"),
            minimum: ToolVersion(raw: "10.0.0")
        ))
    }

    @Test func unparsableProbeOutputIsReported() async {
        let root = makeTempDir()
        writeStub("#!/bin/sh\necho 'no version here'\n", named: "ffmpeg", in: root)
        let manager = ToolManager(
            resolver: ToolLocator(searchDirectories: [root]),
            consent: StaticToolConsent(consented: false)
        )
        #expect(await manager.status(of: .ffmpeg) == .probeFailed(reason: "unrecognized version output"))
    }

    @Test func requireToolThrowsWhenAbsent() {
        let root = makeTempDir()
        let manager = ToolManager(
            resolver: ToolLocator(searchDirectories: [root]),
            consent: StaticToolConsent(consented: false)
        )
        #expect(throws: LiveWallError.toolMissing(tool: "ffmpeg")) {
            try manager.requireTool(.ffmpeg)
        }
    }
}

@Suite struct ToolInstallTests {
    @Test func installWithoutConsentThrowsAndDoesNotRunBrew() async {
        let root = makeTempDir()
        writeStub(brewStub(recordingTo: root), named: "brew", in: root)
        let manager = ToolManager(
            resolver: ToolLocator(searchDirectories: [root]),
            consent: StaticToolConsent(consented: false)
        )
        await #expect(throws: LiveWallError.consentRequired) {
            try await manager.installMissing()
        }
        #expect(!FileManager.default.fileExists(atPath: root.appendingPathComponent("brew-args.txt").path))
    }

    @Test func installRequiresBrewWhenConsented() async {
        let root = makeTempDir()
        let manager = ToolManager(
            resolver: ToolLocator(searchDirectories: [root]),
            consent: StaticToolConsent(consented: true)
        )
        await #expect(throws: LiveWallError.toolMissing(tool: "brew")) {
            try await manager.installMissing()
        }
    }

    @Test func noMissingToolsIsANoOp() async throws {
        let root = makeTempDir()
        writeStub(ffmpegStub(version: "9.0.2"), named: "ffmpeg", in: root)
        writeStub("#!/bin/sh\necho 2026.08.19\n", named: "yt-dlp", in: root)
        writeStub("#!/bin/sh\necho 'deno 2.9.7'\n", named: "deno", in: root)
        writeStub(brewStub(recordingTo: root), named: "brew", in: root)
        let manager = ToolManager(
            resolver: ToolLocator(searchDirectories: [root]),
            consent: StaticToolConsent(consented: false)
        )
        #expect(try await manager.installMissing().isEmpty)
        #expect(!FileManager.default.fileExists(atPath: root.appendingPathComponent("brew-args.txt").path))
    }

    @Test func consentedInstallRunsBrewOnceForAllMissingTools() async throws {
        let root = makeTempDir()
        writeStub(brewStub(recordingTo: root), named: "brew", in: root)
        let manager = ToolManager(
            resolver: ToolLocator(searchDirectories: [root]),
            consent: StaticToolConsent(consented: true)
        )
        let installed = try await manager.installMissing()
        #expect(installed == [.ffmpeg, .ytDlp, .deno])
        #expect(try recordedArguments(in: root) == ["install", "ffmpeg", "yt-dlp", "deno"])

        let environment = try String(contentsOf: root.appendingPathComponent("brew-env.txt"), encoding: .utf8)
        #expect(environment.contains("HOMEBREW_NO_AUTO_UPDATE=1"))
        #expect(environment.contains("HOMEBREW_NO_INSTALL_CLEANUP=1"))
        #expect(environment.contains("HOMEBREW_NO_INTERACTIVE=1"))
    }

    @Test func staleToolIsNotReinstalled() async throws {
        let root = makeTempDir()
        writeStub(ffmpegStub(version: "4.4.4"), named: "ffmpeg", in: root)
        writeStub(brewStub(recordingTo: root), named: "brew", in: root)
        let manager = ToolManager(
            resolver: ToolLocator(searchDirectories: [root]),
            consent: StaticToolConsent(consented: true)
        )
        let installed = try await manager.installMissing()
        #expect(installed == [.ytDlp, .deno])
        #expect(try recordedArguments(in: root) == ["install", "yt-dlp", "deno"])
    }

    @Test func failedBrewInstallSurfacesAcquisitionError() async {
        let root = makeTempDir()
        writeStub("#!/bin/sh\necho boom >&2\nexit 1\n", named: "brew", in: root)
        let manager = ToolManager(
            resolver: ToolLocator(searchDirectories: [root]),
            consent: StaticToolConsent(consented: true)
        )
        await #expect(throws: LiveWallError.toolAcquisitionFailed(
            tools: ["ffmpeg", "yt-dlp", "deno"],
            stderr: "boom\n"
        )) {
            try await manager.installMissing()
        }
    }
}
