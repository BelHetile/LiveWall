import Carbon
import Foundation
import Testing
@testable import LiveWallEngine

@MainActor
final class FakeHotkeyRegistrar: HotkeyRegistrar {
    var probeResult: HotkeyRegistrationResult = .ok
    var registerResult: HotkeyRegistrationResult = .ok
    var installStatus: OSStatus = noErr

    private(set) var handler: (@MainActor (UInt32) -> Void)?
    private(set) var probes: [(keyCode: UInt32, modifiers: UInt32)] = []
    private(set) var registrations: [(keyCode: UInt32, modifiers: UInt32, id: UInt32)] = []
    private(set) var unregistered: [UInt32] = []

    func installHandler(_ handler: @escaping @MainActor (UInt32) -> Void) -> OSStatus {
        self.handler = handler
        return installStatus
    }

    func removeHandler() {
        handler = nil
    }

    func probe(keyCode: UInt32, modifiers: UInt32) -> HotkeyRegistrationResult {
        probes.append((keyCode, modifiers))
        return probeResult
    }

    func register(keyCode: UInt32, modifiers: UInt32, id: UInt32) -> HotkeyRegistrationResult {
        registrations.append((keyCode, modifiers, id))
        return registerResult
    }

    func unregister(id: UInt32) {
        unregistered.append(id)
    }

    func fire(id: UInt32) {
        handler?(id)
    }
}

@Suite struct HotkeyComboValidationTests {
    @Test func comboWithoutModifierIsMissingModifier() {
        #expect(HotkeyComboValidator.validate(HotkeyCombo(key: .z, modifiers: [])) == .missingModifier)
        #expect(HotkeyComboValidator.validate(HotkeyCombo(keyCode: 0x06, modifiers: [])) == .missingModifier)
    }

    @Test func reservedSystemCombosAreRejected() {
        #expect(HotkeyComboValidator.validate(HotkeyCombo(key: .tab, modifiers: .command)) == .systemCombo)
        #expect(HotkeyComboValidator.validate(HotkeyCombo(key: .tab, modifiers: [.command, .shift])) == .systemCombo)
        #expect(HotkeyComboValidator.validate(HotkeyCombo(key: .space, modifiers: .command)) == .systemCombo)
        #expect(HotkeyComboValidator.validate(HotkeyCombo(key: .space, modifiers: .control)) == .systemCombo)
        #expect(HotkeyComboValidator.validate(HotkeyCombo(key: .upArrow, modifiers: .control)) == .systemCombo)
        #expect(HotkeyComboValidator.validate(HotkeyCombo(key: .leftArrow, modifiers: .control)) == .systemCombo)
        #expect(HotkeyComboValidator.validate(HotkeyCombo(key: .q, modifiers: .command)) == .systemCombo)
        #expect(HotkeyComboValidator.validate(HotkeyCombo(key: .q, modifiers: [.control, .command])) == .systemCombo)
    }

    @Test func nonReservedCombosAreValid() {
        #expect(HotkeyComboValidator.validate(HotkeyCombo(key: .z, modifiers: [.command, .option])) == .valid)
        #expect(HotkeyComboValidator.validate(HotkeyCombo(key: .d, modifiers: [.control, .option])) == .valid)
        #expect(HotkeyComboValidator.validate(HotkeyCombo(keyCode: 0x60, modifiers: [.command, .control])) == .valid)
    }

    @Test func reservedMatchingIgnoresNonModifierBits() {
        let polluted = HotkeyCombo(key: .tab, modifiers: HotkeyModifiers(rawValue: 0x0100 | 0x4000))
        #expect(HotkeyComboValidator.validate(polluted) == .systemCombo)
    }

    @Test func modifierFlagsRoundTrip() {
        let modifiers: HotkeyModifiers = [.command, .shift, .option, .control]
        #expect(modifiers.nsEventFlags == [.command, .shift, .option, .control])
        let reconstructed = HotkeyModifiers(nsEventFlags: modifiers.nsEventFlags)
        #expect(reconstructed == modifiers)
    }

    @Test func configurationIssuesDetectDuplicates() {
        let shared = HotkeyCombo(key: .z, modifiers: [.command, .option])
        let configuration = HotkeyConfiguration(bindings: [
            HotkeyBinding(action: .next, combo: shared),
            HotkeyBinding(action: .previous, combo: shared)
        ])
        let issues = HotkeyConfigurationValidator.issues(in: configuration)
        #expect(issues == [.duplicateCombos(shared, [.next, .previous])])
    }

