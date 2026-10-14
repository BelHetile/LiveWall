import CoreGraphics
import Foundation

public struct WindowSnapshot: Equatable, Sendable {
    public var bounds: CGRect
    public var layer: Int
    public var ownerPID: pid_t
    public var ownerName: String?

    public init(bounds: CGRect, layer: Int, ownerPID: pid_t, ownerName: String? = nil) {
        self.bounds = bounds
        self.layer = layer
        self.ownerPID = ownerPID
        self.ownerName = ownerName
    }
}

@MainActor
public protocol WindowInfoProvider: AnyObject {
    func onScreenWindows() -> [WindowSnapshot]
}

@MainActor
public protocol FullscreenDetector: AnyObject {
    func isFullscreenAppPresent() -> Bool
}

@MainActor
public protocol CoverageDetector: AnyObject {
    func maxCoverageRatio() -> Double
}

@MainActor
public protocol AudioPlayingDetector: AnyObject {
    func isExternalAudioPlaying() -> Bool
    func startObserving(_ handler: @escaping @MainActor () -> Void)
    func stopObserving()
}

@MainActor
public protocol FrontmostDetector: AnyObject {
    var frontmostBundleID: String? { get }
    var runningBundleIDs: Set<String> { get }
    func startObserving(_ handler: @escaping @MainActor () -> Void)
    func stopObserving()
}

@MainActor
public protocol DisplayAsleepDetector: AnyObject {
    var isDisplayAsleep: Bool { get }
    func startObserving(_ handler: @escaping @MainActor () -> Void)
    func stopObserving()
}

public enum WindowGeometry {
    public static let normalLayer = 0
    public static let fullscreenTolerance: CGFloat = 2

    static let systemOwners: Set<String> = [
        "Window Server",
        "WindowManager",
        "Dock",
        "SystemUIServer",
        "Control Center",
        "Notification Center"
    ]

    public static func fullscreenAppPresent(windows: [WindowSnapshot], screens: [CGRect], ownPID: pid_t) -> Bool {
        for window in windows {
            guard isCandidate(window, ownPID: ownPID) else {
                continue
            }
            for screen in screens where approximatelyEqual(window.bounds, screen, tolerance: fullscreenTolerance) {
                return true
            }
        }
        return false
    }

    public static func coverageRatio(of screen: CGRect, windows: [WindowSnapshot], ownPID: pid_t) -> Double {
        guard screen.width > 0, screen.height > 0 else {
            return 0
        }
        var covered: CGFloat = 0
        for window in windows where isCandidate(window, ownPID: ownPID) {
            let intersection = window.bounds.intersection(screen)
            if intersection.isNull {
                continue
            }
            covered += intersection.width * intersection.height
        }
        let ratio = Double(covered / (screen.width * screen.height))
        return min(1.0, max(0.0, ratio))
    }

    public static func maxCoverageRatio(windows: [WindowSnapshot], screens: [CGRect], ownPID: pid_t) -> Double {
        var best: Double = 0
        for screen in screens {
            let ratio = coverageRatio(of: screen, windows: windows, ownPID: ownPID)
            if ratio > best {
                best = ratio
            }
        }
        return best
    }

    static func isCandidate(_ window: WindowSnapshot, ownPID: pid_t) -> Bool {
        guard window.ownerPID != ownPID else {
            return false
        }
        guard window.layer <= normalLayer else {
            return false
        }
        if let owner = window.ownerName, systemOwners.contains(owner) {
            return false
        }
        return true
    }

    static func approximatelyEqual(_ lhs: CGRect, _ rhs: CGRect, tolerance: CGFloat) -> Bool {
        return abs(lhs.minX - rhs.minX) <= tolerance
            && abs(lhs.minY - rhs.minY) <= tolerance
            && abs(lhs.width - rhs.width) <= tolerance
            && abs(lhs.height - rhs.height) <= tolerance
    }
}
