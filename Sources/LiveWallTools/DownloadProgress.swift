import Foundation

public struct DownloadProgressAccumulator: Sendable, Equatable {
    public enum Stage: Sendable, Equatable {
        case video
        case audio
    }

    public static let videoCeiling: Double = 90
    public static let floor: Double = 0
    public static let ceiling: Double = 100

    public private(set) var percent: Double

    public init(percent: Double = 0) {
        self.percent = DownloadProgressAccumulator.clamp(percent)
    }

    @discardableResult
    public mutating func apply(_ value: Double, stage: Stage) -> Double {
        let mapped: Double
        switch stage {
        case .video:
            mapped = value * DownloadProgressAccumulator.videoCeiling / 100
        case .audio:
            mapped = DownloadProgressAccumulator.videoCeiling
                + value * (DownloadProgressAccumulator.ceiling - DownloadProgressAccumulator.videoCeiling) / 100
        }
        let capped = DownloadProgressAccumulator.clamp(mapped)
        if capped > percent {
            percent = capped
        }
        return percent
    }

    private static func clamp(_ value: Double) -> Double {
        return min(ceiling, max(floor, value))
    }
}
