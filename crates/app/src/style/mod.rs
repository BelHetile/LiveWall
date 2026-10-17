mod color;
mod document;
mod keys;
mod presets;
mod tags;

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use color::{Rgba, Role};
pub use document::{VERSION, parse, render};
pub use keys::{ACCENT, Fallback, GENERAL, Group, Key, SystemColor, TINT, TINT_STRENGTH};
pub use presets::SYSTEM;
pub use tags::Tag;

use crate::i18n::key;
use crate::model::Appearance;

const MAX_NAME: usize = 64;
const LEGACY: [(&str, &str); 6] = [
    ("violet", "Dusk"),
    ("rose", "Terracotta"),
    ("ocean", "Harbour"),
    ("forest", "Sequoia"),
    ("amber", "Sand"),
    ("graphite", "Graphite"),
];
pub const CUSTOM: &str = key("Custom");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Light,
    Dark,
    LightHc,
    DarkHc,
}

impl Variant {
    pub const ALL: [Self; 4] = [Self::Light, Self::Dark, Self::LightHc, Self::DarkHc];

    pub fn is_dark(self) -> bool {
        return matches!(self, Self::Dark | Self::DarkHc);
    }

    pub fn is_high_contrast(self) -> bool {
        return matches!(self, Self::LightHc | Self::DarkHc);
    }

    pub fn index(self) -> usize {
        return self as usize;
    }
}

pub trait Palette {
    fn system(&self, color: SystemColor, variant: Variant) -> Rgba;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    pub light: Rgba,
    pub dark: Option<Rgba>,
}

impl Pair {
    pub fn new(light: Rgba) -> Self {
        return Self { light, dark: None };
    }

    pub fn dark(&self) -> Rgba {
        return self.dark.unwrap_or_else(|| self.light.derive_dark());
    }

    pub fn pick(&self, dark: bool) -> Rgba {
        return if dark { self.dark() } else { self.light };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accent {
    System,
    Fixed(Pair),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub name: String,
    pub accent: Accent,
    pub tint: Option<Pair>,
    pub tint_strength: f64,
    pub colors: BTreeMap<Key, Pair>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleError {
    Json(String),
    NotStyle,
    Newer(u64),
    Unknown(String),
    Color { field: String, value: String },
    Strength,
    Name,
    Taken(String),
    Missing(String),
    BuiltIn(String),
    Key(String),
}

impl StyleError {
    pub fn message(&self) -> (&'static str, Vec<String>) {
        return match self {
            Self::Json(error) => (key("The style file isn’t valid JSON: %@"), vec![error.clone()]),
            Self::NotStyle => (key("This isn’t a LiveWall style file."), Vec::new()),
            Self::Newer(version) => (
                key("This style was made by a newer version of LiveWall (format %@). Update LiveWall to import it."),
                vec![version.to_string()],
            ),
            Self::Unknown(field) => (key("Unknown style setting “%@”."), vec![field.clone()]),
            Self::Color { field, value } => {
                (key("“%@” isn’t a color for %@. Use #RRGGBB or #RRGGBBAA."), vec![value.clone(), field.clone()])
            }
            Self::Strength => (key("Tint strength must be a number from 0 to 100."), Vec::new()),
            Self::Name => (key("A style name can’t be empty or longer than 64 characters."), Vec::new()),
            Self::Taken(name) => (key("A style named “%@” already exists."), vec![name.clone()]),
            Self::Missing(name) => (key("There’s no style named “%@”."), vec![name.clone()]),
            Self::BuiltIn(name) => (key("“%@” is a built-in style and can’t be changed."), vec![name.clone()]),
            Self::Key(field) => (key("Unknown color “%@”. Run lwctl style keys to list them."), vec![field.clone()]),
        };
    }
}

impl fmt::Display for StyleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (template, args) = self.message();
        let mut out = String::new();
        let mut args = args.into_iter();
        let mut rest = template;
        while let Some(at) = rest.find("%@") {
            out.push_str(&rest[..at]);
            out.push_str(&args.next().unwrap_or_default());
            rest = &rest[at + 2..];
        }
        out.push_str(rest);
        return formatter.write_str(&out);
    }
}

impl std::error::Error for StyleError {}

fn color_value(field: &str, text: &str) -> Result<Rgba, StyleError> {
    return Rgba::parse(text).ok_or_else(|| StyleError::Color { field: field.to_string(), value: text.to_string() });
}

fn pair_value(field: &str, light: &str, dark: Option<&str>) -> Result<Pair, StyleError> {
    let dark = dark.map(|text| color_value(field, text)).transpose()?;
    return Ok(Pair { light: color_value(field, light)?, dark });
}

impl Style {
    pub fn accent(&self, variant: Variant, palette: &impl Palette) -> Rgba {
        return match &self.accent {
            Accent::System => palette.system(SystemColor::ControlAccent, variant),
            Accent::Fixed(pair) => self.fixed(pair, Role::Content, variant, palette),
        };
    }

