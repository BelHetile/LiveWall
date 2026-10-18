use std::fmt;

use proto::{HotkeyAction, Hotkeys, KeyCombo, Modifier};

use crate::i18n::{self, tr};

pub const CMD: u32 = 0x0100;
pub const SHIFT: u32 = 0x0200;
pub const OPTION: u32 = 0x0800;
pub const CONTROL: u32 = 0x1000;

const KEYS: &[(&str, u32)] = &[
    ("a", 0x00),
    ("s", 0x01),
    ("d", 0x02),
    ("f", 0x03),
    ("h", 0x04),
    ("g", 0x05),
    ("z", 0x06),
    ("x", 0x07),
    ("c", 0x08),
    ("v", 0x09),
    ("b", 0x0B),
    ("q", 0x0C),
    ("w", 0x0D),
    ("e", 0x0E),
    ("r", 0x0F),
    ("y", 0x10),
    ("t", 0x11),
    ("1", 0x12),
    ("2", 0x13),
    ("3", 0x14),
    ("4", 0x15),
    ("6", 0x16),
    ("5", 0x17),
    ("=", 0x18),
    ("9", 0x19),
    ("7", 0x1A),
    ("-", 0x1B),
    ("8", 0x1C),
    ("0", 0x1D),
    ("]", 0x1E),
    ("o", 0x1F),
    ("u", 0x20),
    ("[", 0x21),
    ("i", 0x22),
    ("p", 0x23),
    ("return", 0x24),
    ("l", 0x25),
    ("j", 0x26),
    ("'", 0x27),
    ("k", 0x28),
    (";", 0x29),
    ("\\", 0x2A),
    (",", 0x2B),
    ("/", 0x2C),
    ("n", 0x2D),
    ("m", 0x2E),
    (".", 0x2F),
    ("tab", 0x30),
    ("space", 0x31),
    ("`", 0x32),
    ("delete", 0x33),
    ("escape", 0x35),
    ("f17", 0x40),
    ("f18", 0x4F),
    ("f19", 0x50),
    ("f20", 0x5A),
    ("f5", 0x60),
    ("f6", 0x61),
    ("f7", 0x62),
    ("f3", 0x63),
    ("f8", 0x64),
    ("f9", 0x65),
    ("f11", 0x67),
    ("f13", 0x69),
    ("f16", 0x6A),
    ("f14", 0x6B),
    ("f10", 0x6D),
    ("f12", 0x6F),
    ("f15", 0x71),
    ("home", 0x73),
    ("pageup", 0x74),
    ("forwarddelete", 0x75),
    ("f4", 0x76),
    ("end", 0x77),
    ("f2", 0x78),
    ("pagedown", 0x79),
    ("f1", 0x7A),
    ("left", 0x7B),
    ("right", 0x7C),
    ("down", 0x7D),
    ("up", 0x7E),
];

const ALIASES: &[(&str, &str)] = &[
    ("arrowleft", "left"),
    ("arrowright", "right"),
    ("arrowup", "up"),
    ("arrowdown", "down"),
    ("leftarrow", "left"),
    ("rightarrow", "right"),
    ("uparrow", "up"),
    ("downarrow", "down"),
    ("spacebar", "space"),
    ("enter", "return"),
    ("esc", "escape"),
    ("backspace", "delete"),
    ("del", "delete"),
    ("forwarddeletekey", "forwarddelete"),
    ("minus", "-"),
    ("hyphen", "-"),
    ("equal", "="),
    ("equals", "="),
    ("leftbracket", "["),
    ("rightbracket", "]"),
    ("backslash", "\\"),
    ("semicolon", ";"),
    ("quote", "'"),
    ("apostrophe", "'"),
    ("comma", ","),
    ("period", "."),
    ("dot", "."),
    ("slash", "/"),
    ("grave", "`"),
    ("backtick", "`"),
    ("page-up", "pageup"),
    ("page-down", "pagedown"),
];

const GLYPHS: &[(&str, &str)] = &[
    ("left", "←"),
    ("right", "→"),
    ("up", "↑"),
    ("down", "↓"),
    ("space", "Space"),
    ("tab", "⇥"),
    ("return", "↩"),
    ("escape", "⎋"),
    ("delete", "⌫"),
    ("forwarddelete", "⌦"),
    ("home", "↖"),
    ("end", "↘"),
    ("pageup", "⇞"),
    ("pagedown", "⇟"),
];

