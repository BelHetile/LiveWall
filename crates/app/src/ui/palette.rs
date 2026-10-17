use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor, NSColorSpace};
use objc2_foundation::NSArray;

use crate::i18n;
use crate::model::{Appearance, AppearanceMode};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    r: f64,
    g: f64,
    b: f64,
}

impl Rgb {
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.trim().trim_start_matches('#');
        if hex.len() != 6 {
            return None;
        }
        return u32::from_str_radix(hex, 16).ok().map(Self::hex);
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn to_hex(self) -> String {
        let byte = |channel: f64| (channel.clamp(0.0, 1.0) * 255.0).round() as u32;
        return format!("#{:02X}{:02X}{:02X}", byte(self.r), byte(self.g), byte(self.b));
    }

    const WHITE: Self = Self::hex(0x00FF_FFFF);
    const INK: Self = Self::hex(0x001A_181E);
    const PAPER: Self = Self::hex(0x00F4_F2F6);
    const NIGHT: Self = Self::hex(0x001C_1A21);

    const fn hex(value: u32) -> Self {
        return Self {
            r: (value >> 16 & 0xFF) as f64 / 255.0,
            g: (value >> 8 & 0xFF) as f64 / 255.0,
            b: (value & 0xFF) as f64 / 255.0,
        };
    }

    fn mix(self, other: Self, amount: f64) -> Self {
        let lerp = |a: f64, b: f64| a + (b - a) * amount;
        return Self { r: lerp(self.r, other.r), g: lerp(self.g, other.g), b: lerp(self.b, other.b) };
    }

    fn luminance(self) -> f64 {
        let linear = |c: f64| if c <= 0.039_28 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        return 0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b);
    }

    fn legible(self) -> Self {
        let contrast = |other: Self| {
            let (a, b) = (self.luminance(), other.luminance());
            return (a.max(b) + 0.05) / (a.min(b) + 0.05);
        };
        return if contrast(Self::WHITE) >= 3.0 { Self::WHITE } else { Self::INK };
    }

    pub fn from_color(color: &NSColor) -> Option<Self> {
        let srgb = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
        return Some(Self { r: srgb.redComponent(), g: srgb.greenComponent(), b: srgb.blueComponent() });
    }

    pub fn color(self, alpha: f64) -> Retained<NSColor> {
        return NSColor::colorWithSRGBRed_green_blue_alpha(self.r, self.g, self.b, alpha);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub accent: Rgb,
    pub canvas: Rgb,
    pub accent_dark: Option<Rgb>,
}

impl Palette {
    const fn new(accent: u32, accent_dark: u32, canvas: u32) -> Self {
        return Self { accent: Rgb::hex(accent), canvas: Rgb::hex(canvas), accent_dark: Some(Rgb::hex(accent_dark)) };
    }

    pub const BUILT_IN: [(&str, Self); 6] = [
        (i18n::key("Violet"), Self::new(0x0086_75AC, 0x00A9_96D6, 0x007D_6F9E)),
        (i18n::key("Rose"), Self::new(0x00B0_7885, 0x00DB_A2B0, 0x009E_7881)),
        (i18n::key("Ocean"), Self::new(0x0068_889F, 0x0087_B0CC, 0x006F_8B9E)),
        (i18n::key("Forest"), Self::new(0x0060_8875, 0x0088_BDA2, 0x0075_9E8A)),
        (i18n::key("Amber"), Self::new(0x009C_8766, 0x00D1_BA97, 0x009E_8B6F)),
        (i18n::key("Graphite"), Self::new(0x0075_7980, 0x00BE_C2CC, 0x0095_989E)),
    ];

    pub fn custom(accent: Rgb, canvas: Rgb) -> Self {
        return Self { accent, canvas, accent_dark: None };
    }
}

pub struct Theme {
    pub accent: Retained<NSColor>,
    pub accent_text: Retained<NSColor>,
    pub accent_fill: Retained<NSColor>,
    pub accent_soft: Retained<NSColor>,
    pub canvas: Retained<NSColor>,
    pub chrome: Retained<NSColor>,
    pub hairline: Retained<NSColor>,
}