    pub fn tint_color(&self, variant: Variant) -> Option<Rgba> {
        return self.tint.map(|tint| tint.pick(variant.is_dark()));
    }

    fn fixed(&self, pair: &Pair, role: Role, variant: Variant, palette: &impl Palette) -> Rgba {
        let color = pair.pick(variant.is_dark());
        if !variant.is_high_contrast() {
            return color;
        }
        let background = match role {
            Role::Content | Role::Line => self.resolve(Key::Canvas, variant, palette),
            Role::Surface | Role::Shadow => color,
        };
        return color.derive_contrast(background, role);
    }

    pub fn resolve(&self, key: Key, variant: Variant, palette: &impl Palette) -> Rgba {
        if let Some(pair) = self.colors.get(&key) {
            return self.fixed(pair, key.role(), variant, palette);
        }
        return match key.fallback() {
            Fallback::System(color) => palette.system(color, variant),
            Fallback::Neutral(color) => {
                let base = palette.system(color, variant);
                match self.tint_color(variant) {
                    Some(tint) if self.tint_strength > 0.0 => {
                        base.mix(tint.with_alpha(base.alpha()), self.tint_strength)
                    }
                    _ => base,
                }
            }
            Fallback::Accent => self.accent(variant, palette),
            Fallback::AccentAlpha(alpha) => {
                let alpha = if variant.is_high_contrast() { (alpha * 1.5).min(1.0) } else { alpha };
                self.accent(variant, palette).scale_alpha(alpha)
            }
            Fallback::OnAccent => match self.accent {
                Accent::System => palette.system(SystemColor::TextOnAccent, variant),
                Accent::Fixed(_) => {
                    self.accent(variant, palette).over(self.resolve(Key::Canvas, variant, palette).opaque()).on()
                }
            },
            Fallback::Key(next) => self.resolve(next, variant, palette),
            Fallback::Fixed(light, dark) => self.fixed(&Pair { light, dark: Some(dark) }, key.role(), variant, palette),
        };
    }

    pub fn native(&self, key: Key) -> Option<SystemColor> {
        if self.colors.contains_key(&key) {
            return None;
        }
        let system = self.accent == Accent::System;
        return match key.fallback() {
            Fallback::System(color) => Some(color),
            Fallback::Neutral(color) => (self.tint.is_none() || self.tint_strength <= 0.0).then_some(color),
            Fallback::Accent => system.then_some(SystemColor::ControlAccent),
            Fallback::OnAccent => system.then_some(SystemColor::TextOnAccent),
            Fallback::Key(next) => self.native(next),
            Fallback::AccentAlpha(_) | Fallback::Fixed(..) => None,
        };
    }

    pub fn pair(&self, key: Key) -> Option<Pair> {
        return self.colors.get(&key).copied();
    }

