import Foundation

public enum IPCCodecError: Error, Equatable {
    case emptyLine
    case malformedLine
    case unsupportedVersion(Int)
}

public enum IPCCodec {
    private static func makeEncoder() -> JSONEncoder {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        return encoder
    }

    private static func makeDecoder() -> JSONDecoder {
        return JSONDecoder()
    }

    public static func encode(_ request: Request) throws -> Data {
        return try makeEncoder().encode(request)
    }

    public static func encode(_ response: Response) throws -> Data {
        return try makeEncoder().encode(response)
    }

    public static func line(_ request: Request) throws -> Data {
        return try makeEncoder().encode(request) + Data([0x0A])
    }

    public static func line(_ response: Response) throws -> Data {
        return try makeEncoder().encode(response) + Data([0x0A])
    }

    public static func decodeRequest(from data: Data) throws -> Request {
        guard !data.isEmpty else {
            throw IPCCodecError.emptyLine
        }
        let request: Request
        do {
            request = try makeDecoder().decode(Request.self, from: data)
        } catch {
            throw IPCCodecError.malformedLine
        }
        guard request.v == IPCProtocol.version else {
            throw IPCCodecError.unsupportedVersion(request.v)
        }
        return request
    }

    public static func decodeResponse(from data: Data) throws -> Response {
        guard !data.isEmpty else {
            throw IPCCodecError.emptyLine
        }
        do {
            return try makeDecoder().decode(Response.self, from: data)
        } catch {
            throw IPCCodecError.malformedLine
        }
    }

    public static func decodeRequest(from line: String) throws -> Request {
        return try decodeRequest(from: Data(line.utf8))
    }

    public static func decodeResponse(from line: String) throws -> Response {
        return try decodeResponse(from: Data(line.utf8))
    }
}
