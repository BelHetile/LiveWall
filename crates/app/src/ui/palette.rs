use std::path::Path;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor};
use objc2_foundation::NSArray;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(try_from = "String")]
pub struct Rgb {
    r: f64,
    g: f64,
    b: f64,
}

impl TryFrom<String> for Rgb {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        let hex = text.trim().trim_start_matches('#');
        if hex.len() != 6 {
            return Err(format!("expected #RRGGBB, got '{text}'"));
        }
        let channel = |range: std::ops::Range<usize>| {
            return u8::from_str_radix(&hex[range], 16)
                .map(|value| f64::from(value) / 255.0)
                .map_err(|_| format!("invalid hex color '{text}'"));
        };
        return Ok(Self { r: channel(0..2)?, g: channel(2..4)?, b: channel(4..6)? });
    }
}

impl Rgb {
    const WHITE: Self = Self { r: 1.0, g: 1.0, b: 1.0 };
    const INK: Self = Self { r: 26.0 / 255.0, g: 24.0 / 255.0, b: 30.0 / 255.0 };

    fn mix(self, other: Self, amount: f64) -> Self {
        let lerp = |a: f64, b: f64| a + (b - a) * amount;
        return Self { r: lerp(self.r, other.r), g: lerp(self.g, other.g), b: lerp(self.b, other.b) };
    }

    fn luminance(self) -> f64 {
        let linear = |c: f64| if c <= 0.039_28 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        return 0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b);
    }

    fn contrast(self, other: Self) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        return (a.max(b) + 0.05) / (a.min(b) + 0.05);
    }

    fn legible(self) -> Self {
        return if self.contrast(Self::WHITE) >= self.contrast(Self::INK) { Self::WHITE } else { Self::INK };
    }

    fn color(self, alpha: f64) -> Retained<NSColor> {
        return NSColor::colorWithSRGBRed_green_blue_alpha(self.r, self.g, self.b, alpha);
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Palette {
    pub accent: Rgb,
    pub canvas: Rgb,
}

impl Default for Palette {
    fn default() -> Self {
        return Self {
            accent: Rgb { r: 134.0 / 255.0, g: 104.0 / 255.0, b: 143.0 / 255.0 },
            canvas: Rgb { r: 156.0 / 255.0, g: 122.0 / 255.0, b: 146.0 / 255.0 },
        };
    }
}

impl Palette {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
        return serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()));
    }
}

pub struct Theme {
    pub accent: Retained<NSColor>,
    pub accent_text: Retained<NSColor>,
    pub fill: Retained<NSColor>,
    pub canvas: Retained<NSColor>,
    pub chrome: Retained<NSColor>,
}

impl Theme {
    pub fn new(palette: Palette) -> Self {
        let light = palette.accent;
        let dark = palette.accent.mix(Rgb::WHITE, 0.35);
        return Self {
            accent: dynamic(light.color(1.0), dark.color(1.0)),
            accent_text: light.legible().color(1.0),
            fill: light.color(1.0),
            canvas: dynamic(palette.canvas.color(0.14), palette.canvas.color(0.22)),
            chrome: dynamic(
                palette.canvas.mix(Rgb::WHITE, 0.3).color(0.22),
                palette.canvas.mix(Rgb::INK, 0.3).color(0.26),
            ),
        };
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
