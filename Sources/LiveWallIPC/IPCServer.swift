import Darwin
import Foundation

public enum IPCServerError: Error, Equatable {
    case addressTooLong(String)
    case socketFailed(Int32)
    case bindFailed(Int32)
    case listenFailed(Int32)
}

public final class IPCServer: @unchecked Sendable {
    public typealias Handler = @Sendable (Request) async -> Response

    public let socketURL: URL

    private let handler: Handler
    private let queue = DispatchQueue(label: "io.github.belhetile.livewall.ipc.server")
    private var listenFD: Int32 = -1
    private var source: DispatchSourceRead?
    private var busy = false

    public init(socketURL: URL, handler: @escaping Handler) {
        self.socketURL = socketURL
        self.handler = handler
    }

    public func start() throws {
        try queue.sync {
            try startLocked()
        }
    }

    public func stop() {
        queue.sync {
            stopLocked()
        }
    }

    private func startLocked() throws {
        guard listenFD < 0 else {
            return
        }
        let path = socketURL.path
        guard let address = UnixSocket.makeAddress(path) else {
            throw IPCServerError.addressTooLong(path)
        }

        try? FileManager.default.createDirectory(
            at: socketURL.deletingLastPathComponent(),
            withIntermediateDirectories: true,
            attributes: [.posixPermissions: 0o700]
        )
        unlink(path)

        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else {
            throw IPCServerError.socketFailed(errno)
        }

        var mutableAddress = address
        let bindResult = withUnsafePointer(to: &mutableAddress) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { sockaddrPointer in
                bind(fd, sockaddrPointer, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard bindResult == 0 else {
            close(fd)
            throw IPCServerError.bindFailed(errno)
        }

        chmod(path, mode_t(0o600))

        guard listen(fd, 1) == 0 else {
            close(fd)
            unlink(path)
            throw IPCServerError.listenFailed(errno)
        }
        listenFD = fd

        let readSource = DispatchSource.makeReadSource(fileDescriptor: fd, queue: queue)
        readSource.setEventHandler { [weak self] in
            self?.acceptReady()
        }
        readSource.resume()
        source = readSource
    }

    private func stopLocked() {
        source?.cancel()
        source = nil
        if listenFD >= 0 {
            close(listenFD)
            listenFD = -1
        }
        unlink(socketURL.path)
        busy = false
    }

    private func acceptReady() {
        guard listenFD >= 0 else {
            return
        }
        let client = accept(listenFD, nil, nil)
        guard client >= 0 else {
            return
        }
        guard !busy else {
            close(client)
            return
        }
        busy = true

        var timeout = timeval(tv_sec: 2, tv_usec: 0)
        setsockopt(client, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))

        let data = UnixSocket.readLine(client)
        Task { [handler] in
            let response: Response
            do {
                let request = try IPCCodec.decodeRequest(from: data)
                response = await handler(request)
            } catch let error as IPCCodecError {
                response = Self.errorResponse(error)
            } catch {
                response = Response.failure(code: "EIPC", message: String(describing: error))
            }
            self.finish(client: client, response: response)
        }
    }

    private func finish(client: Int32, response: Response) {
        queue.async { [weak self] in
            guard let self else {
                close(client)
                return
            }
            let data = (try? IPCCodec.line(response)) ?? Data()
            UnixSocket.write(data, to: client)
            close(client)
            self.busy = false
        }
    }

    static func errorResponse(_ error: IPCCodecError) -> Response {
        switch error {
        case .emptyLine:
            return .failure(code: "EEMPTY", message: "empty request")
        case .malformedLine:
            return .failure(code: "EMALFORMED", message: "malformed request")
        case .unsupportedVersion(let version):
            return .failure(code: "EVERSION", message: "unsupported protocol version \(version)")
        }
    }

    deinit {
        source?.cancel()
        if listenFD >= 0 {
            close(listenFD)
        }
        unlink(socketURL.path)
    }
}
