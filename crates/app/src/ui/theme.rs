use std::cell::RefCell;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::{Arc, PoisonError, RwLock};

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::{Retained, Weak};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAccessibilityHighContrastAqua, NSAppearanceNameAccessibilityHighContrastDarkAqua,
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSColor, NSColorSpace, NSView,
};
use objc2_foundation::{NSArray, NSString};

use crate::i18n;
use crate::model::{Appearance, AppearanceMode};
use crate::style::{Key, Pair, Palette, Rgba, Role, Style, SystemColor, Tag, UserStyles, Variant};
use crate::ui::widgets::{self, Slot};

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

thread_local! {
    static NAMES: Retained<NSArray<NSString>> = unsafe {
        NSArray::from_slice(&[
            NSAppearanceNameAqua,
            NSAppearanceNameDarkAqua,
            NSAppearanceNameAccessibilityHighContrastAqua,
            NSAppearanceNameAccessibilityHighContrastDarkAqua,
        ])
    };
    static STATE: RefCell<Option<Rc<RefCell<State>>>> = const { RefCell::new(None) };
    static REGISTRY: RefCell<Vec<(Weak<NSView>, Key, Slot)>> = const { RefCell::new(Vec::new()) };
}

pub fn variant(appearance: &NSAppearance) -> Variant {
    let Some(best) = NAMES.with(|names| appearance.bestMatchFromAppearancesWithNames(names)) else {
        return Variant::Light;
    };
    let best: &NSString = &best;
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

fn complete(exact: &[[Option<Rgba>; 4]]) -> Vec<[Rgba; 4]> {
    let page = SystemColor::UnderPageBackground as usize;
    let base = |row: &[Option<Rgba>; 4], dark: bool| {
        let variant = if dark { Variant::Dark } else { Variant::Light };
        return row[variant.index()].unwrap_or(Rgba::CLEAR);
    };
    return exact
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
        return Self { values: complete(&exact) };
    }
}

impl Palette for AppKit {
    fn system(&self, color: SystemColor, variant: Variant) -> Rgba {
        return self.values[color as usize][variant.index()];
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Table {
    keys: Vec<[Rgba; 4]>,
    accent: [Rgba; 4],
}

impl Table {
    fn build(style: &Style, palette: &impl Palette) -> Self {
        let keys =
            Key::ALL.iter().map(|key| Variant::ALL.map(|variant| style.resolve(*key, variant, palette))).collect();
        let accent = Variant::ALL.map(|variant| style.accent(variant, palette));
        return Self { keys, accent };
    }
}

struct Dynamic {
    variants: Arc<RwLock<[Rgba; 4]>>,
    color: Retained<NSColor>,
}

impl Dynamic {
    fn build(variants: &Arc<RwLock<[Rgba; 4]>>) -> Retained<NSColor> {
        let shared = Arc::clone(variants);
        let block = RcBlock::new(move |appearance: NonNull<NSAppearance>| -> NonNull<NSColor> {
            let index = variant(unsafe { appearance.as_ref() }).index();
            let table = shared.read().unwrap_or_else(PoisonError::into_inner);
            let color = ns_color(table[index]);
            return unsafe { NonNull::new_unchecked(Retained::autorelease_return(color)) };
        });
        return unsafe { NSColor::colorWithName_dynamicProvider(None, &block) };
    }

    fn new(values: [Rgba; 4]) -> Self {
        let variants = Arc::new(RwLock::new(values));
        let color = Self::build(&variants);
        return Self { variants, color };
    }

    fn refresh(&mut self, previous: &[Rgba; 4], next: &[Rgba; 4]) {
        if previous == next {
            return;
        }
        let mut table = self.variants.write().unwrap_or_else(PoisonError::into_inner);
        *table = *next;
        self.color = Self::build(&self.variants);
    }
}

struct State {
    style: Style,
    palette: AppKit,
    table: Table,
    keys: Vec<Dynamic>,
    accent: Dynamic,
}

impl State {
    fn new(style: &Style) -> Self {
        let palette = AppKit::capture();
        let table = Table::build(style, &palette);
        let keys = table.keys.iter().map(|values| Dynamic::new(*values)).collect();
        let accent = Dynamic::new(table.accent);
        return Self { style: style.clone(), palette, table, keys, accent };
    }

    fn rebuild(&mut self) -> bool {
        let next = Table::build(&self.style, &self.palette);
        if next == self.table {
            return false;
        }
        for ((dynamic, previous), values) in self.keys.iter_mut().zip(&self.table.keys).zip(&next.keys) {
            dynamic.refresh(previous, values);
        }
        self.accent.refresh(&self.table.accent, &next.accent);
        self.table = next;
        return true;
    }
}

pub fn pair_color(pair: &Pair) -> Retained<NSColor> {
    let (light, dark) = (pair.light, pair.dark());
    return Dynamic::new([light, dark, light, dark]).color;
}

pub fn register(view: &NSView, key: Key, slot: Slot) {
    REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        if registry.len() == registry.capacity() {
            registry.retain(|(weak, _, _)| weak.load().is_some());
        }
        registry.push((Weak::new(view), key, slot));
    });
}

