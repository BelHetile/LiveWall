import AppKit
import Carbon
import Foundation

public enum HotkeyRegistrationResult: Equatable, Sendable {
    case ok
    case comboHeld
    case systemComboRejected
    case missingModifier
    case failed(OSStatus)
}

public enum HotkeyRegistrationStatusMapper {
    public static func result(for status: OSStatus) -> HotkeyRegistrationResult {
        if status == noErr {
            return .ok
        }
        if status == OSStatus(eventHotKeyExistsErr) {
            return .comboHeld
        }
        return .failed(status)
    }
}

@MainActor
public protocol HotkeyRegistrar: AnyObject {
    func installHandler(_ handler: @escaping @MainActor (UInt32) -> Void) -> OSStatus
    func removeHandler()
    func probe(keyCode: UInt32, modifiers: UInt32) -> HotkeyRegistrationResult
    func register(keyCode: UInt32, modifiers: UInt32, id: UInt32) -> HotkeyRegistrationResult
    func unregister(id: UInt32)
}

@MainActor
public final class CarbonHotkeyRegistrar: HotkeyRegistrar {
    public static let defaultSignature: OSType = signature(of: "LVW2")

    private let signature: OSType
    private var refs: [UInt32: EventHotKeyRef] = [:]
    private var handlerRef: EventHandlerRef?
    private var handler: (@MainActor (UInt32) -> Void)?

    public init(signature: OSType = CarbonHotkeyRegistrar.defaultSignature) {
        self.signature = signature
    }

    public func installHandler(_ handler: @escaping @MainActor (UInt32) -> Void) -> OSStatus {
        self.handler = handler
        guard handlerRef == nil else {
            return noErr
        }
        var eventType = EventTypeSpec(
            eventClass: OSType(kEventClassKeyboard),
            eventKind: UInt32(kEventHotKeyPressed)
        )
        let userData = Unmanaged.passUnretained(self).toOpaque()
        let status = InstallEventHandler(
            GetApplicationEventTarget(),
            carbonHotKeyEventHandler,
            1,
            &eventType,
            userData,
            &handlerRef
        )
        if status != noErr {
            handlerRef = nil
            self.handler = nil
        }
        return status
    }

    public func removeHandler() {
        if let handlerRef {
            RemoveEventHandler(handlerRef)
        }
        handlerRef = nil
        handler = nil
    }

    public func probe(keyCode: UInt32, modifiers: UInt32) -> HotkeyRegistrationResult {
        var ref: EventHotKeyRef?
        let id = EventHotKeyID(signature: signature, id: 0)
        let status = RegisterEventHotKey(keyCode, modifiers, id, GetApplicationEventTarget(), 0, &ref)
        if status == noErr, let ref {
            UnregisterEventHotKey(ref)
        }
        return HotkeyRegistrationStatusMapper.result(for: status)
    }

    public func register(keyCode: UInt32, modifiers: UInt32, id: UInt32) -> HotkeyRegistrationResult {
        var ref: EventHotKeyRef?
        let hotKeyID = EventHotKeyID(signature: signature, id: id)
        let status = RegisterEventHotKey(keyCode, modifiers, hotKeyID, GetApplicationEventTarget(), 0, &ref)
        if status == noErr, let ref {
            refs[id] = ref
        }
        return HotkeyRegistrationStatusMapper.result(for: status)
    }

    public func unregister(id: UInt32) {
        guard let ref = refs.removeValue(forKey: id) else {
            return
        }
        UnregisterEventHotKey(ref)
    }

    func dispatch(id: UInt32) {
        handler?(id)
    }

    public static func signature(of string: String) -> OSType {
        var value: OSType = 0
        for scalar in string.unicodeScalars.prefix(4) {
            value = (value << 8) | (OSType(scalar.value) & 0xFF)
        }
        return value
    }

