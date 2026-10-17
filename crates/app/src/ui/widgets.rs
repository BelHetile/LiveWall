use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSAppearanceCustomization, NSBezelStyle, NSBox, NSBoxType,
    NSButton, NSCellImagePosition, NSColor, NSControl, NSControlBorderShape, NSControlSize,
    NSControlTextEditingDelegate, NSEventModifierFlags, NSFocusRingType, NSFont, NSFontAttributeName,
    NSFontWeightMedium, NSForegroundColorAttributeName, NSImage, NSImageSymbolConfiguration, NSImageView,
    NSLayoutConstraintOrientation, NSLineBreakMode, NSMenuItem, NSSegmentedControl, NSSlider, NSStackView,
    NSStackViewDistribution, NSTextAlignment, NSTextField, NSTextFieldDelegate, NSTextView, NSTintProminence,
    NSTitlePosition, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_core_graphics::CGColor;
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSDictionary, NSEdgeInsets, NSObjectProtocol, NSSize, NSString,
};
use objc2_quartz_core::{CAMediaTiming, CATransition, kCATransitionFade};

use crate::style::Key;
use crate::ui::metrics::{self, Surface, Text};
use crate::ui::state::Marker;
use crate::ui::theme::Theme;

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
        theme: &Theme,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(units);
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        unsafe { this.setDelegate(Some(ProtocolObject::from_ref(&*this))) };
        this.setFont(Some(&metrics::digits(Text::Subheadline)));
        this.setAlignment(NSTextAlignment::Right);
        this.setBezeled(false);
        this.setBordered(false);
        this.setDrawsBackground(true);
        this.setFocusRingType(NSFocusRingType::None);
        this.setWantsLayer(true);
        if let Some(layer) = this.layer() {
            layer.setCornerRadius(metrics::RADIUS_CONTROL);
            layer.setMasksToBounds(true);
        }
        this.paint(theme);
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

    pub fn paint(&self, theme: &Theme) {
        self.setBackgroundColor(Some(&theme.color(Key::FieldFill)));
        self.setTextColor(Some(&theme.color(Key::FieldText)));
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

pub fn ns(text: &str) -> Retained<NSString> {
    return NSString::from_str(text);
}

pub fn label(mtm: MainThreadMarker, text: &str, role: Text, color: &NSColor) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&ns(text), mtm);
    field.setFont(Some(&metrics::font(role)));
    field.setTextColor(Some(color));
    field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    field.setMaximumNumberOfLines(1);
    field.setContentCompressionResistancePriority_forOrientation(250.0, NSLayoutConstraintOrientation::Horizontal);
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
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(name), None)?;
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
        context.setTimingFunction(Some(&metrics::ease_in_out()));
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
    transition.setDuration(metrics::MOTION_STANDARD);
    transition.setTimingFunction(Some(&metrics::ease_out()));
    layer.addAnimation_forKey(&transition, None);
}

pub fn alpha(view: &NSView, value: CGFloat) {
    if same(view.alphaValue(), value) {
        return;
    }
    animate(metrics::MOTION_STANDARD, || view.animator().setAlphaValue(value));
}

