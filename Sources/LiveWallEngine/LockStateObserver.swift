import Foundation

final class LockStateObserver: NSObject {
    private let handler: @MainActor (Bool) -> Void

    init(handler: @escaping @MainActor (Bool) -> Void) {
        self.handler = handler
        super.init()
        let center = DistributedNotificationCenter.default()
        center.addObserver(
            self,
            selector: #selector(screenLocked(_:)),
            name: Notification.Name("com.apple.screenIsLocked"),
            object: nil,
            suspensionBehavior: .deliverImmediately
        )
        center.addObserver(
            self,
            selector: #selector(screenUnlocked(_:)),
            name: Notification.Name("com.apple.screenIsUnlocked"),
            object: nil,
            suspensionBehavior: .deliverImmediately
        )
    }

    func invalidate() {
        DistributedNotificationCenter.default().removeObserver(self)
    }

    @objc private func screenLocked(_ notification: Notification) {
        dispatch(true)
    }

    @objc private func screenUnlocked(_ notification: Notification) {
        dispatch(false)
    }

    private func dispatch(_ locked: Bool) {
        let handler = handler
        Task { @MainActor in
            handler(locked)
        }
    }
}
