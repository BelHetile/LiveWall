use std::cell::{Cell, OnceCell, RefCell};

use block2::StackBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject, Sel};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAppearanceCustomization, NSBezelStyle, NSBox, NSBoxType, NSButton, NSCellImagePosition, NSColor,
    NSControl, NSControlBorderShape, NSControlSize, NSControlTextEditingDelegate, NSEvent, NSEventModifierFlags,
    NSFocusRingType, NSFont, NSFontAttributeName, NSFontWeightMedium, NSForegroundColorAttributeName,
    NSGlassEffectView, NSImage, NSImageSymbolConfiguration, NSImageView, NSLayoutConstraintOrientation,
    NSLineBreakMode, NSMenuItem, NSPopUpButton, NSSegmentDistribution, NSSegmentSwitchTracking, NSSegmentedControl,
    NSSlider, NSStackView, NSStackViewDistribution, NSTableCellView, NSTextAlignment, NSTextField,
    NSTextFieldDelegate, NSTextView, NSTintProminence, NSTitlePosition, NSUserInterfaceLayoutOrientation, NSView,
    NSViewLayerContentsRedrawPolicy,
};
use objc2_core_foundation::{CGFloat, CGRect};
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSDictionary, NSEdgeInsets, NSNotification, NSObjectProtocol,
    NSSize, NSString,
};
use objc2_quartz_core::CATransaction;

use crate::style::Key;
use crate::ui::controls::{self, ControlView};
use crate::ui::metrics::{self, Surface, Text};
use crate::ui::state::Marker;
use crate::ui::theme::{self, Theme};

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

pub fn describe(view: &NSView, tip: &str, name: &str) {
    view.setToolTip(Some(&ns(tip)));
    view.setAccessibilityLabel(Some(&ns(name)));
}

pub struct FieldIvars {
    units: Units,
    edge: OnceCell<controls::Edge>,
    control: controls::Control,
    active: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSTextField))]
    #[thread_kind = MainThreadOnly]
    #[ivars = FieldIvars]
    pub struct NumberField;

    unsafe impl NSObjectProtocol for NumberField {}

    impl NumberField {
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            controls::entered(self);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            controls::exited(self);
        }

        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            let taken: Bool = unsafe { msg_send![super(self), becomeFirstResponder] };
            if taken.as_bool() {
                self.ivars().active.set(true);
                self.glow(false);
            }
            taken.as_bool()
        }
    }

    unsafe impl NSControlTextEditingDelegate for NumberField {
        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn command(&self, _control: &NSControl, view: &NSTextView, command: Sel) -> bool {
            self.step(view, command)
        }

        #[unsafe(method(controlTextDidEndEditing:))]
        fn text_did_end(&self, _notification: &NSNotification) {
            self.ivars().active.set(false);
            self.glow(true);
        }
    }

    unsafe impl NSTextFieldDelegate for NumberField {}
);

impl ControlView for NumberField {
    fn view(&self) -> &NSView {
        self
    }

    fn control(&self) -> &controls::Control {
        &self.ivars().control
    }

    fn apply(&self, immediate: bool) {
        self.glow(!immediate);
    }

    fn activate(&self) {}
}

impl NumberField {
    fn glow(&self, animated: bool) {
        let ivars = self.ivars();
        let Some(edge) = ivars.edge.get() else {
            return;
        };
        let level = if ivars.active.get() {
            1.0
        } else if ivars.control.hovered() && self.isEnabled() {
            metrics::EDGE_HOVER
        } else {
            0.0
        };
        edge.show(self, level, animated);
    }

