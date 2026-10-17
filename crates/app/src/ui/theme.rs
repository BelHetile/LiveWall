use std::cell::RefCell;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAccessibilityHighContrastAqua, NSAppearanceNameAccessibilityHighContrastDarkAqua,
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor, NSColorSpace,
};
use objc2_foundation::NSArray;

use crate::i18n;
use crate::model::{Appearance, AppearanceMode};
use crate::style::{Key, Pair, Palette, Rgba, Role, Style, SystemColor, Tag, UserStyles, Variant};

pub fn ns_color(color: Rgba) -> Retained<NSColor> {
    let [r, g, b, a] = color.components();
    return NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, a);
}

pub fn rgba(color: &NSColor) -> Option<Rgba> {
    let srgb = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    return Some(Rgba::unit(srgb.redComponent(), srgb.greenComponent(), srgb.blueComponent(), srgb.alphaComponent()));
}

pub fn system(color: SystemColor) -> Retained<NSColor> {
    return match color {
        SystemColor::Label => NSColor::labelColor(),
        SystemColor::SecondaryLabel => NSColor::secondaryLabelColor(),
        SystemColor::TertiaryLabel => NSColor::tertiaryLabelColor(),
        SystemColor::QuaternaryLabel => NSColor::quaternaryLabelColor(),
        SystemColor::Separator => NSColor::separatorColor(),
        SystemColor::WindowBackground => NSColor::windowBackgroundColor(),
        SystemColor::UnderPageBackground => NSColor::underPageBackgroundColor(),
        SystemColor::ControlBackground => NSColor::controlBackgroundColor(),
        SystemColor::QuaternaryFill => NSColor::quaternarySystemFillColor(),
        SystemColor::ControlAccent => NSColor::controlAccentColor(),
        SystemColor::TextOnAccent => NSColor::alternateSelectedControlTextColor(),
        SystemColor::Red => NSColor::systemRedColor(),
        SystemColor::Orange => NSColor::systemOrangeColor(),
        SystemColor::Yellow => NSColor::systemYellowColor(),
        SystemColor::Green => NSColor::systemGreenColor(),
        SystemColor::Blue => NSColor::systemBlueColor(),
        SystemColor::Purple => NSColor::systemPurpleColor(),
        SystemColor::Pink => NSColor::systemPinkColor(),
        SystemColor::Gray => NSColor::systemGrayColor(),
    };
}

pub fn folder_color(value: &str) -> Option<Retained<NSColor>> {
    if let Some(tag) = Tag::parse(value) {
        return Some(system(tag.system()));
    }
    return Rgba::parse(value).map(ns_color);
}

fn names() -> Retained<NSArray<objc2_foundation::NSString>> {
    return unsafe {
        NSArray::from_slice(&[
            NSAppearanceNameAqua,
            NSAppearanceNameDarkAqua,
            NSAppearanceNameAccessibilityHighContrastAqua,
            NSAppearanceNameAccessibilityHighContrastDarkAqua,
        ])
    };
}

pub fn variant(appearance: &NSAppearance) -> Variant {
    let Some(best) = appearance.bestMatchFromAppearancesWithNames(&names()) else {
        return Variant::Light;
    };
    let best: &objc2_foundation::NSString = &best;
    return unsafe {
        if best == NSAppearanceNameDarkAqua {
            Variant::Dark
        } else if best == NSAppearanceNameAccessibilityHighContrastAqua {
            Variant::LightHc
        } else if best == NSAppearanceNameAccessibilityHighContrastDarkAqua {
            Variant::DarkHc
        } else {
            Variant::Light
        }
    };
}

fn appearance(variant: Variant) -> Option<Retained<NSAppearance>> {
    let name = unsafe {
        match variant {
            Variant::Light => NSAppearanceNameAqua,
            Variant::Dark => NSAppearanceNameDarkAqua,
            Variant::LightHc => NSAppearanceNameAccessibilityHighContrastAqua,
            Variant::DarkHc => NSAppearanceNameAccessibilityHighContrastDarkAqua,
        }
    };
    return NSAppearance::appearanceNamed(name);
}

fn resolve_under(appearance: &NSAppearance, color: &NSColor) -> Option<Rgba> {
    let out = RefCell::new(None);
    appearance.performAsCurrentDrawingAppearance(&block2::StackBlock::new(|| {
        *out.borrow_mut() = rgba(color);
    }));
    return out.into_inner();
}

fn high_contrast_role(color: SystemColor) -> Role {
    return match color {
        SystemColor::WindowBackground
        | SystemColor::UnderPageBackground
        | SystemColor::ControlBackground
        | SystemColor::QuaternaryFill => Role::Surface,
        SystemColor::Separator | SystemColor::QuaternaryLabel => Role::Line,
        _ => Role::Content,
    };
}

struct AppKit {
    values: Vec<[Rgba; 4]>,
}

