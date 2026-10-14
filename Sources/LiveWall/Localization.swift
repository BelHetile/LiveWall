import Foundation

/// Interface language.
enum AppLanguage: String, Codable, CaseIterable, Identifiable {
    case english = "en"
    case russian = "ru"

    var id: String { rawValue }

    /// The language's name in that language.
    var nativeName: String {
        switch self {
        case .english: return "English"
        case .russian: return "Русский"
        }
    }

    /// System language: Russian if it is first in the preferred list.
    static var system: AppLanguage {
        (Locale.preferredLanguages.first ?? "en").lowercased().hasPrefix("ru") ? .russian : .english
    }
}

enum Lang {
    /// Current language; set by Library on load and when settings change.
    static var current: AppLanguage = .system

    fileprivate static let bundles: [AppLanguage: Bundle] = Dictionary(uniqueKeysWithValues:
        AppLanguage.allCases.compactMap { language in
            guard let path = Bundle.main.path(forResource: language.rawValue, ofType: "lproj"),
                  let bundle = Bundle(path: path) else { return nil }
            return (language, bundle)
        })

    static func setCurrent(_ language: AppLanguage) {
        current = language
        UserDefaults.standard.set([language.rawValue], forKey: "AppleLanguages")
    }

    static var appleLanguagesChoice: AppLanguage? {
        guard let id = Bundle.main.bundleIdentifier,
              let languages = UserDefaults.standard.persistentDomain(forName: id)?["AppleLanguages"] as? [String],
              let first = languages.first?.lowercased() else { return nil }
        return AppLanguage.allCases.first { first == $0.rawValue || first.hasPrefix($0.rawValue + "-") }
    }
}

/// Translates an interface string. The key is the English text.
func L(_ key: String) -> String {
    Lang.bundles[Lang.current]?.localizedString(forKey: key, value: key, table: nil) ?? key
}

/// Translates a string with format arguments (%@, %ld).
func LF(_ key: String, _ args: CVarArg...) -> String {
    String(format: L(key), locale: Locale(identifier: Lang.current.rawValue), arguments: args)
}