    pub fn set(&mut self, field: &str, light: &str, dark: Option<&str>) -> Result<(), StyleError> {
        match field {
            ACCENT if light.trim().eq_ignore_ascii_case("system") => self.accent = Accent::System,
            ACCENT => self.accent = Accent::Fixed(pair_value(field, light, dark)?),
            TINT if light.trim().eq_ignore_ascii_case("none") => self.tint = None,
            TINT => self.tint = Some(pair_value(field, light, dark)?),
            TINT_STRENGTH => {
                let percent = light.trim().trim_end_matches('%').parse::<f64>().map_err(|_| StyleError::Strength)?;
                if !(0.0..=100.0).contains(&percent) {
                    return Err(StyleError::Strength);
                }
                self.tint_strength = percent / 100.0;
            }
            id => {
                let key = Key::from_id(id).ok_or_else(|| StyleError::Key(id.to_string()))?;
                self.colors.insert(key, pair_value(id, light, dark)?);
            }
        }
        return Ok(());
    }

    pub fn reset(&mut self, field: &str) -> Result<(), StyleError> {
        match field {
            ACCENT => self.accent = Accent::System,
            TINT => self.tint = None,
            TINT_STRENGTH => self.tint_strength = presets::TINT_STRENGTH,
            id => {
                let key = Key::from_id(id).ok_or_else(|| StyleError::Key(id.to_string()))?;
                self.colors.remove(&key);
            }
        }
        return Ok(());
    }
}

pub fn built_in(name: &str) -> bool {
    return presets::names().any(|preset| preset.eq_ignore_ascii_case(name));
}

pub fn valid_name(name: &str) -> Option<String> {
    let name = name.trim();
    let count = name.chars().count();
    let valid = (1..=MAX_NAME).contains(&count) && !name.chars().any(char::is_control);
    return valid.then(|| name.to_string());
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "Value", into = "Value")]
pub struct UserStyles {
    pub styles: Vec<Style>,
}

impl From<Value> for UserStyles {
    fn from(value: Value) -> Self {
        let styles = value
            .get("styles")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|entry| document::from_value(entry).ok())
            .filter(|style| valid_name(&style.name).is_some())
            .fold(Vec::<Style>::new(), |mut kept, style| {
                let taken =
                    built_in(&style.name) || kept.iter().any(|other| other.name.eq_ignore_ascii_case(&style.name));
                if !taken {
                    kept.push(style);
                }
                return kept;
            });
        return Self { styles };
    }
}

impl From<UserStyles> for Value {
    fn from(styles: UserStyles) -> Self {
        let list: Vec<Value> = styles.styles.iter().map(document::to_value).collect();
        return json!({ "livewall_styles": VERSION, "styles": list });
    }
}

impl UserStyles {
    pub fn is_taken(&self, name: &str) -> bool {
        return built_in(name) || self.styles.iter().any(|style| style.name.eq_ignore_ascii_case(name.trim()));
    }

    pub fn unique_name(&self, wanted: &str) -> String {
        let wanted = valid_name(wanted).unwrap_or_else(|| CUSTOM.to_string());
        if !self.is_taken(&wanted) {
            return wanted;
        }
        let base: String = wanted.chars().take(MAX_NAME - 4).collect();
        let limit = self.styles.len() + 2;
        return (2..=limit)
            .map(|number| format!("{base} {number}"))
            .find(|candidate| !self.is_taken(candidate))
            .unwrap_or(base);
    }

    pub fn find(&self, name: &str) -> Option<&Style> {
        return self.styles.iter().find(|style| style.name.eq_ignore_ascii_case(name.trim()));
    }

    pub fn find_mut(&mut self, name: &str) -> Option<&mut Style> {
        return self.styles.iter_mut().find(|style| style.name.eq_ignore_ascii_case(name.trim()));
    }

    pub fn style(&self, name: &str) -> Option<Style> {
        return presets::find(name).or_else(|| self.find(name).cloned());
    }

    pub fn active(&self, name: &str) -> Style {
        return self.style(name).unwrap_or_else(presets::system);
    }

    pub fn all(&self) -> Vec<Style> {
        let mut all = presets::all();
        all.extend(self.styles.iter().cloned());
        return all;
    }

