import Foundation

public struct Settings: Codable, Sendable, Equatable {
    public static let currentVersion = 2

    public var v: Int
    public var muted: Bool
    public var volume: Double
    public var speed: Double
    public var autoRotate: Bool
    public var rotateMinutes: Int
    public var shuffle: Bool
    public var pauseWhenFocused: Bool
    public var pauseWhenMaximized: Bool
    public var pauseWhenFullscreen: Bool
    public var optimizeOnImport: Bool
    public var optimizeFpsLimit: Int
    public var stopAfterSeconds: Int
    public var launchAtLogin: Bool
    public var openLibraryAtLaunch: Bool
    public var language: String

    public init(
        v: Int = Settings.currentVersion,
        muted: Bool = true,
        volume: Double = 1.0,
        speed: Double = 1.0,
        autoRotate: Bool = false,
        rotateMinutes: Int = 30,
        shuffle: Bool = false,
        pauseWhenFocused: Bool = false,
        pauseWhenMaximized: Bool = true,
        pauseWhenFullscreen: Bool = true,
        optimizeOnImport: Bool = true,
        optimizeFpsLimit: Int = 30,
        stopAfterSeconds: Int = 30,
        launchAtLogin: Bool = false,
        openLibraryAtLaunch: Bool = false,
        language: String = "en"
    ) {
        self.v = v
        self.muted = muted
        self.volume = volume
        self.speed = speed
        self.autoRotate = autoRotate
        self.rotateMinutes = rotateMinutes
        self.shuffle = shuffle
        self.pauseWhenFocused = pauseWhenFocused
        self.pauseWhenMaximized = pauseWhenMaximized
        self.pauseWhenFullscreen = pauseWhenFullscreen
        self.optimizeOnImport = optimizeOnImport
        self.optimizeFpsLimit = optimizeFpsLimit
        self.stopAfterSeconds = stopAfterSeconds
        self.launchAtLogin = launchAtLogin
        self.openLibraryAtLaunch = openLibraryAtLaunch
        self.language = language
    }
}