    @Test func configurationIssuesCollectAllProblems() {
        let configuration = HotkeyConfiguration(bindings: [
            HotkeyBinding(action: .next, combo: HotkeyCombo(key: .a, modifiers: [])),
            HotkeyBinding(action: .previous, combo: HotkeyCombo(key: .tab, modifiers: .command)),
            HotkeyBinding(action: .pauseResume, combo: HotkeyCombo(key: .z, modifiers: [.command, .option]))
        ])
        let issues = HotkeyConfigurationValidator.issues(in: configuration)
        #expect(issues.contains(.missingModifier(.next)))
        #expect(issues.contains(.systemCombo(.previous)))
        #expect(issues.count == 2)
    }

    @Test func comboAndConfigurationCodableRoundTrip() throws {
        let configuration = HotkeyConfiguration(bindings: [
            HotkeyBinding(action: .next, combo: HotkeyCombo(key: .z, modifiers: [.command, .shift])),
            HotkeyBinding(action: .random, combo: HotkeyCombo(keyCode: 0x31, modifiers: [.control, .option]))
        ])
        let data = try JSONEncoder().encode(configuration)
        let decoded = try JSONDecoder().decode(HotkeyConfiguration.self, from: data)
        #expect(decoded == configuration)
        #expect(decoded.combo(for: .next) == HotkeyCombo(key: .z, modifiers: [.command, .shift]))
    }

    @Test func actionRegistrationIDsRoundTrip() {
        for action in HotkeyAction.allCases {
            #expect(HotkeyAction(registrationID: action.registrationID) == action)
        }
        #expect(HotkeyAction(registrationID: 99) == nil)
    }
}

@Suite @MainActor struct HotkeyManagerTests {
    @Test func registerShortCircuitsMissingModifier() {
        let registrar = FakeHotkeyRegistrar()
        let manager = HotkeyManager(registrar: registrar)
        let result = manager.register(HotkeyCombo(key: .z, modifiers: []), for: .next)
        #expect(result == .missingModifier)
        #expect(registrar.probes.isEmpty)
        #expect(registrar.registrations.isEmpty)
        #expect(manager.bindings.isEmpty)
    }

    @Test func registerShortCircuitsSystemCombo() {
        let registrar = FakeHotkeyRegistrar()
        let manager = HotkeyManager(registrar: registrar)
        let result = manager.register(HotkeyCombo(key: .space, modifiers: .command), for: .next)
        #expect(result == .systemComboRejected)
        #expect(registrar.probes.isEmpty)
        #expect(registrar.registrations.isEmpty)
    }

    @Test func registerReportsComboHeldFromProbe() {
        let registrar = FakeHotkeyRegistrar()
        registrar.probeResult = .comboHeld
        let manager = HotkeyManager(registrar: registrar)
        let combo = HotkeyCombo(key: .z, modifiers: [.command, .option])
        let result = manager.register(combo, for: .next)
        #expect(result == .comboHeld)
        #expect(registrar.probes.count == 1)
        #expect(registrar.registrations.isEmpty)
        #expect(manager.bindings.isEmpty)
    }

    @Test func registerStoresBindingOnSuccess() {
        let registrar = FakeHotkeyRegistrar()
        let manager = HotkeyManager(registrar: registrar)
        let combo = HotkeyCombo(key: .z, modifiers: [.command, .option])
        #expect(manager.register(combo, for: .next) == .ok)
        #expect(manager.bindings[.next] == combo)
        #expect(registrar.registrations.count == 1)
        #expect(registrar.registrations.first?.id == HotkeyAction.next.registrationID)
    }

    @Test func registerMapsFailureStatus() {
        let registrar = FakeHotkeyRegistrar()
        registrar.registerResult = .failed(OSStatus(-50))
        let manager = HotkeyManager(registrar: registrar)
        let result = manager.register(HotkeyCombo(key: .z, modifiers: [.command, .option]), for: .next)
        #expect(result == .failed(OSStatus(-50)))
        #expect(manager.bindings.isEmpty)
    }

