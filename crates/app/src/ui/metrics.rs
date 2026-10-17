use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBox, NSBoxType, NSColor, NSFont, NSFontWeight, NSFontWeightMedium, NSFontWeightRegular, NSFontWeightSemibold,
    NSGlassEffectView, NSGlassEffectViewStyle, NSTitlePosition, NSView, NSVisualEffectBlendingMode,
    NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWorkspace,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::NSSize;
use objc2_quartz_core::{
    CAMediaTimingFunction, kCAMediaTimingFunctionEaseIn, kCAMediaTimingFunctionEaseInEaseOut,
    kCAMediaTimingFunctionEaseOut,
};

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
pub const RADIUS_CARD: CGFloat = 12.0;
pub const RING_GAP: CGFloat = 5.0;

pub const BORDER_HAIRLINE: CGFloat = 0.5;
pub const BORDER_EMPHASIS: CGFloat = 1.5;
pub const BORDER_RING: CGFloat = 3.0;

pub const ICON_BADGE: CGFloat = 8.0;
pub const ICON_SMALL: CGFloat = 11.0;
pub const ICON_REGULAR: CGFloat = 13.0;
pub const ICON_ROW: CGFloat = 16.0;
pub const ICON_LARGE: CGFloat = 20.0;
pub const ICON_HERO: CGFloat = 32.0;

pub const ROW_LIST: CGFloat = 28.0;
pub const ROW_SETTINGS: CGFloat = 40.0;
pub const ROW_SETTINGS_HINT: CGFloat = 52.0;
pub const CONTROL_HEIGHT: CGFloat = 26.0;
pub const EMPTY_TOP: CGFloat = 200.0;

pub const MOTION_QUICK: f64 = 0.15;
pub const MOTION_STANDARD: f64 = 0.22;
pub const MOTION_EMPHASIZED: f64 = 0.3;
pub const SCROLL_MIN: f64 = 0.16;
pub const SCROLL_MAX: f64 = 0.35;
pub const HOVER_SCALE: f64 = 1.03;
pub const DIMMED: f64 = 0.4;
pub const MUTED: f64 = 0.85;
pub const SELECT_SCALE: f64 = 0.94;

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
    pub fn size(self) -> CGFloat {
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

pub fn ease_in() -> Retained<CAMediaTimingFunction> {
    return timing(unsafe { kCAMediaTimingFunctionEaseIn });
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

fn pin(view: &NSView, to: &NSView) {
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    view.leadingAnchor().constraintEqualToAnchor(&to.leadingAnchor()).setActive(true);
    view.trailingAnchor().constraintEqualToAnchor(&to.trailingAnchor()).setActive(true);
    view.topAnchor().constraintEqualToAnchor(&to.topAnchor()).setActive(true);
    view.bottomAnchor().constraintEqualToAnchor(&to.bottomAnchor()).setActive(true);
}

fn card(mtm: MainThreadMarker, fill: &NSColor, radius: CGFloat) -> Retained<NSView> {
    let card = NSBox::new(mtm);
    card.setBoxType(NSBoxType::Custom);
    card.setTitlePosition(NSTitlePosition::NoTitle);
    card.setBorderWidth(0.0);
    card.setCornerRadius(radius);
    card.setFillColor(fill);
    card.setContentViewMargins(NSSize::ZERO);
    return Retained::into_super(card);
}

pub struct Surface {
    host: Retained<NSView>,
    content: Retained<NSView>,
    radius: CGFloat,
}

impl Surface {
    pub fn mount(
        mtm: MainThreadMarker,
        content: &NSView,
        radius: CGFloat,
        tint: Option<&NSColor>,
        fill: &NSColor,
    ) -> Self {
        let surface = Self { host: NSView::new(mtm), content: Retained::from(content), radius };
        surface.paint(tint, fill);
        return surface;
    }

    pub fn view(&self) -> &NSView {
        return &self.host;
    }

    pub fn paint(&self, tint: Option<&NSColor>, fill: &NSColor) {
        let mtm = self.host.mtm();
        for child in &self.host.subviews() {
            child.removeFromSuperview();
        }
        self.content.removeFromSuperview();
        let material = if reduce_transparency() {
            let opaque = tint.unwrap_or(fill).colorWithAlphaComponent(1.0);
            let card = card(mtm, &opaque, self.radius);
            card.addSubview(&self.content);
            card
        } else if glass_available() {
            let glass = NSGlassEffectView::new(mtm);
            glass.setStyle(NSGlassEffectViewStyle::Regular);
            glass.setCornerRadius(self.radius);
            glass.setTintColor(tint);
            glass.setContentView(Some(&self.content));
            Retained::into_super(glass)
        } else if let Some(tint) = tint {
            let card = card(mtm, tint, self.radius);
            card.addSubview(&self.content);
            card
        } else {
            let effect = NSVisualEffectView::new(mtm);
            effect.setMaterial(NSVisualEffectMaterial::Popover);
            effect.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
            effect.setState(NSVisualEffectState::Active);
            effect.setWantsLayer(true);
            if let Some(layer) = effect.layer() {
                layer.setCornerRadius(self.radius);
                layer.setMasksToBounds(true);
            }
            effect.addSubview(&self.content);
            Retained::into_super(effect)
        };
        self.host.addSubview(&material);
        pin(&material, &self.host);
        pin(&self.content, &material);
    }
}
