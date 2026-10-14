import Foundation

func pumpRunLoop(_ interval: TimeInterval) {
    RunLoop.main.run(until: Date().addingTimeInterval(interval))
}

enum EngineE2E {
    static let enabled = ProcessInfo.processInfo.environment["LW_E2E"] == "1"
}

enum Fixtures {
    static let directory: URL = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .appendingPathComponent("Fixtures")

    static func url(_ name: String) -> URL {
        return directory.appendingPathComponent(name)
    }

    static let hevcClip = url("clip-hevc-1080p30.mp4")
    static let h264Clip = url("clip-h264-720p60.mp4")
    static let av1Clip = url("clip-av1-1080p.mp4")
    static let animation = url("anim.gif")
    static let still = url("still.png")
    static let audioOnly = url("audio.m4a")
    static let corrupt = url("corrupt.mp4")
    static let zeroBytes = url("zero-bytes.mp4")
}
