import Foundation
import LiveWallCore
import LiveWallIPC
import LiveWallTools

public actor MediaHost {
    private let downloads: DownloadService
    private let optimization: OptimizationCoordinator
    private let maxHeight: @Sendable () -> Int

    public init(
        downloads: DownloadService,
        optimization: OptimizationCoordinator,
        maxHeight: @escaping @Sendable () -> Int = { DownloadFormatSelector.maximumHeight }
    ) {
        self.downloads = downloads
        self.optimization = optimization
        self.maxHeight = maxHeight
    }

    public nonisolated func handles(_ cmd: Verb) -> Bool {
        switch cmd {
        case .download, .downloadStatus, .downloadCancel:
            return true
        default:
            return false
        }
    }

    public func prepareForLaunch() async throws {
        try await downloads.recreateIncoming()
    }

    public func cancelAll() async {
        await downloads.cancelAll()
        await optimization.cancelAll()
    }

    public func handle(_ request: Request) async -> Response {
        switch request.cmd {
        case .download:
            return await handleDownload(request)
        case .downloadStatus:
            return await handleStatus(request)
        case .downloadCancel:
            return await handleCancel(request)
        default:
            return .failure(code: "EUNSUPPORTED", message: "verb is handled by the media host")
        }
    }

    private func handleDownload(_ request: Request) async -> Response {
        guard let raw = request.url, !raw.isEmpty else {
            return .failure(code: "EINVAL", message: "download requires a url")
        }
        guard let url = URL(string: raw) else {
            return .failure(code: "invalidURL", message: "unsupported download url")
        }
        do {
            let jobID = try await downloads.start(
                url: url,
                maxHeight: maxHeight(),
                consentTools: request.consentTools ?? false
            )
            return .success(.object(["jobID": .string(jobID.uuidString)]))
        } catch {
            return Self.failure(error)
        }
    }

    private func handleStatus(_ request: Request) async -> Response {
        guard let raw = request.jobID, let jobID = UUID(uuidString: raw) else {
            return .failure(code: "EINVAL", message: "download-status requires a jobID")
        }
        guard let status = await downloads.status(jobID) else {
            return .failure(code: "ENOJOB", message: "unknown download job")
        }
        return .success(Self.value(status))
    }

    private func handleCancel(_ request: Request) async -> Response {
        guard let raw = request.jobID, let jobID = UUID(uuidString: raw) else {
            return .failure(code: "EINVAL", message: "download-cancel requires a jobID")
        }
        guard await downloads.status(jobID) != nil else {
            return .failure(code: "ENOJOB", message: "unknown download job")
        }
        await downloads.cancel(jobID)
        return .success()
    }

    static func value(_ status: DownloadJobStatus) -> JSONValue {
        var object: [String: JSONValue] = [
            "state": .string(status.state.rawValue),
            "progress": .number(status.progress)
        ]
        if let title = status.title {
            object["title"] = .string(title)
        }
        if let path = status.path {
            object["path"] = .string(path.path)
        }
        if let error = status.error {
            object["error"] = .string(error)
        }
        return .object(object)
    }

    static func failure(_ error: Error) -> Response {
        guard let liveWallError = error as? LiveWallError else {
            return .failure(code: "EIPC", message: String(describing: error))
        }
        switch liveWallError {
        case let .ipc(code, message):
            return .failure(code: code, message: message)
        case let .downloadFailed(_, stderr):
            return .failure(code: "EDOWNLOAD", message: stderr)
        case let .toolMissing(tool):
            return .failure(code: "ETOOLMISSING", message: "missing tool: \(tool)")
        case let .toolAcquisitionFailed(tools, stderr):
            return .failure(
                code: "ETOOLINSTALL",
                message: "install failed (\(tools.joined(separator: ", "))): \(stderr)"
            )
        case .consentRequired:
            return .failure(code: "ECONSENT", message: "install consent required")
        case let .mediaRejected(reason):
            return .failure(code: "EMEDIA", message: reason)
        case let .mediaUnreadable(path, reason):
            return .failure(code: "EMEDIA", message: "unreadable media: \(path) (\(reason))")
        case let .codecUnsupported(path):
            return .failure(code: "EMEDIA", message: "unsupported codec: \(path)")
        case let .libraryCorrupt(path):
            return .failure(code: "ELIBRARY", message: "corrupt library: \(path)")
        case let .swapStateCorrupt(path):
            return .failure(code: "ESWAP", message: "corrupt swap state: \(path)")
        case let .stillUnavailable(path, reason):
            return .failure(code: "ESTILL", message: "still unavailable: \(path) (\(reason))")
        case let .transcodeFailed(reason):
            return .failure(code: "ETRANSCODE", message: reason)
        }
    }
}
