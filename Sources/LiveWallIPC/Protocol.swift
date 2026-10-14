import Foundation

public enum Verb: String, Codable, Sendable, CaseIterable {
    case pause
    case resume
    case mute
    case unmute
    case next
    case previous
    case random
    case open
    case getCurrent = "get-current"
    case getStatus = "get-status"
}

public struct Request: Codable, Sendable, Equatable {
    public var v: Int
    public var cmd: Verb
    public var path: String?

    public init(v: Int = IPCProtocol.version, cmd: Verb, path: String? = nil) {
        self.v = v
        self.cmd = cmd
        self.path = path
    }
}

public struct ErrorBody: Codable, Sendable, Equatable {
    public var code: String
    public var message: String

    public init(code: String, message: String) {
        self.code = code
        self.message = message
    }
}

public struct Response: Codable, Sendable, Equatable {
    public var ok: Bool
    public var data: JSONValue?
    public var error: ErrorBody?

    public init(ok: Bool, data: JSONValue? = nil, error: ErrorBody? = nil) {
        self.ok = ok
        self.data = data
        self.error = error
    }

    public static func success(_ data: JSONValue? = nil) -> Response {
        return Response(ok: true, data: data)
    }

    public static func failure(code: String, message: String) -> Response {
        return Response(ok: false, error: ErrorBody(code: code, message: message))
    }
}

public enum JSONValue: Codable, Sendable, Equatable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
            return
        }
        if let bool = try? container.decode(Bool.self) {
            self = .bool(bool)
            return
        }
        if let number = try? container.decode(Double.self) {
            self = .number(number)
            return
        }
        if let string = try? container.decode(String.self) {
            self = .string(string)
            return
        }
        if let array = try? container.decode([JSONValue].self) {
            self = .array(array)
            return
        }
        if let object = try? container.decode([String: JSONValue].self) {
            self = .object(object)
            return
        }
        throw DecodingError.dataCorruptedError(in: container, debugDescription: "unsupported JSON value")
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .null:
            try container.encodeNil()
        case .bool(let value):
            try container.encode(value)
        case .number(let value):
            try container.encode(value)
        case .string(let value):
            try container.encode(value)
        case .array(let value):
            try container.encode(value)
        case .object(let value):
            try container.encode(value)
        }
    }
}

public enum IPCProtocol {
    public static let version = 1
    public static let socketFileName = "control.sock"
}
