use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyClass;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSColor, NSFont, NSFontWeight, NSFontWeightMedium, NSFontWeightRegular, NSFontWeightSemibold, NSGlassEffectView,
    NSGlassEffectViewStyle, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
    NSVisualEffectView, NSWorkspace,
};
use objc2_core_foundation::CGFloat;
use objc2_quartz_core::{CAMediaTimingFunction, kCAMediaTimingFunctionEaseInEaseOut, kCAMediaTimingFunctionEaseOut};

use crate::style::Key;
use crate::ui::theme::{self, Theme};
use crate::ui::widgets::{self, Slot};

pub const SPACE_2: CGFloat = 2.0;
pub const SPACE_4: CGFloat = 4.0;
pub const SPACE_8: CGFloat = 8.0;
pub const SPACE_12: CGFloat = 12.0;
pub const SPACE_16: CGFloat = 16.0;
pub const SPACE_20: CGFloat = 20.0;
pub const SPACE_24: CGFloat = 24.0;
pub const INSPECTOR_INSET: CGFloat = SPACE_20;
pub const WALL_PADDING: CGFloat = SPACE_16;

pub const RADIUS_CONTROL: CGFloat = 6.0;
pub const RADIUS_SMALL: CGFloat = 8.0;
pub const RADIUS_CARD: CGFloat = 10.0;
pub const RADIUS_TILE: CGFloat = 12.0;
pub const RING_GAP: CGFloat = 5.0;

pub const BORDER_HAIRLINE: CGFloat = 0.5;
pub const BORDER_EMPHASIS: CGFloat = 1.5;
pub const BORDER_RING: CGFloat = 3.0;

pub const ICON_BADGE: CGFloat = 8.0;
pub const ICON_SMALL: CGFloat = 11.0;
pub const ICON_REGULAR: CGFloat = 13.0;
pub const ICON_TOOLBAR: CGFloat = 15.0;
pub const ICON_ROW: CGFloat = 16.0;
pub const ICON_LARGE: CGFloat = 20.0;
pub const ICON_HERO: CGFloat = 32.0;

pub const ROW_LIST: CGFloat = 28.0;
pub const ROW_SETTINGS: CGFloat = 36.0;
pub const ROW_SETTINGS_HINT: CGFloat = 44.0;
pub const CONTROL_HEIGHT: CGFloat = 26.0;
pub const BUTTON_HEIGHT: CGFloat = 28.0;
pub const EMPTY_TOP: CGFloat = 200.0;

pub const MOTION_QUICK: f64 = 0.15;
pub const MOTION_STATE: f64 = 0.2;
pub const MOTION_LAYOUT: f64 = 0.25;
pub const SCROLL_MIN: f64 = 0.2;
pub const SCROLL_MAX: f64 = 0.35;
pub const HOVER_SCALE: f64 = 1.03;
pub const DIMMED: f64 = 0.4;
pub const MUTED: f64 = 0.85;
pub const SELECT_SCALE: f64 = 0.94;
pub const EDGE_HOVER: f32 = 0.45;
pub const EDGE_PRESSED: f32 = 0.7;

pub fn capsule(height: CGFloat) -> CGFloat {
    return height / 2.0;
}