    isolated deinit {
        for id in refs.keys {
            if let ref = refs[id] {
                UnregisterEventHotKey(ref)
            }
        }
        if let handlerRef {
            RemoveEventHandler(handlerRef)
        }
    }
}

private func carbonHotKeyEventHandler(
    _ callRef: EventHandlerCallRef?,
    _ event: EventRef?,
    _ userData: UnsafeMutableRawPointer?
) -> OSStatus {
    guard let event, let userData else {
        return OSStatus(eventNotHandledErr)
    }
    var hotKeyID = EventHotKeyID()
    let status = GetEventParameter(
        event,
        EventParamName(kEventParamDirectObject),
        EventParamType(typeEventHotKeyID),
        nil,
        MemoryLayout<EventHotKeyID>.size,
        nil,
        &hotKeyID
    )
    guard status == noErr else {
        return status
    }
    let registrar = Unmanaged<CarbonHotkeyRegistrar>.fromOpaque(userData).takeUnretainedValue()
    MainActor.assumeIsolated {
        registrar.dispatch(id: hotKeyID.id)
    }
    return noErr
}

@MainActor
public final class HotkeyManager {
    public private(set) var bindings: [HotkeyAction: HotkeyCombo] = [:]
    public var onAction: (@MainActor (HotkeyAction) -> Void)?

    private let registrar: HotkeyRegistrar
    private var registrations: [HotkeyAction: HotkeyCombo] = [:]
    private var handlerInstalled = false

    public init(registrar: HotkeyRegistrar) {
        self.registrar = registrar
    }

    public convenience init() {
        self.init(registrar: CarbonHotkeyRegistrar())
    }

    @discardableResult
    public func register(_ combo: HotkeyCombo, for action: HotkeyAction) -> HotkeyRegistrationResult {
        if registrations[action] != nil {
            registrar.unregister(id: action.registrationID)
            registrations[action] = nil
            bindings[action] = nil
        }

        switch HotkeyComboValidator.validate(combo) {
        case .missingModifier:
            return .missingModifier
        case .systemCombo:
            return .systemComboRejected
        case .valid:
            break
        }

        if !handlerInstalled {
            let status = registrar.installHandler { [weak self] id in
                self?.handleHotKey(id: id)
            }
            if status != noErr {
                return .failed(status)
            }
            handlerInstalled = true
        }

        let normalized = HotkeyComboValidator.normalized(combo)
        if registrar.probe(keyCode: normalized.keyCode, modifiers: normalized.modifiers.rawValue) == .comboHeld {
            return .comboHeld
        }

        let result = registrar.register(
            keyCode: normalized.keyCode,
            modifiers: normalized.modifiers.rawValue,
            id: action.registrationID
        )
        if result == .ok {
            registrations[action] = normalized
            bindings[action] = normalized
        }
        return result
    }

    @discardableResult
    public func unregister(_ action: HotkeyAction) -> Bool {
        guard registrations[action] != nil else {
            return false
        }
        registrar.unregister(id: action.registrationID)
        registrations[action] = nil
        bindings[action] = nil
        return true
    }

    public func unregisterAll() {
        for action in registrations.keys {
            registrar.unregister(id: action.registrationID)
        }
        registrations.removeAll()
        bindings.removeAll()
    }

    public func isComboFree(_ combo: HotkeyCombo) -> Bool {
        guard HotkeyComboValidator.validate(combo) == .valid else {
            return false
        }
        let normalized = HotkeyComboValidator.normalized(combo)
        return registrar.probe(keyCode: normalized.keyCode, modifiers: normalized.modifiers.rawValue) == .ok
    }

    func handleHotKey(id: UInt32) {
        guard let action = HotkeyAction(registrationID: id), registrations[action] != nil else {
            return
        }
        onAction?(action)
    }

    isolated deinit {
        for action in registrations.keys {
            registrar.unregister(id: action.registrationID)
        }
        if handlerInstalled {
            registrar.removeHandler()
        }
    }
}