pub fn forget(view: &NSView) {
    REGISTRY.with(|registry| {
        registry
            .borrow_mut()
            .retain(|(weak, _, _)| weak.load().is_some_and(|loaded| !std::ptr::eq(Retained::as_ptr(&loaded), view)));
    });
}

pub fn redraw(view: &NSView) {
    view.setNeedsDisplay(true);
    for child in &view.subviews() {
        redraw(&child);
    }
}

pub fn invalidate() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(theme) = Theme::current() else {
        return;
    };
    let mut live = Vec::new();
    REGISTRY.with(|registry| {
        registry.borrow_mut().retain(|(weak, key, slot)| {
            let Some(view) = weak.load() else {
                return false;
            };
            live.push((view, *key, *slot));
            return true;
        });
    });
    for (view, key, slot) in &live {
        widgets::paint(view, *key, *slot, &theme);
    }
    for window in &NSApplication::sharedApplication(mtm).windows() {
        if let Some(content) = window.contentView() {
            let root = unsafe { content.superview() }.unwrap_or(content);
            redraw(&root);
        }
    }
}

#[derive(Clone)]
pub struct Theme {
    state: Rc<RefCell<State>>,
}

impl Theme {
    pub fn new(style: &Style) -> Self {
        let state = STATE.with(|cell| {
            let mut slot = cell.borrow_mut();
            if let Some(state) = slot.as_ref() {
                return Rc::clone(state);
            }
            let state = Rc::new(RefCell::new(State::new(style)));
            *slot = Some(Rc::clone(&state));
            return state;
        });
        let theme = Self { state };
        theme.apply(style);
        return theme;
    }

    pub fn current() -> Option<Self> {
        return STATE.with(|cell| cell.borrow().as_ref().map(|state| Self { state: Rc::clone(state) }));
    }

    pub fn apply(&self, style: &Style) -> bool {
        let mut state = self.state.borrow_mut();
        if state.style == *style {
            return false;
        }
        state.style = style.clone();
        return state.rebuild();
    }

    pub fn recapture(&self) -> bool {
        let mut state = self.state.borrow_mut();
        state.palette = AppKit::capture();
        return state.rebuild();
    }

    pub fn color(&self, key: Key) -> Retained<NSColor> {
        return self.state.borrow().keys[key as usize].color.clone();
    }

    pub fn value(&self, key: Key, variant: Variant) -> Rgba {
        return self.state.borrow().table.keys[key as usize][variant.index()];
    }

    pub fn accent(&self) -> Retained<NSColor> {
        return self.state.borrow().accent.color.clone();
    }

