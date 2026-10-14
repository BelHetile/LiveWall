import Darwin
import Foundation

public enum IPCClientError: Error, Equatable {
    case appNotRunning
    case addressTooLong(String)
    case ioFailure
    case malformedResponse
}

public struct IPCClient: Sendable {
    public let socketURL: URL

    public init(socketURL: URL) {
        self.socketURL = socketURL
    }

    public func send(_ request: Request) throws -> Response {
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else {
            throw IPCClientError.ioFailure
        }
        defer { close(fd) }

        guard let address = UnixSocket.makeAddress(socketURL.path) else {
            throw IPCClientError.addressTooLong(socketURL.path)
        }
        var mutableAddress = address
        let result = withUnsafePointer(to: &mutableAddress) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) { sockaddrPointer in
                connect(fd, sockaddrPointer, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard result == 0 else {
            switch errno {
            case ECONNREFUSED, ENOENT, ENOTCONN:
                throw IPCClientError.appNotRunning
            default:
                throw IPCClientError.ioFailure
            }
        }

        var timeout = timeval(tv_sec: 5, tv_usec: 0)
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))

        let line = try IPCCodec.line(request)
        UnixSocket.write(line, to: fd)
        let data = UnixSocket.readLine(fd)
        do {
            return try IPCCodec.decodeResponse(from: data)
        } catch {
            throw IPCClientError.malformedResponse
        }
    }
}
