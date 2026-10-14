import Foundation
import LiveWallCore

public enum ToolStatus: Sendable, Equatable {
    case missing
    case installed(version: ToolVersion)
    case stale(installed: ToolVersion, minimum: ToolVersion)
    case probeFailed(reason: String)
}

public actor ToolManager {
    public static let installEnvironment: [String: String] = [
        "HOMEBREW_NO_AUTO_UPDATE": "1",
        "HOMEBREW_NO_INSTALL_CLEANUP": "1",
        "HOMEBREW_NO_INTERACTIVE": "1"
    ]

    private let resolver: any ToolExecutableResolving
    private let consent: any ToolConsentProviding
    private let minimumVersions: [MediaTool: ToolVersion]
    private let runner: ProcessRunner

    public init(
        resolver: any ToolExecutableResolving = ToolLocator.system,
        consent: any ToolConsentProviding = UserDefaultsToolConsent(),
        minimumVersions: [MediaTool: ToolVersion] = [:],
        runner: ProcessRunner = ProcessRunner()
    ) {
        self.resolver = resolver
        self.consent = consent
        self.minimumVersions = minimumVersions
        self.runner = runner
    }

    public nonisolated var tools: [MediaTool] {
        return MediaTool.allCases
    }

    public nonisolated func resolve(_ tool: MediaTool) -> URL? {
        return resolver.executableURL(for: tool)
    }

    public nonisolated func requireTool(_ tool: MediaTool) throws -> URL {
        guard let url = resolver.executableURL(for: tool) else {
            throw LiveWallError.toolMissing(tool: tool.executableName)
        }
        return url
    }

    public nonisolated func missingTools() -> [MediaTool] {
        return MediaTool.allCases.filter { resolver.executableURL(for: $0) == nil }
    }

    public func status(of tool: MediaTool) async -> ToolStatus {
        guard let url = resolver.executableURL(for: tool) else {
            return .missing
        }
        let minimum = minimumVersions[tool] ?? tool.knownGoodVersion
        do {
            let result = try await runner.run(
                ProcessInvocation(executable: url, arguments: tool.versionArguments)
            )
            guard let version = ToolVersion.parseOutput(result.stdout)
                ?? ToolVersion.parseOutput(result.stderr) else {
                return .probeFailed(reason: "unrecognized version output")
            }
            if version.isParsable, version < minimum {
                return .stale(installed: version, minimum: minimum)
            }
            return .installed(version: version)
        } catch {
            return .probeFailed(reason: String(describing: error))
        }
    }

    public func statuses() async -> [MediaTool: ToolStatus] {
        var result: [MediaTool: ToolStatus] = [:]
        for tool in MediaTool.allCases {
            result[tool] = await status(of: tool)
        }
        return result
    }

    @discardableResult
    public func installMissing() async throws -> [MediaTool] {
        let missing = missingTools()
        guard !missing.isEmpty else {
            return []
        }
        guard consent.isInstallConsented() else {
            throw LiveWallError.consentRequired
        }
        guard let brew = resolver.brewExecutableURL() else {
            throw LiveWallError.toolMissing(tool: ToolLocator.brewExecutableName)
        }
        var environment = ProcessInfo.processInfo.environment
        environment.merge(Self.installEnvironment) { _, new in new }
        let formulas = missing.map { $0.brewFormula }
        let result = try await runner.run(
            ProcessInvocation(
                executable: brew,
                arguments: ["install"] + formulas,
                environment: environment
            )
        )
        guard result.exitCode == 0 else {
            throw LiveWallError.toolAcquisitionFailed(
                tools: missing.map { $0.executableName },
                stderr: result.stderr
            )
        }
        return missing
    }
}
