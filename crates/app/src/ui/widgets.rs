use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ProtocolObject, Sel};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSAppearance, NSAppearanceCustomization,
    NSAppearanceNameDarkAqua, NSApplication, NSBezelStyle, NSBox, NSBoxType, NSButton, NSCellImagePosition, NSColor,
    NSControl, NSControlBorderShape, NSControlSize, NSControlStateValueMixed, NSControlTextEditingDelegate,
    NSEventModifierFlags, NSFocusRingType, NSFont, NSFontAttributeName, NSFontWeight, NSFontWeightMedium,
    NSFontWeightRegular, NSFontWeightSemibold, NSForegroundColorAttributeName, NSGlassEffectView, NSImage,
    NSImageSymbolConfiguration, NSImageView, NSLayoutConstraintOrientation, NSLineBreakMode, NSMenuItem, NSStackView,
    NSStackViewDistribution, NSTextAlignment, NSTextField, NSTextFieldDelegate, NSTextView, NSTintProminence,
    NSTitlePosition, NSUserInterfaceLayoutOrientation, NSView,
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
            view.setString(&ns(&(ivars.format)(value + direction * ivars.step)));
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
        this.setFont(Some(&digits()));
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
        self.setStringValue(&ns(&(self.ivars().format)(value)));
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

/// Interface text: looked up in the active language's table, so every literal passed here is translated.
pub fn ns(text: &str) -> Retained<NSString> {
    return NSString::from_str(&crate::i18n::tr(text));
}

/// User content (titles, folder names, paths, queries) that must never be translated.
pub fn raw(text: &str) -> Retained<NSString> {
    return NSString::from_str(text);
}

pub fn font(size: CGFloat, weight: &Weight) -> Retained<NSFont> {
    return NSFont::systemFontOfSize_weight(size, weight.value());
}

pub fn digits() -> Retained<NSFont> {
    return NSFont::monospacedDigitSystemFontOfSize_weight(11.0, unsafe { NSFontWeightRegular });
}

pub fn label(
    mtm: MainThreadMarker,
    text: &str,
    size: CGFloat,
    weight: &Weight,
    color: &NSColor,
) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&ns(text), mtm);
    field.setFont(Some(&font(size, weight)));
    field.setTextColor(Some(color));
    field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    field.setMaximumNumberOfLines(1);
    field.setContentCompressionResistancePriority_forOrientation(250.0, NSLayoutConstraintOrientation::Horizontal);
    return field;
}

pub fn raw_label(
    mtm: MainThreadMarker,
    text: &str,
    size: CGFloat,
    weight: &Weight,
    color: &NSColor,
) -> Retained<NSTextField> {
    let field = label(mtm, "", size, weight, color);
    field.setStringValue(&raw(text));
    return field;
}

pub fn input(mtm: MainThreadMarker, placeholder: &str, width: CGFloat) -> Retained<NSTextField> {
    let field = NSTextField::textFieldWithString(&ns(""), mtm);
    field.setPlaceholderString(Some(&ns(placeholder)));
    field.setUsesSingleLineMode(true);
    field.setLineBreakMode(NSLineBreakMode::ByClipping);
    if let Some(cell) = field.cell() {
        cell.setWraps(false);
        cell.setScrollable(true);
    }
    field.setFrameSize(NSSize::new(width, field.fittingSize().height));
    field.widthAnchor().constraintEqualToConstant(width).setActive(true);
    return field;
}

pub fn styled(text: &str, font: &NSFont, color: &NSColor) -> Retained<NSAttributedString> {
    let keys: [&NSAttributedStringKey; 2] = unsafe { [NSFontAttributeName, NSForegroundColorAttributeName] };
    let values: [&AnyObject; 2] = [font, color];
    let attributes = NSDictionary::from_slices(&keys, &values);
    return unsafe {
        NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &ns(text), Some(&attributes))
    };
}

pub fn symbol(name: &str, size: CGFloat) -> Option<Retained<NSImage>> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&raw(name), None)?;
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

const ROW_NAME_WIDTH: CGFloat = 92.0;

pub fn row(mtm: MainThreadMarker, title: &str, control: &NSView) -> Retained<NSStackView> {
    let name = label(mtm, title, 12.0, &Weight::Regular, &NSColor::secondaryLabelColor());
    name.widthAnchor().constraintEqualToConstant(ROW_NAME_WIDTH).setActive(true);
    let gap = spacer(mtm);
    let row = stack(mtm, &[&name, &gap, control], false, 8.0);
    row.setDistribution(NSStackViewDistribution::Fill);
    row.heightAnchor().constraintEqualToConstant(26.0).setActive(true);
    return row;
}

