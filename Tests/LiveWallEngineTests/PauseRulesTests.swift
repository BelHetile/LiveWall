import Testing
@testable import LiveWallEngine

@Suite struct PauseRulesTests {
    private static let matrixConfig = RuleConfig(
        displayAsleepAction: .stop,
        screenLockedAction: .stop,
        fullscreenAction: .pause,
        coverageAction: .pause
    )

    @Test func actionOrderingIsLeastToMostRestrictive() {
        #expect(RuleAction.keepRunning < RuleAction.pause)
        #expect(RuleAction.pause < RuleAction.stop)
        #expect(RuleAction.allCases == [.keepRunning, .pause, .stop])
    }

    @Test func mostRestrictiveWinsTruthTable() {
        var rows = 0
        for asleep in [false, true] {
            for locked in [false, true] {
                for fullscreen in [false, true] {
                    for over in [false, true] {
                        let snapshot = SystemSnapshot(
                            displayAsleep: asleep,
                            screenLocked: locked,
                            fullscreenAppPresent: fullscreen,
                            coverageRatio: over ? 0.95 : 0.4
                        )
                        let expected: RuleAction
                        if asleep || locked {
                            expected = .stop
                        } else if fullscreen || over {
                            expected = .pause
                        } else {
                            expected = .keepRunning
                        }
                        #expect(RuleEvaluator.evaluate(snapshot, config: Self.matrixConfig) == expected)
                        rows += 1
                    }
                }
            }
        }
        #expect(rows == 16)
    }

    @Test func displayAsleepAloneTriggersItsAction() {
        let snapshot = SystemSnapshot(displayAsleep: true)
        #expect(RuleEvaluator.evaluate(snapshot, config: Self.matrixConfig) == .stop)
        let paused = RuleConfig(displayAsleepAction: .pause)
        #expect(RuleEvaluator.evaluate(snapshot, config: paused) == .pause)
    }

    @Test func screenLockedAloneTriggersItsAction() {
        let snapshot = SystemSnapshot(screenLocked: true)
        #expect(RuleEvaluator.evaluate(snapshot, config: Self.matrixConfig) == .stop)
    }

    @Test func fullscreenAloneTriggersItsAction() {
        let snapshot = SystemSnapshot(fullscreenAppPresent: true)
        #expect(RuleEvaluator.evaluate(snapshot, config: Self.matrixConfig) == .pause)
    }

    @Test func coverageAloneTriggersItsAction() {
        let snapshot = SystemSnapshot(coverageRatio: 0.95)
        #expect(RuleEvaluator.evaluate(snapshot, config: Self.matrixConfig) == .pause)
    }

    @Test func coverageThresholdBoundaryIsInclusive() {
        let config = RuleConfig(coverageAction: .pause, coverageThreshold: 0.9)
        #expect(RuleEvaluator.evaluate(SystemSnapshot(coverageRatio: 0.9), config: config) == .pause)
        #expect(RuleEvaluator.evaluate(SystemSnapshot(coverageRatio: 0.8999), config: config) == .keepRunning)
    }

    @Test func keepRunningActionNeverTriggers() {
        let config = RuleConfig(
            displayAsleepAction: .keepRunning,
            screenLockedAction: .keepRunning,
            fullscreenAction: .keepRunning,
            coverageAction: .keepRunning
        )
        let snapshot = SystemSnapshot(
            displayAsleep: true,
            screenLocked: true,
            fullscreenAppPresent: true,
            coverageRatio: 1.0
        )
        #expect(RuleEvaluator.evaluate(snapshot, config: config) == .keepRunning)
    }

    @Test func stopOutranksPauseAndImpliesPause() {
        let config = RuleConfig(screenLockedAction: .stop, fullscreenAction: .pause)
        let snapshot = SystemSnapshot(screenLocked: true, fullscreenAppPresent: true)
        #expect(RuleEvaluator.evaluate(snapshot, config: config) == .stop)
    }