pub fn ring_radius(radius: CGFloat) -> CGFloat {
    return radius + RING_GAP;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Text {
    Title2,
    Title3,
    Headline,
    Body,
    BodyMedium,
    Callout,
    CalloutMedium,
    Subheadline,
    SubheadlineMedium,
    CaptionBold,
    Section,
}

impl Text {
    fn size(self) -> CGFloat {
        return match self {
            Self::Title2 => 17.0,
            Self::Title3 => 15.0,
            Self::Headline | Self::Body | Self::BodyMedium => 13.0,
            Self::Callout | Self::CalloutMedium => 12.0,
            Self::Subheadline | Self::SubheadlineMedium | Self::Section => 11.0,
            Self::CaptionBold => 10.0,
        };
    }

    fn weight(self) -> NSFontWeight {
        return unsafe {
            match self {
                Self::Title2 | Self::Title3 | Self::Headline | Self::CaptionBold | Self::Section => {
                    NSFontWeightSemibold
                }
                Self::BodyMedium | Self::CalloutMedium | Self::SubheadlineMedium => NSFontWeightMedium,
                Self::Body | Self::Callout | Self::Subheadline => NSFontWeightRegular,
            }
        };
    }
}

pub fn font(text: Text) -> Retained<NSFont> {
    return NSFont::systemFontOfSize_weight(text.size(), text.weight());
}

pub fn digits(text: Text) -> Retained<NSFont> {
    return NSFont::monospacedDigitSystemFontOfSize_weight(text.size(), text.weight());
}

fn timing(name: &'static objc2_foundation::NSString) -> Retained<CAMediaTimingFunction> {
    return CAMediaTimingFunction::functionWithName(name);
}

pub fn ease_out() -> Retained<CAMediaTimingFunction> {
    return timing(unsafe { kCAMediaTimingFunctionEaseOut });
}

pub fn ease_in_out() -> Retained<CAMediaTimingFunction> {
    return timing(unsafe { kCAMediaTimingFunctionEaseInEaseOut });
}

pub fn reduce_motion() -> bool {
    return NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion();
}

pub fn reduce_transparency() -> bool {
    return NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency();
}

pub fn transform(duration: f64) -> f64 {
    return if reduce_motion() { 0.0 } else { duration };
}

pub fn scale(value: f64) -> f64 {
    return if reduce_motion() { 1.0 } else { value };
}

fn glass_available() -> bool {
    return AnyClass::get(c"NSGlassEffectView").is_some();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Opaque,
    Glass,
    Tinted,
    Effect,
}

fn kind(tint: Option<Key>) -> Kind {
    return if reduce_transparency() {
        Kind::Opaque
    } else if glass_available() {
        Kind::Glass
    } else if tint.is_some() {
        Kind::Tinted
    } else {
        Kind::Effect
    };
}

fn make(mtm: MainThreadMarker, kind: Kind, content: &NSView, radius: CGFloat) -> Retained<NSView> {
    return match kind {
        Kind::Opaque | Kind::Tinted => {
            let card = widgets::card(mtm, &NSColor::clearColor(), radius);
            card.addSubview(content);
            Retained::into_super(card)
        }
        Kind::Glass => {
            let glass = NSGlassEffectView::new(mtm);
            glass.setStyle(NSGlassEffectViewStyle::Regular);
            glass.setCornerRadius(radius);
            glass.setContentView(Some(content));
            Retained::into_super(glass)
        }
        Kind::Effect => {
            let effect = NSVisualEffectView::new(mtm);
            effect.setMaterial(NSVisualEffectMaterial::Popover);
            effect.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
            effect.setState(NSVisualEffectState::Active);
            effect.setWantsLayer(true);
            if let Some(layer) = effect.layer() {
                layer.setCornerRadius(radius);
                layer.setMasksToBounds(true);
            }
            effect.addSubview(content);
            Retained::into_super(effect)
        }
    };
}

thread_local! {
    static SURFACES: RefCell<Vec<Rc<Inner>>> = const { RefCell::new(Vec::new()) };
}

pub fn refresh_surfaces() {
    let live: Vec<Rc<Inner>> = SURFACES.with(|surfaces| {
        let mut surfaces = surfaces.borrow_mut();
        surfaces.retain(|inner| inner.host.load().is_some());
        return surfaces.clone();
    });
    for inner in &live {
        inner.sync();
    }
}

struct Inner {
    host: Weak<NSView>,
    content: Retained<NSView>,
    radius: CGFloat,
    theme: Theme,
    tint: Cell<Option<Key>>,
    fill: Cell<Key>,
    kind: Cell<Kind>,
    material: RefCell<Retained<NSView>>,
}

pub struct Surface {
    host: Retained<NSView>,
    inner: Rc<Inner>,
}

impl Surface {
    pub fn mount(
        mtm: MainThreadMarker,
        content: &NSView,
        radius: CGFloat,
        tint: Option<Key>,
        fill: Key,
        theme: &Theme,
    ) -> Self {
        let host = NSView::new(mtm);
        let initial = kind(tint);
        let material = make(mtm, initial, content, radius);
        host.addSubview(&material);
        widgets::pin(&material, &host, 0.0);
        widgets::pin(content, &material, 0.0);
        let inner = Rc::new(Inner {
            host: Weak::new(&host),
            content: Retained::from(content),
            radius,
            theme: theme.clone(),
            tint: Cell::new(tint),
            fill: Cell::new(fill),
            kind: Cell::new(initial),
            material: RefCell::new(material),
        });
        inner.bind();
        SURFACES.with(|surfaces| {
            let mut surfaces = surfaces.borrow_mut();
            if surfaces.len() == surfaces.capacity() {
                surfaces.retain(|other| other.host.load().is_some());
            }
            surfaces.push(Rc::clone(&inner));
        });
        return Self { host, inner };
    }

    pub fn view(&self) -> &NSView {
        return &self.host;
    }

    pub fn refresh_material(&self) {
        self.inner.sync();
    }
}

impl Inner {
    fn sync(&self) {
        let wanted = kind(self.tint.get());
        if wanted != self.kind.get() {
            self.swap(wanted);
        }
        self.bind();
    }

    fn swap(&self, wanted: Kind) {
        let Some(host) = self.host.load() else {
            return;
        };
        let mtm = host.mtm();
        self.content.removeFromSuperview();
        let old = self.material.borrow().clone();
        theme::forget(&old);
        old.removeFromSuperview();
        let material = make(mtm, wanted, &self.content, self.radius);
        host.addSubview(&material);
        widgets::pin(&material, &host, 0.0);
        widgets::pin(&self.content, &material, 0.0);
        self.material.replace(material);
        self.kind.set(wanted);
    }

    fn bind(&self) {
        let material = self.material.borrow();
        theme::forget(&material);
        let (key, slot) = match self.kind.get() {
            Kind::Opaque => (Some(self.tint.get().unwrap_or(self.fill.get())), Slot::Opaque),
            Kind::Glass => (self.tint.get(), Slot::Glass),
            Kind::Tinted => (self.tint.get(), Slot::Fill),
            Kind::Effect => (None, Slot::Fill),
        };
        match key {
            Some(key) => {
                theme::register(&material, key, slot);
                widgets::paint(&material, key, slot, &self.theme);
            }
            None => {
                if let Some(glass) = material.downcast_ref::<NSGlassEffectView>() {
                    glass.setTintColor(None);
                }
            }
        }
    }
}