const RESERVED: &[(&str, u32, &str)] = &[
    ("space", CMD, i18n::key("Spotlight")),
    ("space", OPTION | CMD, i18n::key("Finder search")),
    ("space", CONTROL | CMD, i18n::key("Emoji & Symbols")),
    ("space", CONTROL, i18n::key("Switch input source")),
    ("tab", CMD, i18n::key("App switcher")),
    ("tab", SHIFT | CMD, i18n::key("App switcher")),
    ("`", CMD, i18n::key("Switch windows")),
    ("`", SHIFT | CMD, i18n::key("Switch windows")),
    ("up", CONTROL, i18n::key("Mission Control")),
    ("down", CONTROL, i18n::key("App windows")),
    ("left", CONTROL, i18n::key("Move one space left")),
    ("right", CONTROL, i18n::key("Move one space right")),
    ("up", CONTROL | CMD, i18n::key("Mission Control")),
    ("down", CONTROL | CMD, i18n::key("Show or hide the Dock")),
    ("3", SHIFT | CMD, i18n::key("Screenshot")),
    ("4", SHIFT | CMD, i18n::key("Screenshot")),
    ("5", SHIFT | CMD, i18n::key("Screenshot")),
    ("6", SHIFT | CMD, i18n::key("Screenshot")),
    ("escape", OPTION | CMD, i18n::key("Force Quit")),
    ("/", SHIFT | CMD, i18n::key("Help menu")),
    ("f", CONTROL | CMD, i18n::key("Full screen")),
    ("q", CONTROL | CMD, i18n::key("Lock Screen")),
    ("q", SHIFT | CMD, i18n::key("Log Out")),
    ("q", CMD, i18n::key("app commands (⌘ + key)")),
    ("w", CMD, i18n::key("app commands (⌘ + key)")),
    ("h", CMD, i18n::key("app commands (⌘ + key)")),
    ("h", OPTION | CMD, i18n::key("Hide Others")),
    ("m", CMD, i18n::key("app commands (⌘ + key)")),
    ("m", OPTION | CMD, i18n::key("Minimize all")),
    ("n", CMD, i18n::key("app commands (⌘ + key)")),
    ("o", CMD, i18n::key("app commands (⌘ + key)")),
    ("p", CMD, i18n::key("app commands (⌘ + key)")),
    ("s", CMD, i18n::key("app commands (⌘ + key)")),
    ("c", CMD, i18n::key("app commands (⌘ + key)")),
    ("v", CMD, i18n::key("app commands (⌘ + key)")),
    ("x", CMD, i18n::key("app commands (⌘ + key)")),
    ("z", CMD, i18n::key("app commands (⌘ + key)")),
    ("z", SHIFT | CMD, i18n::key("app commands (⌘ + key)")),
    ("a", CMD, i18n::key("app commands (⌘ + key)")),
    ("f", CMD, i18n::key("app commands (⌘ + key)")),
    ("g", CMD, i18n::key("app commands (⌘ + key)")),
    ("b", CMD, i18n::key("app commands (⌘ + key)")),
    ("t", CMD, i18n::key("app commands (⌘ + key)")),
    ("r", CMD, i18n::key("app commands (⌘ + key)")),
    (",", CMD, i18n::key("app commands (⌘ + key)")),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    UnknownKey(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            ParseError::Empty => formatter.write_str("no key in combo"),
            ParseError::UnknownKey(key) => write!(formatter, "unknown key {key}"),
        };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    UnknownKey(String),
    MissingModifier,
    Reserved(&'static str),
    Duplicate(HotkeyAction),
}

pub fn title(action: HotkeyAction) -> &'static str {
    return match action {
        HotkeyAction::Next => i18n::key("Next Wallpaper"),
        HotkeyAction::Previous => i18n::key("Previous Wallpaper"),
        HotkeyAction::VolumeUp => i18n::key("Volume Up"),
        HotkeyAction::VolumeDown => i18n::key("Volume Down"),
        HotkeyAction::TogglePause => i18n::key("Play / Pause"),
        HotkeyAction::ToggleMute => i18n::key("Mute"),
        HotkeyAction::Random => i18n::key("Random Wallpaper"),
        HotkeyAction::Lock => i18n::key("Lock Private Folders"),
    };
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            ValidationError::UnknownKey(key) => write!(formatter, "unknown key {key}"),
            ValidationError::MissingModifier => formatter.write_str("needs at least one of ⌘ ⌃ ⌥, or an F-key"),
            ValidationError::Reserved(owner) => write!(formatter, "reserved by {owner}"),
            ValidationError::Duplicate(_) => formatter.write_str("already bound to another action"),
        };
    }
}

