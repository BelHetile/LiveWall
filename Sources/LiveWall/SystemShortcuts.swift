import AppKit
import Carbon.HIToolbox

/// Сочетания, которые уже заняты macOS или общими командами приложений.
/// Глобальный хоткей поверх них сломал бы привычное действие (Spotlight, скриншоты, ⌘Q…).
enum SystemShortcuts {
    private struct Shortcut {
        let keyCode: UInt32
        let modifiers: UInt32   // Carbon
        let name: String
    }

    private static let cmd = UInt32(cmdKey)
    private static let shift = UInt32(shiftKey)
    private static let opt = UInt32(optionKey)
    private static let ctrl = UInt32(controlKey)

    /// Стандартные системные сочетания (номер в com.apple.symbolichotkeys → сочетание).
    private static var defaults: [Int: Shortcut] {
        [
            64: Shortcut(keyCode: UInt32(kVK_Space), modifiers: cmd, name: L("Spotlight")),
            65: Shortcut(keyCode: UInt32(kVK_Space), modifiers: cmd | opt, name: L("Finder search")),
            60: Shortcut(keyCode: UInt32(kVK_Space), modifiers: ctrl, name: L("Switch input source")),
            61: Shortcut(keyCode: UInt32(kVK_Space), modifiers: ctrl | opt, name: L("Switch input source")),
            28: Shortcut(keyCode: UInt32(kVK_ANSI_3), modifiers: cmd | shift, name: L("Screenshot")),
            29: Shortcut(keyCode: UInt32(kVK_ANSI_3), modifiers: cmd | shift | ctrl, name: L("Screenshot")),
            30: Shortcut(keyCode: UInt32(kVK_ANSI_4), modifiers: cmd | shift, name: L("Screenshot")),
            31: Shortcut(keyCode: UInt32(kVK_ANSI_4), modifiers: cmd | shift | ctrl, name: L("Screenshot")),
            184: Shortcut(keyCode: UInt32(kVK_ANSI_5), modifiers: cmd | shift, name: L("Screenshot")),
            32: Shortcut(keyCode: UInt32(kVK_UpArrow), modifiers: ctrl, name: L("Mission Control")),
            33: Shortcut(keyCode: UInt32(kVK_DownArrow), modifiers: ctrl, name: L("App windows")),
            79: Shortcut(keyCode: UInt32(kVK_LeftArrow), modifiers: ctrl, name: L("Move one space left")),
            81: Shortcut(keyCode: UInt32(kVK_RightArrow), modifiers: ctrl, name: L("Move one space right")),
            52: Shortcut(keyCode: UInt32(kVK_ANSI_D), modifiers: cmd | opt, name: L("Show or hide the Dock")),
            98: Shortcut(keyCode: UInt32(kVK_ANSI_Slash), modifiers: cmd | shift, name: L("Help menu")),
        ]
    }

    /// Общие команды, которые есть почти в каждом приложении или в самой macOS.
    private static var appShortcuts: [Shortcut] {
        [
            Shortcut(keyCode: UInt32(kVK_Tab), modifiers: cmd, name: L("App switcher")),
            Shortcut(keyCode: UInt32(kVK_Tab), modifiers: cmd | shift, name: L("App switcher")),
            Shortcut(keyCode: UInt32(kVK_ANSI_Grave), modifiers: cmd, name: L("Switch windows")),
            Shortcut(keyCode: UInt32(kVK_ANSI_Grave), modifiers: cmd | shift, name: L("Switch windows")),
            Shortcut(keyCode: UInt32(kVK_Escape), modifiers: cmd | opt, name: L("Force Quit")),
            Shortcut(keyCode: UInt32(kVK_ANSI_Q), modifiers: cmd | ctrl, name: L("Lock Screen")),
            Shortcut(keyCode: UInt32(kVK_ANSI_F), modifiers: cmd | ctrl, name: L("Full screen")),
            Shortcut(keyCode: UInt32(kVK_Space), modifiers: cmd | ctrl, name: L("Emoji & Symbols")),
            Shortcut(keyCode: UInt32(kVK_ANSI_H), modifiers: cmd | opt, name: L("Hide others")),
            Shortcut(keyCode: UInt32(kVK_ANSI_M), modifiers: cmd | opt, name: L("Minimize all")),
            Shortcut(keyCode: UInt32(kVK_ANSI_W), modifiers: cmd | opt, name: L("Close all windows")),
            Shortcut(keyCode: UInt32(kVK_ANSI_Z), modifiers: cmd | shift, name: L("Redo")),
            Shortcut(keyCode: UInt32(kVK_ANSI_N), modifiers: cmd | shift, name: L("New folder")),
            Shortcut(keyCode: UInt32(kVK_ANSI_T), modifiers: cmd | shift, name: L("Reopen closed tab")),
            Shortcut(keyCode: UInt32(kVK_ANSI_Q), modifiers: cmd | shift, name: L("Log Out")),
            Shortcut(keyCode: UInt32(kVK_ANSI_Q), modifiers: cmd | shift | opt, name: L("Log Out")),
            Shortcut(keyCode: UInt32(kVK_ANSI_G), modifiers: cmd | shift, name: L("Go to Folder")),
        ]
    }

