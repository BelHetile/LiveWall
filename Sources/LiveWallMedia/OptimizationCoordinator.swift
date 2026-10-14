import Foundation
import LiveWallCore
import LiveWallTools

public enum OptimizationState: String, Sendable, Equatable {
    case queued
    case running
    case done
    case failed
    case canceled
}

public enum OptimizationOutcome: Sendable, Equatable {
    case alreadyLightEnough(url: URL)
    case transcoded(url: URL, videoCodec: String, audio: TranscodeAudioOutcome)
    case rejected(reason: String)

    public var url: URL? {
        switch self {
        case let .alreadyLightEnough(url):
            return url
        case let .transcoded(url, _, _):
            return url
        case .rejected:
            return nil
        }
    }

    public var message: String? {
        switch self {
        case .alreadyLightEnough:
            return "already light enough"
        case let .transcoded(_, _, audio):
            return audio.message
        case let .rejected(reason):
            return "media rejected: \(reason)"
        }
    }
}

public struct OptimizationJobStatus: Sendable, Equatable {
    public let jobID: UUID
    public let itemID: String
    public let state: OptimizationState
    public let outcome: OptimizationOutcome?
    public let error: String?

    public init(
        jobID: UUID,
        itemID: String,
        state: OptimizationState,
        outcome: OptimizationOutcome?,
        error: String?
    ) {
        self.jobID = jobID
        self.itemID = itemID
        self.state = state
        self.outcome = outcome
        self.error = error
    }
}

public enum OptimizationEvent: Sendable, Equatable {
    case queued(jobID: UUID)
    case started(jobID: UUID)
    case finished(OptimizationJobStatus)
}

public enum OptimizationDecision: Sendable, Equatable {
    case alreadyLightEnough
    case transcode
    case rejected(reason: String)
}

