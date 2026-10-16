use std::collections::BTreeMap;

use plist::Value;

use crate::model::Language;

const EN_STRINGSDICT: &[u8] = include_bytes!("../../../resources/en.lproj/Localizable.stringsdict");
const RU_STRINGS: &[u8] = include_bytes!("../../../resources/ru.lproj/Localizable.strings");
const RU_STRINGSDICT: &[u8] = include_bytes!("../../../resources/ru.lproj/Localizable.stringsdict");

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    Int(i64),
    Text(String),
}

struct PluralEntry {
    format: String,
    variables: BTreeMap<String, BTreeMap<String, String>>,
}

pub struct Strings {
    language: Language,
    table: BTreeMap<String, String>,
    plurals: BTreeMap<String, PluralEntry>,
}

impl Strings {
    pub fn new(language: Language) -> Self {
        return match language {
            Language::En => Self { language, table: BTreeMap::new(), plurals: parse_stringsdict(EN_STRINGSDICT) },
            Language::Ru => {
                Self { language, table: parse_strings(RU_STRINGS), plurals: parse_stringsdict(RU_STRINGSDICT) }
            }
        };
    }

    pub fn get(&self, key: &str) -> String {
        return self.table.get(key).cloned().unwrap_or_else(|| key.to_string());
    }

    pub fn format(&self, key: &str, args: &[Arg]) -> String {
        return substitute(&self.get(key), args);
    }

    pub fn plural(&self, key: &str, count: i64, args: &[Arg]) -> String {
        let mut all = Vec::with_capacity(args.len() + 1);
        all.push(Arg::Int(count));
        all.extend_from_slice(args);
        let Some(entry) = self.plurals.get(key) else {
            return self.format(key, &all);
        };
        let category = plural_category(self.language, count);
        let mut resolved = entry.format.clone();
        for (name, rules) in &entry.variables {
            let variant = rules
                .get(category)
                .or_else(|| rules.get("other"))
                .or_else(|| rules.get("many"))
                .or_else(|| rules.get("few"))
                .map_or("", String::as_str);
            resolved = resolved.replace(&format!("%#@{name}@"), variant);
        }
        return substitute(&resolved, &all);
    }
}

pub fn select_language(setting: Option<Language>, preferred: &[String]) -> Language {
    if let Some(language) = setting {
        return language;
    }
    for tag in preferred {
        if let Some(language) = supported(tag) {
            return language;
        }
    }
    return Language::En;
}

pub fn supported(tag: &str) -> Option<Language> {
    let lower = tag.to_lowercase();
    let primary = lower.split(['-', '_']).next().unwrap_or("");
    return match primary {
        "en" => Some(Language::En),
        "ru" => Some(Language::Ru),
        _ => None,
    };
}

pub fn plural_category(language: Language, count: i64) -> &'static str {
    return match language {
        Language::En => {
            if count == 1 {
                "one"
            } else {
                "other"
            }
        }
        Language::Ru => {
            let value = count.unsigned_abs();
            let last_two = value % 100;
            let last = value % 10;
            if last == 1 && last_two != 11 {
                "one"
            } else if (2..=4).contains(&last) && !(12..=14).contains(&last_two) {
                "few"
            } else {
                "many"
            }
        }
    };
}

