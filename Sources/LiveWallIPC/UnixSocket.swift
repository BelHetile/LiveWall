import Darwin
import Foundation

enum UnixSocket {
    static func makeAddress(_ path: String) -> sockaddr_un? {
        var address = sockaddr_un()
        address.sun_family = sa_family_t(AF_UNIX)
        let capacity = MemoryLayout.size(ofValue: address.sun_path)
        guard path.utf8.count < capacity else {
            return nil
        }
        withUnsafeMutablePointer(to: &address.sun_path.0) { pointer in
            path.withCString { source in
                _ = strlcpy(pointer, source, capacity)
            }
        }
        return address
    }

    static func readLine(_ fd: Int32) -> Data {
        var buffer = [UInt8](repeating: 0, count: 4096)
        var collected = Data()
        while true {
            let count = Darwin.read(fd, &buffer, buffer.count)
            if count <= 0 {
                break
            }
            if let newline = buffer[0..<count].firstIndex(of: 0x0A) {
                collected.append(contentsOf: buffer[0..<newline])
                break
            }
            collected.append(contentsOf: buffer[0..<count])
            if collected.count > 64 * 1024 {
                break
            }
        }
        return collected
    }

    static func write(_ data: Data, to fd: Int32) {
        data.withUnsafeBytes { raw in
            guard let base = raw.baseAddress else {
                return
            }
            var offset = 0
            while offset < raw.count {
                let written = Darwin.write(fd, base.advanced(by: offset), raw.count - offset)
                if written <= 0 {
                    break
                }
                offset += written
            }
        }
    }
}
