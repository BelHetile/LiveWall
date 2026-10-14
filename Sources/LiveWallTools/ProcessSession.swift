import Foundation

public struct ProcessLine: Sendable, Equatable {
    public let text: String
    public let isStandardError: Bool

    public init(text: String, isStandardError: Bool) {
        self.text = text
        self.isStandardError = isStandardError
    }
}

public final class ProcessSession: @unchecked Sendable {
    public let lines: AsyncStream<ProcessLine>

    private let process: Process
    private let completion: Task<ProcessResult, Error>
    private let lock = NSLock()
    private var terminated = false

    init(process: Process, lines: AsyncStream<ProcessLine>, completion: Task<ProcessResult, Error>) {
        self.process = process
        self.lines = lines
        self.completion = completion
    }

    public var pid: Int32 {
        return process.processIdentifier
    }

    public func terminate() {
        lock.lock()
        let alreadyTerminated = terminated
        terminated = true
        let identifier = process.processIdentifier
        let running = process.isRunning
        lock.unlock()
        guard !alreadyTerminated, running, identifier > 0 else {
            return
        }
        kill(identifier, SIGTERM)
    }

    public func wait() async throws -> ProcessResult {
        return try await completion.value
    }
}

public enum ProcessLauncher {
    public static func launch(_ invocation: ProcessInvocation) throws -> ProcessSession {
        let process = Process()
        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()

        process.executableURL = invocation.executable
        process.arguments = invocation.arguments
        if let environment = invocation.environment {
            process.environment = environment
        }
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe

        let (stream, continuation) = AsyncStream<ProcessLine>.makeStream()
        let stdoutReader = LineReader(continuation: continuation, isStandardError: false)
        let stderrReader = LineReader(continuation: continuation, isStandardError: true)
        let resultBox = ResultBox()

        process.terminationHandler = { terminated in
            stdoutPipe.fileHandleForReading.readabilityHandler = nil
            stderrPipe.fileHandleForReading.readabilityHandler = nil

            stdoutReader.append(stdoutPipe.fileHandleForReading.readDataToEndOfFile())
            stdoutReader.flush()
            stderrReader.append(stderrPipe.fileHandleForReading.readDataToEndOfFile())
            stderrReader.flush()

            continuation.finish()
            resultBox.resume(returning: ProcessResult(
                exitCode: terminated.terminationStatus,
                stdout: stdoutReader.captured,
                stderr: stderrReader.captured
            ))
        }

        stdoutPipe.fileHandleForReading.readabilityHandler = { handle in
            let data = handle.availableData
            guard !data.isEmpty else {
                return
            }
            stdoutReader.append(data)
        }
        stderrPipe.fileHandleForReading.readabilityHandler = { handle in
            let data = handle.availableData
            guard !data.isEmpty else {
                return
            }
            stderrReader.append(data)
        }

        do {
            try process.run()
        } catch {
            stdoutPipe.fileHandleForReading.readabilityHandler = nil
            stderrPipe.fileHandleForReading.readabilityHandler = nil
            continuation.finish()
            throw ProcessRunError.launchFailed(error.localizedDescription)
        }

        let completion = Task {
            return try await resultBox.wait()
        }
        return ProcessSession(process: process, lines: stream, completion: completion)
    }
}

private final class LineReader: @unchecked Sendable {
    private let lock = NSLock()
    private var pending = Data()
    private var capturedData = Data()
    private let continuation: AsyncStream<ProcessLine>.Continuation
    private let isStandardError: Bool

    init(continuation: AsyncStream<ProcessLine>.Continuation, isStandardError: Bool) {
        self.continuation = continuation
        self.isStandardError = isStandardError
    }

    var captured: String {
        lock.lock()
        defer { lock.unlock() }
        return String(decoding: capturedData, as: UTF8.self)
    }

    func append(_ data: Data) {
        guard !data.isEmpty else {
            return
        }
        var lines: [String] = []
        lock.lock()
        capturedData.append(data)
        pending.append(data)
        while let newlineIndex = pending.firstIndex(of: 0x0A) {
            let lineData = pending[pending.startIndex..<newlineIndex]
            lines.append(String(decoding: lineData, as: UTF8.self))
            pending.removeSubrange(pending.startIndex...newlineIndex)
        }
        lock.unlock()
        for line in lines {
            continuation.yield(ProcessLine(text: line, isStandardError: isStandardError))
        }
    }

    func flush() {
        var trailing: String?
        lock.lock()
        if !pending.isEmpty {
            trailing = String(decoding: pending, as: UTF8.self)
            pending.removeAll()
        }
        lock.unlock()
        if let trailing, !trailing.isEmpty {
            continuation.yield(ProcessLine(text: trailing, isStandardError: isStandardError))
        }
    }
}

private final class ResultBox: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: Result<ProcessResult, Error>?
    private var continuation: CheckedContinuation<ProcessResult, Error>?

    func resume(returning result: ProcessResult) {
        resolve(.success(result))
    }

    func resume(throwing error: Error) {
        resolve(.failure(error))
    }

    func wait() async throws -> ProcessResult {
        return try await withCheckedThrowingContinuation { continuation in
            lock.lock()
            if let stored {
                self.stored = nil
                lock.unlock()
                continuation.resume(with: stored)
            } else {
                self.continuation = continuation
                lock.unlock()
            }
        }
    }

    private func resolve(_ result: Result<ProcessResult, Error>) {
        lock.lock()
        if let continuation {
            self.continuation = nil
            lock.unlock()
            continuation.resume(with: result)
        } else {
            stored = result
            lock.unlock()
        }
    }
}
