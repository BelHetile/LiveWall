use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::style::keys::{ACCENT, Key, TINT, TINT_STRENGTH};
use crate::style::{Accent, Pair, Rgba, Style, StyleError, presets};

pub const VERSION: u64 = 1;
const MARKER: &str = "livewall_style";
const NAME: &str = "name";
const COLORS: &str = "colors";
const LIGHT: &str = "light";
const DARK: &str = "dark";
const SYSTEM_ACCENT: &str = "system";

fn pair_to_value(pair: &Pair) -> Value {
    return match pair.dark {
        None => json!(pair.light.to_hex()),
        Some(dark) => json!({ LIGHT: pair.light.to_hex(), DARK: dark.to_hex() }),
    };
}

pub fn to_value(style: &Style) -> Value {
    let mut out = Map::new();
    out.insert(MARKER.into(), json!(VERSION));
    out.insert(NAME.into(), json!(style.name));
    out.insert(
        ACCENT.into(),
        match &style.accent {
            Accent::System => json!(SYSTEM_ACCENT),
            Accent::Fixed(pair) => pair_to_value(pair),
        },
    );
    if let Some(tint) = &style.tint {
        out.insert(TINT.into(), pair_to_value(tint));
    }
    out.insert(TINT_STRENGTH.into(), json!((style.tint_strength * 100.0).round()));
    let colors: Map<String, Value> =
        style.colors.iter().map(|(key, pair)| (key.id().to_string(), pair_to_value(pair))).collect();
    if !colors.is_empty() {
        out.insert(COLORS.into(), Value::Object(colors));
    }
    return Value::Object(out);
}

pub fn render(style: &Style) -> String {
    let mut text = serde_json::to_string_pretty(&to_value(style)).unwrap_or_default();
    text.push('\n');
    return text;
}

fn color(field: &str, value: &Value) -> Result<Rgba, StyleError> {
    let invalid = || StyleError::Color { field: field.to_string(), value: value.to_string() };
    return value.as_str().and_then(Rgba::parse).ok_or_else(invalid);
}

fn pair(field: &str, value: &Value) -> Result<Pair, StyleError> {
    if value.is_string() {
        return Ok(Pair::new(color(field, value)?));
    }
    let Some(object) = value.as_object() else {
        return Err(StyleError::Color { field: field.to_string(), value: value.to_string() });
    };
    if let Some(unknown) = object.keys().find(|name| !matches!(name.as_str(), LIGHT | DARK)) {
        return Err(StyleError::Unknown(format!("{field}.{unknown}")));
    }
    let light = object.get(LIGHT).ok_or_else(|| StyleError::Unknown(format!("{field}.{LIGHT}")))?;
    let dark = object.get(DARK).map(|dark| color(field, dark)).transpose()?;
    return Ok(Pair { light: color(field, light)?, dark });
}

pub fn from_value(value: &Value) -> Result<Style, StyleError> {
    let object = value.as_object().ok_or(StyleError::NotStyle)?;
    let version = object.get(MARKER).and_then(Value::as_u64).ok_or(StyleError::NotStyle)?;
    if version > VERSION {
        return Err(StyleError::Newer(version));
    }
    if version == 0 {
        return Err(StyleError::NotStyle);
    }
    let known = [MARKER, NAME, ACCENT, TINT, TINT_STRENGTH, COLORS];
    if let Some(unknown) = object.keys().find(|name| !known.contains(&name.as_str())) {
        return Err(StyleError::Unknown(unknown.clone()));
    }
    let name = match object.get(NAME) {
        None => String::new(),
        Some(name) => name.as_str().ok_or_else(|| StyleError::Unknown(NAME.into()))?.trim().to_string(),
    };
    let accent = match object.get(ACCENT) {
        None => Accent::System,
        Some(value) if value.as_str().is_some_and(|text| text.eq_ignore_ascii_case(SYSTEM_ACCENT)) => Accent::System,
        Some(value) => Accent::Fixed(pair(ACCENT, value)?),
    };
    let tint = object.get(TINT).filter(|value| !value.is_null()).map(|value| pair(TINT, value)).transpose()?;
    let tint_strength = match object.get(TINT_STRENGTH) {
        None => presets::TINT_STRENGTH,
        Some(value) => {
            let percent = value.as_f64().filter(|percent| (0.0..=100.0).contains(percent));
            percent.ok_or(StyleError::Strength)? / 100.0
        }
    };
    let mut colors = BTreeMap::new();
    if let Some(entries) = object.get(COLORS) {
        let entries = entries.as_object().ok_or_else(|| StyleError::Unknown(COLORS.into()))?;
        for (id, value) in entries {
            let key = Key::from_id(id).ok_or_else(|| StyleError::Unknown(format!("{COLORS}.{id}")))?;
            colors.insert(key, pair(id, value)?);
        }
    }
    return Ok(Style { name, accent, tint, tint_strength, colors });
}