    /// Перевод модификаторов из настроек macOS (как у NSEvent) в Carbon.
    private static func carbon(fromCocoa flags: Int) -> UInt32 {
        var m: UInt32 = 0
        if flags & 0x20000 != 0 { m |= shift }
        if flags & 0x40000 != 0 { m |= ctrl }
        if flags & 0x80000 != 0 { m |= opt }
        if flags & 0x100000 != 0 { m |= cmd }
        return m
    }

    /// Системные сочетания с учётом того, что пользователь поменял или выключил в Системных настройках.
    private static func systemShortcuts() -> [Shortcut] {
        var table = defaults
        let prefs = UserDefaults(suiteName: "com.apple.symbolichotkeys")?
            .dictionary(forKey: "AppleSymbolicHotKeys") ?? [:]
        for (key, raw) in prefs {
            guard let id = Int(key), let entry = raw as? [String: Any] else { continue }
            let enabled: Bool
            if let b = entry["enabled"] as? Bool {
                enabled = b
            } else {
                enabled = ((entry["enabled"] as? Int) ?? 0) != 0
            }
            if !enabled {
                table[id] = nil
                continue
            }
            guard let value = entry["value"] as? [String: Any],
                  let params = value["parameters"] as? [Int], params.count >= 3,
                  params[1] != 65535 else { continue }
            let name = table[id]?.name ?? L("a macOS shortcut")
            table[id] = Shortcut(keyCode: UInt32(params[1]), modifiers: carbon(fromCocoa: params[2]), name: name)
        }
        return Array(table.values)
    }

    /// Чем занято сочетание (nil — свободно).
    static func conflict(_ binding: HotkeyBinding) -> String? {
        let mods = binding.modifiers & (cmd | shift | opt | ctrl)
        if let s = systemShortcuts().first(where: { $0.keyCode == binding.keyCode && $0.modifiers == mods }) {
            return LF("macOS (%@)", s.name)
        }
        if let s = appShortcuts.first(where: { $0.keyCode == binding.keyCode && $0.modifiers == mods }) {
            return LF("macOS (%@)", s.name)
        }
        // ⌘ + клавиша без других модификаторов — команды приложений (⌘C, ⌘Q, ⌘W…)
        if mods == cmd {
            return L("app commands (⌘ + key)")
        }
        return nil
    }

    /// Свободно ли сочетание для других приложений: пробуем зарегистрировать и сразу отпускаем.
    static func takenByAnotherApp(_ binding: HotkeyBinding) -> Bool {
        var ref: EventHotKeyRef?
        let id = EventHotKeyID(signature: OSType(0x4C575454), id: 9999) // 'LWTT'
        let status = RegisterEventHotKey(binding.keyCode, binding.modifiers, id, GetApplicationEventTarget(), 0, &ref)
        if let ref = ref { UnregisterEventHotKey(ref) }
        return status == OSStatus(eventHotKeyExistsErr)
    }
}
