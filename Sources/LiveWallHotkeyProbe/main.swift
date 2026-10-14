import AppKit
import CoreGraphics
import Foundation
import LiveWallEngine

let arguments = CommandLine.arguments
guard arguments.count > 2 else {
    FileHandle.standardError.write(Data("usage: LiveWallHotkeyProbe <keyCode> <modifiersRaw>\n".utf8))
    exit(2)
}

guard let keyCode = UInt32(arguments[1]), let modifiersRaw = UInt32(arguments[2]) else {
    FileHandle.standardError.write(Data("invalid keyCode or modifiers\n".utf8))
    exit(2)
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

let combo = HotkeyCombo(keyCode: keyCode, modifiers: HotkeyModifiers(rawValue: modifiersRaw))
let manager = HotkeyManager()

manager.onAction = { action in
    manager.unregisterAll()
    print("HOTKEY FIRED action=\(action.rawValue)")
    fflush(stdout)
    exit(0)
}

let registration = manager.register(combo, for: .next)
guard registration == .ok else {
    manager.unregisterAll()
    FileHandle.standardError.write(Data("registration failed: \(registration)\n".utf8))
    exit(3)
}

DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
    let source = CGEventSource(stateID: .hidSystemState)
    let virtualKey = CGKeyCode(combo.keyCode)
    let keyDown = CGEvent(keyboardEventSource: source, virtualKey: virtualKey, keyDown: true)
    keyDown?.flags = combo.modifiers.cgEventFlags
    let keyUp = CGEvent(keyboardEventSource: source, virtualKey: virtualKey, keyDown: false)
    keyUp?.flags = combo.modifiers.cgEventFlags
    keyDown?.post(tap: .cghidEventTap)
    keyUp?.post(tap: .cghidEventTap)
}

DispatchQueue.main.asyncAfter(deadline: .now() + 4.0) {
    manager.unregisterAll()
    print("HOTKEY TIMEOUT")
    fflush(stdout)
    exit(1)
}

app.run()
