import Foundation
import Carbon.HIToolbox

/// Global hotkeys via Carbon (no permissions needed in System Settings).
final class HotkeyCenter {
    static let shared = HotkeyCenter()

    private var refs: [EventHotKeyRef] = []
    private var actions: [UInt32: () -> Void] = [:]
    private var installed = false

    func unregisterAll() {
        for r in refs { UnregisterEventHotKey(r) }
        refs.removeAll()
        actions.removeAll()
    }

    @discardableResult
    func register(id: UInt32, binding: HotkeyBinding, action: @escaping () -> Void) -> Bool {
        installHandlerIfNeeded()
        var ref: EventHotKeyRef?
        let hkID = EventHotKeyID(signature: OSType(0x4C575742), id: id) // 'LWWB'
        let status = RegisterEventHotKey(binding.keyCode,
                                         binding.modifiers,
                                         hkID,
                                         GetApplicationEventTarget(),
                                         0,
                                         &ref)
        guard status == noErr, let ref = ref else {
            NSLog("LiveWall: hotkey \(HotkeyFormat.string(binding)) not registered (status \(status))")
            return false
        }
        refs.append(ref)
        actions[id] = action
        return true
    }

    fileprivate func fire(_ id: UInt32) {
        actions[id]?()
    }

    private func installHandlerIfNeeded() {
        guard !installed else { return }
        installed = true
        var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard),
                                 eventKind: UInt32(kEventHotKeyPressed))
        InstallEventHandler(GetApplicationEventTarget(), hotKeyHandler, 1, &spec, nil, nil)
    }
}

private func hotKeyHandler(_ next: EventHandlerCallRef?,
                           _ event: EventRef?,
                           _ userData: UnsafeMutableRawPointer?) -> OSStatus {
    var hk = EventHotKeyID()
    let status = GetEventParameter(event,
                                   EventParamName(kEventParamDirectObject),
                                   EventParamType(typeEventHotKeyID),
                                   nil,
                                   MemoryLayout<EventHotKeyID>.size,
                                   nil,
                                   &hk)
    if status == noErr {
        let id = hk.id
        DispatchQueue.main.async {
            HotkeyCenter.shared.fire(id)
        }
    }
    return noErr
}
