import Foundation

public enum RuleAction: Int, Sendable, Comparable, CaseIterable {
    case keepRunning = 0
    case pause = 1
    case stop = 2

    public static func < (lhs: RuleAction, rhs: RuleAction) -> Bool {
        return lhs.rawValue < rhs.rawValue
    }
}

public struct AppRule: Equatable, Sendable {
    public var bundleID: String
    public var action: RuleAction
    public var onlyWhenFrontmost: Bool

    public init(bundleID: String, action: RuleAction = .pause, onlyWhenFrontmost: Bool = false) {
        self.bundleID = bundleID
        self.action = action
        self.onlyWhenFrontmost = onlyWhenFrontmost
    }
}

public struct RuleConfig: Equatable, Sendable {
    public var displayAsleepAction: RuleAction
    public var screenLockedAction: RuleAction
    public var fullscreenAction: RuleAction
    public var coverageAction: RuleAction
    public var coverageThreshold: Double
    public var focusAction: RuleAction?
    public var appRules: [AppRule]

    public init(
        displayAsleepAction: RuleAction = .stop,
        screenLockedAction: RuleAction = .stop,
        fullscreenAction: RuleAction = .pause,
        coverageAction: RuleAction = .pause,
        coverageThreshold: Double = 0.9,
        focusAction: RuleAction? = nil,
        appRules: [AppRule] = []
    ) {
        self.displayAsleepAction = displayAsleepAction
        self.screenLockedAction = screenLockedAction
        self.fullscreenAction = fullscreenAction
        self.coverageAction = coverageAction
        self.coverageThreshold = coverageThreshold
        self.focusAction = focusAction
        self.appRules = appRules
    }
}

public struct SystemSnapshot: Equatable, Sendable {
    public var displayAsleep: Bool
    public var screenLocked: Bool
    public var fullscreenAppPresent: Bool
    public var coverageRatio: Double
    public var frontmostBundleID: String?
    public var runningBundleIDs: Set<String>
    public var selfBundleID: String?

    public init(
        displayAsleep: Bool = false,
        screenLocked: Bool = false,
        fullscreenAppPresent: Bool = false,
        coverageRatio: Double = 0,
        frontmostBundleID: String? = nil,
        runningBundleIDs: Set<String> = [],
        selfBundleID: String? = nil
    ) {
        self.displayAsleep = displayAsleep
        self.screenLocked = screenLocked
        self.fullscreenAppPresent = fullscreenAppPresent
        self.coverageRatio = coverageRatio
        self.frontmostBundleID = frontmostBundleID
        self.runningBundleIDs = runningBundleIDs
        self.selfBundleID = selfBundleID
    }
}

public enum RuleEvaluator {
    static let finderBundleID = "com.apple.finder"

    public static func evaluate(_ snapshot: SystemSnapshot, config: RuleConfig) -> RuleAction {
        var result = RuleAction.keepRunning
        func consider(_ action: RuleAction) {
            if action > result {
                result = action
            }
        }

        if snapshot.displayAsleep {
            consider(config.displayAsleepAction)
        }
        if snapshot.screenLocked {
            consider(config.screenLockedAction)
        }
        if snapshot.fullscreenAppPresent {
            consider(config.fullscreenAction)
        }
        if snapshot.coverageRatio >= config.coverageThreshold {
            consider(config.coverageAction)
        }
        if let focusAction = config.focusAction, !isFocusExempt(snapshot) {
            consider(focusAction)
        }
        for rule in config.appRules where matches(rule, snapshot) {
            consider(rule.action)
        }

        return result
    }

    private static func isFocusExempt(_ snapshot: SystemSnapshot) -> Bool {
        guard let frontmost = snapshot.frontmostBundleID else {
            return false
        }
        return frontmost == snapshot.selfBundleID || frontmost == finderBundleID
    }

    private static func matches(_ rule: AppRule, _ snapshot: SystemSnapshot) -> Bool {
        if rule.onlyWhenFrontmost {
            return snapshot.frontmostBundleID == rule.bundleID
        }
        return snapshot.runningBundleIDs.contains(rule.bundleID)
    }
}
