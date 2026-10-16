use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ProtocolObject, Sel};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSAppearance, NSAppearanceCustomization,
    NSAppearanceNameDarkAqua, NSApplication, NSBezelStyle, NSBox, NSBoxType, NSButton, NSCellImagePosition, NSColor,
    NSControl, NSControlBorderShape, NSControlSize, NSControlTextEditingDelegate, NSFocusRingType, NSFont,
    NSFontAttributeName, NSFontWeight, NSFontWeightMedium, NSFontWeightRegular, NSFontWeightSemibold,
    NSForegroundColorAttributeName, NSGlassEffectView, NSImage, NSImageSymbolConfiguration, NSImageView,
    NSLayoutConstraintOrientation, NSLineBreakMode, NSStackView, NSStackViewDistribution, NSTextAlignment, NSTextField,
    NSTextFieldDelegate, NSTextView, NSTintProminence, NSTitlePosition, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_core_graphics::CGColor;
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSDictionary, NSEdgeInsets, NSObjectProtocol, NSSize, NSString,
};
use objc2_quartz_core::{
    CAMediaTiming, CAMediaTimingFunction, CATransition, kCAMediaTimingFunctionEaseInEaseOut,
    kCAMediaTimingFunctionEaseOut, kCATransitionFade,
};

use crate::ui::palette::Theme;
use crate::ui::state::Marker;

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

pub struct Units {
    pub step: f64,
    pub format: fn(f64) -> String,
    pub parse: fn(&str) -> Option<f64>,
}

define_class!(
    #[unsafe(super(NSTextField))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Units]
    pub struct NumberField;

    unsafe impl NSObjectProtocol for NumberField {}

    unsafe impl NSControlTextEditingDelegate for NumberField {
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn command(&self, _control: &NSControl, view: &NSTextView, command: Sel) -> bool {
            self.step(view, command)
        }
    }

    unsafe impl NSTextFieldDelegate for NumberField {}
);

impl NumberField {
    fn step(&self, view: &NSTextView, command: Sel) -> bool {
        let direction = if command == sel!(moveUp:) {
            1.0
        } else if command == sel!(moveDown:) {
            -1.0
        } else {
            return false;
        };
        let ivars = self.ivars();
        if let Some(value) = (ivars.parse)(&view.string().to_string()) {
            view.setString(&NSString::from_str(&(ivars.format)(value + direction * ivars.step)));
            unsafe { self.sendAction_to(self.action(), self.target().as_deref()) };
        }
        return true;
    }

    pub fn new(
        mtm: MainThreadMarker,
        units: Units,
        tag: isize,
        width: CGFloat,
        action: Sel,
        target: &AnyObject,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(units);
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        unsafe { this.setDelegate(Some(ProtocolObject::from_ref(&*this))) };
        this.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(11.0, unsafe { NSFontWeightRegular })));
        this.setAlignment(NSTextAlignment::Right);
        this.setBezeled(false);
        this.setBordered(false);
        this.setDrawsBackground(true);
        this.setBackgroundColor(Some(&NSColor::quaternarySystemFillColor()));
        this.setFocusRingType(NSFocusRingType::None);
        this.setTextColor(Some(&NSColor::secondaryLabelColor()));
        this.setWantsLayer(true);
        if let Some(layer) = this.layer() {
            layer.setCornerRadius(5.0);
            layer.setMasksToBounds(true);
        }
        if let Some(cell) = this.cell() {
            cell.setSendsActionOnEndEditing(true);
            cell.setScrollable(true);
        }
        this.setTag(tag);
        unsafe {
            this.setTarget(Some(target));
            this.setAction(Some(action));
        }
        this.widthAnchor().constraintEqualToConstant(width).setActive(true);
        this.setContentCompressionResistancePriority_forOrientation(1000.0, NSLayoutConstraintOrientation::Horizontal);
        return this;
    }

    pub fn value(&self) -> Option<f64> {
        return (self.ivars().parse)(&self.stringValue().to_string());
    }

    pub fn show(&self, value: f64) {
        self.setStringValue(&NSString::from_str(&(self.ivars().format)(value)));
    }

    pub fn editing(&self) -> bool {
        return self.currentEditor().is_some();
    }
}

