import Foundation
import AppKit
import CommonCrypto
import Security

/// Пароли закрытых папок: храним только соль и PBKDF2-SHA256 хеш.
enum PasswordHasher {
    static let rounds: UInt32 = 200_000

    static func makeSalt() -> Data {
        var bytes = [UInt8](repeating: 0, count: 16)
        let status = SecRandomCopyBytes(kSecRandomDefault, bytes.count, &bytes)
        if status != errSecSuccess {
            // запасной вариант, если системный генератор недоступен
            bytes = (0..<16).map { _ in UInt8.random(in: 0...255) }
        }
        return Data(bytes)
    }

    static func hash(_ password: String, salt: Data) -> Data {
        var derived = [UInt8](repeating: 0, count: 32)
        let passwordBytes = Array(password.utf8)
        let saltBytes = [UInt8](salt)
        passwordBytes.withUnsafeBufferPointer { pw in
            pw.baseAddress!.withMemoryRebound(to: CChar.self, capacity: passwordBytes.count) { pwChars in
                _ = CCKeyDerivationPBKDF(CCPBKDFAlgorithm(kCCPBKDF2),
                                         pwChars, passwordBytes.count,
                                         saltBytes, saltBytes.count,
                                         CCPseudoRandomAlgorithm(kCCPRFHmacAlgSHA256),
                                         rounds,
                                         &derived, derived.count)
            }
        }
        return Data(derived)
    }

    static func verify(_ password: String, saltB64: String, hashB64: String) -> Bool {
        guard let salt = Data(base64Encoded: saltB64), let expected = Data(base64Encoded: hashB64) else { return false }
        let actual = hash(password, salt: salt)
        guard actual.count == expected.count else { return false }
        // сравнение за постоянное время
        var diff: UInt8 = 0
        for (a, b) in zip(actual, expected) { diff |= a ^ b }
        return diff == 0
    }
}

enum PasswordPrompt {
    /// Окно ввода пароля. При `confirm` — два поля и проверка совпадения.
    static func ask(title: String, message: String?, button: String, confirm: Bool) -> String? {
        let alert = NSAlert()
        alert.messageText = title
        if let message = message { alert.informativeText = message }
        alert.addButton(withTitle: button)
        alert.addButton(withTitle: L("Cancel"))

        let width: CGFloat = 280
        let container = NSView(frame: NSRect(x: 0, y: 0, width: width, height: confirm ? 56 : 24))
        let first = NSSecureTextField(frame: NSRect(x: 0, y: confirm ? 32 : 0, width: width, height: 24))
        first.placeholderString = L("Password")
        container.addSubview(first)
        var second: NSSecureTextField?
        if confirm {
            let field = NSSecureTextField(frame: NSRect(x: 0, y: 0, width: width, height: 24))
            field.placeholderString = L("Repeat password")
            container.addSubview(field)
            first.nextKeyView = field
            second = field
        }
        alert.accessoryView = container
        alert.window.initialFirstResponder = first
        NSApp.activate(ignoringOtherApps: true)

        guard alert.runModal() == .alertFirstButtonReturn else { return nil }
        let password = first.stringValue
        guard !password.isEmpty else { return nil }
        if let second = second, second.stringValue != password {
            let err = NSAlert()
            err.messageText = L("Passwords don't match")
            err.informativeText = L("The folder was not changed.")
            err.runModal()
            return nil
        }
        return password
    }
}
