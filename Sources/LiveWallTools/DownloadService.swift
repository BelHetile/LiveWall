import Foundation
import LiveWallCore

public enum DownloadState: String, Sendable, Equatable {
    case running
    case done
    case failed
    case canceled
}

public struct DownloadJobStatus: Sendable, Equatable {
    public let jobID: UUID
    public let state: DownloadState
    public let progress: Double
    public let title: String?
    public let path: URL?
    public let error: String?

    public init(
        jobID: UUID,
        state: DownloadState,
        progress: Double,
        title: String?,
        path: URL?,
        error: String?
    ) {
        self.jobID = jobID
        self.state = state
        self.progress = progress
        self.title = title
        self.path = path
        self.error = error
    }
}

public enum DownloadEvent: Sendable, Equatable {
    case started(jobID: UUID)
    case progress(jobID: UUID, percent: Double, title: String?)
    case finished(DownloadJobStatus)
}

public actor DownloadService {
    public static let titlePrefix = "LWTITLE:"

    private struct Job {
        let id: UUID
        let url: URL
        let maxHeight: Int
        let directory: URL
        var state: DownloadState
        var progress: Double
        var title: String?
        var path: URL?
        var error: String?
        var cancelRequested: Bool
        var accumulator: DownloadProgressAccumulator
        var process: ProcessSession?
    }

    private enum PassOutcome {
        case completed
        case failed(stderr: String)
    }

    private let resolver: any ToolExecutableResolving
    private let incomingDirectory: URL
    private let hardwareAV1: @Sendable () -> Bool
    private var jobs: [UUID: Job] = [:]
    private var listeners: [UUID: AsyncStream<DownloadEvent>.Continuation] = [:]

    public init(
        resolver: any ToolExecutableResolving = ToolLocator.system,
        incomingDirectory: URL = LiveWallPaths.incomingDirectory(),
        hardwareAV1: @escaping @Sendable () -> Bool = AV1HardwareSupport.isSupported
    ) {
        self.resolver = resolver
        self.incomingDirectory = incomingDirectory
        self.hardwareAV1 = hardwareAV1
    }

    @discardableResult
    public func start(url: URL, maxHeight: Int) async throws -> UUID {
        guard let scheme = url.scheme?.lowercased(), scheme == "http" || scheme == "https",
              let host = url.host, !host.isEmpty else {
            throw LiveWallError.ipc(code: "invalidURL", message: "unsupported download url")
        }
        let jobID = UUID()
        let directory = incomingDirectory.appendingPathComponent(jobID.uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        jobs[jobID] = Job(
            id: jobID,
            url: url,
            maxHeight: maxHeight,
            directory: directory,
            state: .running,
            progress: 0,
            title: nil,
            path: nil,
            error: nil,
            cancelRequested: false,
            accumulator: DownloadProgressAccumulator(),
            process: nil
        )
        emit(.started(jobID: jobID))
        Task { [weak self] in
            await self?.run(jobID)
        }
        return jobID
    }

    public func status(_ jobID: UUID) -> DownloadJobStatus? {
        guard let job = jobs[jobID] else {
            return nil
        }
        return makeStatus(job)
    }

    public func cancel(_ jobID: UUID) {
        guard var job = jobs[jobID], job.state == .running else {
            return
        }
        job.cancelRequested = true
        jobs[jobID] = job
        job.process?.terminate()
    }

    public func events() -> AsyncStream<DownloadEvent> {
        let (stream, continuation) = AsyncStream<DownloadEvent>.makeStream()
        let identifier = UUID()
        listeners[identifier] = continuation
        continuation.onTermination = { [weak self] _ in
            Task { await self?.removeListener(identifier) }
        }
        return stream
    }

    public func recreateIncoming() throws {
        let manager = FileManager.default
        if manager.fileExists(atPath: incomingDirectory.path) {
            try manager.removeItem(at: incomingDirectory)
        }
        try manager.createDirectory(at: incomingDirectory, withIntermediateDirectories: true)
    }

    private func removeListener(_ identifier: UUID) {
        listeners[identifier] = nil
    }

    private func run(_ jobID: UUID) async {
        do {
            let downloaded = try await performDownload(jobID)
            try finalize(jobID, downloaded: downloaded)
        } catch is CancellationError {
            wipe(jobID, state: .canceled, error: nil)
        } catch {
            if jobs[jobID]?.cancelRequested == true {
                wipe(jobID, state: .canceled, error: nil)
            } else {
                wipe(jobID, state: .failed, error: describe(error))
            }
        }
    }

    private func performDownload(_ jobID: UUID) async throws -> URL {
        let job = try requireJob(jobID)
        let ytDlp = try requireTool(.ytDlp)

        let videoDirectory = job.directory.appendingPathComponent("video", isDirectory: true)
        let audioDirectory = job.directory.appendingPathComponent("audio", isDirectory: true)
        let singleDirectory = job.directory.appendingPathComponent("source", isDirectory: true)

        let videoFormat = DownloadFormatSelector.videoExpression(maxHeight: job.maxHeight, hwAV1: hardwareAV1())
        let videoOutcome = try await runDownloadPass(
            jobID: jobID,
            executable: ytDlp,
            format: videoFormat,
            outputDirectory: videoDirectory,
            stage: .video
        )
        switch videoOutcome {
        case .completed:
            break
        case let .failed(stderr):
            if isFormatUnavailable(stderr) {
                return try await downloadSingle(jobID: jobID, executable: ytDlp, outputDirectory: singleDirectory)
            }
            throw LiveWallError.downloadFailed(jobID: jobID.uuidString, stderr: stderr)
        }
        let videoFile = try firstFile(in: videoDirectory, jobID: jobID)

        let audioOutcome = try await runDownloadPass(
            jobID: jobID,
            executable: ytDlp,
            format: DownloadFormatSelector.audioExpression(),
            outputDirectory: audioDirectory,
            stage: .audio
        )
        switch audioOutcome {
        case .completed:
            break
        case let .failed(stderr):
            if isFormatUnavailable(stderr) {
                try? FileManager.default.removeItem(at: videoDirectory)
                return try await downloadSingle(jobID: jobID, executable: ytDlp, outputDirectory: singleDirectory)
            }
            throw LiveWallError.downloadFailed(jobID: jobID.uuidString, stderr: stderr)
        }
        let audioFile = try firstFile(in: audioDirectory, jobID: jobID)

        return try await mux(jobID: jobID, video: videoFile, audio: audioFile)
    }

    private func downloadSingle(jobID: UUID, executable: URL, outputDirectory: URL) async throws -> URL {
        let job = try requireJob(jobID)
        let format = DownloadFormatSelector.singleExpression(maxHeight: job.maxHeight)
        let outcome = try await runDownloadPass(
            jobID: jobID,
            executable: executable,
            format: format,
            outputDirectory: outputDirectory,
            stage: .video
        )
        if case let .failed(stderr) = outcome {
            throw LiveWallError.downloadFailed(jobID: jobID.uuidString, stderr: stderr)
        }
        return try firstFile(in: outputDirectory, jobID: jobID)
    }

    private func runDownloadPass(
        jobID: UUID,
        executable: URL,
        format: String,
        outputDirectory: URL,
        stage: DownloadProgressAccumulator.Stage
    ) async throws -> PassOutcome {
        let job = try requireJob(jobID)
        try FileManager.default.createDirectory(at: outputDirectory, withIntermediateDirectories: true)
        let invocation = downloadInvocation(
            executable: executable,
            url: job.url,
            format: format,
            outputDirectory: outputDirectory
        )
        let session = try ProcessLauncher.launch(invocation)
        jobs[jobID]?.process = session
        for await line in session.lines {
            handleLine(line, jobID: jobID, stage: stage)
        }
        let result = try await session.wait()
        jobs[jobID]?.process = nil
        if try requireJob(jobID).cancelRequested {
            throw CancellationError()
        }
        if result.exitCode == 0 {
            return .completed
        }
        return .failed(stderr: result.stderr)
    }

    private func mux(jobID: UUID, video: URL, audio: URL) async throws -> URL {
        let job = try requireJob(jobID)
        let ffmpeg = try requireTool(.ffmpeg)
        let output = job.directory.appendingPathComponent("output.mp4")
        let arguments = [
            "-hide_banner", "-loglevel", "error", "-y",
            "-i", video.path,
            "-i", audio.path,
            "-c", "copy",
            "-movflags", "+faststart",
            output.path
        ]
        let session = try ProcessLauncher.launch(ProcessInvocation(executable: ffmpeg, arguments: arguments))
        jobs[jobID]?.process = session
        for await _ in session.lines {}
        let result = try await session.wait()
        jobs[jobID]?.process = nil
        if try requireJob(jobID).cancelRequested {
            throw CancellationError()
        }
        guard result.exitCode == 0, FileManager.default.fileExists(atPath: output.path) else {
            throw LiveWallError.downloadFailed(jobID: jobID.uuidString, stderr: "mux failed: \(result.stderr)")
        }
        return output
    }

    private func downloadInvocation(
        executable: URL,
        url: URL,
        format: String,
        outputDirectory: URL
    ) -> ProcessInvocation {
        var arguments: [String] = []
        arguments += YTDLP.progressArguments
        arguments += ["--newline", "--progress", "--no-simulate", "--no-playlist"]
        arguments += ["--print", "before_dl:\(Self.titlePrefix)%(title)s"]
        arguments += ["-f", format]
        arguments += ["-o", outputDirectory.appendingPathComponent("out.%(ext)s").path]
        arguments.append(url.absoluteString)

        var environment = ProcessInfo.processInfo.environment
        environment["LIVE_WALL_OUTPUT_DIR"] = outputDirectory.path
        return ProcessInvocation(executable: executable, arguments: arguments, environment: environment)
    }

    private func handleLine(_ line: ProcessLine, jobID: UUID, stage: DownloadProgressAccumulator.Stage) {
        let text = line.text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard var job = jobs[jobID] else {
            return
        }
        if text.hasPrefix(Self.titlePrefix) {
            if job.title == nil {
                let title = String(text.dropFirst(Self.titlePrefix.count)).trimmingCharacters(in: .whitespaces)
                job.title = title.isEmpty ? nil : title
                jobs[jobID] = job
            }
            return
        }
        guard let update = YTDLP.parseProgress(line: text) else {
            return
        }
        let percent = job.accumulator.apply(update.percent, stage: stage)
        job.progress = percent
        jobs[jobID] = job
        emit(.progress(jobID: jobID, percent: percent, title: job.title))
    }

    private func finalize(_ jobID: UUID, downloaded: URL) throws {
        guard var job = jobs[jobID] else {
            return
        }
        let fileExtension = downloaded.pathExtension.isEmpty ? "mp4" : downloaded.pathExtension
        let base = sanitizedBaseName(job: job) + "." + fileExtension
        let destination = try uniqueDestination(base: base)
        try FileManager.default.moveItem(at: downloaded, to: destination)
        job.state = .done
        job.progress = 100
        job.path = destination
        jobs[jobID] = job
        try? FileManager.default.removeItem(at: job.directory)
        emit(.finished(makeStatus(job)))
    }

    private func wipe(_ jobID: UUID, state: DownloadState, error: String?) {
        guard var job = jobs[jobID] else {
            return
        }
        job.process?.terminate()
        job.process = nil
        try? FileManager.default.removeItem(at: job.directory)
        job.state = state
        job.error = error
        jobs[jobID] = job
        emit(.finished(makeStatus(job)))
    }

    private func makeStatus(_ job: Job) -> DownloadJobStatus {
        return DownloadJobStatus(
            jobID: job.id,
            state: job.state,
            progress: job.progress,
            title: job.title,
            path: job.path,
            error: job.error
        )
    }

    private func sanitizedBaseName(job: Job) -> String {
        guard let title = job.title, !title.isEmpty else {
            return job.id.uuidString
        }
        let sanitized = sanitize(title)
        return sanitized.isEmpty ? job.id.uuidString : sanitized
    }

    private func sanitize(_ title: String) -> String {
        let invalid = CharacterSet(charactersIn: "/\\:*?\"<>|").union(.controlCharacters).union(.newlines)
        var scalars = String.UnicodeScalarView()
        for scalar in title.unicodeScalars {
            if invalid.contains(scalar) {
                scalars.append("_")
            } else {
                scalars.append(scalar)
            }
        }
        var result = String(scalars).trimmingCharacters(in: .whitespacesAndNewlines)
        while result.hasPrefix(".") {
            result.removeFirst()
        }
        if result.count > 200 {
            result = String(result.prefix(200))
        }
        return result.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func uniqueDestination(base: String) throws -> URL {
        try FileManager.default.createDirectory(at: incomingDirectory, withIntermediateDirectories: true)
        let nsBase = base as NSString
        let stem = nsBase.deletingPathExtension
        let fileExtension = nsBase.pathExtension
        var candidate = incomingDirectory.appendingPathComponent(base)
        var index = 1
        while FileManager.default.fileExists(atPath: candidate.path) {
            let name = fileExtension.isEmpty ? "\(stem)-\(index)" : "\(stem)-\(index).\(fileExtension)"
            candidate = incomingDirectory.appendingPathComponent(name)
            index += 1
        }
        return candidate
    }

    private func firstFile(in directory: URL, jobID: UUID) throws -> URL {
        let contents = try FileManager.default.contentsOfDirectory(
            at: directory,
            includingPropertiesForKeys: [.isDirectoryKey],
            options: [.skipsHiddenFiles]
        )
        let ignored: Set<String> = ["part", "ytdl", "temp"]
        let file = contents.first { url in
            let ext = url.pathExtension.lowercased()
            let isDirectory = (try? url.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) ?? false
            return !isDirectory && !ignored.contains(ext)
        }
        guard let file else {
            throw LiveWallError.downloadFailed(jobID: jobID.uuidString, stderr: "no downloaded file produced")
        }
        return file
    }

    private func isFormatUnavailable(_ stderr: String) -> Bool {
        let lowered = stderr.lowercased()
        return lowered.contains("requested format is not available")
            || lowered.contains("no video formats found")
            || lowered.contains("requested format not available")
    }

    private func describe(_ error: Error) -> String {
        if let liveWallError = error as? LiveWallError {
            switch liveWallError {
            case let .downloadFailed(_, stderr):
                return stderr
            case let .toolMissing(tool):
                return "missing tool: \(tool)"
            case let .ipc(code, message):
                return "\(code): \(message)"
            case let .toolAcquisitionFailed(tools, stderr):
                return "tool install failed: \(tools.joined(separator: ", ")) (\(stderr))"
            case let .mediaUnreadable(path, reason):
                return "unreadable media: \(path) (\(reason))"
            case let .codecUnsupported(path):
                return "unsupported codec: \(path)"
            case let .mediaRejected(reason):
                return "media rejected: \(reason)"
            case .consentRequired:
                return "tool install consent required"
            case let .libraryCorrupt(path):
                return "corrupt library: \(path)"
            case let .swapStateCorrupt(path):
                return "corrupt swap state: \(path)"
            case let .stillUnavailable(path, reason):
                return "still unavailable: \(path) (\(reason))"
            }
        }
        return String(describing: error)
    }

    private func requireJob(_ jobID: UUID) throws -> Job {
        guard let job = jobs[jobID] else {
            throw CancellationError()
        }
        return job
    }

    private nonisolated func requireTool(_ tool: MediaTool) throws -> URL {
        guard let url = resolver.executableURL(for: tool) else {
            throw LiveWallError.toolMissing(tool: tool.executableName)
        }
        return url
    }

    private func emit(_ event: DownloadEvent) {
        for continuation in listeners.values {
            continuation.yield(event)
        }
    }
}