pub fn keycode(key: &str) -> Option<u32> {
    let key = normalize_key(key);
    return KEYS.iter().find(|(name, _)| *name == key).map(|(_, code)| *code);
}

pub fn key_name(code: u32) -> Option<&'static str> {
    return KEYS.iter().find(|(_, value)| *value == code).map(|(name, _)| *name);
}

pub fn modifier_mask(modifiers: &[Modifier]) -> u32 {
    let mut mask = 0;
    for modifier in modifiers {
        mask |= match modifier {
            Modifier::Control => CONTROL,
            Modifier::Option => OPTION,
            Modifier::Command => CMD,
            Modifier::Shift => SHIFT,
        };
    }
    return mask;
}

pub fn parse(input: &str) -> Result<KeyCombo, ParseError> {
    let mut modifiers: Vec<Modifier> = Vec::new();
    let mut key: Option<String> = None;
    for token in input.split('+') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        if let Some(modifier) = modifier_token(token) {
            if !modifiers.contains(&modifier) {
                modifiers.push(modifier);
            }
            continue;
        }
        let normalized = normalize_key(token);
        if keycode(&normalized).is_none() {
            return Err(ParseError::UnknownKey(token.to_string()));
        }
        key = Some(normalized);
    }
    let Some(key) = key else { return Err(ParseError::Empty) };
    sort_modifiers(&mut modifiers);
    return Ok(KeyCombo { key, modifiers });
}

#[cfg(test)]
pub fn format(combo: &KeyCombo) -> String {
    let mut parts: Vec<String> = Vec::new();
    for modifier in canonical_modifiers(&combo.modifiers) {
        parts.push(
            match modifier {
                Modifier::Control => "ctrl",
                Modifier::Option => "opt",
                Modifier::Command => "cmd",
                Modifier::Shift => "shift",
            }
            .to_string(),
        );
    }
    parts.push(normalize_key(&combo.key));
    return parts.join("+");
}

pub fn display(combo: &KeyCombo) -> String {
    let mut text = String::new();
    for modifier in canonical_modifiers(&combo.modifiers) {
        text.push_str(match modifier {
            Modifier::Control => "⌃",
            Modifier::Option => "⌥",
            Modifier::Command => "⌘",
            Modifier::Shift => "⇧",
        });
    }
    text.push_str(&glyph(&normalize_key(&combo.key)));
    return text;
}

pub fn reserved(combo: &KeyCombo) -> Option<&'static str> {
    let key = normalize_key(&combo.key);
    let mask = modifier_mask(&combo.modifiers);
    return RESERVED.iter().find(|(name, needed, _)| *name == key && *needed == mask).map(|(_, _, owner)| *owner);
}

pub fn validate(candidate: &KeyCombo, action: HotkeyAction, hotkeys: &Hotkeys) -> Result<(), ValidationError> {
    if keycode(&candidate.key).is_none() {
        return Err(ValidationError::UnknownKey(candidate.key.clone()));
    }
    let mask = modifier_mask(&candidate.modifiers);
    if mask & (CMD | CONTROL | OPTION) == 0 && !is_function_key(&candidate.key) {
        return Err(ValidationError::MissingModifier);
    }
    if let Some(owner) = reserved(candidate) {
        return Err(ValidationError::Reserved(owner));
    }
    for other in HotkeyAction::ALL.into_iter().filter(|other| *other != action) {
        if let Some(existing) = hotkeys.get(other)
            && same(existing, candidate)
        {
            return Err(ValidationError::Duplicate(other));
        }
    }
    return Ok(());
}

pub fn same(left: &KeyCombo, right: &KeyCombo) -> bool {
    return keycode(&left.key) == keycode(&right.key)
        && modifier_mask(&left.modifiers) == modifier_mask(&right.modifiers);
}

