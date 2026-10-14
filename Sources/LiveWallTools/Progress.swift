import Foundation

public struct ProgressUpdate: Sendable, Equatable {
    public var percent: Double
    public var etaSeconds: Int?
    public var speed: String?
    public var formatID: String

    public init(percent: Double, etaSeconds: Int?, speed: String?, formatID: String) {
        self.percent = percent
        self.etaSeconds = etaSeconds
        self.speed = speed
        self.formatID = formatID
    }
}

public enum YTDLP {
    public static let progressPrefix = "LWPROG:"
    public static let progressTemplateFlag = "--progress-template"
    public static let progressTemplateValue =
        "download:LWPROG:%(progress._percent_str)s:%(progress.eta)s:%(progress._speed_str)s:%(info.format_id)s"

    public static var progressArguments: [String] {
        return [progressTemplateFlag, progressTemplateValue]
    }

    private static let unavailableTokens: Set<String> = ["na", "n/a", "none", "unknown", "null"]

    public static func parseProgress(line: String) -> ProgressUpdate? {
        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.hasPrefix(progressPrefix) else {
            return nil
        }
        let remainder = trimmed.dropFirst(progressPrefix.count)
        let fields = remainder.split(separator: ":", omittingEmptySubsequences: false)
        guard fields.count >= 4 else {
            return nil
        }
        guard let percent = parsePercent(String(fields[0])) else {
            return nil
        }
        guard let eta = parseETA(String(fields[1])) else {
            return nil
        }
        let speed = parseOptional(String(fields[2]))
        let formatID = String(fields[3]).trimmingCharacters(in: .whitespaces)
        guard !formatID.isEmpty, !isUnavailable(formatID) else {
            return nil
        }
        return ProgressUpdate(percent: percent, etaSeconds: eta, speed: speed, formatID: formatID)
    }

    private static func parsePercent(_ raw: String) -> Double? {
        var value = raw.trimmingCharacters(in: .whitespaces)
        if value.hasSuffix("%") {
            value.removeLast()
        }
        value = value.trimmingCharacters(in: .whitespaces)
        guard let percent = Double(value), percent >= 0, percent <= 100 else {
            return nil
        }
        return percent
    }

    private static func parseETA(_ raw: String) -> Int?? {
        let value = raw.trimmingCharacters(in: .whitespaces)
        if isUnavailable(value) {
            return .some(nil)
        }
        guard let seconds = Int(value), seconds >= 0 else {
            return nil
        }
        return .some(seconds)
    }

    private static func parseOptional(_ raw: String) -> String? {
        let value = raw.trimmingCharacters(in: .whitespaces)
        guard !value.isEmpty, !isUnavailable(value) else {
            return nil
        }
        return value
    }

    private static func isUnavailable(_ value: String) -> Bool {
        return unavailableTokens.contains(value.lowercased())
    }
}