impl AppKit {
    fn capture() -> Self {
        let appearances = Variant::ALL.map(appearance);
        let exact: Vec<[Option<Rgba>; 4]> = SystemColor::ALL
            .iter()
            .map(|color| {
                let native = system(*color);
                return Variant::ALL.map(|variant| {
                    appearances[variant.index()].as_ref().and_then(|appearance| resolve_under(appearance, &native))
                });
            })
            .collect();
        let page = SystemColor::ALL.iter().position(|color| *color == SystemColor::UnderPageBackground).unwrap_or(0);
        let base = |row: &[Option<Rgba>; 4], dark: bool| {
            let variant = if dark { Variant::Dark } else { Variant::Light };
            return row[variant.index()].unwrap_or(Rgba::CLEAR);
        };
        let values = exact
            .iter()
            .zip(SystemColor::ALL)
            .map(|(row, color)| {
                return Variant::ALL.map(|variant| {
                    row[variant.index()].unwrap_or_else(|| {
                        let plain = base(row, variant.is_dark());
                        if !variant.is_high_contrast() {
                            return plain;
                        }
                        return plain.derive_contrast(base(&exact[page], variant.is_dark()), high_contrast_role(color));
                    })
                });
            })
            .collect();
        return Self { values };
    }
}

impl Palette for AppKit {
    fn system(&self, color: SystemColor, variant: Variant) -> Rgba {
        let row = SystemColor::ALL.iter().position(|other| *other == color).unwrap_or(0);
        return self.values[row][variant.index()];
    }
}

fn dynamic(variants: [Retained<NSColor>; 4]) -> Retained<NSColor> {
    let block = RcBlock::new(move |appearance: NonNull<NSAppearance>| -> NonNull<NSColor> {
        let index = variant(unsafe { appearance.as_ref() }).index();
        return NonNull::from(&*variants[index]);
    });
    return unsafe { NSColor::colorWithName_dynamicProvider(None, &block) };
}

pub fn pair_color(pair: &Pair) -> Retained<NSColor> {
    let (light, dark) = (pair.light, pair.dark());
    return dynamic([light, dark, light, dark].map(ns_color));
}

pub struct Theme {
    colors: Vec<Retained<NSColor>>,
    resolved: Vec<[Rgba; 4]>,
    accent: Retained<NSColor>,
    accents: [Rgba; 4],
}

impl Theme {
    pub fn new(style: &Style) -> Self {
        let palette = AppKit::capture();
        let mut colors = Vec::with_capacity(Key::ALL.len());
        let mut resolved = Vec::with_capacity(Key::ALL.len());
        for key in Key::ALL {
            let values = Variant::ALL.map(|variant| style.resolve(key, variant, &palette));
            let color = match style.native(key) {
                Some(native) => system(native),
                None => dynamic(values.map(ns_color)),
            };
            colors.push(color);
            resolved.push(values);
        }
        let accents = Variant::ALL.map(|variant| style.accent(variant, &palette));
        let accent = match style.native(Key::Selection) {
            Some(native) if style.pair(Key::Selection).is_none() => system(native),
            _ => dynamic(accents.map(ns_color)),
        };
        return Self { colors, resolved, accent, accents };
    }

    pub fn color(&self, key: Key) -> Retained<NSColor> {
        return self.colors[key as usize].clone();
    }

    pub fn value(&self, key: Key, variant: Variant) -> Rgba {
        return self.resolved[key as usize][variant.index()];
    }

    pub fn accent(&self) -> Retained<NSColor> {
        return self.accent.clone();
    }

    pub fn accent_value(&self, variant: Variant) -> Rgba {
        return self.accents[variant.index()];
    }
}

pub struct Look {
    pub mode: usize,
    pub styles: Vec<Style>,
    pub active: usize,
    pub theme: Theme,
}

impl Look {
    pub const MODES: [&str; 3] = [i18n::key("System"), i18n::key("Light"), i18n::key("Dark")];

    fn choose(appearance: &Appearance, user: &UserStyles) -> (usize, Vec<Style>, usize) {
        let styles = user.all();
        let active = styles.iter().position(|style| style.name.eq_ignore_ascii_case(&appearance.style)).unwrap_or(0);
        let mode = match appearance.mode {
            AppearanceMode::System => 0,
            AppearanceMode::Light => 1,
            AppearanceMode::Dark => 2,
        };
        return (mode, styles, active);
    }

    pub fn new(appearance: &Appearance, user: &UserStyles) -> Self {
        let (mode, styles, active) = Self::choose(appearance, user);
        let theme = Theme::new(&styles[active]);
        return Self { mode, styles, active, theme };
    }

    pub fn update(&mut self, appearance: &Appearance, user: &UserStyles) -> bool {
        let (mode, styles, active) = Self::choose(appearance, user);
        if mode == self.mode && active == self.active && styles == self.styles {
            return false;
        }
        let rebuild = active != self.active || styles != self.styles;
        self.mode = mode;
        self.styles = styles;
        self.active = active;
        if rebuild {
            self.rebuild();
        }
        return true;
    }

    pub fn style(&self) -> &Style {
        return &self.styles[self.active];
    }

    pub fn rebuild(&mut self) {
        self.theme = Theme::new(&self.styles[self.active]);
    }
}