    fn step(&self, view: &NSTextView, command: Sel) -> bool {
        let direction = if command == sel!(moveUp:) {
            1.0
        } else if command == sel!(moveDown:) {
            -1.0
        } else {
            return false;
        };
        let units = &self.ivars().units;
        if let Some(value) = (units.parse)(&view.string().to_string()) {
            view.setString(&ns(&(units.format)(value + direction * units.step)));
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
        let this = Self::alloc(mtm).set_ivars(FieldIvars {
            units,
            edge: OnceCell::new(),
            control: controls::Control::new(),
            active: Cell::new(false),
        });
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
        if let Some(edge) = controls::Edge::mount(&this, metrics::RADIUS_CONTROL, theme.color(Key::Selection)) {
            let _ = this.ivars().edge.set(edge);
        }
        controls::track(&this, &this);
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
        return (self.ivars().units.parse)(&self.stringValue().to_string());
    }

    pub fn show(&self, value: f64) {
        self.setStringValue(&ns(&(self.ivars().units.format)(value)));
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconRole {
    Toolbar,
    Inline,
    Small,
    Badge,
    Tab,
    Hero,
}

impl IconRole {
    fn size(self) -> CGFloat {
        return match self {
            Self::Toolbar => metrics::ICON_TOOLBAR,
            Self::Inline => metrics::ICON_REGULAR,
            Self::Small => metrics::ICON_SMALL,
            Self::Badge => metrics::ICON_BADGE,
            Self::Tab => metrics::ICON_LARGE,
            Self::Hero => metrics::ICON_HERO,
        };
    }
}

pub fn icon(name: &str, role: IconRole) -> Option<Retained<NSImage>> {
    return symbol(name, role.size());
}

pub fn symbol(name: &str, size: CGFloat) -> Option<Retained<NSImage>> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(name), None)?;
    let config = NSImageSymbolConfiguration::configurationWithPointSize_weight(size, unsafe { NSFontWeightMedium });
    return image.imageWithSymbolConfiguration(&config);
}

pub fn symbol_view(mtm: MainThreadMarker, name: &str, role: IconRole, tint: &NSColor) -> Retained<NSView> {
    let Some(image) = icon(name, role) else {
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

pub fn alpha(view: &NSView, value: CGFloat) {
    if same(view.alphaValue(), value) {
        return;
    }
    view.setAlphaValue(value);
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

pub fn divider(mtm: MainThreadMarker, key: Key, theme: &Theme) -> Retained<NSBox> {
    let line = card(mtm, &theme.color(key), 0.0);
    theme::register(&line, key, Slot::Fill);
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

pub fn resolving(view: &NSView, body: impl Fn()) {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    view.effectiveAppearance().performAsCurrentDrawingAppearance(&StackBlock::new(&body));
    CATransaction::commit();
}

fn concrete(view: &NSView, color: &NSColor) -> Retained<NSColor> {
    let out: RefCell<Option<Retained<NSColor>>> = RefCell::new(None);
    resolving(view, || {
        *out.borrow_mut() = theme::rgba(color).map(theme::ns_color);
    });
    return out.into_inner().unwrap_or_else(|| Retained::from(color));
}

pub struct SwatchIvars {
    fill: RefCell<Option<Retained<NSColor>>>,
    border: Option<Retained<NSColor>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = SwatchIvars]
    pub struct Swatch;

    impl Swatch {
        #[unsafe(method(wantsUpdateLayer))]
        fn wants_update_layer(&self) -> bool {
            true
        }

        #[unsafe(method(updateLayer))]
        fn update_layer(&self) {
            self.colorize();
        }
    }
);

impl Swatch {
    pub fn new(
        mtm: MainThreadMarker,
        radius: CGFloat,
        fill: Option<&NSColor>,
        border: Option<(CGFloat, &NSColor)>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SwatchIvars {
            fill: RefCell::new(fill.map(Retained::from)),
            border: border.map(|(_, color)| Retained::from(color)),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setWantsLayer(true);
        this.setLayerContentsRedrawPolicy(NSViewLayerContentsRedrawPolicy::OnSetNeedsDisplay);
        if let Some(layer) = this.layer() {
            layer.setCornerRadius(radius);
            layer.setMasksToBounds(true);
            if let Some((width, _)) = border {
                layer.setBorderWidth(width);
            }
        }
        this.setNeedsDisplay(true);
        return this;
    }

    fn colorize(&self) {
        let Some(layer) = self.layer() else {
            return;
        };
        let ivars = self.ivars();
        resolving(self, || {
            let fill = ivars.fill.borrow().as_ref().map(|color| color.CGColor());
            layer.setBackgroundColor(fill.as_deref());
            let border = ivars.border.as_ref().map(|color| color.CGColor());
            layer.setBorderColor(border.as_deref());
        });
    }
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
    let ink_key = if marker.active { Key::OnAccent } else { Key::ChipText };
    let ink = theme.color(ink_key);
    let glyph = symbol_view(mtm, marker.icon, IconRole::Small, &ink);
    theme::register(&glyph, ink_key, Slot::Tint);
    let text = label(mtm, &marker.text, Text::Section, &ink);
    text.setContentCompressionResistancePriority_forOrientation(750.0, NSLayoutConstraintOrientation::Horizontal);
    theme::register(&text, ink_key, Slot::Text);
    let content = stack(mtm, &[&glyph, &text], false, metrics::SPACE_4);
    content.setEdgeInsets(insets(0.0, metrics::SPACE_8, 0.0, metrics::SPACE_8));
    let height = metrics::CONTROL_HEIGHT - metrics::SPACE_4;
    content.heightAnchor().constraintEqualToConstant(height).setActive(true);
    let fill = if marker.active { Key::ChipActive } else { Key::ChipFill };
    let tint = marker.active.then_some(Key::ChipActive);
    let surface = Surface::mount(mtm, &content, metrics::capsule(height), tint, fill, theme);
    let view: Retained<NSView> = Retained::from(surface.view());
    view.setToolTip(Some(&ns(&marker.tip)));
    return view;
}

fn shell(button: &NSButton) {
    button.setBezelStyle(NSBezelStyle::Glass);
    button.setBorderShape(NSControlBorderShape::Capsule);
    button.setControlSize(NSControlSize::Large);
    button.setFont(Some(&metrics::font(Text::Callout)));
    button.heightAnchor().constraintEqualToConstant(metrics::BUTTON_HEIGHT).setActive(true);
}

fn make(
    mtm: MainThreadMarker,
    title: &str,
    glyph: Option<(&str, IconRole)>,
    action: Sel,
    target: &AnyObject,
) -> Retained<NSButton> {
    let button = unsafe { NSButton::buttonWithTitle_target_action(&ns(title), Some(target), Some(action), mtm) };
    shell(&button);
    if let Some((name, role)) = glyph
        && let Some(image) = icon(name, role)
    {
        button.setImage(Some(&image));
        button.setImagePosition(if title.is_empty() {
            NSCellImagePosition::ImageOnly
        } else {
            NSCellImagePosition::ImageLeading
        });
    }
    return button;
}

pub fn primary(
    mtm: MainThreadMarker,
    title: &str,
    glyph: Option<(&str, IconRole)>,
    action: Sel,
    target: &AnyObject,
    theme: &Theme,
) -> Retained<NSButton> {
    let button = make(mtm, title, glyph, action, target);
    button.setTintProminence(NSTintProminence::Primary);
    paint(&button, Key::OnAccent, Slot::Prominent, theme);
    theme::register(&button, Key::OnAccent, Slot::Prominent);
    return button;
}

pub fn secondary(
    mtm: MainThreadMarker,
    title: &str,
    glyph: Option<(&str, IconRole)>,
    action: Sel,
    target: &AnyObject,
    theme: &Theme,
) -> Retained<NSButton> {
    let button = make(mtm, title, glyph, action, target);
    quiet(&button, Key::TextSecondary, theme);
    return button;
}

pub fn quiet(button: &NSButton, key: Key, theme: &Theme) {
    let color = theme.color(key);
    let font = button.font().unwrap_or_else(|| metrics::font(Text::Callout));
    theme::forget(button);
    button.setAttributedTitle(&styled(&button.title().to_string(), &font, &color));
    button.setContentTintColor(Some(&color));
    theme::register(button, key, Slot::Title);
}

pub fn popup(mtm: MainThreadMarker, pulls_down: bool, theme: &Theme) -> Retained<NSPopUpButton> {
    let popup = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), CGRect::ZERO, pulls_down);
    popup.setBezelStyle(NSBezelStyle::Glass);
    popup.setBorderShape(NSControlBorderShape::Capsule);
    popup.setControlSize(NSControlSize::Large);
    popup.setFont(Some(&metrics::font(Text::Callout)));
    popup.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    popup.setContentTintColor(Some(&theme.color(Key::TextSecondary)));
    theme::register(&popup, Key::TextSecondary, Slot::Tint);
    popup.heightAnchor().constraintEqualToConstant(metrics::BUTTON_HEIGHT).setActive(true);
    return popup;
}

pub fn segmented(
    mtm: MainThreadMarker,
    labels: &[&str],
    action: Sel,
    target: &AnyObject,
    key: Key,
    theme: &Theme,
) -> Retained<NSSegmentedControl> {
    let strings: Vec<Retained<NSString>> = labels.iter().map(|label| ns(label)).collect();
    let control = unsafe {
        NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
            &NSArray::from_retained_slice(&strings),
            NSSegmentSwitchTracking::SelectOne,
            Some(target),
            Some(action),
            mtm,
        )
    };
    control.setBorderShape(NSControlBorderShape::Capsule);
    control.setControlSize(NSControlSize::Large);
    control.setSegmentDistribution(NSSegmentDistribution::FillEqually);
    control.setSelectedSegmentBezelColor(Some(&theme.color(key)));
    theme::register(&control, key, Slot::Bezel);
    control.heightAnchor().constraintEqualToConstant(metrics::BUTTON_HEIGHT).setActive(true);
    return control;
}

pub fn retint(view: &NSView, key: Key, theme: &Theme) {
    theme::forget(view);
    theme::register(view, key, Slot::Tint);
    paint(view, key, Slot::Tint, theme);
}

fn retitle(button: &NSButton, color: &NSColor) {
    let font = button.font().unwrap_or_else(|| metrics::font(Text::Body));
    button.setAttributedTitle(&styled(&button.title().to_string(), &font, color));
    button.setContentTintColor(Some(color));
}

pub fn compact(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<Sel>,
    target: Option<&AnyObject>,
) -> Retained<NSButton> {
    let button = unsafe { NSButton::buttonWithTitle_target_action(&ns(title), target, action, mtm) };
    shell(&button);
    if let Some(theme) = Theme::current() {
        quiet(&button, Key::TextSecondary, &theme);
    }
    return button;
}

pub fn icon_button(
    mtm: MainThreadMarker,
    name: &str,
    action: Sel,
    target: &AnyObject,
    key: Key,
    theme: &Theme,
) -> Retained<NSButton> {
    let image = icon(name, IconRole::Toolbar).unwrap_or_default();
    let button = unsafe { NSButton::buttonWithImage_target_action(&image, Some(target), Some(action), mtm) };
    hoverable(&button);
    button.setContentTintColor(Some(&theme.color(key)));
    theme::register(&button, key, Slot::Tint);
    button.widthAnchor().constraintEqualToConstant(metrics::BUTTON_HEIGHT).setActive(true);
    button.heightAnchor().constraintEqualToConstant(metrics::BUTTON_HEIGHT).setActive(true);
    return button;
}

pub fn hoverable(button: &NSButton) {
    button.setBordered(true);
    button.setBezelStyle(NSBezelStyle::Glass);
    button.setBorderShape(NSControlBorderShape::Capsule);
    button.setControlSize(NSControlSize::Large);
    button.setShowsBorderOnlyWhileMouseInside(true);
}

pub fn set_icon(button: &NSButton, name: &str) {
    if let Some(image) = icon(name, IconRole::Toolbar) {
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
    Glass,
    Opaque,
    Title,
    Prominent,
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
        theme::register(view, key, slot);
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
            paint(view, *key, *slot, theme);
        }
    }
}

pub fn paint(view: &NSView, key: Key, slot: Slot, theme: &Theme) {
    let color = theme.color(key);
    match slot {
        Slot::Text => {
            if let Some(field) = view.downcast_ref::<NSTextField>() {
                field.setTextColor(Some(&color));
            }
        }
        Slot::Fill => {
            if let Some(card) = view.downcast_ref::<NSBox>() {
                card.setFillColor(&color);
            }
        }
        Slot::Tint => {
            if let Some(image) = view.downcast_ref::<NSImageView>() {
                image.setContentTintColor(Some(&color));
            } else if let Some(button) = view.downcast_ref::<NSButton>() {
                button.setContentTintColor(Some(&color));
            } else if let Some(cell) = view.downcast_ref::<NSTableCellView>() {
                cell.setBackgroundStyle(cell.backgroundStyle());
            }
        }
        Slot::Track => {
            if let Some(slider) = view.downcast_ref::<NSSlider>() {
                slider.setTrackFillColor(Some(&color));
            }
        }
        Slot::Bezel => {
            if let Some(control) = view.downcast_ref::<NSSegmentedControl>() {
                control.setSelectedSegmentBezelColor(Some(&color));
            }
        }
        Slot::Glass => {
            if let Some(glass) = view.downcast_ref::<NSGlassEffectView>() {
                glass.setTintColor(Some(&color));
            }
        }
        Slot::Opaque => {
            if let Some(card) = view.downcast_ref::<NSBox>() {
                card.setFillColor(&color.colorWithAlphaComponent(1.0));
            }
        }
        Slot::Title => {
            if let Some(button) = view.downcast_ref::<NSButton>() {
                retitle(button, &concrete(view, &color));
            }
        }
        Slot::Prominent => {
            if let Some(button) = view.downcast_ref::<NSButton>() {
                let ink = concrete(view, &color);
                button.setBezelColor(Some(&theme.accent()));
                retitle(button, &ink);
                let inks: [&NSColor; 1] = [&ink];
                let palette = NSImageSymbolConfiguration::configurationWithPaletteColors(&NSArray::from_slice(&inks));
                if let Some(image) = button.image().and_then(|image| image.imageWithSymbolConfiguration(&palette)) {
                    button.setImage(Some(&image));
                }
            }
        }
    }
}