    @Test func allMatrixRulesFirePicksStop() {
        let snapshot = SystemSnapshot(
            displayAsleep: true,
            screenLocked: true,
            fullscreenAppPresent: true,
            coverageRatio: 1.0
        )
        #expect(RuleEvaluator.evaluate(snapshot, config: Self.matrixConfig) == .stop)
    }

    @Test func focusRuleDisabledByDefault() {
        let snapshot = SystemSnapshot(frontmostBundleID: "com.example.other", selfBundleID: "io.github.belhetile.livewall")
        #expect(RuleEvaluator.evaluate(snapshot, config: RuleConfig()) == .keepRunning)
    }

    @Test func focusRuleFiresForOtherApp() {
        let config = RuleConfig(focusAction: .pause)
        let snapshot = SystemSnapshot(frontmostBundleID: "com.example.other", selfBundleID: "io.github.belhetile.livewall")
        #expect(RuleEvaluator.evaluate(snapshot, config: config) == .pause)
    }

    @Test func focusRuleExemptForSelfAndFinder() {
        let config = RuleConfig(focusAction: .pause)
        let selfFront = SystemSnapshot(frontmostBundleID: "io.github.belhetile.livewall", selfBundleID: "io.github.belhetile.livewall")
        let finderFront = SystemSnapshot(frontmostBundleID: "com.apple.finder", selfBundleID: "io.github.belhetile.livewall")
        #expect(RuleEvaluator.evaluate(selfFront, config: config) == .keepRunning)
        #expect(RuleEvaluator.evaluate(finderFront, config: config) == .keepRunning)
    }

    @Test func focusRuleFiresWhenNoFrontmostApp() {
        let config = RuleConfig(focusAction: .pause)
        #expect(RuleEvaluator.evaluate(SystemSnapshot(frontmostBundleID: nil), config: config) == .pause)
    }

    @Test func appRuleMatchesWhenListedAppIsRunning() {
        let config = RuleConfig(appRules: [AppRule(bundleID: "com.example.game", action: .pause)])
        let running = SystemSnapshot(runningBundleIDs: ["com.example.game"])
        let stopped = SystemSnapshot(runningBundleIDs: ["com.example.other"])
        #expect(RuleEvaluator.evaluate(running, config: config) == .pause)
        #expect(RuleEvaluator.evaluate(stopped, config: config) == .keepRunning)
    }

    @Test func appRuleNotRunningButFrontmostDoesNotMatchRunningForm() {
        let config = RuleConfig(appRules: [AppRule(bundleID: "com.example.game", action: .stop)])
        let snapshot = SystemSnapshot(frontmostBundleID: "com.example.game", runningBundleIDs: [])
        #expect(RuleEvaluator.evaluate(snapshot, config: config) == .keepRunning)
    }

    @Test func frontmostOnlyAppRuleMatchesOnlyWhenFrontmost() {
        let config = RuleConfig(appRules: [AppRule(bundleID: "com.example.game", action: .pause, onlyWhenFrontmost: true)])
        let front = SystemSnapshot(frontmostBundleID: "com.example.game", runningBundleIDs: ["com.example.game"])
        let background = SystemSnapshot(frontmostBundleID: "com.example.other", runningBundleIDs: ["com.example.game"])
        #expect(RuleEvaluator.evaluate(front, config: config) == .pause)
        #expect(RuleEvaluator.evaluate(background, config: config) == .keepRunning)
    }

    @Test func appRuleStopOutranksMatrixPause() {
        let config = RuleConfig(appRules: [AppRule(bundleID: "com.example.game", action: .stop)])
        let snapshot = SystemSnapshot(fullscreenAppPresent: true, runningBundleIDs: ["com.example.game"])
        #expect(RuleEvaluator.evaluate(snapshot, config: config) == .stop)
    }

    @Test func defaultsMatchPhaseSpec() {
        let config = RuleConfig()
        #expect(config.displayAsleepAction == .stop)
        #expect(config.screenLockedAction == .stop)
        #expect(config.fullscreenAction == .pause)
        #expect(config.coverageAction == .pause)
        #expect(config.coverageThreshold == 0.9)
        #expect(config.focusAction == nil)
        #expect(config.appRules.isEmpty)
    }
}