pub fn registration_id(action: HotkeyAction) -> u32 {
    return match action {
        HotkeyAction::Next => 1,
        HotkeyAction::Previous => 2,
        HotkeyAction::TogglePause => 3,
        HotkeyAction::ToggleMute => 4,
        HotkeyAction::Random => 5,
        HotkeyAction::VolumeUp => 6,
        HotkeyAction::VolumeDown => 7,
        HotkeyAction::Lock => 8,
    };
}

pub fn action_from_id(id: u32) -> Option<HotkeyAction> {
    return HotkeyAction::ALL.into_iter().find(|action| registration_id(*action) == id);
}

fn is_function_key(key: &str) -> bool {
    let normalized = normalize_key(key);
    let Some(digits) = normalized.strip_prefix('f') else { return false };
    return !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
}

fn modifier_token(token: &str) -> Option<Modifier> {
    return match normalize_key(token).as_str() {
        "ctrl" | "control" | "⌃" => Some(Modifier::Control),
        "opt" | "option" | "alt" | "⌥" => Some(Modifier::Option),
        "cmd" | "command" | "⌘" => Some(Modifier::Command),
        "shift" | "⇧" => Some(Modifier::Shift),
        _ => None,
    };
}

fn normalize_key(key: &str) -> String {
    let lowered: String =
        key.trim().chars().filter(|character| !character.is_whitespace()).flat_map(char::to_lowercase).collect();
    if lowered.chars().count() == 1 {
        return lowered;
    }
    if let Some((_, canonical)) = ALIASES.iter().find(|(alias, _)| *alias == lowered) {
        return (*canonical).to_string();
    }
    if let Some((name, _)) = KEYS.iter().find(|(name, _)| *name == lowered) {
        return (*name).to_string();
    }
    return lowered;
}

fn glyph(key: &str) -> String {
    if key == "space" {
        return tr("Space");
    }
    if let Some((_, glyph)) = GLYPHS.iter().find(|(name, _)| *name == key) {
        return (*glyph).to_string();
    }
    return key.to_uppercase();
}

fn rank(modifier: Modifier) -> u8 {
    return match modifier {
        Modifier::Control => 0,
        Modifier::Option => 1,
        Modifier::Shift => 2,
        Modifier::Command => 3,
    };
}

fn sort_modifiers(modifiers: &mut [Modifier]) {
    modifiers.sort_by_key(|modifier| rank(*modifier));
}

