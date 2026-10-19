use std::sync::OnceLock;

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyObject, Sel};
use objc2::{ClassType, msg_send, sel};
use objc2_foundation::{
    NSArray, NSBundle, NSByteCountFormatter, NSByteCountFormatterCountStyle, NSDate, NSDateFormatter,
    NSDateFormatterStyle, NSLocale, NSNumber, NSNumberFormatter, NSNumberFormatterStyle, NSString,
};

const SUPPORTED: [&str; 2] = ["en", "ru"];
const DEVELOPMENT: &str = "en";
const RESOURCES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources");
const MAX_ARGS: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    Int(i64),
    Text(String),
}

pub struct Catalog {
    bundle: Retained<NSBundle>,
    language: String,
}

impl Catalog {
    pub fn system() -> Self {
        let main = NSBundle::mainBundle();
        let bundled = main.localizations().to_vec().iter().any(|code| SUPPORTED.contains(&code.to_string().as_str()));
        if bundled {
            let language =
                main.preferredLocalizations().firstObject().map_or(DEVELOPMENT.to_string(), |code| code.to_string());
            return Self { bundle: main, language };
        }
        let available = NSArray::from_retained_slice(&SUPPORTED.map(NSString::from_str));
        let preferred = NSLocale::preferredLanguages();
        let language = NSBundle::preferredLocalizationsFromArray_forPreferences(&available, Some(&preferred))
            .firstObject()
            .map_or(DEVELOPMENT.to_string(), |code| code.to_string());
        return Self::resources(&language);
    }

    pub fn resources(language: &str) -> Self {
        let path = NSString::from_str(&format!("{RESOURCES}/{language}.lproj"));
        return match NSBundle::bundleWithPath(&path) {
            Some(bundle) => Self { bundle, language: language.to_string() },
            None => Self { bundle: NSBundle::mainBundle(), language: DEVELOPMENT.to_string() },
        };
    }

    pub fn language(&self) -> &str {
        return &self.language;
    }

    pub fn get(&self, key: &str) -> String {
        return autoreleasepool(|_| self.lookup(key).to_string());
    }

    pub fn format(&self, key: &str, args: &[Arg]) -> String {
        return autoreleasepool(|_| render(&self.lookup(key), &self.language, args));
    }

    pub fn plural(&self, key: &str, count: i64) -> String {
        return self.format(key, &[Arg::Int(count)]);
    }

    fn lookup(&self, key: &str) -> Retained<NSString> {
        return self.bundle.localizedStringForKey_value_table(&NSString::from_str(key), None, None);
    }
}

static CATALOG: OnceLock<Catalog> = OnceLock::new();

fn catalog() -> &'static Catalog {
    return CATALOG.get_or_init(Catalog::system);
}