pub fn parse(text: &str) -> Result<Style, StyleError> {
    let value: Value = serde_json::from_str(text).map_err(|error| StyleError::Json(error.to_string()))?;
    return from_value(&value);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edited() -> Style {
        let mut style = presets::find("Terracotta").expect("preset");
        style.name = "Terracotta (edited)".into();
        style.set("grid.ring", "#FF000080", Some("#00FF00")).expect("ring");
        style.set("canvas", "#FAFAFA", None).expect("canvas");
        style.set("tint_strength", "35", None).expect("strength");
        return style;
    }

    #[test]
    fn documents_round_trip() {
        for style in presets::all().into_iter().chain([edited()]) {
            let text = render(&style);
            assert!(text.ends_with('\n'));
            assert_eq!(parse(&text), Ok(style.clone()), "{text}");
        }
        let value = to_value(&edited());
        assert_eq!(value[MARKER], json!(1));
        assert_eq!(value[COLORS]["canvas"], json!("#FAFAFA"));
        assert_eq!(value[COLORS]["grid.ring"], json!({ "light": "#FF000080", "dark": "#00FF00" }));
        assert_eq!(value[TINT_STRENGTH], json!(35.0));
        assert_eq!(to_value(&presets::system())[ACCENT], json!("system"));
    }

    #[test]
    fn partial_documents_fill_in_defaults() {
        let style = parse(r#"{ "livewall_style": 1 }"#).expect("minimal");
        assert_eq!(style.name, "");
        assert_eq!(style.accent, Accent::System);
        assert!(style.tint.is_none() && style.colors.is_empty());
        let style = parse(r##"{ "livewall_style": 1, "accent": "#112233", "colors": { "text": "#222222" } }"##)
            .expect("partial");
        assert_eq!(style.accent, Accent::Fixed(Pair::new(Rgba::hex(0x11_2233))));
        assert_eq!(style.pair(Key::Text), Some(Pair::new(Rgba::hex(0x22_2222))));
    }

    #[test]
    fn newer_and_foreign_documents_are_rejected() {
        assert_eq!(parse(r#"{ "livewall_style": 2, "name": "Future" }"#), Err(StyleError::Newer(2)));
        assert_eq!(parse(r#"{ "livewall_style": 0 }"#), Err(StyleError::NotStyle));
        assert_eq!(parse(r#"{ "name": "x" }"#), Err(StyleError::NotStyle));
        assert_eq!(parse("[1, 2]"), Err(StyleError::NotStyle));
        assert!(matches!(parse("{"), Err(StyleError::Json(_))));
    }

    #[test]
    fn unknown_keys_in_a_known_version_are_rejected() {
        let cases = [
            (r#"{ "livewall_style": 1, "font": "x" }"#, "font"),
            (r##"{ "livewall_style": 1, "colors": { "nope": "#FFFFFF" } }"##, "colors.nope"),
            (r##"{ "livewall_style": 1, "colors": { "text": { "light": "#FFFFFF", "hc": "#000000" } } }"##, "text.hc"),
            (r##"{ "livewall_style": 1, "tint": { "dark": "#000000" } }"##, "tint.light"),
        ];
        for (text, field) in cases {
            assert_eq!(parse(text), Err(StyleError::Unknown(field.into())), "{text}");
        }
        assert!(matches!(
            parse(r#"{ "livewall_style": 1, "colors": { "text": "pink" } }"#),
            Err(StyleError::Color { .. })
        ));
        assert_eq!(parse(r#"{ "livewall_style": 1, "tint_strength": 150 }"#), Err(StyleError::Strength));
    }
}