pub fn number(text: &str) -> Option<f64> {
    let cleaned: String = text
        .chars()
        .map(|character| match character {
            ',' => '.',
            '−' => '-',
            other => other,
        })
        .filter(|character| character.is_ascii_digit() || matches!(character, '.' | '-' | '+'))
        .collect();
    return cleaned.parse().ok();
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

pub fn styled(text: &str, font: &NSFont, color: &NSColor) -> Retained<NSAttributedString> {
    let keys: [&NSAttributedStringKey; 2] = unsafe { [NSFontAttributeName, NSForegroundColorAttributeName] };
    let values: [&AnyObject; 2] = [font, color];
    let attributes = NSDictionary::from_slices(&keys, &values);
    return unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(text),
            Some(&attributes),
        )
    };
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

fn same(left: CGFloat, right: CGFloat) -> bool {
    return (left - right).abs() < 1e-6;
}

pub fn animate(duration: CGFloat, body: impl Fn()) {
    let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
        let context = unsafe { context.as_ref() };
        context.setDuration(duration);
        context.setTimingFunction(Some(&CAMediaTimingFunction::functionWithName(unsafe {
            kCAMediaTimingFunctionEaseInEaseOut
        })));
        context.setAllowsImplicitAnimation(true);
        body();
    });
    NSAnimationContext::runAnimationGroup_completionHandler(&changes, None);
}

pub fn fade(view: &NSView) {
    let Some(layer) = view.layer() else {
        return;
    };
    let transition = CATransition::new();
    transition.setType(unsafe { kCATransitionFade });
    transition.setDuration(0.2);
    transition
        .setTimingFunction(Some(&CAMediaTimingFunction::functionWithName(unsafe { kCAMediaTimingFunctionEaseOut })));
    layer.addAnimation_forKey(&transition, None);
}

pub fn alpha(view: &NSView, value: CGFloat) {
    if same(view.alphaValue(), value) {
        return;
    }
    animate(0.2, || view.animator().setAlphaValue(value));
}

pub fn reveal(view: &NSView, visible: bool) {
    let value = if visible { 1.0 } else { 0.0 };
    if view.isHidden() != visible && same(view.alphaValue(), value) {
        return;
    }
    animate(0.2, || {
        view.animator().setHidden(!visible);
        view.animator().setAlphaValue(value);
    });
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
    let name = label(mtm, title, 12.0, &Weight::Regular, &NSColor::secondaryLabelColor());
    name.widthAnchor().constraintEqualToConstant(72.0).setActive(true);
    let gap = spacer(mtm);
    let row = stack(mtm, &[&name, &gap, control], false, 8.0);
    row.setDistribution(NSStackViewDistribution::Fill);
    row.heightAnchor().constraintEqualToConstant(26.0).setActive(true);
    return row;
}

pub fn section(mtm: MainThreadMarker, title: &str, rows: &[&NSView]) -> Retained<NSStackView> {
    let header = label(mtm, title, 11.0, &Weight::Semibold, &NSColor::tertiaryLabelColor());
    let mut views: Vec<&NSView> = vec![&header];
    views.extend_from_slice(rows);
    let section = stack(mtm, &views, true, 2.0);
    section.setAlignment(objc2_app_kit::NSLayoutAttribute::Leading);
    section.setCustomSpacing_afterView(4.0, &header);
    for view in rows {
        view.leadingAnchor().constraintEqualToAnchor(&section.leadingAnchor()).setActive(true);
        view.trailingAnchor().constraintEqualToAnchor(&section.trailingAnchor()).setActive(true);
    }
    return section;
}