pub fn substitute(template: &str, args: &[Arg]) -> String {
    let chars: Vec<char> = template.chars().collect();
    let mut out = String::with_capacity(template.len());
    let mut index = 0;
    let mut cursor = 0;
    while index < chars.len() {
        if chars[index] != '%' {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        if chars.get(index + 1) == Some(&'%') {
            out.push('%');
            index += 2;
            continue;
        }
        let start = index;
        let mut scan = index + 1;
        let digits = scan;
        while scan < chars.len() && chars[scan].is_ascii_digit() {
            scan += 1;
        }
        let position = if scan > digits && chars.get(scan) == Some(&'$') {
            scan += 1;
            chars[digits..scan - 1].iter().collect::<String>().parse::<usize>().ok()
        } else {
            scan = digits;
            None
        };
        while chars.get(scan).is_some_and(|character| matches!(character, 'l' | 'h' | 'z' | 'q')) {
            scan += 1;
        }
        let Some(specifier) = chars.get(scan).copied() else {
            out.extend(&chars[start..]);
            break;
        };
        let take = |cursor: &mut usize| -> Option<&Arg> {
            return position.map_or_else(
                || {
                    let at = *cursor;
                    *cursor += 1;
                    args.get(at)
                },
                |number| number.checked_sub(1).and_then(|at| args.get(at)),
            );
        };
        let rendered = match specifier {
            'd' | 'i' | 'u' => take(&mut cursor).map(|arg| match arg {
                Arg::Int(value) => value.to_string(),
                Arg::Text(text) => text.clone(),
            }),
            '@' | 's' => take(&mut cursor).map(|arg| match arg {
                Arg::Text(text) => text.clone(),
                Arg::Int(value) => value.to_string(),
            }),
            _ => None,
        };
        if let Some(value) = rendered {
            out.push_str(&value);
            index = scan + 1;
        } else {
            out.push('%');
            index = start + 1;
        }
    }
    return out;
}

fn parse_strings(bytes: &[u8]) -> BTreeMap<String, String> {
    return plist::from_bytes(bytes).unwrap_or_default();
}

fn parse_stringsdict(bytes: &[u8]) -> BTreeMap<String, PluralEntry> {
    let Ok(value) = plist::from_bytes::<Value>(bytes) else { return BTreeMap::new() };
    let Some(root) = value.as_dictionary() else { return BTreeMap::new() };
    let mut plurals = BTreeMap::new();
    for (key, entry) in root {
        let Some(entry) = entry.as_dictionary() else { continue };
        let format = entry.get("NSStringLocalizedFormatKey").and_then(Value::as_string).unwrap_or_default();
        let mut variables = BTreeMap::new();
        for (name, variable) in entry {
            let Some(variable) = variable.as_dictionary() else { continue };
            if !variable.contains_key("NSStringFormatSpecTypeKey") {
                continue;
            }
            let mut rules = BTreeMap::new();
            for (category, variant) in variable {
                if let Some(variant) = variant.as_string() {
                    rules.insert(category.clone(), variant.to_string());
                }
            }
            variables.insert(name.clone(), rules);
        }
        plurals.insert(key.clone(), PluralEntry { format: format.to_string(), variables });
    }
    return plurals;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn russian_plural_categories_follow_cldr() {
        let cases = [
            (0, "many"),
            (1, "one"),
            (2, "few"),
            (5, "many"),
            (11, "many"),
            (12, "many"),
            (14, "many"),
            (21, "one"),
            (22, "few"),
            (25, "many"),
            (101, "one"),
            (111, "many"),
        ];
        for (count, expected) in cases {
            assert_eq!(plural_category(Language::Ru, count), expected, "ru count {count}");
        }
    }

    #[test]
    fn english_plural_category_is_one_or_other() {
        assert_eq!(plural_category(Language::En, 1), "one");
        for count in [0, 2, 5, 21, 101] {
            assert_eq!(plural_category(Language::En, count), "other");
        }
    }

    #[test]
    fn substitution_handles_positional_and_integer_specifiers() {
        assert_eq!(
            substitute(
                "%1$@ is already used by %2$@. Press another combination.",
                &[Arg::Text("⌃⌥⌘→".into()), Arg::Text("Spotlight".into())]
            ),
            "⌃⌥⌘→ is already used by Spotlight. Press another combination."
        );
        assert_eq!(substitute("%ld file(s)", &[Arg::Int(3)]), "3 file(s)");
        assert_eq!(substitute("100%% %@", &[Arg::Text("done".into())]), "100% done");
    }

    #[test]
    fn missing_arguments_leave_the_specifier_visible() {
        assert_eq!(substitute("%@ and %@", &[Arg::Text("a".into())]), "a and %@");
    }

    #[test]
    fn russian_strings_load_from_the_embedded_resource() {
        let strings = Strings::new(Language::Ru);
        assert_eq!(strings.get("Pause"), "Пауза");
        assert_eq!(strings.get("Quit LiveWall"), "Завершить LiveWall");
        assert_eq!(strings.get("Spotlight"), "Spotlight");
        assert_eq!(strings.get("Nope, no such key"), "Nope, no such key");
    }

    #[test]
    fn russian_plurals_use_the_stringsdict_rules() {
        let strings = Strings::new(Language::Ru);
        assert_eq!(strings.plural("Every %ld min", 1, &[]), "Каждую 1 мин");
        assert_eq!(strings.plural("Every %ld min", 2, &[]), "Каждые 2 мин");
        assert_eq!(strings.plural("Every %ld min", 5, &[]), "Каждые 5 мин");
        assert_eq!(strings.plural("%ld icon(s) · right-click to remove", 1, &[]), "1 иконка · удалить — правый клик");
        assert_eq!(strings.plural("%ld icon(s) · right-click to remove", 3, &[]), "3 иконки · удалить — правый клик");
    }

    #[test]
    fn english_plurals_fall_back_to_the_key_with_the_count() {
        let strings = Strings::new(Language::En);
        assert_eq!(strings.plural("%ld icon(s) · right-click to remove", 1, &[]), "1 icon · right-click to remove");
        assert_eq!(strings.plural("%ld icon(s) · right-click to remove", 4, &[]), "4 icons · right-click to remove");
    }

    #[test]
    fn english_lookup_returns_the_key() {
        let strings = Strings::new(Language::En);
        assert_eq!(strings.get("Pause"), "Pause");
    }

    #[test]
    fn language_selection_prefers_the_setting_then_the_system() {
        assert_eq!(select_language(Some(Language::Ru), &["en-US".into()]), Language::Ru);
        assert_eq!(select_language(None, &["fr-FR".into(), "ru-RU".into()]), Language::Ru);
        assert_eq!(select_language(None, &["fr-FR".into()]), Language::En);
        assert_eq!(select_language(None, &[]), Language::En);
    }

    #[test]
    fn supported_tags_match_the_primary_subtag() {
        assert_eq!(supported("ru"), Some(Language::Ru));
        assert_eq!(supported("ru-RU"), Some(Language::Ru));
        assert_eq!(supported("en_US"), Some(Language::En));
        assert_eq!(supported("de"), None);
    }
}