    pub fn add(&mut self, mut style: Style) -> String {
        style.name = self.unique_name(&style.name);
        let name = style.name.clone();
        self.styles.push(style);
        return name;
    }

    pub fn remove(&mut self, name: &str) -> Result<Style, StyleError> {
        if built_in(name) {
            return Err(StyleError::BuiltIn(name.to_string()));
        }
        let index = self
            .styles
            .iter()
            .position(|style| style.name.eq_ignore_ascii_case(name.trim()))
            .ok_or_else(|| StyleError::Missing(name.to_string()))?;
        return Ok(self.styles.remove(index));
    }

    pub fn rename(&mut self, name: &str, to: &str) -> Result<String, StyleError> {
        if built_in(name) {
            return Err(StyleError::BuiltIn(name.to_string()));
        }
        let to = valid_name(to).ok_or(StyleError::Name)?;
        let same = name.trim().eq_ignore_ascii_case(&to);
        if !same && self.is_taken(&to) {
            return Err(StyleError::Taken(to));
        }
        let style = self.find_mut(name).ok_or_else(|| StyleError::Missing(name.to_string()))?;
        style.name.clone_from(&to);
        return Ok(to);
    }

    pub fn import(&mut self, text: &str) -> Result<String, StyleError> {
        let style = parse(text)?;
        return Ok(self.add(style));
    }
}

