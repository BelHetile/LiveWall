import AppKit
import CoreGraphics
import Foundation

public struct HotkeyModifiers: OptionSet, Sendable, Hashable {
    public let rawValue: UInt32

    public init(rawValue: UInt32) {
        self.rawValue = rawValue
    }

    public static let command = HotkeyModifiers(rawValue: 0x0100)
    public static let shift = HotkeyModifiers(rawValue: 0x0200)
    public static let option = HotkeyModifiers(rawValue: 0x0800)
    public static let control = HotkeyModifiers(rawValue: 0x1000)

    public static let all: HotkeyModifiers = [.command, .shift, .option, .control]

    public var lacksModifier: Bool {
        return intersection(.all).isEmpty
    }

    public var nsEventFlags: NSEvent.ModifierFlags {
        var flags: NSEvent.ModifierFlags = []
        if contains(.command) {
            flags.insert(.command)
        }
        if contains(.shift) {
            flags.insert(.shift)
        }
        if contains(.option) {
            flags.insert(.option)
        }
        if contains(.control) {
            flags.insert(.control)
        }
        return flags
    }

    public var cgEventFlags: CGEventFlags {
        var flags: CGEventFlags = []
        if contains(.command) {
            flags.insert(.maskCommand)
        }
        if contains(.shift) {
            flags.insert(.maskShift)
        }
        if contains(.option) {
            flags.insert(.maskAlternate)
        }
        if contains(.control) {
            flags.insert(.maskControl)
        }
        return flags
    }

    public init(nsEventFlags: NSEvent.ModifierFlags) {
        let flags = nsEventFlags.intersection(.deviceIndependentFlagsMask)
        var raw: UInt32 = 0
        if flags.contains(.command) {
            raw |= HotkeyModifiers.command.rawValue
        }
        if flags.contains(.shift) {
            raw |= HotkeyModifiers.shift.rawValue
        }
        if flags.contains(.option) {
            raw |= HotkeyModifiers.option.rawValue
        }
        if flags.contains(.control) {
            raw |= HotkeyModifiers.control.rawValue
        }
        self.init(rawValue: raw)
    }
}

extension HotkeyModifiers: Codable {
    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        self.init(rawValue: try container.decode(UInt32.self))
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        try container.encode(rawValue)
    }
}

public enum HotkeyKey: UInt32, Sendable, CaseIterable {
    case a = 0x00
    case s = 0x01
    case d = 0x02
    case f = 0x03
    case h = 0x04
    case g = 0x05
    case z = 0x06
    case x = 0x07
    case c = 0x08
    case v = 0x09
    case b = 0x0B
    case q = 0x0C
    case w = 0x0D
    case e = 0x0E
    case r = 0x0F
    case y = 0x10
    case t = 0x11
    case o = 0x1F
    case p = 0x23
    case n = 0x2D
    case m = 0x2E
    case tab = 0x30
    case space = 0x31
    case escape = 0x35
    case leftArrow = 0x7B
    case rightArrow = 0x7C
    case downArrow = 0x7D
    case upArrow = 0x7E
}

public struct HotkeyCombo: Equatable, Hashable, Sendable, Codable {
    public let keyCode: UInt32
    public let modifiers: HotkeyModifiers

    public init(keyCode: UInt32, modifiers: HotkeyModifiers) {
        self.keyCode = keyCode
        self.modifiers = modifiers
    }

    public init(key: HotkeyKey, modifiers: HotkeyModifiers) {
        self.init(keyCode: key.rawValue, modifiers: modifiers)
    }
}

public enum HotkeyAction: String, CaseIterable, Sendable, Codable {
    case next
    case previous
    case pauseResume
    case toggleMute
    case random

    public var registrationID: UInt32 {
        switch self {
        case .next:
            return 1
        case .previous:
            return 2
        case .pauseResume:
            return 3
        case .toggleMute:
            return 4
        case .random:
            return 5
        }
    }

    public init?(registrationID: UInt32) {
        guard let action = HotkeyAction.allCases.first(where: { $0.registrationID == registrationID }) else {
            return nil
        }
        self = action
    }
}

public enum HotkeyComboValidation: Equatable, Sendable {
    case valid
    case missingModifier
    case systemCombo
}

