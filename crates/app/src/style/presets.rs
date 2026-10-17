use std::collections::BTreeMap;

use crate::i18n::key;
use crate::style::color::Rgba;
use crate::style::{Accent, Pair, Style};

pub const SYSTEM: &str = key("System");
pub const TINT_STRENGTH: f64 = 0.5;

const PRESETS: [(&str, u32, u32, u32, u32); 8] = [
    (key("Graphite"), 0x67_676C, 0x98_989D, 0xF2_F2F4, 0x1F_1F22),
    (key("Sequoia"), 0x25_7554, 0x4F_B98A, 0xEE_F3EF, 0x1B_241F),
    (key("Harbour"), 0x2B_6BA4, 0x5F_A8DC, 0xED_F2F7, 0x1B_2129),
    (key("Dusk"), 0x6C_5B9E, 0x9B_8AD1, 0xF1_EFF7, 0x20_1D2B),
    (key("Terracotta"), 0x9F_5237, 0xD6_8A6E, 0xF6_EFEB, 0x25_1C18),
    (key("Sand"), 0x82_6410, 0xD4_B45F, 0xF7_F3E8, 0x24_1F16),
    (key("Linen"), 0x74_6655, 0xB3_A593, 0xF5_F2ED, 0x20_1D19),
    (key("Slate"), 0x4A_5A6A, 0x7C_90A3, 0xEF_F2F5, 0x19_1D22),
];

fn pair(light: u32, dark: u32) -> Pair {
    return Pair { light: Rgba::hex(light), dark: Some(Rgba::hex(dark)) };
}

pub fn system() -> Style {
    return Style {
        name: SYSTEM.to_string(),
        accent: Accent::System,
        tint: None,
        tint_strength: TINT_STRENGTH,
        colors: BTreeMap::new(),
    };
}

pub fn all() -> Vec<Style> {
    let mut styles = vec![system()];
    styles.extend(PRESETS.iter().map(|(name, accent, accent_dark, tint, tint_dark)| Style {
        name: (*name).to_string(),
        accent: Accent::Fixed(pair(*accent, *accent_dark)),
        tint: Some(pair(*tint, *tint_dark)),
        tint_strength: TINT_STRENGTH,
        colors: BTreeMap::new(),
    }));
    return styles;
}

pub fn names() -> impl Iterator<Item = &'static str> {
    return std::iter::once(SYSTEM).chain(PRESETS.iter().map(|(name, ..)| *name));
}

pub fn find(name: &str) -> Option<Style> {
    return all().into_iter().find(|style| style.name.eq_ignore_ascii_case(name));
}