pub fn migrate(appearance: &mut Appearance, styles: &mut UserStyles) -> bool {
    if !appearance.style.is_empty() {
        return false;
    }
    let theme = appearance.theme.take();
    let accent = appearance.accent.take().as_deref().and_then(Rgba::parse);
    let canvas = appearance.canvas.take().as_deref().and_then(Rgba::parse);
    appearance.style = match theme.as_deref().map(str::trim) {
        Some(theme) if theme.eq_ignore_ascii_case(CUSTOM) => styles.add(Style {
            name: CUSTOM.to_string(),
            accent: accent.map_or(Accent::System, |accent| Accent::Fixed(Pair::new(accent))),
            tint: canvas.map(Pair::new),
            tint_strength: presets::TINT_STRENGTH,
            colors: BTreeMap::new(),
        }),
        Some(theme) => LEGACY
            .iter()
            .find(|(old, _)| old.eq_ignore_ascii_case(theme))
            .map_or(SYSTEM, |(_, preset)| preset)
            .to_string(),
        None => SYSTEM.to_string(),
    };
    return true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AppearanceMode;
    use crate::style::color::contrast;

    pub struct Reference;

    impl Palette for Reference {
        fn system(&self, color: SystemColor, variant: Variant) -> Rgba {
            let dark = variant.is_dark();
            let pick = |light: u32, dark_value: u32| Rgba::hex(if dark { dark_value } else { light });
            let gray = |alpha: f64| if dark { Rgba::WHITE.with_alpha(alpha) } else { Rgba::BLACK.with_alpha(alpha) };
            return match color {
                SystemColor::Label => gray(0.85),
                SystemColor::SecondaryLabel => gray(0.5),
                SystemColor::TertiaryLabel => gray(0.26),
                SystemColor::QuaternaryLabel | SystemColor::Separator => gray(0.1),
                SystemColor::QuaternaryFill => gray(0.03),
                SystemColor::WindowBackground => pick(0xEC_ECEC, 0x32_3232),
                SystemColor::UnderPageBackground => pick(0xE6_E6E6, 0x28_2828),
                SystemColor::ControlBackground => pick(0xFF_FFFF, 0x1E_1E1E),
                SystemColor::ControlAccent | SystemColor::Blue => pick(0x00_7AFF, 0x0A_84FF),
                SystemColor::TextOnAccent => Rgba::WHITE,
                SystemColor::Red => pick(0xFF_3B30, 0xFF_453A),
                SystemColor::Orange => pick(0xFF_9500, 0xFF_9F0A),
                SystemColor::Yellow => pick(0xFF_CC00, 0xFF_D60A),
                SystemColor::Green => pick(0x28_CD41, 0x32_D74B),
                SystemColor::Purple => pick(0xAF_52DE, 0xBF_5AF2),
                SystemColor::Pink => pick(0xFF_2D55, 0xFF_375F),
                SystemColor::Gray => pick(0x8E_8E93, 0x98_989D),
            };
        }
    }

    fn sample(name: &str) -> Style {
        return Style { name: name.to_string(), ..presets::system() };
    }

    #[test]
    fn system_style_defers_to_appkit_everywhere_it_can() {
        let style = presets::system();
        for key in Key::ALL {
            let native = style.native(key);
            let expected = !matches!(
                key,
                Key::SelectionSoft
                    | Key::Shadow
                    | Key::GlassTint
                    | Key::GridHighlight
                    | Key::GridShadow
                    | Key::ChipFill
                    | Key::ChipText
                    | Key::PickerSelected
            );
            assert_eq!(native.is_some(), expected, "{key:?}");
        }
        assert_eq!(style.native(Key::GridRing), Some(SystemColor::ControlAccent));
        assert_eq!(style.native(Key::OnAccent), Some(SystemColor::TextOnAccent));
        assert_eq!(style.resolve(Key::GridRing, Variant::Dark, &Reference), Rgba::hex(0x0A_84FF));
    }

    #[test]
    fn parts_fall_back_to_shared_roles() {
        let mut style = presets::find("Harbour").expect("preset");
        let accent = Rgba::hex(0x2B_6BA4);
        assert_eq!(style.resolve(Key::GridRing, Variant::Light, &Reference), accent);
        assert_eq!(style.resolve(Key::InspectorSlider, Variant::Light, &Reference), accent);
        assert_eq!(style.resolve(Key::GridHighlight, Variant::Light, &Reference), accent.with_alpha(0.16));
        style.set("selection", "#FF0000", None).expect("set");
        assert_eq!(style.resolve(Key::GridRing, Variant::Light, &Reference), Rgba::hex(0xFF_0000));
        assert_eq!(style.resolve(Key::GridRing, Variant::Dark, &Reference), Rgba::hex(0xFF_0000).derive_dark());
        style.set("grid.ring", "#00FF00", Some("#0000FF")).expect("set");
        assert_eq!(style.resolve(Key::GridRing, Variant::Dark, &Reference), Rgba::hex(0x00_00FF));
        assert_eq!(style.resolve(Key::InspectorSlider, Variant::Light, &Reference), Rgba::hex(0xFF_0000));
        assert!(style.native(Key::GridRing).is_none());
        style.reset("grid.ring").expect("reset");
        assert_eq!(style.resolve(Key::GridRing, Variant::Light, &Reference), Rgba::hex(0xFF_0000));
    }

    #[test]
    fn tint_strength_mixes_system_neutrals_toward_the_tint() {
        let mut style = presets::find("Sequoia").expect("preset");
        let plain = Reference.system(SystemColor::UnderPageBackground, Variant::Light);
        let tinted = style.resolve(Key::Canvas, Variant::Light, &Reference);
        assert_ne!(tinted, plain);
        assert!(style.native(Key::Canvas).is_none());
        style.set("tint_strength", "0", None).expect("strength");
        assert_eq!(style.resolve(Key::Canvas, Variant::Light, &Reference), plain);
        assert_eq!(style.native(Key::Canvas), Some(SystemColor::UnderPageBackground));
        assert_eq!(style.set("tint_strength", "101", None), Err(StyleError::Strength));
        assert_eq!(style.set("tint_strength", "lots", None), Err(StyleError::Strength));
        style.set("tint_strength", "100%", None).expect("percent sign");
        assert!((style.tint_strength - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn presets_keep_text_legible_in_light_and_dark() {
        for style in presets::all().into_iter().filter(|style| style.accent != Accent::System) {
            for variant in [Variant::Light, Variant::Dark] {
                let canvas = style.resolve(Key::Canvas, variant, &Reference);
                let accent = style.accent(variant, &Reference);
                let on = style.resolve(Key::OnAccent, variant, &Reference);
                assert!(contrast(accent, canvas) >= 4.5, "{} {variant:?} accent on canvas", style.name);
                assert!(contrast(on, accent) >= 4.5, "{} {variant:?} text on accent", style.name);
                let text = style.resolve(Key::Text, variant, &Reference);
                assert!(contrast(text, canvas) >= 4.5, "{} {variant:?} text on canvas", style.name);
            }
        }
        assert_eq!(presets::all().len(), 9);
        assert_eq!(presets::names().count(), 9);
    }

    #[test]
    fn high_contrast_variants_are_derived_from_fixed_colors() {
        let mut style = presets::find("Dusk").expect("preset");
        style.set("text.secondary", "#808080", None).expect("set");
        let canvas = style.resolve(Key::Canvas, Variant::LightHc, &Reference);
        let normal = style.resolve(Key::TextSecondary, Variant::Light, &Reference);
        let boosted = style.resolve(Key::TextSecondary, Variant::LightHc, &Reference);
        assert!(contrast(boosted, canvas) > contrast(normal, canvas));
        let soft = style.resolve(Key::SelectionSoft, Variant::LightHc, &Reference);
        assert!(soft.alpha() > style.resolve(Key::SelectionSoft, Variant::Light, &Reference).alpha());
        let ring = style.resolve(Key::GridRing, Variant::DarkHc, &Reference);
        let dark_canvas = style.resolve(Key::Canvas, Variant::DarkHc, &Reference);
        assert!(contrast(ring, dark_canvas) >= 7.0);
    }

    #[test]
    fn editing_fields_validates_input() {
        let mut style = sample("Mine");
        assert_eq!(style.set("nope", "#FFFFFF", None), Err(StyleError::Key("nope".into())));
        assert!(matches!(style.set("canvas", "blue", None), Err(StyleError::Color { .. })));
        assert!(matches!(style.set("canvas", "#FFFFFF", Some("x")), Err(StyleError::Color { .. })));
        style.set("accent", "#123456", None).expect("accent");
        assert_eq!(style.accent, Accent::Fixed(Pair::new(Rgba::hex(0x12_3456))));
        style.set("accent", "System", None).expect("system accent");
        assert_eq!(style.accent, Accent::System);
        style.set("tint", "#EEEEEE", Some("#111111")).expect("tint");
        assert_eq!(style.tint.map(|tint| tint.dark()), Some(Rgba::hex(0x11_1111)));
        style.reset("tint").expect("reset tint");
        assert!(style.tint.is_none());
        style.set("tint", "#EEEEEE", None).expect("tint");
        style.set("tint", "none", None).expect("clear tint");
        assert!(style.tint.is_none());
        assert_eq!(style.reset("nope"), Err(StyleError::Key("nope".into())));
    }

    #[test]
    fn names_are_unique_and_built_ins_are_protected() {
        let mut styles = UserStyles::default();
        assert!(styles.is_taken("dusk"));
        assert_eq!(styles.unique_name("Dusk"), "Dusk 2");
        assert_eq!(styles.add(sample("Mine")), "Mine");
        assert_eq!(styles.add(sample("mine")), "mine 2");
        assert_eq!(styles.add(sample("  ")), CUSTOM);
        assert_eq!(styles.rename("Mine", "Ours"), Ok("Ours".into()));
        assert_eq!(styles.rename("Ours", "OURS"), Ok("OURS".into()));
        assert_eq!(styles.rename("OURS", "mine 2"), Err(StyleError::Taken("mine 2".into())));
        assert_eq!(styles.rename("OURS", "Dusk"), Err(StyleError::Taken("Dusk".into())));
        assert_eq!(styles.rename("OURS", ""), Err(StyleError::Name));
        assert_eq!(styles.rename("Dusk", "X"), Err(StyleError::BuiltIn("Dusk".into())));
        assert_eq!(styles.rename("ghost", "X"), Err(StyleError::Missing("ghost".into())));
        assert_eq!(styles.remove("System"), Err(StyleError::BuiltIn("System".into())));
        assert!(styles.remove("ours").is_ok());
        assert_eq!(styles.remove("ours"), Err(StyleError::Missing("ours".into())));
        assert_eq!(styles.active("ghost").name, SYSTEM);
        assert_eq!(styles.active("slate").name, "Slate");
        assert_eq!(styles.all().len(), 11);
        let long = "x".repeat(MAX_NAME + 1);
        assert!(valid_name(&long).is_none());
        assert!(valid_name("a\nb").is_none());
        assert_eq!(styles.unique_name(&"y".repeat(MAX_NAME)), "y".repeat(MAX_NAME));
    }

    #[test]
    fn user_styles_survive_a_save_and_skip_broken_entries() {
        let mut styles = UserStyles::default();
        let mut style = presets::find("Slate").expect("preset");
        style.name = "Mine".into();
        style.set("grid.ring", "#FF000080", Some("#00FF00")).expect("set");
        styles.add(style);
        let text = serde_json::to_string(&styles).expect("serialize");
        let back: UserStyles = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, styles);
        let mut value: Value = serde_json::from_str(&text).expect("value");
        let list = value["styles"].as_array_mut().expect("list");
        list.push(json!({ "livewall_style": 1, "name": "Broken", "colors": { "nope": "#FFFFFF" } }));
        list.push(json!({ "livewall_style": 1, "name": "Dusk" }));
        list.push(json!({ "livewall_style": 1, "name": "mine" }));
        let back: UserStyles = serde_json::from_value(value).expect("lenient");
        assert_eq!(back, styles);
        let empty: UserStyles = serde_json::from_str("{}").expect("empty");
        assert!(empty.styles.is_empty());
    }

    fn legacy(theme: Option<&str>) -> Appearance {
        return Appearance {
            mode: AppearanceMode::Dark,
            style: String::new(),
            list: true,
            theme: theme.map(str::to_string),
            accent: Some("#866A8C".into()),
            canvas: Some("#947C8E".into()),
        };
    }

    #[test]
    fn old_themes_migrate_to_the_nearest_preset() {
        for (old, new) in [("Violet", "Dusk"), ("Ocean", "Harbour"), ("graphite", "Graphite"), ("Weird", SYSTEM)] {
            let mut appearance = legacy(Some(old));
            let mut styles = UserStyles::default();
            assert!(migrate(&mut appearance, &mut styles));
            assert_eq!(appearance.style, new);
            assert!(styles.styles.is_empty());
            assert!(appearance.theme.is_none() && appearance.accent.is_none() && appearance.canvas.is_none());
            assert!(!migrate(&mut appearance, &mut styles));
        }
        let mut fresh = Appearance::default();
        assert!(migrate(&mut fresh, &mut UserStyles::default()));
        assert_eq!(fresh.style, SYSTEM);
    }

    #[test]
    fn custom_theme_becomes_a_user_style() {
        let mut appearance = legacy(Some("Custom"));
        let mut styles = UserStyles::default();
        styles.add(sample(CUSTOM));
        assert!(migrate(&mut appearance, &mut styles));
        assert_eq!(appearance.style, "Custom 2");
        let style = styles.find("Custom 2").expect("migrated");
        assert_eq!(style.accent, Accent::Fixed(Pair::new(Rgba::hex(0x86_6A8C))));
        assert_eq!(style.tint, Some(Pair::new(Rgba::hex(0x94_7C8E))));
        let text = serde_json::to_string(&appearance).expect("serialize");
        assert!(!text.contains("theme") && !text.contains("canvas") && !text.contains("accent"), "{text}");
    }

    #[test]
    fn errors_read_as_english_sentences() {
        assert_eq!(StyleError::Taken("A".into()).to_string(), "A style named “A” already exists.");
        assert_eq!(
            StyleError::Color { field: "canvas".into(), value: "x".into() }.to_string(),
            "“x” isn’t a color for canvas. Use #RRGGBB or #RRGGBBAA."
        );
        assert!(StyleError::Newer(2).to_string().contains("(format 2)"));
    }
}