pub fn divider(mtm: MainThreadMarker, theme: &Theme) -> Retained<NSView> {
    let line = card(mtm, &theme.hairline, 0.0);
    line.heightAnchor().constraintEqualToConstant(1.0).setActive(true);
    return Retained::into_super(line);
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

pub fn card(mtm: MainThreadMarker, fill: &NSColor, radius: CGFloat) -> Retained<NSBox> {
    let card = NSBox::new(mtm);
    card.setBoxType(NSBoxType::Custom);
    card.setTitlePosition(NSTitlePosition::NoTitle);
    card.setBorderWidth(0.0);
    card.setCornerRadius(radius);
    card.setFillColor(fill);
    card.setContentViewMargins(NSSize::ZERO);
    return card;
}

pub fn glass(mtm: MainThreadMarker, content: &NSView, radius: CGFloat, tint: Option<&NSColor>) -> Retained<NSView> {
    if AnyClass::get(c"NSGlassEffectView").is_some() {
        let glass = NSGlassEffectView::new(mtm);
        glass.setCornerRadius(radius);
        glass.setTintColor(tint);
        glass.setContentView(Some(content));
        pin(content, &glass, 0.0);
        return Retained::into_super(glass);
    }
    let fill = tint.map_or_else(|| NSColor::colorWithWhite_alpha(0.1, 0.55), |tint| tint.colorWithAlphaComponent(0.85));
    let card = card(mtm, &fill, radius);
    card.addSubview(content);
    pin(content, &card, 0.0);
    return Retained::into_super(card);
}

pub fn chip(mtm: MainThreadMarker, marker: &Marker, theme: &Theme) -> Retained<NSView> {
    let white = NSColor::whiteColor();
    let glyph = symbol_view(mtm, marker.icon, 10.0, &white);
    let text = label(mtm, &marker.text, 11.0, &Weight::Semibold, &white);
    text.setContentCompressionResistancePriority_forOrientation(750.0, NSLayoutConstraintOrientation::Horizontal);
    let content = stack(mtm, &[&glyph, &text], false, 4.0);
    content.setEdgeInsets(insets(0.0, 8.0, 0.0, 8.0));
    content.heightAnchor().constraintEqualToConstant(22.0).setActive(true);
    let tint = marker.active.then(|| theme.accent.colorWithAlphaComponent(0.85));
    let view = glass(mtm, &content, 11.0, tint.as_deref());
    view.setAppearance(dark().as_deref());
    view.setToolTip(Some(&NSString::from_str(&marker.tip)));
    return view;
}

pub fn button(
    mtm: MainThreadMarker,
    title: &str,
    icon: Option<&str>,
    action: Sel,
    target: &AnyObject,
) -> Retained<NSButton> {
    let button =
        unsafe { NSButton::buttonWithTitle_target_action(&NSString::from_str(title), Some(target), Some(action), mtm) };
    button.setBezelStyle(NSBezelStyle::Glass);
    button.setBorderShape(NSControlBorderShape::Capsule);
    button.setControlSize(NSControlSize::Large);
    if let Some(image) = icon.and_then(|name| symbol(name, 13.0)) {
        button.setImage(Some(&image));
        button.setImagePosition(if title.is_empty() {
            NSCellImagePosition::ImageOnly
        } else {
            NSCellImagePosition::ImageLeading
        });
    }
    return button;
}

pub fn secondary(button: &NSButton) {
    button.setBordered(true);
    button.setBezelStyle(NSBezelStyle::Glass);
    button.setBorderShape(NSControlBorderShape::Capsule);
    button.setControlSize(NSControlSize::Regular);
    button.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    button.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
}

pub fn prominent(button: &NSButton, theme: &Theme) {
    button.setBezelColor(Some(&theme.accent_fill));
    button.setTintProminence(NSTintProminence::Primary);
    button.setContentTintColor(Some(&theme.accent_text));
    let font = button.font().unwrap_or_else(|| NSFont::systemFontOfSize(13.0));
    button.setAttributedTitle(&styled(&button.title().to_string(), &font, &theme.accent_text));
    let tint = NSImageSymbolConfiguration::configurationWithPaletteColors(&NSArray::from_slice(&[&*theme.accent_text]));
    if let Some(image) = button.image().and_then(|image| image.imageWithSymbolConfiguration(&tint)) {
        button.setImage(Some(&image));
    }
}

pub fn icon_button(mtm: MainThreadMarker, name: &str, action: Sel, target: &AnyObject) -> Retained<NSButton> {
    let image = symbol(name, 13.0).unwrap_or_default();
    let button = unsafe { NSButton::buttonWithImage_target_action(&image, Some(target), Some(action), mtm) };
    button.setBordered(false);
    button.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
    button.widthAnchor().constraintEqualToConstant(22.0).setActive(true);
    return button;
}

pub fn set_icon(button: &NSButton, name: &str) {
    if let Some(image) = symbol(name, 13.0) {
        button.setImage(Some(&image));
    }
}