public actor OptimizationCoordinator {
    private struct Job {
        let id: UUID
        let url: URL
        let itemID: String
        let maxHeight: Int
        let replaceInPlace: Bool
        let presetProbe: MediaProbeResult?
        var state: OptimizationState
        var outcome: OptimizationOutcome?
        var error: String?
        var cancelRequested: Bool
        let workDirectory: URL
    }

    private let probe: MediaProbeService
    private let transcoder: TranscodeService
    private let workDirectory: URL
    private let fpsLimitProvider: @Sendable () -> Int
    private var jobs: [UUID: Job] = [:]
    private var queue: [UUID] = []
    private var isPumping = false
    private var listeners: [UUID: AsyncStream<OptimizationEvent>.Continuation] = [:]

    public init(
        resolver: any ToolExecutableResolving = ToolLocator.system,
        workDirectory: URL = LiveWallPaths.cacheDirectory(),
        av1HardwareSupport: @escaping @Sendable () -> Bool = AV1HardwareSupport.isSupported,
        hevcAvailable: @escaping @Sendable () -> Bool = VideoToolboxHEVCEncoder.isSupported,
        fpsLimitProvider: @escaping @Sendable () -> Int = OptimizationCoordinator.defaultFpsLimit,
        runner: ProcessRunner = ProcessRunner()
    ) {
        self.workDirectory = workDirectory
        self.fpsLimitProvider = fpsLimitProvider
        self.probe = MediaProbeService(
            resolver: resolver,
            runner: runner,
            av1HardwareSupport: av1HardwareSupport
        )
        self.transcoder = TranscodeService(resolver: resolver, hevcAvailable: hevcAvailable)
    }

    @discardableResult
    public func enqueue(
        url: URL,
        itemID: String,
        maxHeight: Int,
        replaceInPlace: Bool = false,
        presetProbe: MediaProbeResult? = nil
    ) -> UUID {
        let jobID = UUID()
        let directory = workDirectory.appendingPathComponent(jobID.uuidString, isDirectory: true)
        jobs[jobID] = Job(
            id: jobID,
            url: url,
            itemID: itemID,
            maxHeight: maxHeight,
            replaceInPlace: replaceInPlace,
            presetProbe: presetProbe,
            state: .queued,
            outcome: nil,
            error: nil,
            cancelRequested: false,
            workDirectory: directory
        )
        queue.append(jobID)
        emit(.queued(jobID: jobID))
        pump()
        return jobID
    }

    public func status(_ jobID: UUID) -> OptimizationJobStatus? {
        guard let job = jobs[jobID] else {
            return nil
        }
        return makeStatus(job)
    }

    public func cancel(_ jobID: UUID) async {
        guard let job = jobs[jobID] else {
            return
        }
        switch job.state {
        case .queued:
            var updated = job
            updated.state = .canceled
            updated.cancelRequested = true
            jobs[jobID] = updated
            queue.removeAll { $0 == jobID }
            emit(.finished(makeStatus(updated)))
        case .running:
            var updated = job
            updated.cancelRequested = true
            jobs[jobID] = updated
            await transcoder.cancelActive()
        case .done, .failed, .canceled:
            return
        }
    }

    public func cancelAll() async {
        for jobID in queue {
            guard var job = jobs[jobID], job.state == .queued else {
                continue
            }
            job.state = .canceled
            job.cancelRequested = true
            jobs[jobID] = job
            emit(.finished(makeStatus(job)))
        }
        queue.removeAll()
        if let running = jobs.values.first(where: { $0.state == .running }) {
            var job = running
            job.cancelRequested = true
            jobs[running.id] = job
            await transcoder.cancelActive()
        }
    }

    public func events() -> AsyncStream<OptimizationEvent> {
        let (stream, continuation) = AsyncStream<OptimizationEvent>.makeStream()
        let identifier = UUID()
        listeners[identifier] = continuation
        continuation.onTermination = { [weak self] _ in
            Task { await self?.removeListener(identifier) }
        }
        return stream
    }

    public static func defaultFpsLimit() -> Int {
        let suite = UserDefaults(suiteName: "io.github.belhetile.livewall")
        if let value = suite?.object(forKey: "optimizeFpsLimit") as? Int {
            return TranscodeArguments.clampFpsLimit(value)
        }
        if let blob = suite?.data(forKey: "settings"),
           let object = try? JSONSerialization.jsonObject(with: blob) as? [String: Any],
           let value = object["optimizeFpsLimit"] as? Int {
            return TranscodeArguments.clampFpsLimit(value)
        }
        return 30
    }

    static func decide(probeResult: MediaProbeResult, maxHeight: Int, fpsLimit: Int) -> OptimizationDecision {
        switch probeResult.verdict {
        case let .rejected(reason):
            return .rejected(reason: reason)
        case .transcodeToHEVC:
            return .transcode
        case .native:
            let video = probeResult.metadata?.video
            let height = video?.height ?? 0
            let frameRate = video?.frameRate ?? 0
            let withinHeight = height <= maxHeight
            let withinFps = frameRate <= Double(fpsLimit)
            return withinHeight && withinFps ? .alreadyLightEnough : .transcode
        }
    }

    private func removeListener(_ identifier: UUID) {
        listeners[identifier] = nil
    }

    private func pump() {
        guard !isPumping else {
            return
        }
        isPumping = true
        Task { [weak self] in
            await self?.drain()
        }
    }

    private func drain() async {
        while let jobID = dequeue() {
            await runJob(jobID)
        }
        isPumping = false
    }

    private func dequeue() -> UUID? {
        while !queue.isEmpty {
            let jobID = queue.removeFirst()
            if let job = jobs[jobID], job.state == .queued {
                return jobID
            }
        }
        return nil
    }

    private func runJob(_ jobID: UUID) async {
        guard var job = jobs[jobID], job.state == .queued else {
            return
        }
        job.state = .running
        jobs[jobID] = job
        emit(.started(jobID: jobID))

        do {
            let probeResult = try await resolveProbe(job)
            try checkCancel(jobID)
            let decision = Self.decide(
                probeResult: probeResult,
                maxHeight: job.maxHeight,
                fpsLimit: fpsLimitProvider()
            )
            switch decision {
            case .alreadyLightEnough:
                finish(jobID, state: .done, outcome: .alreadyLightEnough(url: job.url), error: nil)
            case let .rejected(reason):
                finish(jobID, state: .failed, outcome: .rejected(reason: reason), error: "media rejected: \(reason)")
            case .transcode:
                guard let metadata = probeResult.metadata, metadata.video != nil else {
                    finish(
                        jobID,
                        state: .failed,
                        outcome: .rejected(reason: "no video stream"),
                        error: "media rejected: no video stream"
                    )
                    return
                }
                let result = try await performTranscode(job: job, metadata: metadata)
                try checkCancel(jobID)
                finalizeTranscode(jobID, result: result)
            }
        } catch is CancellationError {
            finish(jobID, state: .canceled, outcome: nil, error: nil)
        } catch {
            finish(jobID, state: .failed, outcome: nil, error: describe(error))
        }
    }

    private func resolveProbe(_ job: Job) async throws -> MediaProbeResult {
        if let presetProbe = job.presetProbe {
            return presetProbe
        }
        return try await probe.probe(job.url)
    }

    private func performTranscode(job: Job, metadata: MediaMetadata) async throws -> TranscodeResult {
        try FileManager.default.createDirectory(at: job.workDirectory, withIntermediateDirectories: true)
        let output = job.workDirectory.appendingPathComponent("\(outputStem(job.url)).mov")
        let parameters = TranscodeParameters(maxHeight: job.maxHeight, fpsLimit: fpsLimitProvider())
        return try await transcoder.transcode(
            input: job.url,
            output: output,
            source: metadata,
            parameters: parameters
        )
    }

    private func finalizeTranscode(_ jobID: UUID, result: TranscodeResult) {
        guard var job = jobs[jobID] else {
            return
        }
        var finalURL = result.outputURL
        if job.replaceInPlace {
            do {
                finalURL = try Self.replace(source: job.url, with: result.outputURL)
            } catch {
                finish(jobID, state: .failed, outcome: nil, error: describe(error))
                return
            }
            try? FileManager.default.removeItem(at: job.workDirectory)
        }
        job.state = .done
        job.outcome = .transcoded(url: finalURL, videoCodec: result.videoCodec, audio: result.audio)
        job.error = nil
        jobs[jobID] = job
        emit(.finished(makeStatus(job)))
    }

    private static func replace(source: URL, with output: URL) throws -> URL {
        let manager = FileManager.default
        if let replaced = try? manager.replaceItemAt(source, withItemAt: output) {
            return replaced
        }
        if manager.fileExists(atPath: source.path) {
            try manager.removeItem(at: source)
        }
        try manager.moveItem(at: output, to: source)
        return source
    }

    private func finish(
        _ jobID: UUID,
        state: OptimizationState,
        outcome: OptimizationOutcome?,
        error: String?
    ) {
        guard var job = jobs[jobID] else {
            return
        }
        try? FileManager.default.removeItem(at: job.workDirectory)
        job.state = state
        job.outcome = outcome
        job.error = error
        jobs[jobID] = job
        emit(.finished(makeStatus(job)))
    }

    private func checkCancel(_ jobID: UUID) throws {
        if jobs[jobID]?.cancelRequested == true {
            throw CancellationError()
        }
    }

    private func makeStatus(_ job: Job) -> OptimizationJobStatus {
        return OptimizationJobStatus(
            jobID: job.id,
            itemID: job.itemID,
            state: job.state,
            outcome: job.outcome,
            error: job.error
        )
    }

    private func outputStem(_ url: URL) -> String {
        let stem = url.deletingPathExtension().lastPathComponent
        return stem.isEmpty ? "optimized" : stem
    }

    private func describe(_ error: Error) -> String {
        if let liveWallError = error as? LiveWallError {
            switch liveWallError {
            case let .mediaUnreadable(path, reason):
                return "unreadable media: \(path) (\(reason))"
            case let .mediaRejected(reason):
                return "media rejected: \(reason)"
            case let .codecUnsupported(path):
                return "unsupported codec: \(path)"
            case let .downloadFailed(jobID, stderr):
                return "download failed: \(jobID) (\(stderr))"
            case let .transcodeFailed(reason):
                return "transcode failed: \(reason)"
            case let .toolMissing(tool):
                return "missing tool: \(tool)"
            case let .toolAcquisitionFailed(tools, stderr):
                return "tool install failed: \(tools.joined(separator: ", ")) (\(stderr))"
            case .consentRequired:
                return "tool install consent required"
            case let .libraryCorrupt(path):
                return "corrupt library: \(path)"
            case let .ipc(code, message):
                return "\(code): \(message)"
            case let .swapStateCorrupt(path):
                return "corrupt swap state: \(path)"
            case let .stillUnavailable(path, reason):
                return "still unavailable: \(path) (\(reason))"
            }
        }
        return String(describing: error)
    }

    private func emit(_ event: OptimizationEvent) {
        for continuation in listeners.values {
            continuation.yield(event)
        }
    }
}
