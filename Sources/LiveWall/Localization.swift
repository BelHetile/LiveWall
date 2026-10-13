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
}

/// Translates an interface string. The key is the English text.
func L(_ key: String) -> String {
    guard Lang.current == .russian else { return key }
    return RussianStrings.table[key] ?? key
}

/// Translates a string with format arguments (%@, %ld).
func LF(_ key: String, _ args: CVarArg...) -> String {
    String(format: L(key), arguments: args)
}
