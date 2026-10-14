import Foundation

public protocol ToolConsentProviding: Sendable {
    func isInstallConsented() -> Bool
}

public struct StaticToolConsent: ToolConsentProviding {
    public let consented: Bool

    public init(consented: Bool) {
        self.consented = consented
    }

    public func isInstallConsented() -> Bool {
        return consented
    }
}

public struct UserDefaultsToolConsent: ToolConsentProviding {
    public static let suiteName = "io.github.belhetile.livewall"
    public static let settingsBlobKey = "settings"
    public static let consentKey = "tools.installConsent"

    private let suiteName: String
    private let blobKey: String
    private let consentKey: String

    public init(
        suiteName: String = UserDefaultsToolConsent.suiteName,
        blobKey: String = UserDefaultsToolConsent.settingsBlobKey,
        consentKey: String = UserDefaultsToolConsent.consentKey
    ) {
        self.suiteName = suiteName
        self.blobKey = blobKey
        self.consentKey = consentKey
    }

    public func isInstallConsented() -> Bool {
        guard let defaults = UserDefaults(suiteName: suiteName) else {
            return false
        }
        if let consented = consentFromBlob(defaults) {
            return consented
        }
        return defaults.bool(forKey: consentKey)
    }

    private func consentFromBlob(_ defaults: UserDefaults) -> Bool? {
        if let blob = defaults.data(forKey: blobKey) {
            guard let object = try? JSONSerialization.jsonObject(with: blob) else {
                return nil
            }
            return consentFromObject(object)
        }
        if let dictionary = defaults.dictionary(forKey: blobKey) {
            return consentFromObject(dictionary)
        }
        return nil
    }

    private func consentFromObject(_ object: Any) -> Bool? {
        guard let dictionary = object as? [String: Any] else {
            return nil
        }
        if let direct = dictionary[consentKey] as? Bool {
            return direct
        }
        if let nested = dictionary["tools"] as? [String: Any],
           let value = nested["installConsent"] as? Bool {
            return value
        }
        return nil
    }
}
