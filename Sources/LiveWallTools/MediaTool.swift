import Foundation

public enum MediaTool: String, CaseIterable, Sendable, Codable {
    case ffmpeg
    case ytDlp = "yt-dlp"
    case deno

    public var executableName: String {
        return rawValue
    }

    public var brewFormula: String {
        return rawValue
    }

    public var versionArguments: [String] {
        switch self {
        case .ffmpeg:
            return ["-version"]
        case .ytDlp, .deno:
            return ["--version"]
        }
    }

    public var knownGoodVersion: ToolVersion {
        switch self {
        case .ffmpeg:
            return ToolVersion(raw: "9.0.2")
        case .ytDlp:
            return ToolVersion(raw: "2026.08.19")
        case .deno:
            return ToolVersion(raw: "2.9.7")
        }
    }
}

public struct ToolVersion: Sendable, Equatable, Comparable, CustomStringConvertible {
    public let raw: String
    public let components: [Int]

    public init(raw: String) {
        self.raw = raw
        self.components = ToolVersion.parseComponents(raw)
    }

    public var description: String {
        return raw
    }

    public var isParsable: Bool {
        return !components.isEmpty
    }

    public static func == (lhs: ToolVersion, rhs: ToolVersion) -> Bool {
        if lhs.components.isEmpty || rhs.components.isEmpty {
            return lhs.raw == rhs.raw
        }
        return !(lhs < rhs) && !(rhs < lhs)
    }

    public static func < (lhs: ToolVersion, rhs: ToolVersion) -> Bool {
        let width = max(lhs.components.count, rhs.components.count)
        guard width > 0 else {
            return false
        }
        for index in 0..<width {
            let left = index < lhs.components.count ? lhs.components[index] : 0
            let right = index < rhs.components.count ? rhs.components[index] : 0
            if left != right {
                return left < right
            }
        }
        return false
    }

    public static func parseOutput(_ output: String) -> ToolVersion? {
        guard let token = firstVersionToken(in: output) else {
            return nil
        }
        return ToolVersion(raw: token)
    }

    static func firstVersionToken(in text: String) -> String? {
        var token = ""
        for character in text {
            if character.isNumber {
                token.append(character)
            } else if character == "." && !token.isEmpty {
                token.append(character)
            } else if !token.isEmpty {
                break
            }
        }
        while token.hasSuffix(".") {
            token.removeLast()
        }
        return token.isEmpty ? nil : token
    }

    static func parseComponents(_ raw: String) -> [Int] {
        guard let token = firstVersionToken(in: raw) else {
            return []
        }
        let parts = token.split(separator: ".", omittingEmptySubsequences: false)
        var result: [Int] = []
        for part in parts {
            guard let value = Int(part) else {
                return []
            }
            result.append(value)
        }
        return result
    }
}