public enum HotkeyComboValidator {
    public static let reserved: Set<HotkeyCombo> = [
        HotkeyCombo(key: .tab, modifiers: [.command]),
        HotkeyCombo(key: .tab, modifiers: [.command, .shift]),
        HotkeyCombo(key: .space, modifiers: [.command]),
        HotkeyCombo(key: .space, modifiers: [.control]),
        HotkeyCombo(key: .space, modifiers: [.command, .option]),
        HotkeyCombo(key: .escape, modifiers: [.command, .option]),
        HotkeyCombo(key: .upArrow, modifiers: [.control]),
        HotkeyCombo(key: .downArrow, modifiers: [.control]),
        HotkeyCombo(key: .leftArrow, modifiers: [.control]),
        HotkeyCombo(key: .rightArrow, modifiers: [.control]),
        HotkeyCombo(key: .upArrow, modifiers: [.control, .command]),
        HotkeyCombo(key: .downArrow, modifiers: [.control, .command]),
        HotkeyCombo(key: .q, modifiers: [.control, .command]),
        HotkeyCombo(key: .q, modifiers: [.command]),
        HotkeyCombo(key: .w, modifiers: [.command]),
        HotkeyCombo(key: .h, modifiers: [.command]),
        HotkeyCombo(key: .m, modifiers: [.command]),
        HotkeyCombo(key: .n, modifiers: [.command]),
        HotkeyCombo(key: .o, modifiers: [.command]),
        HotkeyCombo(key: .p, modifiers: [.command]),
        HotkeyCombo(key: .s, modifiers: [.command]),
        HotkeyCombo(key: .c, modifiers: [.command]),
        HotkeyCombo(key: .v, modifiers: [.command]),
        HotkeyCombo(key: .x, modifiers: [.command]),
        HotkeyCombo(key: .z, modifiers: [.command]),
        HotkeyCombo(key: .z, modifiers: [.command, .shift]),
        HotkeyCombo(key: .a, modifiers: [.command]),
        HotkeyCombo(key: .f, modifiers: [.command]),
        HotkeyCombo(key: .g, modifiers: [.command]),
        HotkeyCombo(key: .b, modifiers: [.command]),
        HotkeyCombo(key: .t, modifiers: [.command]),
        HotkeyCombo(key: .r, modifiers: [.command])
    ]

    public static func validate(_ combo: HotkeyCombo) -> HotkeyComboValidation {
        if combo.modifiers.lacksModifier {
            return .missingModifier
        }
        if reserved.contains(normalized(combo)) {
            return .systemCombo
        }
        return .valid
    }

    public static func isReserved(_ combo: HotkeyCombo) -> Bool {
        return reserved.contains(normalized(combo))
    }

    static func normalized(_ combo: HotkeyCombo) -> HotkeyCombo {
        return HotkeyCombo(keyCode: combo.keyCode, modifiers: combo.modifiers.intersection(.all))
    }
}

public struct HotkeyBinding: Equatable, Sendable, Codable {
    public var action: HotkeyAction
    public var combo: HotkeyCombo

    public init(action: HotkeyAction, combo: HotkeyCombo) {
        self.action = action
        self.combo = combo
    }
}

public struct HotkeyConfiguration: Equatable, Sendable, Codable {
    public var bindings: [HotkeyBinding]

    public init(bindings: [HotkeyBinding] = []) {
        self.bindings = bindings
    }

    public func combo(for action: HotkeyAction) -> HotkeyCombo? {
        return bindings.first { $0.action == action }?.combo
    }
}

public enum HotkeyConfigurationIssue: Equatable, Sendable {
    case missingModifier(HotkeyAction)
    case systemCombo(HotkeyAction)
    case duplicateCombos(HotkeyCombo, [HotkeyAction])
}

public enum HotkeyConfigurationValidator {
    public static func issues(in configuration: HotkeyConfiguration) -> [HotkeyConfigurationIssue] {
        var result: [HotkeyConfigurationIssue] = []
        var grouped: [HotkeyCombo: [HotkeyAction]] = [:]
        for binding in configuration.bindings {
            switch HotkeyComboValidator.validate(binding.combo) {
            case .missingModifier:
                result.append(.missingModifier(binding.action))
            case .systemCombo:
                result.append(.systemCombo(binding.action))
            case .valid:
                break
            }
            grouped[HotkeyComboValidator.normalized(binding.combo), default: []].append(binding.action)
        }
        let duplicates = grouped
            .filter { $0.value.count > 1 }
            .map { entry in
                HotkeyConfigurationIssue.duplicateCombos(
                    entry.key,
                    entry.value.sorted { $0.registrationID < $1.registrationID }
                )
            }
            .sorted { lhs, rhs in
                guard case let .duplicateCombos(leftCombo, _) = lhs,
                      case let .duplicateCombos(rightCombo, _) = rhs else {
                    return false
                }
                return (leftCombo.keyCode, leftCombo.modifiers.rawValue)
                    < (rightCombo.keyCode, rightCombo.modifiers.rawValue)
            }
        result.append(contentsOf: duplicates)
        return result
    }
}