pub const fn key(text: &'static str) -> &'static str {
    return text;
}

pub fn tr(key: &str) -> String {
    return catalog().get(key);
}

pub fn trf(key: &str, args: &[Arg]) -> String {
    return catalog().format(key, args);
}

pub fn trp(key: &str, count: i64) -> String {
    return catalog().plural(key, count);
}

pub fn date(timestamp: jiff::Timestamp) -> String {
    return autoreleasepool(|_| {
        let formatter = NSDateFormatter::new();
        formatter.setDateStyle(NSDateFormatterStyle::MediumStyle);
        formatter.setTimeStyle(NSDateFormatterStyle::NoStyle);
        #[allow(clippy::cast_precision_loss)]
        let date = NSDate::dateWithTimeIntervalSince1970(timestamp.as_millisecond() as f64 / 1000.0);
        return formatter.stringFromDate(&date).to_string();
    });
}

pub fn bytes(count: i64) -> String {
    return autoreleasepool(|_| {
        return NSByteCountFormatter::stringFromByteCount_countStyle(count, NSByteCountFormatterCountStyle::File)
            .to_string();
    });
}

pub fn percent(fraction: f64) -> String {
    return autoreleasepool(|_| {
        let number = NSNumber::numberWithDouble(fraction);
        return NSNumberFormatter::localizedStringFromNumber_numberStyle(&number, NSNumberFormatterStyle::PercentStyle)
            .to_string();
    });
}

pub fn english(key: &str, args: &[Arg]) -> String {
    return autoreleasepool(|_| render(&NSString::from_str(key), DEVELOPMENT, args));
}

type FormatSend = unsafe extern "C" fn(*mut AnyObject, Sel, *const NSString, *const NSLocale, ...) -> *mut NSString;

fn render(template: &NSString, language: &str, args: &[Arg]) -> String {
    if args.len() > MAX_ARGS {
        return template.to_string();
    }
    let locale = NSLocale::localeWithLocaleIdentifier(&NSString::from_str(language));
    let texts: Vec<Option<Retained<NSString>>> = args
        .iter()
        .map(|arg| match arg {
            Arg::Text(text) => Some(NSString::from_str(text)),
            Arg::Int(_) => None,
        })
        .collect();
    let mut words = [0usize; MAX_ARGS];
    for (index, arg) in args.iter().enumerate() {
        words[index] = match (arg, &texts[index]) {
            (Arg::Int(value), _) => usize::from_ne_bytes(value.to_ne_bytes()),
            (Arg::Text(_), Some(text)) => Retained::as_ptr(text).addr(),
            (Arg::Text(_), None) => 0,
        };
    }
    let template = std::ptr::from_ref::<NSString>(template);
    let locale = std::ptr::from_ref::<NSLocale>(&locale);
    let raw = unsafe {
        let send: FormatSend = std::mem::transmute::<*const (), FormatSend>(objc2::ffi::objc_msgSend as *const ());
        let allocated: *mut AnyObject = msg_send![NSString::class(), alloc];
        let selector = sel!(initWithFormat:locale:);
        let w = words;
        match args.len() {
            0 => send(allocated, selector, template, locale),
            1 => send(allocated, selector, template, locale, w[0]),
            2 => send(allocated, selector, template, locale, w[0], w[1]),
            3 => send(allocated, selector, template, locale, w[0], w[1], w[2]),
            4 => send(allocated, selector, template, locale, w[0], w[1], w[2], w[3]),
            5 => send(allocated, selector, template, locale, w[0], w[1], w[2], w[3], w[4]),
            _ => send(allocated, selector, template, locale, w[0], w[1], w[2], w[3], w[4], w[5]),
        }
    };
    drop(texts);
    return unsafe { Retained::from_raw(raw) }
        .map_or_else(|| unsafe { &*template }.to_string(), |text| text.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn russian_lookup_reads_the_lproj_bundle() {
        let catalog = Catalog::resources("ru");
        assert_eq!(catalog.language(), "ru");
        assert_eq!(catalog.get("Pause"), "Пауза");
        assert_eq!(catalog.get("Quit LiveWall"), "Завершить LiveWall");
    }

    #[test]
    fn english_lookup_returns_the_key() {
        let catalog = Catalog::resources("en");
        assert_eq!(catalog.get("Pause"), "Pause");
        assert_eq!(catalog.get("no such key at all"), "no such key at all");
    }

    #[test]
    fn missing_lproj_falls_back_to_english() {
        let catalog = Catalog::resources("xx");
        assert_eq!(catalog.language(), "en");
        assert_eq!(catalog.get("Pause"), "Pause");
    }

    #[test]
    fn positional_arguments_reorder() {
        let catalog = Catalog::resources("ru");
        let text = catalog.format(
            "%@ is already used by %@. Press another combination.",
            &[Arg::Text("⌃⌥⌘→".into()), Arg::Text("Spotlight".into())],
        );
        assert_eq!(text, "⌃⌥⌘→ уже занято: Spotlight. Нажмите другое сочетание.");
    }

    #[test]
    fn english_formats_mixed_arguments() {
        let text = english(
            "%@ skipped %ld files on %@",
            &[Arg::Text("Import".into()), Arg::Int(7), Arg::Text("Desktop".into())],
        );
        assert_eq!(text, "Import skipped 7 files on Desktop");
        assert_eq!(english("100%% done", &[]), "100% done");
        assert_eq!(english("App rule: %@", &[Arg::Text("50% «x»".into())]), "App rule: 50% «x»");
    }

    #[test]
    fn plurals_follow_the_bundle_language() {
        let key = "%ld wallpapers inside move to Unfiled.";
        let en = Catalog::resources("en");
        assert_eq!(en.plural(key, 1), "1 wallpaper inside moves to Unfiled.");
        assert_eq!(en.plural(key, 4), "4 wallpapers inside move to Unfiled.");
        let ru = Catalog::resources("ru");
        assert_eq!(ru.plural(key, 21), "Обои (21) внутри перейдут в «Без папки».");
    }

    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;

    const CALLS: [(&str, bool); 4] = [("tr", false), ("trf", false), ("trp", true), ("key", false)];

    fn unescape(literal: &str) -> String {
        let mut out = String::with_capacity(literal.len());
        let mut chars = literal.chars();
        while let Some(character) = chars.next() {
            if character != '\\' {
                out.push(character);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => {}
            }
        }
        return out;
    }

    fn literal_after(source: &str, start: usize) -> Option<String> {
        let rest = source[start..].trim_start();
        let body = rest.strip_prefix('"')?;
        let mut escaped = false;
        for (index, character) in body.char_indices() {
            match character {
                '\\' if !escaped => escaped = true,
                '"' if !escaped => return Some(unescape(&body[..index])),
                _ => escaped = false,
            }
        }
        return None;
    }

    fn scan(source: &str, keys: &mut BTreeMap<String, bool>) {
        for (name, plural) in CALLS {
            let needle = format!("{name}(");
            for (at, _) in source.match_indices(&needle) {
                let before = source[..at].chars().next_back();
                if before.is_some_and(|character| character.is_alphanumeric() || character == '_') {
                    continue;
                }
                if let Some(key) = literal_after(source, at + needle.len()) {
                    keys.insert(key, plural);
                }
            }
        }
    }

    fn source_keys() -> BTreeMap<String, bool> {
        let mut keys = BTreeMap::new();
        let mut pending = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).expect("src dir").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|extension| extension == "rs") && !path.ends_with("i18n.rs") {
                    scan(&std::fs::read_to_string(&path).expect("source"), &mut keys);
                }
            }
        }
        return keys;
    }

    fn table(language: &str, name: &str) -> BTreeMap<String, plist::Value> {
        let path = format!("{RESOURCES}/{language}.lproj/{name}");
        return plist::from_file(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    }

    fn specifiers(text: &str) -> BTreeMap<usize, char> {
        let chars: Vec<char> = text.chars().collect();
        let mut found = BTreeMap::new();
        let mut sequential = 0;
        let mut index = 0;
        while index < chars.len() {
            if chars[index] != '%' {
                index += 1;
                continue;
            }
            if chars.get(index + 1) == Some(&'%') {
                index += 2;
                continue;
            }
            let mut scan = index + 1;
            let digits = scan;
            while chars.get(scan).is_some_and(char::is_ascii_digit) {
                scan += 1;
            }
            let position = if scan > digits && chars.get(scan) == Some(&'$') {
                let number: String = chars[digits..scan].iter().collect();
                scan += 1;
                number.parse::<usize>().ok()
            } else {
                scan = digits;
                None
            };
            let mut long = false;
            while chars.get(scan) == Some(&'l') {
                long = true;
                scan += 1;
            }
            let kind = match chars.get(scan) {
                Some('@') => '@',
                Some('d') if long => 'd',
                Some('#') => '#',
                other => panic!("unsupported specifier {other:?} in {text:?}"),
            };
            let slot = position.unwrap_or_else(|| {
                sequential += 1;
                return sequential;
            });
            found.insert(slot, kind);
            index = scan + 1;
        }
        return found;
    }

    #[test]
    fn every_key_is_translated_and_every_translation_is_used() {
        let keys = source_keys();
        let strings = table("ru", "Localizable.strings");
        let ru_plurals = table("ru", "Localizable.stringsdict");
        let en_plurals = table("en", "Localizable.stringsdict");
        let plain: BTreeSet<&String> = keys.iter().filter(|(_, plural)| !**plural).map(|(key, _)| key).collect();
        let plural: BTreeSet<&String> = keys.iter().filter(|(_, plural)| **plural).map(|(key, _)| key).collect();
        let missing: Vec<&&String> = plain.iter().filter(|key| !strings.contains_key(key.as_str())).collect();
        assert!(missing.is_empty(), "missing ru translations: {missing:#?}");
        let unused: Vec<&String> = strings.keys().filter(|key| !plain.contains(key)).collect();
        assert!(unused.is_empty(), "unused ru translations: {unused:#?}");
        for (name, dict) in [("ru", &ru_plurals), ("en", &en_plurals)] {
            let missing: Vec<&&String> = plural.iter().filter(|key| !dict.contains_key(key.as_str())).collect();
            assert!(missing.is_empty(), "missing {name} plurals: {missing:#?}");
            let unused: Vec<&String> = dict.keys().filter(|key| !plural.contains(key)).collect();
            assert!(unused.is_empty(), "unused {name} plurals: {unused:#?}");
        }
    }

    #[test]
    fn translations_keep_the_key_specifiers() {
        for (key, plural) in source_keys() {
            let expected = specifiers(&key);
            assert!(expected.values().all(|kind| matches!(kind, '@' | 'd')), "key {key:?} must use %@ or %ld only");
            if plural {
                assert_eq!(expected, BTreeMap::from([(1, 'd')]), "plural key {key:?} must take only its %ld count");
            }
        }
        for (key, value) in table("ru", "Localizable.strings") {
            let text = value.as_string().unwrap_or_else(|| panic!("{key:?} is not a string"));
            assert_eq!(specifiers(text), specifiers(&key), "ru {key:?} -> {text:?}");
        }
        let categories = [("en", vec!["one", "other"]), ("ru", vec!["one", "few", "many", "other"])];
        for (language, needed) in categories {
            for (key, entry) in table(language, "Localizable.stringsdict") {
                let entry = entry.as_dictionary().unwrap_or_else(|| panic!("{language} {key:?}"));
                let rules = entry
                    .iter()
                    .filter_map(|(_, value)| value.as_dictionary())
                    .find(|value| value.contains_key("NSStringFormatSpecTypeKey"))
                    .unwrap_or_else(|| panic!("{language} {key:?} has no plural rule"));
                for category in &needed {
                    let variant = rules.get(category).and_then(plist::Value::as_string);
                    let variant = variant.unwrap_or_else(|| panic!("{language} {key:?} lacks {category}"));
                    assert!(variant.contains("%ld"), "{language} {key:?} {category} must show the count: {variant:?}");
                }
            }
        }
    }

    #[test]
    fn lookups_work_off_the_main_thread() {
        let text = std::thread::spawn(|| Catalog::resources("ru").get("Pause")).join().expect("thread");
        assert_eq!(text, "Пауза");
    }
}
