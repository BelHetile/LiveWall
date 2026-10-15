use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameDarkAqua, NSApplication, NSColor, NSFont, NSFontWeight,
    NSFontWeightMedium, NSFontWeightRegular, NSFontWeightSemibold, NSGlassEffectView, NSImage,
    NSImageSymbolConfiguration, NSImageView, NSLayoutConstraintOrientation, NSLineBreakMode, NSStackView,
    NSStackViewDistribution, NSTextField, NSUserInterfaceLayoutOrientation, NSView, NSVisualEffectBlendingMode,
    NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
};
use objc2_core_foundation::CGFloat;
use objc2_core_graphics::CGColor;
use objc2_foundation::{NSArray, NSEdgeInsets, NSString};

use crate::ui::fixture::{Library, Target};

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    pub struct Flipped;

    impl Flipped {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl Flipped {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        return unsafe { msg_send![super(this), init] };
    }
}

pub enum Weight {
    Regular,
    Medium,
    Semibold,
}

impl Weight {
    fn value(&self) -> NSFontWeight {
        return unsafe {
            match self {
                Weight::Regular => NSFontWeightRegular,
                Weight::Medium => NSFontWeightMedium,
                Weight::Semibold => NSFontWeightSemibold,
            }
        };
    }
}

pub fn label(
    mtm: MainThreadMarker,
    text: &str,
    size: CGFloat,
    weight: &Weight,
    color: &NSColor,
) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    field.setFont(Some(&NSFont::systemFontOfSize_weight(size, weight.value())));
    field.setTextColor(Some(color));
    field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    field.setMaximumNumberOfLines(1);
    field.setContentCompressionResistancePriority_forOrientation(250.0, NSLayoutConstraintOrientation::Horizontal);
    return field;
}

pub fn symbol(name: &str, size: CGFloat) -> Option<Retained<NSImage>> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), None)?;
    let config = NSImageSymbolConfiguration::configurationWithPointSize_weight(size, unsafe { NSFontWeightMedium });
    return image.imageWithSymbolConfiguration(&config);
}

pub fn symbol_view(mtm: MainThreadMarker, name: &str, size: CGFloat, tint: &NSColor) -> Retained<NSView> {
    let Some(image) = symbol(name, size) else {
        return NSView::new(mtm);
    };
    let view = NSImageView::imageViewWithImage(&image, mtm);
    view.setContentTintColor(Some(tint));
    return self::view(&view);
}

pub fn view(view: &NSView) -> Retained<NSView> {
    return Retained::from(view);
}

pub fn stack(mtm: MainThreadMarker, views: &[&NSView], vertical: bool, spacing: CGFloat) -> Retained<NSStackView> {
    let list: Vec<Retained<NSView>> = views.iter().map(|view| Retained::from(*view)).collect();
    let stack = NSStackView::stackViewWithViews(&NSArray::from_retained_slice(&list), mtm);
    stack.setOrientation(if vertical {
        NSUserInterfaceLayoutOrientation::Vertical
    } else {
        NSUserInterfaceLayoutOrientation::Horizontal
    });
    stack.setSpacing(spacing);
    return stack;
}

pub fn insets(top: CGFloat, left: CGFloat, bottom: CGFloat, right: CGFloat) -> NSEdgeInsets {
    return NSEdgeInsets { top, left, bottom, right };
}

pub fn spacer(mtm: MainThreadMarker) -> Retained<NSView> {
    let view = NSView::new(mtm);
    view.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
    return view;
}

pub fn row(mtm: MainThreadMarker, title: &str, control: &NSView) -> Retained<NSStackView> {
    let name = label(mtm, title, 13.0, &Weight::Regular, &NSColor::labelColor());
    let gap = spacer(mtm);
    let row = stack(mtm, &[&name, &gap, control], false, 8.0);
    row.setDistribution(NSStackViewDistribution::Fill);
    row.setEdgeInsets(insets(0.0, 12.0, 0.0, 12.0));
    row.heightAnchor().constraintEqualToConstant(38.0).setActive(true);
    return row;
}