impl Theme {
    pub fn new(palette: Palette) -> Self {
        let Palette { accent, canvas, accent_dark } = palette;
        let dark = accent_dark.unwrap_or_else(|| accent.mix(Rgb::WHITE, 0.35));
        return Self {
            accent: dynamic(accent.color(1.0), dark.color(1.0)),
            accent_text: dynamic(accent.legible().color(1.0), dark.legible().color(1.0)),
            accent_fill: dynamic(accent.mix(Rgb::WHITE, 0.2).color(1.0), dark.color(1.0)),
            accent_soft: dynamic(accent.color(0.14), dark.color(0.2)),
            canvas: dynamic(Rgb::PAPER.mix(canvas, 0.1).color(1.0), Rgb::NIGHT.mix(canvas, 0.2).color(1.0)),
            chrome: dynamic(
                Rgb::PAPER.mix(canvas.mix(Rgb::WHITE, 0.3), 0.08).mix(Rgb::WHITE, 0.72).color(1.0),
                Rgb::NIGHT.mix(canvas.mix(Rgb::INK, 0.3), 0.14).mix(Rgb::WHITE, 0.04).color(1.0),
            ),
            hairline: dynamic(Rgb::INK.color(0.08), Rgb::WHITE.color(0.1)),
        };
    }
}

pub struct Look {
    pub mode: usize,
    pub theme: usize,
    pub themes: Vec<(String, Palette)>,
    pub colors: Theme,
}

impl Look {
    pub const MODES: [&str; 3] = [i18n::key("System"), i18n::key("Light"), i18n::key("Dark")];

    pub fn new(appearance: &Appearance) -> Self {
        let mut themes: Vec<(String, Palette)> =
            Palette::BUILT_IN.iter().map(|(name, palette)| ((*name).to_string(), *palette)).collect();
        let accent = Rgb::parse(&appearance.accent).unwrap_or(Rgb::hex(0x0086_6A8C));
        let canvas = Rgb::parse(&appearance.canvas).unwrap_or(Rgb::hex(0x0094_7C8E));
        themes.push((i18n::key("Custom").to_string(), Palette::custom(accent, canvas)));
        let selected = themes.iter().position(|(name, _)| name.eq_ignore_ascii_case(&appearance.theme)).unwrap_or(0);
        let mode = match appearance.mode {
            AppearanceMode::System => 0,
            AppearanceMode::Light => 1,
            AppearanceMode::Dark => 2,
        };
        let colors = Theme::new(themes[selected].1);
        return Self { mode, theme: selected, themes, colors };
    }

    pub fn differs(&self, other: &Self) -> bool {
        return self.mode != other.mode
            || self.theme != other.theme
            || self.themes.iter().map(|(_, palette)| palette).ne(other.themes.iter().map(|(_, palette)| palette));
    }

    pub fn custom_index(&self) -> usize {
        return self.themes.len() - 1;
    }

    pub fn palette(&self) -> Palette {
        return self.themes[self.theme].1;
    }
}

pub fn is_dark(appearance: &NSAppearance) -> bool {
    let names = unsafe { NSArray::from_slice(&[NSAppearanceNameAqua, NSAppearanceNameDarkAqua]) };
    let best = appearance.bestMatchFromAppearancesWithNames(&names);
    return best.is_some_and(|name| unsafe { &*name == NSAppearanceNameDarkAqua });
}

fn dynamic(light: Retained<NSColor>, dark: Retained<NSColor>) -> Retained<NSColor> {
    let block = block2::RcBlock::new(move |appearance: NonNull<NSAppearance>| -> NonNull<NSColor> {
        let chosen = if is_dark(unsafe { appearance.as_ref() }) { &dark } else { &light };
        return NonNull::from(&**chosen);
    });
    return unsafe { NSColor::colorWithName_dynamicProvider(None, &block) };
}
