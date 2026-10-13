import Foundation

/// Язык интерфейса.
enum AppLanguage: String, Codable, CaseIterable, Identifiable {
    case english = "en"
    case russian = "ru"

    var id: String { rawValue }

    /// Название языка на нём самом.
    var nativeName: String {
        switch self {
        case .english: return "English"
        case .russian: return "Русский"
        }
    }

    /// Язык системы: русский, если он первый в списке предпочтительных.
    static var system: AppLanguage {
        (Locale.preferredLanguages.first ?? "en").lowercased().hasPrefix("ru") ? .russian : .english
    }
}

enum Lang {
    /// Текущий язык; выставляет Library при загрузке и смене настроек.
    static var current: AppLanguage = .system
}

/// Перевод строки интерфейса. Ключ — английский текст.
func L(_ key: String) -> String {
    guard Lang.current == .russian else { return key }
    return RussianStrings.table[key] ?? key
}

/// Перевод строки с подстановками (%@, %ld).
func LF(_ key: String, _ args: CVarArg...) -> String {
    String(format: L(key), arguments: args)
}