    pub fn accent_value(&self, variant: Variant) -> Rgba {
        return self.state.borrow().table.accent[variant.index()];
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LookChange {
    pub mode: bool,
    pub styles: bool,
    pub repaint: bool,
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

    pub fn update(&mut self, appearance: &Appearance, user: &UserStyles) -> LookChange {
        let (mode, styles, active) = Self::choose(appearance, user);
        let change = LookChange {
            mode: mode != self.mode,
            styles: active != self.active || styles != self.styles,
            repaint: false,
        };
        if !change.mode && !change.styles {
            return change;
        }
        self.mode = mode;
        self.styles = styles;
        self.active = active;
        let repaint = self.theme.apply(&self.styles[self.active]);
        return LookChange { repaint, ..change };
    }

    pub fn style(&self) -> &Style {
        return &self.styles[self.active];
    }

    pub fn rebuild(&mut self) -> bool {
        return self.theme.recapture();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed;

    fn system_style() -> Style {
        return UserStyles::default().active("");
    }

    impl Palette for Fixed {
        fn system(&self, color: SystemColor, variant: Variant) -> Rgba {
            let shade = if variant.is_dark() { 0x20 } else { 0xE0 };
            return match color {
                SystemColor::ControlAccent => Rgba::hex(if variant.is_dark() { 0x0A_84FF } else { 0x00_7AFF }),
                other => Rgba::rgb(shade, shade.wrapping_add(other as u8), shade),
            };
        }
    }

    #[test]
    fn variants_index_the_four_appearances() {
        let indexes: Vec<usize> = Variant::ALL.iter().map(|variant| variant.index()).collect();
        assert_eq!(indexes, [0, 1, 2, 3]);
    }

    #[test]
    fn system_color_order_matches_the_capture_rows() {
        for (position, color) in SystemColor::ALL.iter().enumerate() {
            assert_eq!(*color as usize, position);
        }
    }

    #[test]
    fn table_has_a_row_per_key_and_follows_the_style() {
        let mut style = system_style();
        let before = Table::build(&style, &Fixed);
        assert_eq!(before.keys.len(), Key::ALL.len());
        assert_eq!(before, Table::build(&style, &Fixed));
        style.set("accent", "#123456", None).expect("accent");
        let after = Table::build(&style, &Fixed);
        assert_ne!(before, after);
        assert_eq!(after.accent[Variant::Light.index()], Rgba::hex(0x12_3456));
        assert_eq!(after.keys[Key::Selection as usize][Variant::Light.index()], Rgba::hex(0x12_3456));
    }

    #[test]
    fn dynamic_colors_reread_the_live_table_after_a_generation_bump() {
        let values = [Rgba::WHITE, Rgba::BLACK, Rgba::WHITE, Rgba::BLACK];
        let mut dynamic = Dynamic::new(values);
        let light = appearance(Variant::Light).expect("aqua");
        let dark = appearance(Variant::Dark).expect("darkAqua");
        assert_eq!(resolve_under(&light, &dynamic.color), Some(Rgba::WHITE));
        assert_eq!(resolve_under(&dark, &dynamic.color), Some(Rgba::BLACK));
        let next = [Rgba::hex(0x12_3456); 4];
        let old = Retained::as_ptr(&dynamic.color);
        dynamic.refresh(&values, &next);
        assert_ne!(old, Retained::as_ptr(&dynamic.color));
        assert_eq!(resolve_under(&light, &dynamic.color), Some(Rgba::hex(0x12_3456)));
        assert_eq!(resolve_under(&dark, &dynamic.color), Some(Rgba::hex(0x12_3456)));
    }

    #[test]
    fn unchanged_values_keep_the_same_color_generation() {
        let values = [Rgba::WHITE; 4];
        let mut dynamic = Dynamic::new(values);
        let old = Retained::as_ptr(&dynamic.color);
        dynamic.refresh(&values, &values);
        assert_eq!(old, Retained::as_ptr(&dynamic.color));
    }

    #[test]
    fn rebuild_bumps_generations_only_where_the_table_changed() {
        let mut style = system_style();
        let mut state = State::new(&style);
        let before: Vec<*const NSColor> = state.keys.iter().map(|dynamic| Retained::as_ptr(&dynamic.color)).collect();
        style.set("accent", "#123456", None).expect("accent");
        state.style = style;
        assert!(state.rebuild());
        assert_ne!(before[Key::Selection as usize], Retained::as_ptr(&state.keys[Key::Selection as usize].color));
        let unchanged =
            state.keys.iter().zip(&before).filter(|(dynamic, old)| Retained::as_ptr(&dynamic.color) == **old).count();
        assert!(unchanged > 0);
        assert!(!state.rebuild());
    }

    #[test]
    fn system_accent_follows_the_palette_and_text_on_it_is_white() {
        let table = Table::build(&system_style(), &Fixed);
        assert_eq!(table.accent[Variant::Light.index()], Rgba::hex(0x00_7AFF));
        assert_eq!(table.accent[Variant::Dark.index()], Rgba::hex(0x0A_84FF));
        for variant in Variant::ALL {
            assert_eq!(table.keys[Key::OnAccent as usize][variant.index()], Rgba::WHITE, "{variant:?}");
        }
    }

    #[test]
    fn missing_high_contrast_values_are_derived_from_their_plain_variant() {
        let rows = SystemColor::ALL.len();
        let gray = Rgba::rgb(0x80, 0x80, 0x80);
        let mut exact = vec![[Some(gray), Some(gray), None, None]; rows];
        exact[SystemColor::Label as usize] = [Some(Rgba::BLACK), Some(Rgba::WHITE), Some(Rgba::BLACK), None];
        let values = complete(&exact);
        assert_eq!(values.len(), rows);
        let label = values[SystemColor::Label as usize];
        assert_eq!(label[Variant::LightHc.index()], Rgba::BLACK);
        assert_eq!(label[Variant::DarkHc.index()], label[Variant::DarkHc.index()].opaque());
        let tertiary = values[SystemColor::TertiaryLabel as usize];
        assert_eq!(tertiary[Variant::Light.index()], gray);
        assert_ne!(tertiary[Variant::LightHc.index()], Rgba::CLEAR);
    }
}