pub fn group(mtm: MainThreadMarker, title: Option<&str>, rows: &[&NSView]) -> Retained<NSStackView> {
    let body = stack(mtm, rows, true, 0.0);
    body.setEdgeInsets(insets(4.0, 0.0, 4.0, 0.0));
    let card = NSView::new(mtm);
    card.setWantsLayer(true);
    if let Some(layer) = card.layer() {
        layer.setCornerRadius(12.0);
        layer.setBackgroundColor(Some(&cg(mtm, &NSColor::quaternarySystemFillColor())));
    }
    card.addSubview(&body);
    pin(&body, &card, 0.0);
    for view in rows {
        view.leadingAnchor().constraintEqualToAnchor(&body.leadingAnchor()).setActive(true);
        view.trailingAnchor().constraintEqualToAnchor(&body.trailingAnchor()).setActive(true);
    }
    let mut views: Vec<&NSView> = Vec::new();
    let header = title.map(|title| label(mtm, title, 13.0, &Weight::Semibold, &NSColor::secondaryLabelColor()));
    if let Some(header) = &header {
        views.push(header);
    }
    views.push(&card);
    let section = stack(mtm, &views, true, 8.0);
    section.setAlignment(objc2_app_kit::NSLayoutAttribute::Leading);
    card.leadingAnchor().constraintEqualToAnchor(&section.leadingAnchor()).setActive(true);
    card.trailingAnchor().constraintEqualToAnchor(&section.trailingAnchor()).setActive(true);
    if let Some(header) = &header {
        header.leadingAnchor().constraintEqualToAnchor_constant(&section.leadingAnchor(), 4.0).setActive(true);
    }
    return section;
}

pub fn has_glass() -> bool {
    return AnyClass::get(c"NSGlassEffectView").is_some();
}

pub fn glass(mtm: MainThreadMarker, content: &NSView, radius: CGFloat, tint: Option<&NSColor>) -> Retained<NSView> {
    if has_glass() {
        let view = NSGlassEffectView::new(mtm);
        view.setCornerRadius(radius);
        view.setTintColor(tint);
        view.setContentView(Some(content));
        pin(content, &view, 0.0);
        return Retained::into_super(view);
    }
    let view = NSVisualEffectView::new(mtm);
    view.setMaterial(NSVisualEffectMaterial::HUDWindow);
    view.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
    view.setState(NSVisualEffectState::Active);
    view.setWantsLayer(true);
    if let Some(layer) = view.layer() {
        layer.setCornerRadius(radius);
        layer.setMasksToBounds(true);
    }
    content.setTranslatesAutoresizingMaskIntoConstraints(false);
    view.addSubview(content);
    pin(content, &view, 0.0);
    return Retained::into_super(view);
}

pub fn pin(view: &NSView, to: &NSView, inset: CGFloat) {
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    view.leadingAnchor().constraintEqualToAnchor_constant(&to.leadingAnchor(), inset).setActive(true);
    view.trailingAnchor().constraintEqualToAnchor_constant(&to.trailingAnchor(), -inset).setActive(true);
    view.topAnchor().constraintEqualToAnchor_constant(&to.topAnchor(), inset).setActive(true);
    view.bottomAnchor().constraintEqualToAnchor_constant(&to.bottomAnchor(), -inset).setActive(true);
}

pub fn dark() -> Option<Retained<NSAppearance>> {
    return NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua });
}

pub fn cg(mtm: MainThreadMarker, color: &NSColor) -> Retained<CGColor> {
    let appearance = NSApplication::sharedApplication(mtm).effectiveAppearance();
    let out = std::cell::RefCell::new(None);
    appearance.performAsCurrentDrawingAppearance(&block2::StackBlock::new(|| {
        *out.borrow_mut() = Some(color.CGColor());
    }));
    return out.into_inner().unwrap_or_else(|| NSColor::clearColor().CGColor());
}

pub fn marker_text(library: &Library, markers: &[Target]) -> Vec<(String, &'static str)> {
    let mut chips = Vec::new();
    for (display, _) in library.displays.iter().enumerate() {
        let spaces: Vec<String> = markers
            .iter()
            .filter_map(|target| match target {
                Target::Space { display: d, space } if *d == display => Some((space + 1).to_string()),
                _ => None,
            })
            .collect();
        if markers.iter().any(|target| matches!(target, Target::Display { display: d } if *d == display)) {
            chips.push(((display + 1).to_string(), "display"));
        } else if !spaces.is_empty() {
            chips.push((format!("{}·{}", display + 1, spaces.join(",")), "display"));
        }
    }
    if markers.contains(&Target::Lock) {
        chips.push(("Lock".to_string(), "lock.fill"));
    }
    return chips;
}

pub fn chip(mtm: MainThreadMarker, text: &str, icon: &str) -> Retained<NSView> {
    let white = NSColor::whiteColor();
    let glyph = symbol_view(mtm, icon, 10.0, &white);
    let mut views: Vec<&NSView> = vec![&glyph];
    let title = label(mtm, text, 11.0, &Weight::Semibold, &white);
    title.setContentCompressionResistancePriority_forOrientation(750.0, NSLayoutConstraintOrientation::Horizontal);
    if !text.is_empty() {
        views.push(&title);
    }
    let content = stack(mtm, &views, false, 4.0);
    content.setEdgeInsets(insets(3.0, 8.0, 3.0, 8.0));
    let view = glass(mtm, &content, 11.0, None);
    view.setAppearance(dark().as_deref());
    return view;
}