fn canonical_modifiers(modifiers: &[Modifier]) -> Vec<Modifier> {
    let mut ordered: Vec<Modifier> = modifiers.to_vec();
    ordered.sort_by_key(|modifier| rank(*modifier));
    ordered.dedup();
    return ordered;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_canonical_form() {
        let combo = parse("ctrl+opt+cmd+right").expect("parse");
        assert_eq!(combo.key, "right");
        assert_eq!(combo.modifiers, vec![Modifier::Control, Modifier::Option, Modifier::Command]);
        assert_eq!(format(&combo), "ctrl+opt+cmd+right");
        assert_eq!(display(&combo), "⌃⌥⌘→");
    }

    #[test]
    fn parses_case_and_alias_variants() {
        assert_eq!(parse("Cmd+P").expect("parse"), parse("command+ p ").expect("parse"));
        assert_eq!(parse("option+cmd+f1").expect("parse").key, "f1");
        assert_eq!(parse("shift+cmd+3").expect("parse").modifiers, vec![Modifier::Shift, Modifier::Command]);
        assert_eq!(parse("enter").expect("parse").key, "return");
        assert_eq!(parse("esc").expect("parse").key, "escape");
    }

    #[test]
    fn rejects_unknown_and_empty() {
        assert!(matches!(parse("ctrl+banana"), Err(ParseError::UnknownKey(_))));
        assert!(matches!(parse("ctrl"), Err(ParseError::Empty)));
    }

    #[test]
    fn modifier_order_is_canonical() {
        let combo = parse("cmd+shift+ctrl+opt+a").expect("parse");
        assert_eq!(combo.modifiers, vec![Modifier::Control, Modifier::Option, Modifier::Shift, Modifier::Command]);
        assert_eq!(display(&combo), "⌃⌥⇧⌘A");
        assert_eq!(format(&combo), "ctrl+opt+shift+cmd+a");
    }

    #[test]
    fn masks_match_the_carbon_header() {
        assert_eq!(modifier_mask(&[Modifier::Command]), 0x0100);
        assert_eq!(modifier_mask(&[Modifier::Shift]), 0x0200);
        assert_eq!(modifier_mask(&[Modifier::Option]), 0x0800);
        assert_eq!(modifier_mask(&[Modifier::Control]), 0x1000);
        assert_eq!(modifier_mask(&[Modifier::Control, Modifier::Option, Modifier::Command]), 0x1000 | 0x0800 | 0x0100);
    }

    #[test]
    fn keycodes_match_the_header() {
        assert_eq!(keycode("a"), Some(0x00));
        assert_eq!(keycode("p"), Some(0x23));
        assert_eq!(keycode("m"), Some(0x2E));
        assert_eq!(keycode("space"), Some(0x31));
        assert_eq!(keycode("escape"), Some(0x35));
        assert_eq!(keycode("f1"), Some(0x7A));
        assert_eq!(keycode("f12"), Some(0x6F));
        assert_eq!(keycode("left"), Some(0x7B));
        assert_eq!(keycode("right"), Some(0x7C));
        assert_eq!(keycode("down"), Some(0x7D));
        assert_eq!(keycode("up"), Some(0x7E));
        assert_eq!(key_name(0x7C), Some("right"));
    }

    #[test]
    fn reserved_combos_name_their_owner() {
        assert_eq!(reserved(&parse("cmd+space").expect("parse")), Some("Spotlight"));
        assert_eq!(reserved(&parse("ctrl+space").expect("parse")), Some("Switch input source"));
        assert_eq!(reserved(&parse("cmd+tab").expect("parse")), Some("App switcher"));
        assert_eq!(reserved(&parse("cmd+q").expect("parse")), Some("app commands (⌘ + key)"));
        assert_eq!(reserved(&parse("cmd+shift+4").expect("parse")), Some("Screenshot"));
        assert_eq!(reserved(&parse("ctrl+up").expect("parse")), Some("Mission Control"));
        assert_eq!(reserved(&parse("ctrl+down").expect("parse")), Some("App windows"));
        assert_eq!(reserved(&parse("ctrl+left").expect("parse")), Some("Move one space left"));
        assert_eq!(reserved(&parse("ctrl+right").expect("parse")), Some("Move one space right"));
    }

    #[test]
    fn free_combos_are_not_reserved() {
        assert_eq!(reserved(&parse("ctrl+opt+cmd+right").expect("parse")), None);
        assert_eq!(reserved(&parse("ctrl+opt+cmd+p").expect("parse")), None);
        assert_eq!(reserved(&parse("ctrl+opt+cmd+space").expect("parse")), None);
    }

    #[test]
    fn validation_requires_a_non_shift_modifier() {
        let hotkeys = Hotkeys {
            next: None,
            previous: None,
            pause: None,
            mute: None,
            random: None,
            volume_up: None,
            volume_down: None,
            lock: None,
        };
        assert_eq!(
            validate(&parse("shift+a").expect("parse"), HotkeyAction::Next, &hotkeys),
            Err(ValidationError::MissingModifier)
        );
        assert_eq!(
            validate(&parse("a").expect("parse"), HotkeyAction::Next, &hotkeys),
            Err(ValidationError::MissingModifier)
        );
        assert_eq!(validate(&parse("f5").expect("parse"), HotkeyAction::Next, &hotkeys), Ok(()));
        assert_eq!(validate(&parse("shift+f5").expect("parse"), HotkeyAction::Next, &hotkeys), Ok(()));
    }

    #[test]
    fn validation_rejects_reserved_and_duplicates() {
        let hotkeys = Hotkeys::default();
        assert_eq!(
            validate(&parse("cmd+space").expect("parse"), HotkeyAction::Next, &hotkeys),
            Err(ValidationError::Reserved("Spotlight"))
        );
        assert_eq!(
            validate(&parse("ctrl+opt+cmd+space").expect("parse"), HotkeyAction::Next, &hotkeys),
            Err(ValidationError::Duplicate(HotkeyAction::TogglePause))
        );
        assert_eq!(validate(&parse("ctrl+opt+cmd+space").expect("parse"), HotkeyAction::TogglePause, &hotkeys), Ok(()));
    }

    #[test]
    fn action_ids_round_trip() {
        for action in HotkeyAction::ALL {
            assert_eq!(action_from_id(registration_id(action)), Some(action));
        }
        assert_eq!(action_from_id(0), None);
        assert_eq!(action_from_id(99), None);
    }
}
