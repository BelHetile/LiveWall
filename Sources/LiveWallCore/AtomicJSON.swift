import Foundation

public enum AtomicJSONError: Error, Equatable {
    case writeFailed(String)
}

public enum AtomicJSON {
    public static func makeEncoder() -> JSONEncoder {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        encoder.dateEncodingStrategy = .iso8601
        return encoder
    }

    public static func makeDecoder() -> JSONDecoder {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return decoder
    }

    public static func encode<T: Encodable>(_ value: T) throws -> Data {
        return try makeEncoder().encode(value)
    }

    public static func write<T: Encodable>(_ value: T, to url: URL) throws {
        try writeData(try encode(value), to: url)
    }

    public static func writeData(_ data: Data, to url: URL) throws {
        let fm = FileManager.default
        let directory = url.deletingLastPathComponent()
        try fm.createDirectory(at: directory, withIntermediateDirectories: true)

        let temp = directory.appendingPathComponent(".\(url.lastPathComponent).\(UUID().uuidString).tmp")
        do {
            try data.write(to: temp)
        } catch {
            try? fm.removeItem(at: temp)
            throw AtomicJSONError.writeFailed(error.localizedDescription)
        }

        guard rename(temp.path, url.path) == 0 else {
            let code = errno
            try? fm.removeItem(at: temp)
            throw AtomicJSONError.writeFailed(String(cString: strerror(code)))
        }
    }

    public static func readData(from url: URL) throws -> Data {
        return try Data(contentsOf: url)
    }

    public static func read<T: Decodable>(_ type: T.Type, from url: URL) throws -> T {
        return try makeDecoder().decode(T.self, from: readData(from: url))
    }
}
