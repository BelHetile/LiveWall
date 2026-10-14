import Foundation
import Testing
@testable import LiveWallEngine

@MainActor
final class FakePowerStateProvider: PowerStateProviding {
    var isLowPowerModeEnabled: Bool

    init(_ value: Bool) {
        self.isLowPowerModeEnabled = value
    }
}

@Suite @MainActor struct PowerMonitorTests {
    @Test func initializesFromProvider() {
        let active = PowerMonitor(provider: FakePowerStateProvider(true), center: NotificationCenter())
        #expect(active.isLowPowerModeEnabled)

        let inactive = PowerMonitor(provider: FakePowerStateProvider(false), center: NotificationCenter())
        #expect(!inactive.isLowPowerModeEnabled)
    }

    @Test func notificationPathReQueriesProvider() {
        let provider = FakePowerStateProvider(false)
        let center = NotificationCenter()
        let monitor = PowerMonitor(provider: provider, center: center)
        var observed: [Bool] = []
        monitor.onChange = { observed.append($0) }
        monitor.start()
        defer { monitor.stop() }

        #expect(!monitor.isLowPowerModeEnabled)

        provider.isLowPowerModeEnabled = true
        center.post(name: Notification.Name.NSProcessInfoPowerStateDidChange, object: nil)
        pumpRunLoop(0.1)
        #expect(monitor.isLowPowerModeEnabled)
        #expect(observed == [true])

        provider.isLowPowerModeEnabled = false
        center.post(name: Notification.Name.NSProcessInfoPowerStateDidChange, object: nil)
        pumpRunLoop(0.1)
        #expect(!monitor.isLowPowerModeEnabled)
        #expect(observed == [true, false])
    }

    @Test func refreshIsChangeOnly() {
        let provider = FakePowerStateProvider(false)
        let monitor = PowerMonitor(provider: provider, center: NotificationCenter())
        var observed: [Bool] = []
        monitor.onChange = { observed.append($0) }

        #expect(!monitor.refresh())
        #expect(observed.isEmpty)

        provider.isLowPowerModeEnabled = true
        #expect(monitor.refresh())
        #expect(observed == [true])

        #expect(!monitor.refresh())
        #expect(observed == [true])
    }

    @Test func startIsIdempotent() {
        let provider = FakePowerStateProvider(false)
        let center = NotificationCenter()
        let monitor = PowerMonitor(provider: provider, center: center)
        var observed: [Bool] = []
        monitor.onChange = { observed.append($0) }
        monitor.start()
        monitor.start()
        defer { monitor.stop() }

        provider.isLowPowerModeEnabled = true
        center.post(name: Notification.Name.NSProcessInfoPowerStateDidChange, object: nil)
        pumpRunLoop(0.1)
        #expect(observed == [true])
    }

    @Test func stopUnsubscribes() {
        let provider = FakePowerStateProvider(false)
        let center = NotificationCenter()
        let monitor = PowerMonitor(provider: provider, center: center)
        monitor.start()
        monitor.stop()

        provider.isLowPowerModeEnabled = true
        center.post(name: Notification.Name.NSProcessInfoPowerStateDidChange, object: nil)
        pumpRunLoop(0.1)
        #expect(!monitor.isLowPowerModeEnabled)
    }

    @Test func stopWithoutStartIsSafe() {
        let monitor = PowerMonitor(provider: FakePowerStateProvider(false), center: NotificationCenter())
        monitor.stop()
        #expect(!monitor.isLowPowerModeEnabled)
    }

    @Test func processInfoProviderMatchesSystem() {
        let provider = ProcessInfoPowerState()
        #expect(provider.isLowPowerModeEnabled == ProcessInfo.processInfo.isLowPowerModeEnabled)
    }
}