pub fn reveal(view: &NSView, visible: bool) {
    let value = if visible { 1.0 } else { 0.0 };
    if view.isHidden() != visible && same(view.alphaValue(), value) {
        return;
    }
    animate(metrics::MOTION_STANDARD, || {
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

pub fn row(mtm: MainThreadMarker, title: &str, control: &NSView, theme: &Theme) -> Retained<NSStackView> {
    let name = label(mtm, title, Text::Callout, &theme.color(Key::TextSecondary));
    name.widthAnchor().constraintEqualToConstant(72.0).setActive(true);
    let gap = spacer(mtm);
    let row = stack(mtm, &[&name, &gap, control], false, metrics::SPACE_8);
    row.setDistribution(NSStackViewDistribution::Fill);
    row.heightAnchor().constraintEqualToConstant(metrics::ROW_LIST).setActive(true);
    return row;
}

pub fn header(mtm: MainThreadMarker, title: &str, theme: &Theme) -> Retained<NSTextField> {
    return label(mtm, title, Text::Section, &theme.color(Key::TextSecondary));
}

pub fn section(mtm: MainThreadMarker, title: &str, rows: &[&NSView], theme: &Theme) -> Retained<NSStackView> {
    let header = header(mtm, title, theme);
    let mut views: Vec<&NSView> = vec![&header];
    views.extend_from_slice(rows);
    let section = stack(mtm, &views, true, metrics::SPACE_2);
    section.setAlignment(objc2_app_kit::NSLayoutAttribute::Leading);
    section.setCustomSpacing_afterView(metrics::SPACE_4, &header);
    for view in rows {
        view.leadingAnchor().constraintEqualToAnchor(&section.leadingAnchor()).setActive(true);
        view.trailingAnchor().constraintEqualToAnchor(&section.trailingAnchor()).setActive(true);
    }
    return section;
}

pub fn divider(mtm: MainThreadMarker, color: &NSColor) -> Retained<NSBox> {
    let line = card(mtm, color, 0.0);
    line.heightAnchor().constraintEqualToConstant(metrics::BORDER_HAIRLINE).setActive(true);
    return line;
}

pub fn pin(view: &NSView, to: &NSView, inset: CGFloat) {
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    view.leadingAnchor().constraintEqualToAnchor_constant(&to.leadingAnchor(), inset).setActive(true);
    view.trailingAnchor().constraintEqualToAnchor_constant(&to.trailingAnchor(), -inset).setActive(true);
    view.topAnchor().constraintEqualToAnchor_constant(&to.topAnchor(), inset).setActive(true);
    view.bottomAnchor().constraintEqualToAnchor_constant(&to.bottomAnchor(), -inset).setActive(true);
}

pub fn cg(view: &NSView, color: &NSColor) -> Retained<CGColor> {
    let appearance = view.effectiveAppearance();
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

pub fn chip(mtm: MainThreadMarker, marker: &Marker, theme: &Theme) -> Retained<NSView> {
    let ink = theme.color(Key::ChipText);
    let glyph = symbol_view(mtm, marker.icon, metrics::ICON_SMALL - 1.0, &ink);
    let text = label(mtm, &marker.text, Text::Section, &ink);
    text.setContentCompressionResistancePriority_forOrientation(750.0, NSLayoutConstraintOrientation::Horizontal);
    let content = stack(mtm, &[&glyph, &text], false, metrics::SPACE_4);
    content.setEdgeInsets(insets(0.0, metrics::SPACE_8, 0.0, metrics::SPACE_8));
    let height = metrics::CONTROL_HEIGHT - metrics::SPACE_4;
    content.heightAnchor().constraintEqualToConstant(height).setActive(true);
    let fill = theme.color(if marker.active { Key::ChipActive } else { Key::ChipFill });
    let surface = Surface::mount(mtm, &content, metrics::capsule(height), Some(&fill), &fill);
    let view: Retained<NSView> = Retained::from(surface.view());
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
    button.setBezelStyle(NSBezelStyle::Glass);
    button.setBorderShape(NSControlBorderShape::Capsule);
    button.setControlSize(NSControlSize::Large);
    if let Some(image) = icon.and_then(|name| symbol(name, metrics::ICON_REGULAR)) {
        button.setImage(Some(&image));
        button.setImagePosition(if title.is_empty() {
            NSCellImagePosition::ImageOnly
        } else {
            NSCellImagePosition::ImageLeading
        });
    }
    return button;
}

pub fn secondary(button: &NSButton, theme: &Theme) {
    button.setBordered(true);
    button.setBezelStyle(NSBezelStyle::Glass);
    button.setBorderShape(NSControlBorderShape::Capsule);
    button.setControlSize(NSControlSize::Regular);
    button.setFont(Some(&metrics::font(Text::Callout)));
    button.setContentTintColor(Some(&theme.color(Key::TextSecondary)));
}

pub fn quiet(button: &NSButton, title: &str, color: &NSColor) {
    button.setAttributedTitle(&styled(title, &metrics::font(Text::Callout), color));
    button.setContentTintColor(Some(color));
}

pub fn prominent(button: &NSButton, theme: &Theme) {
    let ink = theme.color(Key::OnAccent);
    button.setBezelColor(Some(&theme.accent()));
    button.setTintProminence(NSTintProminence::Primary);
    button.setContentTintColor(Some(&ink));
    let current = button.font().unwrap_or_else(|| metrics::font(Text::Body));
    button.setAttributedTitle(&styled(&button.title().to_string(), &current, &ink));
    let tint = NSImageSymbolConfiguration::configurationWithPaletteColors(&NSArray::from_slice(&[&*ink]));
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

pub fn icon_button(
    mtm: MainThreadMarker,
    name: &str,
    action: Sel,
    target: &AnyObject,
    tint: &NSColor,
) -> Retained<NSButton> {
    let image = symbol(name, metrics::ICON_REGULAR).unwrap_or_default();
    let button = unsafe { NSButton::buttonWithImage_target_action(&image, Some(target), Some(action), mtm) };
    button.setBordered(false);
    button.setContentTintColor(Some(tint));
    button.widthAnchor().constraintEqualToConstant(metrics::CONTROL_HEIGHT - metrics::SPACE_4).setActive(true);
    return button;
}

pub fn set_icon(button: &NSButton, name: &str) {
    if let Some(image) = symbol(name, metrics::ICON_REGULAR) {
        button.setImage(Some(&image));
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Text,
    Fill,
    Tint,
    Track,
    Bezel,
}

#[derive(Default)]
pub struct Paint {
    entries: Vec<(Retained<NSView>, Key, Slot)>,
}

impl Paint {
    pub fn new() -> Self {
        return Self::default();
    }

    pub fn add(&mut self, view: &NSView, key: Key, slot: Slot) {
        self.entries.push((Retained::from(view), key, slot));
    }

    pub fn label(
        &mut self,
        mtm: MainThreadMarker,
        text: &str,
        role: Text,
        key: Key,
        theme: &Theme,
    ) -> Retained<NSTextField> {
        let field = label(mtm, text, role, &theme.color(key));
        self.add(&field, key, Slot::Text);
        return field;
    }

    pub fn fill(&mut self, mtm: MainThreadMarker, key: Key, radius: CGFloat, theme: &Theme) -> Retained<NSBox> {
        let card = card(mtm, &theme.color(key), radius);
        self.add(&card, key, Slot::Fill);
        return card;
    }

    pub fn apply(&self, theme: &Theme) {
        for (view, key, slot) in &self.entries {
            paint(view, &theme.color(*key), *slot);
        }
    }
}

pub fn paint(view: &NSView, color: &NSColor, slot: Slot) {
    match slot {
        Slot::Text => {
            if let Some(field) = view.downcast_ref::<NSTextField>() {
                field.setTextColor(Some(color));
            }
        }
        Slot::Fill => {
            if let Some(card) = view.downcast_ref::<NSBox>() {
                card.setFillColor(color);
            }
        }
        Slot::Tint => {
            if let Some(image) = view.downcast_ref::<NSImageView>() {
                image.setContentTintColor(Some(color));
            } else if let Some(button) = view.downcast_ref::<NSButton>() {
                button.setContentTintColor(Some(color));
            }
        }
        Slot::Track => {
            if let Some(slider) = view.downcast_ref::<NSSlider>() {
                slider.setTrackFillColor(Some(color));
            }
        }
        Slot::Bezel => {
            if let Some(control) = view.downcast_ref::<NSSegmentedControl>() {
                control.setSelectedSegmentBezelColor(Some(color));
            }
        }
    }
}