    @Test func registerSurfacesHandlerInstallFailure() {
        let registrar = FakeHotkeyRegistrar()
        registrar.installStatus = OSStatus(-50)
        let manager = HotkeyManager(registrar: registrar)
        let result = manager.register(HotkeyCombo(key: .z, modifiers: [.command, .option]), for: .next)
        #expect(result == .failed(OSStatus(-50)))
        #expect(registrar.registrations.isEmpty)
    }

    @Test func reRegistrationUnregistersPreviousComboFirst() {
        let registrar = FakeHotkeyRegistrar()
        let manager = HotkeyManager(registrar: registrar)
        let first = HotkeyCombo(key: .z, modifiers: [.command, .option])
        let second = HotkeyCombo(key: .z, modifiers: [.command, .control])
        #expect(manager.register(first, for: .next) == .ok)
        #expect(manager.register(second, for: .next) == .ok)
        #expect(registrar.unregistered == [HotkeyAction.next.registrationID])
        #expect(manager.bindings[.next] == second)
    }

    @Test func unregisterRemovesBinding() {
        let registrar = FakeHotkeyRegistrar()
        let manager = HotkeyManager(registrar: registrar)
        #expect(manager.register(HotkeyCombo(key: .z, modifiers: [.command, .option]), for: .next) == .ok)
        #expect(manager.unregister(.next))
        #expect(manager.bindings.isEmpty)
        #expect(!manager.unregister(.next))
        #expect(registrar.unregistered == [HotkeyAction.next.registrationID])
    }

    @Test func unregisterAllClearsEveryAction() {
        let registrar = FakeHotkeyRegistrar()
        let manager = HotkeyManager(registrar: registrar)
        #expect(manager.register(HotkeyCombo(key: .z, modifiers: [.command, .option]), for: .next) == .ok)
        #expect(manager.register(HotkeyCombo(key: .x, modifiers: [.command, .option]), for: .random) == .ok)
        manager.unregisterAll()
        #expect(manager.bindings.isEmpty)
        #expect(Set(registrar.unregistered) == [HotkeyAction.next.registrationID, HotkeyAction.random.registrationID])
    }

    @Test func installedHandlerDispatchesByHotKeyID() {
        let registrar = FakeHotkeyRegistrar()
        let manager = HotkeyManager(registrar: registrar)
        var received: [HotkeyAction] = []
        manager.onAction = { received.append($0) }
        #expect(manager.register(HotkeyCombo(key: .z, modifiers: [.command, .option]), for: .random) == .ok)

        registrar.fire(id: HotkeyAction.random.registrationID)
        #expect(received == [.random])

        registrar.fire(id: 99)
        #expect(received == [.random])
    }

    @Test func unregisteredHotKeyIDDoesNotDispatch() {
        let registrar = FakeHotkeyRegistrar()
        let manager = HotkeyManager(registrar: registrar)
        var received: [HotkeyAction] = []
        manager.onAction = { received.append($0) }
        #expect(manager.register(HotkeyCombo(key: .z, modifiers: [.command, .option]), for: .next) == .ok)
        manager.unregister(.next)
        registrar.fire(id: HotkeyAction.next.registrationID)
        #expect(received.isEmpty)
    }

    @Test func isComboFreeUsesProbeAndRejectsInvalid() {
        let registrar = FakeHotkeyRegistrar()
        let manager = HotkeyManager(registrar: registrar)
        #expect(manager.isComboFree(HotkeyCombo(key: .z, modifiers: [.command, .option])))
        #expect(!manager.isComboFree(HotkeyCombo(key: .z, modifiers: [])))
        #expect(!manager.isComboFree(HotkeyCombo(key: .tab, modifiers: .command)))
        registrar.probeResult = .comboHeld
        #expect(!manager.isComboFree(HotkeyCombo(key: .z, modifiers: [.command, .option])))
    }

    @Test func statusMapperMapsConflictAndOtherFailures() {
        #expect(HotkeyRegistrationStatusMapper.result(for: noErr) == .ok)
        #expect(HotkeyRegistrationStatusMapper.result(for: OSStatus(eventHotKeyExistsErr)) == .comboHeld)
        #expect(HotkeyRegistrationStatusMapper.result(for: OSStatus(-50)) == .failed(OSStatus(-50)))
    }

    @Test func carbonSignatureEncodesFourCC() {
        #expect(CarbonHotkeyRegistrar.signature(of: "LVW2") == 0x4C565732)
    }
}