pub fn section(mtm: MainThreadMarker, title: &str, rows: &[&NSView]) -> Retained<NSStackView> {
    let header =
        raw_label(mtm, &crate::i18n::tr(title).to_uppercase(), 11.0, &Weight::Semibold, &NSColor::tertiaryLabelColor());
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

/// Liquid Glass (macOS 26 and newer): `NSGlassEffectView`, the glass bezel, border shapes and tint prominence.
pub fn liquid_glass() -> bool {
    return AnyClass::get(c"NSGlassEffectView").is_some();
}

/// Glass bezel with a capsule border where the system has it, the regular rounded bezel before macOS 26.
pub fn glass_bezel(button: &NSButton) {
    if !liquid_glass() {
        button.setBezelStyle(NSBezelStyle::Push);
        return;
    }
    button.setBezelStyle(NSBezelStyle::Glass);
    if button.respondsToSelector(sel!(setBorderShape:)) {
        button.setBorderShape(NSControlBorderShape::Capsule);
    }
}

pub fn glass(mtm: MainThreadMarker, content: &NSView, radius: CGFloat, tint: Option<&NSColor>) -> Retained<NSView> {
    if liquid_glass() {
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
    view.setToolTip(Some(&ns(&marker.tip)));
    return view;
}

pub fn button(
    mtm: MainThreadMarker,
    title: &str,
    icon: Option<&str>,
    action: Sel,
    target: &AnyObject,
) -> Retained<NSButton> {
    let button = unsafe { NSButton::buttonWithTitle_target_action(&ns(title), Some(target), Some(action), mtm) };
    glass_bezel(&button);
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
    glass_bezel(button);
    button.setControlSize(NSControlSize::Regular);
    button.setFont(Some(&font(12.0, &Weight::Regular)));
    button.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
}

/// The accent-filled main action. The title and symbol keep the system colours: with Liquid Glass an inactive window
/// drops the tint, and a hard-coded light-on-accent (or dark-on-accent) title turned unreadable on the clear glass.
pub fn prominent(button: &NSButton, theme: &Theme) {
    button.setBezelColor(Some(&theme.accent_fill));
    if liquid_glass() && button.respondsToSelector(sel!(setTintProminence:)) {
        button.setTintProminence(NSTintProminence::Primary);
        return;
    }
    button.setContentTintColor(Some(&theme.accent_text));
    let current = button.font().unwrap_or_else(|| font(13.0, &Weight::Regular));
    button.setAttributedTitle(&styled(&button.title().to_string(), &current, &theme.accent_text));
    let tint = NSImageSymbolConfiguration::configurationWithPaletteColors(&NSArray::from_slice(&[&*theme.accent_text]));
    if let Some(image) = button.image().and_then(|image| image.imageWithSymbolConfiguration(&tint)) {
        button.setImage(Some(&image));
    }
}

pub fn compact(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<Sel>,
    target: Option<&AnyObject>,
) -> Retained<NSButton> {
    let button = unsafe { NSButton::buttonWithTitle_target_action(&ns(title), target, action, mtm) };
    button.setControlSize(NSControlSize::Small);
    return button;
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

pub fn lock_mark(item: &NSMenuItem, icon: Option<&str>, checked: bool) {
    item.setState(isize::from(checked));
    let Some(image) = icon.and_then(|name| symbol(name, 12.0)) else {
        return;
    };
    unsafe {
        item.setOnStateImage(Some(&image));
        item.setMixedStateImage(Some(&image));
    }
    if !checked {
        item.setState(NSControlStateValueMixed);
    }
}

pub fn menu_item(
    mtm: MainThreadMarker,
    title: &str,
    action: Sel,
    target: Option<&AnyObject>,
    tag: isize,
    shortcut: Option<(&str, NSEventModifierFlags)>,
) -> Retained<NSMenuItem> {
    let key = shortcut.map_or_else(NSString::new, |(key, _)| ns(key));
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &ns(title), Some(action), &key)
    };
    if let Some((_, modifiers)) = shortcut {
        item.setKeyEquivalentModifierMask(modifiers);
    }
    unsafe { item.setTarget(target) };
    item.setTag(tag);
    return item;
}
