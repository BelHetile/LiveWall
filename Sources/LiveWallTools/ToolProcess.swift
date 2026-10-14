import Foundation

public struct ProcessInvocation: Sendable {
    public let executable: URL
    public let arguments: [String]
    public let environment: [String: String]?

    public init(executable: URL, arguments: [String] = [], environment: [String: String]? = nil) {
        self.executable = executable
        self.arguments = arguments
        self.environment = environment
    }
}

public struct ProcessResult: Sendable, Equatable {
    public let exitCode: Int32
    public let stdout: String
    public let stderr: String

    public init(exitCode: Int32, stdout: String, stderr: String) {
        self.exitCode = exitCode
        self.stdout = stdout
        self.stderr = stderr
    }
}

public enum ProcessRunError: Error, Sendable, Equatable {
    case launchFailed(String)
}

public struct ProcessRunner: Sendable {
    public init() {}

    public func run(_ invocation: ProcessInvocation) async throws -> ProcessResult {
        let box = ProcessBox(invocation: invocation)
        return try await withCheckedThrowingContinuation { continuation in
            box.launch { result in
                continuation.resume(with: result)
            }
        }
    }
}

private final class DataBuffer: @unchecked Sendable {
    private let lock = NSLock()
    private var storage = Data()

    func append(_ data: Data) {
        lock.lock()
        storage.append(data)
        lock.unlock()
    }

    func snapshot() -> Data {
        lock.lock()
        defer { lock.unlock() }
        return storage
    }
}

private final class ProcessBox: @unchecked Sendable {
    private let process = Process()
    private let stdoutPipe = Pipe()
    private let stderrPipe = Pipe()
    private let stdoutBuffer = DataBuffer()
    private let stderrBuffer = DataBuffer()
    private var completion: (@Sendable (Result<ProcessResult, Error>) -> Void)?

    init(invocation: ProcessInvocation) {
        process.executableURL = invocation.executable
        process.arguments = invocation.arguments
        if let environment = invocation.environment {
            process.environment = environment
        }
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe
    }

    func launch(completion: @escaping @Sendable (Result<ProcessResult, Error>) -> Void) {
        self.completion = completion

        let stdoutBuffer = self.stdoutBuffer
        let stderrBuffer = self.stderrBuffer

        stdoutPipe.fileHandleForReading.readabilityHandler = { handle in
            let data = handle.availableData
            if !data.isEmpty {
                stdoutBuffer.append(data)
            }
        }
        stderrPipe.fileHandleForReading.readabilityHandler = { handle in
            let data = handle.availableData
            if !data.isEmpty {
                stderrBuffer.append(data)
            }
        }

        process.terminationHandler = { [weak self] terminated in
            guard let self else {
                return
            }
            self.stdoutPipe.fileHandleForReading.readabilityHandler = nil
            self.stderrPipe.fileHandleForReading.readabilityHandler = nil

            let stdoutTail = self.stdoutPipe.fileHandleForReading.readDataToEndOfFile()
            if !stdoutTail.isEmpty {
                self.stdoutBuffer.append(stdoutTail)
            }
            let stderrTail = self.stderrPipe.fileHandleForReading.readDataToEndOfFile()
            if !stderrTail.isEmpty {
                self.stderrBuffer.append(stderrTail)
            }

            let result = ProcessResult(
                exitCode: terminated.terminationStatus,
                stdout: String(decoding: self.stdoutBuffer.snapshot(), as: UTF8.self),
                stderr: String(decoding: self.stderrBuffer.snapshot(), as: UTF8.self)
            )
            self.completion?(.success(result))
        }

        do {
            try process.run()
        } catch {
            stdoutPipe.fileHandleForReading.readabilityHandler = nil
            stderrPipe.fileHandleForReading.readabilityHandler = nil
            completion(.failure(ProcessRunError.launchFailed(error.localizedDescription)))
        }
    }
}
