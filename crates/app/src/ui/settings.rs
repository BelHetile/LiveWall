use std::cell::{Cell, OnceCell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, Sel};
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityButtonRole, NSAccessibilityRadioButtonRole, NSAccessibilityRadioGroupRole,
    NSApplication, NSBezelStyle, NSBezierPath, NSButton, NSButtonType, NSColor, NSControl, NSControlSize,
    NSControlStateValueOff, NSControlStateValueOn, NSEvent, NSEventModifierFlags, NSImageView, NSLayoutAttribute,
    NSLayoutConstraint, NSPopUpButton, NSScrollView, NSSegmentedControl, NSSlider,
    NSStackView, NSTextAlignment, NSTextField, NSTextView, NSView, NSViewController, NSViewLayerContentsRedrawPolicy,
    NSWindow, NSWindowButton, NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect};
use objc2_foundation::{NSArray, NSString};
use objc2_quartz_core::CATransaction;
use proto::HotkeyAction;
use serde_json::{Value, json};

use crate::hotkey;
use crate::i18n::{self, Arg, tr, trf};
use crate::style::{self, ACCENT, Accent, Group, Key, Pair, Rgba, Style, SystemColor, TINT, TINT_STRENGTH, Variant};
use crate::ui::controls::{self, ControlView, Edge, Switch};
use crate::ui::metrics::{self, Text};
use crate::ui::swatches::{Dot, RING_PAD};
use crate::ui::theme::{self, Look, Theme};
use crate::ui::widgets::{self, IconRole, NumberField, Paint, Slot, Units, resolving};
use crate::ui::{actions, pick_style};

const WIDTH: CGFloat = 580.0;
const ESCAPE: u32 = 53;
const DELETE: u32 = 51;
const FORWARD_DELETE: u32 = 117;
pub const ROTATE: isize = 1;
pub const TRANSITION: isize = 2;
pub const PAUSE: isize = 3;
const CLOSE_INSET: CGFloat = metrics::SPACE_16 + metrics::SPACE_2;
const LOGIN: isize = 4;
const OPEN_LIBRARY: isize = 5;
const FULLSCREEN: isize = 7;
const COVERED: isize = 8;
const HOT: isize = 9;
const OPTIMIZE: isize = 10;
const SOUND: isize = 11;
const SHUFFLE: isize = 12;
const FRAME_RATE: isize = 14;
const RELEASE: isize = 15;
const STYLE: isize = 16;
const EVERY: isize = 17;
const LOCK_SCREEN: isize = 18;
const LOCK_DISPLAY: isize = 19;
const LOCK_IDLE: isize = 22;
const IDLE_AFTER: isize = 23;
const HIDE_DESKTOP: isize = 24;
const VOLUME: isize = 20;
const DURATION: isize = 21;
pub const STRENGTH: isize = 25;
const RECORDER_WIDTH: CGFloat = 128.0;
const CIRCLE: CGFloat = 26.0;
const CIRCLE_PITCH: CGFloat = CIRCLE + RING_PAD * 2.0;
const CIRCLE_COLUMNS: usize = 13;
const SWATCH: CGFloat = 22.0;
const MARKER_WIDTH: CGFloat = 32.0;
const KNOB_WIDTH: CGFloat = 150.0;
const VALUE_WIDTH: CGFloat = 62.0;
const NAV_TOP: CGFloat = metrics::SPACE_24 + metrics::SPACE_12;
const NAV_INSET: CGFloat = metrics::SPACE_20;
const NAV_GAP: CGFloat = metrics::SPACE_4;
const ITEM_HEIGHT: CGFloat = 32.0;
const ITEM_PAD: CGFloat = metrics::SPACE_8;
const BAND: CGFloat = NAV_TOP + ITEM_HEIGHT;
const ARROW_LEFT: u16 = 123;
const ARROW_RIGHT: u16 = 124;

const TABS: [(&str, &str); 5] = [
    (i18n::key("General"), "gearshape"),
    (i18n::key("Appearance"), "paintpalette"),
    (i18n::key("Playback"), "play.rectangle"),
    (i18n::key("Shortcuts"), "keyboard"),
    (i18n::key("Privacy"), "lock"),
];

const FRAME_RATES: [u32; 6] = [24, 30, 48, 60, 90, 120];
const RELEASES: [(&str, u32); 5] = [
    (i18n::key("Never"), 0),
    (i18n::key("After 15 Seconds"), 15),
    (i18n::key("After 30 Seconds"), 30),
    (i18n::key("After 1 Minute"), 60),
    (i18n::key("After 5 Minutes"), 300),
];
const STYLES: [(&str, &str); 4] = [
    (i18n::key("Fade"), "fade"),
    (i18n::key("Wipe"), "wipe"),
    (i18n::key("Grow"), "grow"),
    (i18n::key("Wave"), "wave"),
];
const INTERVALS: [(&str, u32); 5] = [
    (i18n::key("5 Minutes"), 5),
    (i18n::key("15 Minutes"), 15),
    (i18n::key("1 Hour"), 60),
    (i18n::key("Daily"), 1440),
    (i18n::key("On Login"), 0),
];
const IDLE_MINUTES: [(&str, u32); 5] = [
    (i18n::key("1 Minute"), 1),
    (i18n::key("5 Minutes"), 5),
    (i18n::key("15 Minutes"), 15),
    (i18n::key("30 Minutes"), 30),
    (i18n::key("1 Hour"), 60),
];
const PAUSE_KEYS: [&str; 4] = ["pause_when_fullscreen", "pause_when_maximized", "pause_when_hot", "pause_when_focused"];

type Knob = (Retained<NSSlider>, Retained<NumberField>);

pub enum Input {
    Flag(bool),
    Index(usize),
    Number(f64),
}

pub enum Change {
    Patch(Value),
    Login(bool),
    Style(String, String),
}

pub fn change(tag: isize, input: &Input) -> Option<Change> {
    let patch = |value: Value| Some(Change::Patch(value));
    return match (tag, input) {
        (LOGIN, Input::Flag(on)) => Some(Change::Login(*on)),
        (OPEN_LIBRARY, Input::Flag(on)) => patch(json!({ "open_library_at_launch": on })),
        (FULLSCREEN, Input::Flag(on)) => patch(json!({ "pause_when_fullscreen": on })),
        (COVERED, Input::Flag(on)) => patch(json!({ "pause_when_maximized": on })),
        (HOT, Input::Flag(on)) => patch(json!({ "pause_when_hot": on })),
        (OPTIMIZE, Input::Flag(on)) => patch(json!({ "optimize_on_import": on })),
        (SOUND, Input::Flag(on)) => patch(json!({ "muted": !on })),
        (SHUFFLE, Input::Flag(on)) => patch(json!({ "shuffle": on })),
        (LOCK_SCREEN, Input::Flag(on)) => patch(json!({ "privacy": { "lock_on_screen_lock": on } })),
        (LOCK_DISPLAY, Input::Flag(on)) => patch(json!({ "privacy": { "lock_on_display": on } })),
        (LOCK_IDLE, Input::Flag(on)) => patch(json!({ "privacy": { "lock_on_idle": on } })),
        (HIDE_DESKTOP, Input::Flag(on)) => patch(json!({ "privacy": { "hide_desktop": on } })),
        (ROTATE, Input::Flag(on)) => patch(json!({ "auto_rotate": on })),
        (PAUSE, Input::Flag(on)) => {
            let mut map = serde_json::Map::new();
            for key in PAUSE_KEYS {
                map.insert(key.to_string(), json!(*on && key != "pause_when_focused"));
            }
            patch(Value::Object(map))
        }
        (TRANSITION, Input::Flag(on)) => {
            patch(json!({ "transition": { "kind": if *on { Value::Null } else { json!("none") } } }))
        }
        (FRAME_RATE, Input::Index(index)) => {
            FRAME_RATES.get(*index).and_then(|rate| patch(json!({ "optimize_fps_limit": rate })))
        }
        (RELEASE, Input::Index(index)) => {
            RELEASES.get(*index).and_then(|(_, seconds)| patch(json!({ "stop_after_seconds": seconds })))
        }
        (STYLE, Input::Index(index)) => {
            STYLES.get(*index).and_then(|(_, kind)| patch(json!({ "transition": { "kind": kind } })))
        }
        (IDLE_AFTER, Input::Index(index)) => {
            IDLE_MINUTES.get(*index).and_then(|(_, minutes)| patch(json!({ "privacy": { "idle_minutes": minutes } })))
        }
        (EVERY, Input::Index(index)) => {
            INTERVALS.get(*index).filter(|(_, minutes)| *minutes > 0).and_then(|(_, minutes)| {
                return patch(json!({ "rotate_minutes": minutes }));
            })
        }
        (VOLUME, Input::Number(value)) => patch(json!({ "volume": (value / 100.0).clamp(0.0, 1.0) })),
        (DURATION, Input::Number(value)) => patch(json!({ "transition": { "duration": value.max(0.0) / 1000.0 } })),
        (STRENGTH, Input::Number(value)) => {
            Some(Change::Style(TINT_STRENGTH.to_string(), format!("{:.0}", value.clamp(0.0, 100.0))))
        }
        _ => None,
    };
}

pub fn style_field(tag: isize) -> Option<&'static str> {
    return Some(Key::ALL.get(usize::try_from(tag).ok()?)?.id());
}

fn key_tag(key: Key) -> isize {
    return isize::try_from(key as usize).unwrap_or(0);
}

struct RecorderInk {
    text: Retained<NSColor>,
    faint: Retained<NSColor>,
    hint: Retained<NSColor>,
    active: Retained<NSColor>,
    error: Retained<NSColor>,
    fill: Retained<NSColor>,
}

impl RecorderInk {
    fn new(theme: &Theme) -> Self {
        return Self {
            text: theme.color(Key::Text),
            faint: theme.color(Key::TextTertiary),
            hint: theme.color(Key::TextSecondary),
            active: theme.color(Key::SettingsRecorderActive),
            error: theme.color(Key::Destructive),
            fill: theme.color(Key::SettingsRecorder),
        };
    }
}

pub struct RecorderIvars {
    action: HotkeyAction,
    label: Retained<NSTextField>,
    recording: Cell<bool>,
    ink: RefCell<RecorderInk>,
    border: RefCell<Option<Retained<NSColor>>>,
    shown: RefCell<Option<proto::KeyCombo>>,
    edge: OnceCell<Edge>,
    control: controls::Control,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = RecorderIvars]
    struct RecorderView;

    impl RecorderView {
        #[unsafe(method(wantsUpdateLayer))]
        fn wants_update_layer(&self) -> bool {
            true
        }

        #[unsafe(method(updateLayer))]
        fn update_layer(&self) {
            self.colorize();
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeKeyView))]
        fn can_become_key_view(&self) -> bool {
            false
        }

        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            self.begin();
            true
        }

        #[unsafe(method(resignFirstResponder))]
        fn resign_first_responder(&self) -> bool {
            self.end();
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            self.focus();
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            controls::entered(self);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            controls::exited(self);
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_perform_press(&self) -> bool {
            self.focus();
            true
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            self.key(event);
        }

        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            self.equivalent(event)
        }
    }
);

impl ControlView for RecorderView {
    fn view(&self) -> &NSView {
        self
    }

    fn control(&self) -> &controls::Control {
        &self.ivars().control
    }

    fn apply(&self, immediate: bool) {
        self.glow(!immediate);
    }

    fn activate(&self) {
        self.focus();
    }
}

impl RecorderView {
    fn new(mtm: MainThreadMarker, action: HotkeyAction, theme: &Theme) -> Retained<Self> {
        let ink = RecorderInk::new(theme);
        let label = widgets::label(mtm, "", Text::CalloutMedium, &ink.text);
        label.setAlignment(NSTextAlignment::Center);
        let this = Self::alloc(mtm).set_ivars(RecorderIvars {
            action,
            label,
            recording: Cell::new(false),
            ink: RefCell::new(ink),
            border: RefCell::new(None),
            shown: RefCell::new(None),
            edge: OnceCell::new(),
            control: controls::Control::new(),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setWantsLayer(true);
        this.setLayerContentsRedrawPolicy(NSViewLayerContentsRedrawPolicy::OnSetNeedsDisplay);
        if let Some(layer) = this.layer() {
            layer.setCornerRadius(metrics::RADIUS_CONTROL);
        }
        let active = this.ivars().ink.borrow().active.clone();
        if let Some(edge) = Edge::mount(&this, metrics::RADIUS_CONTROL, active) {
            let _ = this.ivars().edge.set(edge);
        }
        controls::track(&this, &this);
        this.setAccessibilityElement(true);
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        let label = &this.ivars().label;
        this.addSubview(label);
        label.setTranslatesAutoresizingMaskIntoConstraints(false);
        label.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), metrics::SPACE_4).setActive(true);
        label
            .trailingAnchor()
            .constraintEqualToAnchor_constant(&this.trailingAnchor(), -metrics::SPACE_4)
            .setActive(true);
        label.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()).setActive(true);
        this.widthAnchor().constraintEqualToConstant(RECORDER_WIDTH).setActive(true);
        this.heightAnchor().constraintEqualToConstant(metrics::CONTROL_HEIGHT - metrics::SPACE_2).setActive(true);
        this.setToolTip(Some(&widgets::ns(&tr("Click, then press a shortcut. ⌫ clears it, ⎋ cancels."))));
        this.show(None);
        this.setNeedsDisplay(true);
        return this;
    }

    fn colorize(&self) {
        let ivars = self.ivars();
        let fill = ivars.ink.borrow().fill.clone();
        let border = ivars.border.borrow().clone();
        resolving(self, || {
            if let Some(layer) = self.layer() {
                layer.setBackgroundColor(Some(&fill.CGColor()));
                layer.setBorderColor(border.as_ref().map(|color| color.CGColor()).as_deref());
            }
        });
    }

    fn show(&self, combo: Option<&proto::KeyCombo>) {
        if self.ivars().recording.get() {
            return;
        }
        self.ivars().shown.replace(combo.cloned());
        let (text, color) = {
            let ink = self.ivars().ink.borrow();
            match combo {
                Some(combo) => (hotkey::display(combo), ink.text.clone()),
                None => (tr("None"), ink.faint.clone()),
            }
        };
        self.paint(&text, &color, None);
    }

    fn focus(&self) {
        if let Some(window) = self.window() {
            window.makeFirstResponder(Some(self));
        }
    }

    fn glow(&self, animated: bool) {
        let ivars = self.ivars();
        let Some(edge) = ivars.edge.get() else {
            return;
        };
        let level = if ivars.control.hovered() && !ivars.recording.get() { metrics::EDGE_HOVER } else { 0.0 };
        edge.show(self, level, animated);
    }

    fn paint(&self, text: &str, color: &NSColor, border: Option<&NSColor>) {
        let label = &self.ivars().label;
        label.setStringValue(&widgets::ns(text));
        label.setTextColor(Some(color));
        let title = tr(crate::hotkey::title(self.ivars().action));
        self.setAccessibilityLabel(Some(&widgets::ns(&format!("{title}, {text}"))));
        if let Some(layer) = self.layer() {
            layer.setBorderWidth(if border.is_some() { metrics::BORDER_EMPHASIS } else { 0.0 });
        }
        *self.ivars().border.borrow_mut() = border.map(Retained::from);
        self.setNeedsDisplay(true);
    }

    fn begin(&self) {
        self.ivars().recording.set(true);
        let (hint, active) = {
            let ink = self.ivars().ink.borrow();
            (ink.hint.clone(), ink.active.clone())
        };
        self.paint(&tr("Type Shortcut…"), &hint, Some(&active));
        self.glow(false);
        crate::ui::recording(true);
    }

    fn end(&self) {
        self.ivars().recording.set(false);
        self.glow(true);
        crate::ui::recording(false);
    }

    fn finish(&self) {
        if let Some(window) = self.window() {
            window.makeFirstResponder(None);
        }
    }

    fn equivalent(&self, event: &NSEvent) -> bool {
        if !self.ivars().recording.get() {
            return false;
        }
        self.key(event);
        return true;
    }

    fn key(&self, event: &NSEvent) {
        let action = self.ivars().action;
        let code = u32::from(event.keyCode());
        match code {
            ESCAPE => return self.finish(),
            DELETE | FORWARD_DELETE => {
                let _ = crate::ui::rebind(action, None);
                return self.finish();
            }
            _ => {}
        }
        let error = self.ivars().ink.borrow().error.clone();
        let Some(name) = hotkey::key_name(code) else {
            return self.paint(&tr("Unsupported Key"), &error, Some(&error));
        };
        let flags = event.modifierFlags();
        let mut parts: Vec<&str> = [
            (NSEventModifierFlags::Control, "ctrl"),
            (NSEventModifierFlags::Option, "opt"),
            (NSEventModifierFlags::Shift, "shift"),
            (NSEventModifierFlags::Command, "cmd"),
        ]
        .into_iter()
        .filter(|(flag, _)| flags.contains(*flag))
        .map(|(_, part)| part)
        .collect();
        parts.push(name);
        match crate::ui::rebind(action, Some(parts.join("+"))) {
            Ok(()) => self.finish(),
            Err(message) => self.paint(&message, &error, Some(&error)),
        }
    }
}

struct Colors {
    light: Retained<Dot>,
    dark: Retained<Dot>,
    auto: Retained<NSTextField>,
    reset: Option<Retained<NSButton>>,
    shown: Cell<[Option<Rgba>; 2]>,
}

impl Colors {
    fn new(kit: &mut Kit, field: &'static str, title: &str, reset: Option<isize>) -> Self {
        let dot = |dark: bool| {
            let dot = Dot::new(kit.mtm, SWATCH, 0.0, false, Some("minus"), kit.theme);
            let tip = if dark { tr("Dark appearance") } else { tr("Light appearance") };
            widgets::describe(&dot, &tip, &format!("{title}, {tip}"));
            dot.on_press(move |dot| actions::choose_color(dot, field, dark));
            dot.fit();
            return dot;
        };
        let (light, dark) = (dot(false), dot(true));
        let auto = kit.paint.label(kit.mtm, "", Text::Subheadline, Key::TextTertiary, kit.theme);
        auto.setAlignment(NSTextAlignment::Right);
        auto.setToolTip(Some(&widgets::ns(&tr("The dark color follows the light one until you change it"))));
        auto.widthAnchor().constraintEqualToConstant(MARKER_WIDTH).setActive(true);
        let reset = reset.map(|tag| {
            let button = widgets::icon_button(
                kit.mtm,
                "arrow.counterclockwise",
                sel!(styleReset:),
                kit.target,
                Key::TextSecondary,
                kit.theme,
            );
            button.setTag(tag);
            widgets::describe(&button, &tr("Reset"), &format!("{title}, {}", tr("Reset")));
            return button;
        });
        return Self { light, dark, auto, reset, shown: Cell::new([None, None]) };
    }

    fn view(&self, mtm: MainThreadMarker) -> Retained<NSView> {
        let (auto, light, dark): (&NSView, &NSView, &NSView) = (&self.auto, &self.light, &self.dark);
        let mut views = vec![auto, light, dark];
        if let Some(reset) = &self.reset {
            views.push(reset);
        }
        return widgets::view(&widgets::stack(mtm, &views, false, metrics::SPACE_8));
    }

    fn show(&self, light: Option<Rgba>, dark: Option<Rgba>, auto: bool, custom: bool) {
        if self.shown.replace([light, dark]) != [light, dark] {
            self.light.set_fill(light.map(theme::ns_color).as_deref());
            self.dark.set_fill(dark.map(theme::ns_color).as_deref());
        }
        self.auto.setStringValue(&widgets::ns(&if auto { tr("Auto") } else { String::new() }));
        if let Some(reset) = &self.reset {
            reset.setEnabled(custom);
            widgets::alpha(reset, if custom { 1.0 } else { metrics::DIMMED });
        }
    }
}

struct Circles {
    mtm: MainThreadMarker,
    view: Retained<NSView>,
    height: Retained<NSLayoutConstraint>,
    dots: RefCell<Vec<Retained<Dot>>>,
    names: RefCell<Vec<String>>,
    faces: RefCell<Vec<(Accent, Option<Pair>)>>,
}

fn step(index: usize) -> CGFloat {
    return f64::from(u32::try_from(index).unwrap_or(0));
}

fn paint_circle(dot: &Dot, accent: &Accent, tint: Option<&Pair>) {
    let fill = match accent {
        Accent::System => theme::system(SystemColor::ControlAccent),
        Accent::Fixed(pair) => theme::pair_color(pair),
    };
    dot.set_fill(Some(&fill));
    dot.set_rim(tint.map(theme::pair_color).as_deref());
}

impl Circles {
    fn new(mtm: MainThreadMarker) -> Self {
        let view: Retained<NSView> = Retained::into_super(widgets::Flipped::new(mtm));
        let height = view.heightAnchor().constraintEqualToConstant(CIRCLE);
        height.setActive(true);
        return Self {
            mtm,
            view,
            height,
            dots: RefCell::new(Vec::new()),
            names: RefCell::new(Vec::new()),
            faces: RefCell::new(Vec::new()),
        };
    }

    fn sync(&self, styles: &[Style], active: usize, theme: &Theme) {
        let names: Vec<String> = styles.iter().map(|style| style.name.clone()).collect();
        if *self.names.borrow() != names {
            self.rebuild(&names, theme);
            *self.names.borrow_mut() = names;
            self.faces.borrow_mut().clear();
        }
        let faces: Vec<(Accent, Option<Pair>)> = styles.iter().map(|style| (style.accent, style.tint)).collect();
        if *self.faces.borrow() != faces {
            for (dot, (accent, tint)) in self.dots.borrow().iter().zip(&faces) {
                paint_circle(dot, accent, tint.as_ref());
            }
            *self.faces.borrow_mut() = faces;
        }
        for (index, dot) in self.dots.borrow().iter().enumerate() {
            dot.set_selected(index == active);
        }
    }

    fn rebuild(&self, names: &[String], theme: &Theme) {
        let mut dots = self.dots.borrow_mut();
        for dot in dots.drain(..) {
            dot.removeFromSuperview();
        }
        for (index, name) in names.iter().enumerate() {
            let dot = Dot::new(self.mtm, CIRCLE, RING_PAD, true, None, theme);
            let origin = CGPoint::new(
                step(index % CIRCLE_COLUMNS) * CIRCLE_PITCH - RING_PAD,
                step(index / CIRCLE_COLUMNS) * CIRCLE_PITCH - RING_PAD,
            );
            dot.setFrameOrigin(origin);
            let title = if style::built_in(name) { tr(name) } else { name.clone() };
            widgets::describe(&dot, &title, &title);
            dot.on_press(move |_| pick_style(index));
            self.view.addSubview(&dot);
            dots.push(dot);
        }
        let rows = names.len().div_ceil(CIRCLE_COLUMNS).max(1);
        self.height.setConstant(step(rows) * CIRCLE_PITCH - RING_PAD * 2.0);
    }
}

pub struct Appearance {
    page: Retained<NSView>,
    height: CGFloat,
    mode: Retained<NSSegmentedControl>,
    circles: Circles,
    chosen: Retained<NSTextField>,
    rename: Retained<NSButton>,
    delete: Retained<NSButton>,
    accent: Colors,
    tint: Colors,
    strength: Knob,
    strength_label: Retained<NSTextField>,
    disclosure: Retained<NSButton>,
    advanced: Retained<NSStackView>,
    built: Cell<bool>,
    keys: RefCell<Vec<(Key, Colors)>>,
}

pub struct Settings {
    pub window: Retained<NSWindow>,
    switches: Vec<Retained<Switch>>,
    popups: Vec<Retained<NSPopUpButton>>,
    knobs: Vec<Knob>,
    rotate: Vec<Retained<NSView>>,
    transition: Vec<Retained<NSView>>,
    pause: Vec<Retained<NSView>>,
    idle: Vec<Retained<NSView>>,
    recorders: Vec<Retained<RecorderView>>,
    shell: Retained<Shell>,
    sections: Vec<Retained<TabButton>>,
    pages: Vec<Retained<NSView>>,
    titles: Vec<String>,
    paint: Paint,
    appearance: Appearance,
}

struct General {
    page: Retained<NSView>,
    pause: Vec<Retained<NSView>>,
    switches: Vec<Retained<Switch>>,
    popups: Vec<Retained<NSPopUpButton>>,
}

struct Locking {
    page: Retained<NSView>,
    idle: Vec<Retained<NSView>>,
    switches: Vec<Retained<Switch>>,
    popups: Vec<Retained<NSPopUpButton>>,
}

struct Kit<'a> {
    mtm: MainThreadMarker,
    target: &'a AnyObject,
    theme: &'a Theme,
    paint: Paint,
}

pub struct TabIvars {
    control: controls::Control,
    tag: Cell<isize>,
    icon: Retained<NSImageView>,
    label: Retained<NSTextField>,
    target: RefCell<Option<Retained<AnyObject>>>,
    action: Cell<Option<Sel>>,
    accent: RefCell<Retained<NSColor>>,
    ink: RefCell<Retained<NSColor>>,
    fill: RefCell<Retained<NSColor>>,
    soft: RefCell<Retained<NSColor>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = TabIvars]
    pub struct TabButton;

    impl TabButton {
        #[unsafe(method(wantsUpdateLayer))]
        fn wants_update_layer(&self) -> bool {
            true
        }

        #[unsafe(method(updateLayer))]
        fn update_layer(&self) {
            self.paint(true);
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            controls::mouse_down(self);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            controls::mouse_dragged(self, event);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            controls::mouse_up(self, event);
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            controls::entered(self);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            controls::exited(self);
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn did_move_to_window(&self) {
            let () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            controls::window_changed(self);
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if controls::key_event(event) {
                self.fire();
                return;
            }
            let _: () = unsafe { msg_send![super(self), keyDown: event] };
        }

        #[unsafe(method(drawFocusRingMask))]
        fn draw_focus_ring_mask(&self) {
            NSBezierPath::bezierPathWithRect(self.bounds()).fill();
        }

        #[unsafe(method(focusRingMaskBounds))]
        fn focus_ring_mask_bounds(&self) -> CGRect {
            self.bounds()
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_perform_press(&self) -> bool {
            let enabled = self.ivars().control.enabled();
            if enabled {
                self.fire();
            }
            enabled
        }
    }
);

impl ControlView for TabButton {
    fn view(&self) -> &NSView {
        self
    }

    fn control(&self) -> &controls::Control {
        &self.ivars().control
    }

    fn apply(&self, immediate: bool) {
        self.paint(immediate);
    }

    fn activate(&self) {
        self.fire();
    }
}

impl TabButton {
    fn new(
        mtm: MainThreadMarker,
        tag: isize,
        title: &str,
        symbol: &str,
        action: Sel,
        target: &AnyObject,
        theme: &Theme,
    ) -> Retained<Self> {
        let image = widgets::icon(symbol, IconRole::Tab);
        let icon = NSImageView::new(mtm);
        icon.setImage(image.as_deref());
        icon.setContentTintColor(Some(&theme.color(Key::TextSecondary)));
        icon.widthAnchor().constraintEqualToConstant(metrics::ICON_LARGE).setActive(true);
        icon.heightAnchor().constraintEqualToConstant(metrics::ICON_LARGE).setActive(true);
        let label = widgets::label(mtm, title, Text::Body, &theme.color(Key::TextSecondary));
        let content = widgets::stack(mtm, &[&icon, &label], false, metrics::SPACE_8);
        content.setAlignment(NSLayoutAttribute::CenterY);
        content.setEdgeInsets(widgets::insets(0.0, ITEM_PAD, 0.0, ITEM_PAD));
        let this = Self::alloc(mtm).set_ivars(TabIvars {
            control: controls::Control::new(),
            tag: Cell::new(tag),
            icon,
            label,
            target: RefCell::new(Some(target.retain())),
            action: Cell::new(Some(action)),
            accent: RefCell::new(theme.accent()),
            ink: RefCell::new(theme.color(Key::TextSecondary)),
            fill: RefCell::new(theme.color(Key::ToolbarPillHover)),
            soft: RefCell::new(theme.color(Key::SelectionSoft)),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setWantsLayer(true);
        this.setLayerContentsRedrawPolicy(NSViewLayerContentsRedrawPolicy::OnSetNeedsDisplay);
        if let Some(layer) = this.layer() {
            layer.setCornerRadius(metrics::RADIUS_CONTROL);
        }
        this.addSubview(&content);
        widgets::pin(&content, &this, 0.0);
        controls::track(&this, &this);
        this.setAccessibilityElement(true);
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityRadioButtonRole }));
        this.setAccessibilityLabel(Some(&widgets::ns(title)));
        this.heightAnchor().constraintEqualToConstant(ITEM_HEIGHT).setActive(true);
        this.paint(true);
        this.setNeedsDisplay(true);
        return this;
    }

    pub fn tag(&self) -> isize {
        return self.ivars().tag.get();
    }

    pub fn set_selected(&self, selected: bool) {
        controls::set_selected(self, selected);
    }

    pub fn repaint(&self, theme: &Theme) {
        let ivars = self.ivars();
        *ivars.accent.borrow_mut() = theme.accent();
        *ivars.ink.borrow_mut() = theme.color(Key::TextSecondary);
        *ivars.fill.borrow_mut() = theme.color(Key::ToolbarPillHover);
        *ivars.soft.borrow_mut() = theme.color(Key::SelectionSoft);
        self.paint(true);
    }

    fn paint(&self, immediate: bool) {
        let ivars = self.ivars();
        let selected = ivars.control.selected();
        let hover = (ivars.control.hovered() || ivars.control.pressed()) && ivars.control.enabled();
        let fill = if selected { ivars.soft.borrow().clone() } else { ivars.fill.borrow().clone() };
        CATransaction::begin();
        if immediate || ivars.control.pressed() {
            CATransaction::setDisableActions(true);
        } else {
            CATransaction::setAnimationDuration(metrics::MOTION_QUICK);
            CATransaction::setAnimationTimingFunction(Some(&metrics::ease_out()));
        }
        resolving(self, || {
            if let Some(layer) = self.layer() {
                let glow = fill.CGColor();
                layer.setBackgroundColor(if selected || hover { Some(&glow) } else { None });
            }
        });
        CATransaction::commit();
        let tint = if selected { ivars.accent.borrow().clone() } else { ivars.ink.borrow().clone() };
        ivars.icon.setContentTintColor(Some(&tint));
        ivars.label.setTextColor(Some(&tint));
    }

    fn fire(&self) {
        let ivars = self.ivars();
        let Some(action) = ivars.action.get() else {
            return;
        };
        let target = ivars.target.borrow().clone();
        unsafe {
            NSApplication::sharedApplication(self.mtm()).sendAction_to_from(action, target.as_deref(), Some(self));
        }
    }
}

struct ShellIvars {
    buttons: Vec<Retained<TabButton>>,
    target: Retained<AnyObject>,
    current: Cell<usize>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ShellIvars]
    struct Shell;

    impl Shell {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            self.equivalent(event)
        }
    }
);

impl Shell {
    fn new(mtm: MainThreadMarker, buttons: Vec<Retained<TabButton>>, target: &AnyObject) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ShellIvars { buttons, target: target.retain(), current: Cell::new(0) });
        return unsafe { msg_send![super(this), init] };
    }

    fn set_current(&self, index: usize) {
        self.ivars().current.set(index);
    }

    fn equivalent(&self, event: &NSEvent) -> bool {
        let step: isize = match event.keyCode() {
            ARROW_LEFT => -1,
            ARROW_RIGHT => 1,
            _ => {
                let handled: Bool = unsafe { msg_send![super(self), performKeyEquivalent: event] };
                return handled.as_bool();
            }
        };
        let ivars = self.ivars();
        if self.captive() || ivars.buttons.is_empty() {
            let handled: Bool = unsafe { msg_send![super(self), performKeyEquivalent: event] };
            return handled.as_bool();
        }
        let count = isize::try_from(ivars.buttons.len()).unwrap_or(1);
        let current = isize::try_from(ivars.current.get()).unwrap_or(0);
        let next = usize::try_from((current + step).rem_euclid(count)).unwrap_or(0);
        let Some(button) = ivars.buttons.get(next) else {
            return false;
        };
        return unsafe {
            NSApplication::sharedApplication(self.mtm()).sendAction_to_from(
                sel!(selectTab:),
                Some(&ivars.target),
                Some(&**button),
            )
        };
    }

    fn captive(&self) -> bool {
        let Some(window) = self.window() else {
            return true;
        };
        let Some(responder) = window.firstResponder() else {
            return false;
        };
        let class = responder.class();
        return std::ptr::eq(class, NSTextView::class())
            || std::ptr::eq(class, NSSlider::class())
            || std::ptr::eq(class, RecorderView::class());
    }
}

fn navigation(mtm: MainThreadMarker, items: &[&NSView]) -> Retained<NSStackView> {
    let row = widgets::stack(mtm, items, false, NAV_GAP);
    row.setAccessibilityElement(true);
    row.setAccessibilityRole(Some(unsafe { NSAccessibilityRadioGroupRole }));
    return row;
}

impl Settings {
    pub fn new(mtm: MainThreadMarker, look: &Look, location: &str, target: &AnyObject) -> Self {
        let mut kit = Kit { mtm, target, theme: &look.theme, paint: Paint::new() };
        let General { page: general, pause, mut switches, mut popups } = general(&mut kit, location);
        let (playback, rotate, transition, mut knobs, more_switches, more_popups) = playback(&mut kit);
        switches.extend(more_switches);
        popups.extend(more_popups);
        let (shortcuts, recorders) = shortcuts(&mut kit);
        let Locking { page: privacy, idle, switches: lock_switches, popups: lock_popups } = locking(&mut kit);
        switches.extend(lock_switches);
        popups.extend(lock_popups);
        let height = [&general, &playback, &shortcuts, &privacy]
            .iter()
            .filter_map(|view| view.subviews().firstObject())
            .map(|stack| stack.fittingSize().height)
            .fold(0.0, CGFloat::max);
        let appearance = appearance(&mut kit, height);
        knobs.push(appearance.strength.clone());
        let pages = [general, appearance.page.clone(), playback, shortcuts, privacy];
        let titles: Vec<String> = TABS.iter().map(|(title, _)| tr(title)).collect();
        let theme = kit.theme;
        let buttons: Vec<Retained<TabButton>> = TABS
            .iter()
            .enumerate()
            .map(|(index, (_, symbol))| {
                return TabButton::new(
                    mtm,
                    isize::try_from(index).unwrap_or(0),
                    &titles[index],
                    symbol,
                    sel!(selectTab:),
                    target,
                    theme,
                );
            })
            .collect();
        let refs: Vec<&NSView> = buttons.iter().map(|button| &***button).collect();
        let nav = navigation(mtm, &refs);
        let host = NSView::new(mtm);
        for page in &pages {
            host.addSubview(page);
            widgets::pin(page, &host, 0.0);
        }
        let shell = Shell::new(mtm, buttons.clone(), target);
        shell.addSubview(&nav);
        shell.addSubview(&host);
        nav.setTranslatesAutoresizingMaskIntoConstraints(false);
        host.setTranslatesAutoresizingMaskIntoConstraints(false);
        nav.leadingAnchor().constraintEqualToAnchor_constant(&shell.leadingAnchor(), NAV_INSET).setActive(true);
        nav.topAnchor().constraintEqualToAnchor_constant(&shell.topAnchor(), NAV_TOP).setActive(true);
        nav.trailingAnchor()
            .constraintLessThanOrEqualToAnchor_constant(&shell.trailingAnchor(), -NAV_INSET)
            .setActive(true);
        host.leadingAnchor().constraintEqualToAnchor(&shell.leadingAnchor()).setActive(true);
        host.trailingAnchor().constraintEqualToAnchor(&shell.trailingAnchor()).setActive(true);
        host.topAnchor().constraintEqualToAnchor_constant(&shell.topAnchor(), BAND).setActive(true);
        host.bottomAnchor().constraintEqualToAnchor(&shell.bottomAnchor()).setActive(true);
        let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::FullSizeContentView;
        let controller = NSViewController::new(mtm);
        controller.setView(&shell);
        let window = NSWindow::windowWithContentViewController(&controller);
        window.setStyleMask(style);
        window.setTitlebarAppearsTransparent(true);
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        unsafe { window.setReleasedWhenClosed(false) };
        window.setPreventsApplicationTerminationWhenModal(false);
        window.setContentSize(objc2_foundation::NSSize::new(WIDTH, BAND + appearance.height));
        add_close(mtm, &window, style, target);
        let this = Self {
            window,
            switches,
            popups,
            knobs,
            rotate,
            transition,
            pause,
            idle,
            recorders,
            shell,
            sections: buttons,
            pages: pages.to_vec(),
            titles,
            paint: kit.paint,
            appearance,
        };
        this.select(0);
        return this;
    }

    pub fn show(&self, parent: &NSWindow, tab: Option<isize>) {
        if let Some(tab) = tab {
            self.select(usize::try_from(tab).unwrap_or(0));
        }
        if self.window.sheetParent().is_none() {
            parent.beginSheet_completionHandler(&self.window, None);
        }
        self.window.makeFirstResponder(None);
    }

    pub fn select(&self, index: usize) {
        let index = index.min(self.pages.len().saturating_sub(1));
        self.shell.set_current(index);
        for (position, button) in self.sections.iter().enumerate() {
            button.set_selected(position == index);
        }
        for (position, page) in self.pages.iter().enumerate() {
            page.setHidden(position != index);
        }
        if let Some(title) = self.titles.get(index) {
            self.window.setTitle(&widgets::ns(title));
        }
        self.window.makeFirstResponder(None);
    }

    pub fn close(&self) {
        if let Some(parent) = self.window.sheetParent() {
            parent.endSheet(&self.window);
        }
    }

    pub fn repaint(&self, theme: &Theme) {
        self.paint.apply(theme);
        for button in &self.sections {
            button.repaint(theme);
        }
        for (_, field) in &self.knobs {
            field.paint(theme);
        }
    }

    pub fn sync_look(&self, look: &Look) {
        let page = &self.appearance;
        let theme = &look.theme;
        let style = look.style();
        page.mode.setSelectedSegment(isize::try_from(look.mode).unwrap_or(0));
        page.circles.sync(&look.styles, look.active, theme);
        let name = if style::built_in(&style.name) { tr(&style.name) } else { style.name.clone() };
        page.chosen.setStringValue(&widgets::ns(&name));
        let user = !style::built_in(&style.name);
        page.rename.setEnabled(user);
        page.delete.setEnabled(user);
        let (custom, auto) = match style.accent {
            Accent::Fixed(pair) => (true, pair.dark.is_none()),
            Accent::System => (false, false),
        };
        page.accent.show(
            Some(theme.accent_value(Variant::Light)),
            Some(theme.accent_value(Variant::Dark)),
            custom && auto,
            custom,
        );
        match style.tint {
            Some(tint) => page.tint.show(Some(tint.light), Some(tint.dark()), tint.dark.is_none(), true),
            None => page.tint.show(None, None, false, false),
        }
        let has_tint = style.tint.is_some();
        page.tint.dark.set_enabled(has_tint);
        let (slider, field) = &page.strength;
        slider.setEnabled(has_tint);
        field.setEnabled(has_tint);
        let level = if has_tint { 1.0 } else { metrics::DIMMED };
        let dimmed: [&NSView; 3] = [slider, field, &page.strength_label];
        for view in dimmed {
            widgets::alpha(view, level);
        }
        if !actions::dragging(self.window.mtm()) && !field.editing() {
            let percent = (style.tint_strength * 100.0).round();
            slider.setDoubleValue(percent);
            field.show(percent);
        }
        for (key, colors) in page.keys.borrow().iter() {
            let pair = style.pair(*key);
            colors.show(
                Some(theme.value(*key, Variant::Light)),
                Some(theme.value(*key, Variant::Dark)),
                pair.is_none_or(|pair| pair.dark.is_none()),
                pair.is_some(),
            );
        }
    }

    pub fn flip_advanced(&self, mtm: MainThreadMarker, look: &Look, target: &AnyObject) {
        let disclosure = &self.appearance.disclosure;
        disclosure.setState(if disclosure.state() == NSControlStateValueOn {
            NSControlStateValueOff
        } else {
            NSControlStateValueOn
        });
        self.toggle_advanced(mtm, look, target);
    }

    pub fn toggle_advanced(&self, mtm: MainThreadMarker, look: &Look, target: &AnyObject) {
        let page = &self.appearance;
        let open = page.disclosure.state() == NSControlStateValueOn;
        if open && !page.built.replace(true) {
            let mut kit = Kit { mtm, target, theme: &look.theme, paint: Paint::new() };
            *page.keys.borrow_mut() = advanced(&mut kit, &page.advanced);
            self.sync_look(look);
        }
        page.advanced.setHidden(!open);
    }

    pub fn sync(&self, values: &Value) {
        let flag = |key: &str| values.get(key).and_then(Value::as_bool).unwrap_or(false);
        let number = |key: &str| values.get(key).and_then(Value::as_f64).unwrap_or(0.0);
        let privacy = values.get("privacy");
        let locks = |key: &str| privacy.and_then(|map| map.get(key)).and_then(Value::as_bool).unwrap_or(false);
        let transition = values.get("transition");
        let kind = transition.and_then(|map| map.get("kind")).and_then(Value::as_str);
        let master = PAUSE_KEYS.iter().any(|key| flag(key));
        let animate = kind != Some("none");
        let rotate = flag("auto_rotate");
        for switch in &self.switches {
            let on = match switch.tag() {
                LOGIN => flag("launch_at_login"),
                OPEN_LIBRARY => flag("open_library_at_launch"),
                PAUSE => master,
                FULLSCREEN => flag("pause_when_fullscreen"),
                COVERED => flag("pause_when_maximized"),
                HOT => flag("pause_when_hot"),
                OPTIMIZE => flag("optimize_on_import"),
                SOUND => !flag("muted"),
                TRANSITION => animate,
                ROTATE => rotate,
                SHUFFLE => flag("shuffle"),
                LOCK_SCREEN => locks("lock_on_screen_lock"),
                LOCK_DISPLAY => locks("lock_on_display"),
                LOCK_IDLE => locks("lock_on_idle"),
                HIDE_DESKTOP => locks("hide_desktop"),
                _ => continue,
            };
            switch.set_state(if on { NSControlStateValueOn } else { NSControlStateValueOff });
        }
        let style = STYLES.iter().position(|(_, name)| Some(*name) == kind).or(Some(0));
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (rate, release, interval) =
            (number("optimize_fps_limit") as u32, number("stop_after_seconds") as u32, number("rotate_minutes") as u32);
        let idle_minutes = privacy
            .and_then(|map| map.get("idle_minutes"))
            .and_then(Value::as_u64)
            .and_then(|minutes| u32::try_from(minutes).ok())
            .unwrap_or(0);
        for popup in &self.popups {
            match popup.tag() {
                IDLE_AFTER => show_choice(
                    popup,
                    IDLE_MINUTES.len(),
                    IDLE_MINUTES.iter().position(|(_, value)| *value == idle_minutes),
                    &trf("%ld Minutes", &[Arg::Int(i64::from(idle_minutes))]),
                ),
                FRAME_RATE => show_choice(
                    popup,
                    FRAME_RATES.len(),
                    FRAME_RATES.iter().position(|value| *value == rate),
                    &format!("{rate} fps"),
                ),
                RELEASE => show_choice(
                    popup,
                    RELEASES.len(),
                    RELEASES.iter().position(|(_, value)| *value == release),
                    &trf("After %ld Seconds", &[Arg::Int(i64::from(release))]),
                ),
                STYLE => show_choice(popup, STYLES.len(), style, &tr("Fade")),
                EVERY => show_choice(
                    popup,
                    INTERVALS.len(),
                    INTERVALS.iter().position(|(_, value)| *value == interval && *value > 0),
                    &trf("%ld Minutes", &[Arg::Int(i64::from(interval))]),
                ),
                _ => {}
            }
        }
        for (slider, field) in &self.knobs {
            let value = match slider.tag() {
                VOLUME => values.get("volume").and_then(Value::as_f64).unwrap_or(1.0) * 100.0,
                DURATION => {
                    transition
                        .and_then(|map| map.get("duration"))
                        .and_then(Value::as_f64)
                        .unwrap_or(proto::Transition::DEFAULT_DURATION)
                        * 1000.0
                }
                _ => continue,
            };
            slider.setDoubleValue(value);
            field.show(value.round());
        }
        self.toggled(PAUSE, master);
        self.toggled(TRANSITION, animate);
        self.toggled(ROTATE, rotate);
        self.toggled(LOCK_IDLE, locks("lock_on_idle"));
    }

    pub fn toggled(&self, tag: isize, on: bool) {
        let group = match tag {
            ROTATE => &self.rotate,
            TRANSITION => &self.transition,
            PAUSE => &self.pause,
            LOCK_IDLE => &self.idle,
            _ => return,
        };
        for view in group {
            widgets::alpha(view, if on { 1.0 } else { metrics::DIMMED });
            if let Some(control) = view.downcast_ref::<NSControl>() {
                control.setEnabled(on);
            } else if let Some(control) = view.downcast_ref::<Switch>() {
                control.set_enabled(on);
            }
        }
    }

    pub fn sync_shortcuts(&self, hotkeys: &proto::Hotkeys) {
        for recorder in &self.recorders {
            recorder.show(hotkeys.get(recorder.ivars().action));
        }
    }

    pub fn slid(&self, tag: isize, value: f64) {
        if let Some((_, field)) = self.knobs.iter().find(|(slider, _)| slider.tag() == tag) {
            field.show(value.round());
        }
    }

    pub fn typed(&self, tag: isize, value: f64) -> f64 {
        let value = match tag {
            VOLUME | STRENGTH => value.clamp(0.0, 100.0),
            _ => value.clamp(200.0, 3000.0),
        };
        if let Some((slider, field)) = self.knobs.iter().find(|(slider, _)| slider.tag() == tag) {
            slider.setDoubleValue(value);
            field.show(value);
        }
        return value;
    }
}

fn show_choice(popup: &NSPopUpButton, base: usize, index: Option<usize>, extra: &str) {
    let base = isize::try_from(base).unwrap_or(0);
    while popup.numberOfItems() > base {
        popup.removeItemAtIndex(base);
    }
    if let Some(index) = index.and_then(|index| isize::try_from(index).ok()) {
        popup.selectItemAtIndex(index);
    } else {
        popup.addItemWithTitle(&widgets::ns(extra));
        popup.selectItemAtIndex(base);
    }
}

fn add_close(mtm: MainThreadMarker, window: &NSWindow, style: NSWindowStyleMask, target: &AnyObject) {
    let Some(close) = NSWindow::standardWindowButton_forStyleMask(NSWindowButton::CloseButton, style, mtm) else {
        return;
    };
    let Some(frame) = window.contentView().and_then(|content| unsafe { content.superview() }) else {
        return;
    };
    unsafe {
        close.setTarget(Some(target));
        close.setAction(Some(sel!(closeSettings:)));
    }
    close.setKeyEquivalent(&widgets::ns("\u{1b}"));
    frame.addSubview(&close);
    close.setTranslatesAutoresizingMaskIntoConstraints(false);
    close.leadingAnchor().constraintEqualToAnchor_constant(&frame.leadingAnchor(), CLOSE_INSET).setActive(true);
    close.topAnchor().constraintEqualToAnchor_constant(&frame.topAnchor(), CLOSE_INSET).setActive(true);
}

fn column(mtm: MainThreadMarker, sections: &[&NSView], inset: CGFloat) -> Retained<NSStackView> {
    let stack = widgets::stack(mtm, sections, true, metrics::SPACE_20);
    stack.setAlignment(NSLayoutAttribute::Leading);
    stack.setEdgeInsets(widgets::insets(metrics::SPACE_20, inset, inset, inset));
    for view in sections {
        view.leadingAnchor().constraintEqualToAnchor_constant(&stack.leadingAnchor(), inset).setActive(true);
        view.trailingAnchor().constraintEqualToAnchor_constant(&stack.trailingAnchor(), -inset).setActive(true);
    }
    stack.widthAnchor().constraintEqualToConstant(WIDTH).setActive(true);
    return stack;
}

fn page(mtm: MainThreadMarker, sections: &[&NSView]) -> Retained<NSView> {
    let stack = column(mtm, sections, metrics::SPACE_24);
    let container = widgets::Flipped::new(mtm);
    container.addSubview(&stack);
    stack.setTranslatesAutoresizingMaskIntoConstraints(false);
    stack.topAnchor().constraintEqualToAnchor(&container.topAnchor()).setActive(true);
    stack.leadingAnchor().constraintEqualToAnchor(&container.leadingAnchor()).setActive(true);
    stack.trailingAnchor().constraintEqualToAnchor(&container.trailingAnchor()).setActive(true);
    stack.bottomAnchor().constraintLessThanOrEqualToAnchor(&container.bottomAnchor()).setActive(true);
    return Retained::into_super(container);
}

fn scrolling(mtm: MainThreadMarker, sections: &[&NSView]) -> Retained<NSView> {
    let stack = column(mtm, sections, metrics::SPACE_20);
    let document = widgets::Flipped::new(mtm);
    document.setTranslatesAutoresizingMaskIntoConstraints(false);
    document.addSubview(&stack);
    widgets::pin(&stack, &document, 0.0);
    let scroll = NSScrollView::new(mtm);
    scroll.setDrawsBackground(false);
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    scroll.setDocumentView(Some(&document));
    let clip = scroll.contentView();
    document.leadingAnchor().constraintEqualToAnchor(&clip.leadingAnchor()).setActive(true);
    document.trailingAnchor().constraintEqualToAnchor(&clip.trailingAnchor()).setActive(true);
    document.topAnchor().constraintEqualToAnchor(&clip.topAnchor()).setActive(true);
    return Retained::into_super(scroll);
}

fn form(kit: &mut Kit, title: &str, rows: &[Retained<NSView>]) -> Retained<NSView> {
    return form_in(kit, title, Key::Text, 0.0, rows);
}

fn section(kit: &mut Kit, title: &str, rows: &[Retained<NSView>]) -> Retained<NSView> {
    return form_in(kit, title, Key::TextSecondary, metrics::SPACE_16, rows);
}

fn form_in(kit: &mut Kit, title: &str, header: Key, separator: CGFloat, rows: &[Retained<NSView>]) -> Retained<NSView> {
    let mtm = kit.mtm;
    let card = kit.paint.fill(mtm, Key::SettingsCard, metrics::RADIUS_CARD, kit.theme);
    let mut views: Vec<(Retained<NSView>, CGFloat)> = Vec::new();
    for (position, row) in rows.iter().enumerate() {
        if position > 0 {
            let line = widgets::divider(mtm, Key::SettingsSeparator, kit.theme);
            kit.paint.add(&line, Key::SettingsSeparator, Slot::Fill);
            views.push((Retained::into_super(line), separator));
        }
        views.push((row.clone(), 0.0));
    }
    let refs: Vec<&NSView> = views.iter().map(|(view, _)| &**view).collect();
    let body = widgets::stack(mtm, &refs, true, 0.0);
    for (view, inset) in &views {
        view.leadingAnchor().constraintEqualToAnchor_constant(&body.leadingAnchor(), *inset).setActive(true);
        view.trailingAnchor().constraintEqualToAnchor_constant(&body.trailingAnchor(), -*inset).setActive(true);
    }
    card.addSubview(&body);
    widgets::pin(&body, &card, 0.0);
    let header = kit.paint.label(mtm, title, Text::Headline, header, kit.theme);
    let section = widgets::stack(mtm, &[&header, &card], true, metrics::SPACE_8);
    section.setAlignment(NSLayoutAttribute::Leading);
    card.leadingAnchor().constraintEqualToAnchor(&section.leadingAnchor()).setActive(true);
    card.trailingAnchor().constraintEqualToAnchor(&section.trailingAnchor()).setActive(true);
    header.leadingAnchor().constraintEqualToAnchor_constant(&section.leadingAnchor(), metrics::SPACE_2).setActive(true);
    return widgets::view(&section);
}

fn line(kit: &mut Kit, title: &str, note: Option<&str>, control: &NSView) -> Retained<NSView> {
    return labelled(kit, title, note, control, metrics::SPACE_12).0;
}

fn labelled(
    kit: &mut Kit,
    title: &str,
    note: Option<&str>,
    control: &NSView,
    inset: CGFloat,
) -> (Retained<NSView>, Retained<NSTextField>) {
    let mtm = kit.mtm;
    let name = kit.paint.label(mtm, title, Text::Body, Key::Text, kit.theme);
    let text: Retained<NSView> = match note {
        Some(note) => {
            let hint = kit.paint.label(mtm, note, Text::Subheadline, Key::TextSecondary, kit.theme);
            let stack = widgets::stack(mtm, &[&name, &hint], true, metrics::SPACE_2);
            stack.setAlignment(NSLayoutAttribute::Leading);
            widgets::view(&stack)
        }
        None => widgets::view(&name),
    };
    let gap = widgets::spacer(mtm);
    let row = widgets::stack(mtm, &[&text, &gap, control], false, metrics::SPACE_12);
    let mut tip = String::from(title);
    if let Some(note) = note {
        tip.push_str("\n\n");
        tip.push_str(note);
    }
    row.setToolTip(Some(&widgets::ns(&tip)));
    row.setEdgeInsets(widgets::insets(0.0, inset, 0.0, inset));
    let height = if note.is_some() { metrics::ROW_SETTINGS_HINT } else { metrics::ROW_SETTINGS };
    row.heightAnchor().constraintGreaterThanOrEqualToConstant(height).setActive(true);
    return (widgets::view(&row), name);
}

fn switch(kit: &Kit, tag: isize) -> Retained<Switch> {
    let control = Switch::new(kit.mtm, kit.theme);
    control.set_tag(tag);
    control.set_target(Some(kit.target));
    control.set_action(Some(sel!(toggle:)));
    return control;
}

fn fixed_switch(kit: &Kit) -> Retained<Switch> {
    let control = Switch::new(kit.mtm, kit.theme);
    control.set_state(NSControlStateValueOn);
    control.set_enabled(false);
    return control;
}

fn choice(kit: &Kit, items: &[&str], tag: isize) -> Retained<NSPopUpButton> {
    let popup = widgets::popup(kit.mtm, false, kit.theme);
    if let Some(menu) = popup.menu() {
        menu.setAutoenablesItems(false);
    }
    let titles: Vec<Retained<NSString>> = items.iter().map(|title| widgets::ns(title)).collect();
    popup.addItemsWithTitles(&NSArray::from_retained_slice(&titles));
    popup.setTag(tag);
    unsafe {
        popup.setTarget(Some(kit.target));
        popup.setAction(Some(sel!(choose:)));
    }
    return popup;
}

fn knob(kit: &mut Kit, value: f64, min: f64, max: f64, tag: isize) -> Knob {
    let action: Sel = sel!(settingSlide:);
    let slider = unsafe {
        NSSlider::sliderWithValue_minValue_maxValue_target_action(
            value,
            min,
            max,
            Some(kit.target),
            Some(action),
            kit.mtm,
        )
    };
    slider.setControlSize(NSControlSize::Small);
    slider.setTag(tag);
    slider.setTrackFillColor(Some(&kit.theme.color(Key::SettingsSlider)));
    kit.paint.add(&slider, Key::SettingsSlider, Slot::Track);
    let title = match tag {
        VOLUME => i18n::key("Default Volume"),
        DURATION => i18n::key("Duration"),
        _ => i18n::key("Tint Strength"),
    };
    slider.setAccessibilityLabel(Some(&widgets::ns(&tr(title))));
    slider.widthAnchor().constraintEqualToConstant(KNOB_WIDTH).setActive(true);
    let (step, format): (f64, fn(f64) -> String) =
        if tag == DURATION { (50.0, |value| format!("{value:.0} ms")) } else { (5.0, |value| format!("{value:.0}%")) };
    let units = Units { step, format, parse: widgets::number };
    let field = NumberField::new(kit.mtm, units, tag, VALUE_WIDTH, sel!(settingTyped:), kit.target, kit.theme);
    field.show(value);
    field.setAccessibilityLabel(Some(&widgets::ns(&tr(title))));
    return (slider, field);
}

fn pair(mtm: MainThreadMarker, knob: &Knob) -> Retained<NSView> {
    return widgets::view(&widgets::stack(mtm, &[&knob.0, &knob.1], false, metrics::SPACE_8));
}

fn general(kit: &mut Kit, location: &str) -> General {
    let mtm = kit.mtm;
    let login = switch(kit, LOGIN);
    let open_library = switch(kit, OPEN_LIBRARY);
    let rows = [
        line(kit, &tr("Open at Login"), None, &login),
        line(kit, &tr("Open Library at Launch"), Some(&tr("Not when LiveWall starts at login")), &open_library),
    ];
    let startup = form(kit, &tr("Startup"), &rows);
    let master = switch(kit, PAUSE);
    let fullscreen = switch(kit, FULLSCREEN);
    let covered = switch(kit, COVERED);
    let hot = switch(kit, HOT);
    let release = choice(kit, &RELEASES.map(|(title, _)| tr(title)).each_ref().map(String::as_str), RELEASE);
    let pause = vec![widgets::view(&fullscreen), widgets::view(&covered), widgets::view(&hot)];
    let rows = [
        line(kit, &tr("Pause Automatically"), None, &master),
        line(kit, &tr("When an App Is Full Screen"), None, &fullscreen),
        line(
            kit,
            &tr("When the Desktop Is Covered"),
            Some(&tr("Uses more battery to keep track of windows")),
            &covered,
        ),
        line(kit, &tr("When the Mac Gets Hot"), None, &hot),
        line(kit, &tr("Free Memory When Paused"), Some(&tr("Swaps a paused video for a still frame")), &release),
    ];
    let power = form(kit, &tr("Pause Playback"), &rows);
    let place = kit.paint.label(mtm, location, Text::Callout, Key::TextSecondary, kit.theme);
    let change = widgets::compact(mtm, &tr("Change…"), None, None);
    change.setEnabled(false);
    let library_row = widgets::stack(mtm, &[&place, &change], false, metrics::SPACE_8);
    let optimize = switch(kit, OPTIMIZE);
    let frame_rate =
        choice(kit, &FRAME_RATES.map(|rate| format!("{rate} fps")).each_ref().map(String::as_str), FRAME_RATE);
    let rows = [
        line(kit, &tr("Location"), None, &library_row),
        line(kit, &tr("Copy Added Files into Library"), None, &fixed_switch(kit)),
        line(kit, &tr("Optimize on Import"), Some(&tr("Convert videos that need it for smooth playback")), &optimize),
        line(kit, &tr("Frame Rate Limit"), None, &frame_rate),
    ];
    let library = form(kit, &tr("Library"), &rows);
    return General {
        page: page(mtm, &[&startup, &power, &library]),
        pause,
        switches: vec![login, open_library, master, fullscreen, covered, hot, optimize],
        popups: vec![release, frame_rate],
    };
}

type PlaybackPage = (
    Retained<NSView>,
    Vec<Retained<NSView>>,
    Vec<Retained<NSView>>,
    Vec<Knob>,
    Vec<Retained<Switch>>,
    Vec<Retained<NSPopUpButton>>,
);

fn playback(kit: &mut Kit) -> PlaybackPage {
    let mtm = kit.mtm;
    let volume = knob(kit, 100.0, 0.0, 100.0, VOLUME);
    let sound = switch(kit, SOUND);
    let rows = [
        line(kit, &tr("Play Wallpaper Sound"), None, &sound),
        line(kit, &tr("Default Volume"), None, &pair(mtm, &volume)),
    ];
    let sound_card = form(kit, &tr("Sound"), &rows);
    let animate = switch(kit, TRANSITION);
    let style = choice(kit, &STYLES.map(|(title, _)| tr(title)).each_ref().map(String::as_str), STYLE);
    let length = knob(kit, 1000.0, 200.0, 3000.0, DURATION);
    let transition_group = vec![widgets::view(&style), widgets::view(&length.0), widgets::view(&length.1)];
    let rows = [
        line(kit, &tr("Animate Wallpaper Changes"), None, &animate),
        line(kit, &tr("Style"), None, &style),
        line(kit, &tr("Duration"), None, &pair(mtm, &length)),
    ];
    let transitions = form(kit, &tr("Transitions"), &rows);
    let every = choice(kit, &INTERVALS.map(|(title, _)| tr(title)).each_ref().map(String::as_str), EVERY);
    if let Some(item) = every.itemAtIndex(4) {
        item.setEnabled(false);
    }
    let shuffle = switch(kit, SHUFFLE);
    let rotate = switch(kit, ROTATE);
    let rotate_group = vec![widgets::view(&every), widgets::view(&shuffle)];
    let rows = [
        line(kit, &tr("Change Wallpaper"), Some(&tr("Cycles through the current folder")), &rotate),
        line(kit, &tr("Every"), None, &every),
        line(kit, &tr("Shuffle"), None, &shuffle),
    ];
    let rotation = form(kit, &tr("Rotation"), &rows);
    return (
        page(mtm, &[&sound_card, &transitions, &rotation]),
        rotate_group,
        transition_group,
        vec![volume, length],
        vec![sound, animate, shuffle, rotate],
        vec![style, every],
    );
}

fn shortcuts(kit: &mut Kit) -> (Retained<NSView>, Vec<Retained<RecorderView>>) {
    let mtm = kit.mtm;
    let recorders: Vec<Retained<RecorderView>> =
        HotkeyAction::ALL.iter().map(|action| RecorderView::new(mtm, *action, kit.theme)).collect();
    let rows: Vec<Retained<NSView>> = recorders
        .iter()
        .map(|recorder| line(kit, &tr(crate::hotkey::title(recorder.ivars().action)), None, recorder))
        .collect();
    let global = form(kit, &tr("Global"), &rows);
    let restore = widgets::compact(mtm, &tr("Restore Defaults"), Some(sel!(restoreShortcuts:)), Some(kit.target));
    let hint = kit.paint.label(
        mtm,
        &tr("Work in any app, on the current desktop."),
        Text::Subheadline,
        Key::TextSecondary,
        kit.theme,
    );
    let gap = widgets::spacer(mtm);
    let footer = widgets::view(&widgets::stack(mtm, &[&hint, &gap, &restore], false, metrics::SPACE_8));
    let digits = kit.paint.label(mtm, "1 – 9", Text::CalloutMedium, Key::TextSecondary, kit.theme);
    let grave = kit.paint.label(mtm, "`", Text::CalloutMedium, Key::TextSecondary, kit.theme);
    let rows = [line(kit, &tr("Set on Desktop"), None, &digits), line(kit, &tr("Set on All Desktops"), None, &grave)];
    let library = form(kit, &tr("In the Library"), &rows);
    return (page(mtm, &[&global, &footer, &library]), recorders);
}

fn locking(kit: &mut Kit) -> Locking {
    let mtm = kit.mtm;
    let screen = switch(kit, LOCK_SCREEN);
    let display = switch(kit, LOCK_DISPLAY);
    let idle = switch(kit, LOCK_IDLE);
    let after = choice(kit, &IDLE_MINUTES.map(|(title, _)| tr(title)).each_ref().map(String::as_str), IDLE_AFTER);
    let rows = [
        line(kit, &tr("Screen Locks or Sleeps"), Some(&tr("Includes the screen saver")), &screen),
        line(kit, &tr("External Display Connects"), Some(&tr("Includes AirPlay and mirroring")), &display),
        line(kit, &tr("After Inactivity"), None, &idle),
        line(kit, &tr("Inactive For"), None, &after),
    ];
    let automatic = form(kit, &tr("Lock Automatically"), &rows);
    let hide = switch(kit, HIDE_DESKTOP);
    let rows = [line(
        kit,
        &tr("Take Private Wallpapers off the Desktop"),
        Some(&tr("Off: the desktop and the lock screen keep showing them; the library and the menu bar hide them")),
        &hide,
    )];
    let desktop = form(kit, &tr("While Locked"), &rows);
    let hint = kit.paint.label(
        mtm,
        &tr("Always locked at launch and quit. Unlocking needs Touch ID or your password."),
        Text::Subheadline,
        Key::TextSecondary,
        kit.theme,
    );
    return Locking {
        page: page(mtm, &[&automatic, &desktop, &hint]),
        idle: vec![widgets::view(&after)],
        switches: vec![screen, display, idle, hide],
        popups: vec![after],
    };
}

fn segmented(kit: &Kit, labels: &[&str], action: Sel) -> Retained<NSSegmentedControl> {
    let titles: Vec<String> = labels.iter().map(|title| tr(title)).collect();
    let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
    return widgets::segmented(kit.mtm, &refs, action, kit.target, Key::Selection, kit.theme);
}

fn button(kit: &Kit, title: &str, action: Sel) -> Retained<NSButton> {
    return widgets::compact(kit.mtm, title, Some(action), Some(kit.target));
}

fn advanced(kit: &mut Kit, stack: &NSStackView) -> Vec<(Key, Colors)> {
    let mtm = kit.mtm;
    let hint = kit.paint.label(
        mtm,
        &tr("Fine-tune single parts. Dark colors follow the light ones until you change them."),
        Text::Subheadline,
        Key::TextSecondary,
        kit.theme,
    );
    stack.addArrangedSubview(&hint);
    let mut keys = Vec::new();
    for group in Group::ALL {
        let mut rows = Vec::new();
        for key in group.keys() {
            let title = tr(key.title());
            let colors = Colors::new(kit, key.id(), &title, Some(key_tag(key)));
            rows.push(labelled(kit, &title, None, &colors.view(mtm), metrics::SPACE_16).0);
            keys.push((key, colors));
        }
        let card = section(kit, &tr(group.title()), &rows);
        stack.addArrangedSubview(&card);
        card.leadingAnchor().constraintEqualToAnchor(&stack.leadingAnchor()).setActive(true);
        card.trailingAnchor().constraintEqualToAnchor(&stack.trailingAnchor()).setActive(true);
    }
    return keys;
}

fn appearance(kit: &mut Kit, height: CGFloat) -> Appearance {
    let mtm = kit.mtm;
    let mode = segmented(kit, &Look::MODES, sel!(mode:));
    let mode_row = labelled(kit, &tr("Appearance"), None, &mode, metrics::SPACE_16).0;
    let mode_card = section(kit, &tr("Mode"), &[mode_row]);

    let circles = Circles::new(mtm);
    let chosen = kit.paint.label(mtm, "", Text::Body, Key::Text, kit.theme);
    let picker = widgets::stack(mtm, &[&circles.view, &chosen], true, metrics::SPACE_8);
    picker.setAlignment(NSLayoutAttribute::Leading);
    picker.setEdgeInsets(widgets::insets(metrics::SPACE_16, metrics::SPACE_16, metrics::SPACE_12, metrics::SPACE_16));
    circles
        .view
        .trailingAnchor()
        .constraintEqualToAnchor_constant(&picker.trailingAnchor(), -metrics::SPACE_16)
        .setActive(true);
    let rename = button(kit, &tr("Rename…"), sel!(renameStyle:));
    let delete = button(kit, &tr("Delete…"), sel!(deleteStyle:));
    let export = button(kit, &tr("Export…"), sel!(exportStyle:));
    let import = button(kit, &tr("Import…"), sel!(importStyle:));
    let buttons = widgets::stack(mtm, &[&rename, &delete, &export, &import], false, metrics::SPACE_8);
    let style_rows = [widgets::view(&picker), labelled(kit, &tr("Style File"), None, &buttons, metrics::SPACE_16).0];
    let style_card = section(kit, &tr("Style"), &style_rows);

    let accent = Colors::new(kit, ACCENT, &tr("Accent"), None);
    let tint = Colors::new(kit, TINT, &tr("Tint"), None);
    let strength = knob(kit, 50.0, 0.0, 100.0, STRENGTH);
    let (strength_row, strength_label) =
        labelled(kit, &tr("Tint Strength"), None, &pair(mtm, &strength), metrics::SPACE_16);
    let general_rows = [
        labelled(kit, &tr("Accent"), Some(&tr("Buttons, selection and sliders")), &accent.view(mtm), metrics::SPACE_16)
            .0,
        labelled(
            kit,
            &tr("Tint"),
            Some(&tr("Colors the window, the sidebar and cards")),
            &tint.view(mtm),
            metrics::SPACE_16,
        )
        .0,
        strength_row,
    ];
    let general_card = section(kit, &tr("General Colors"), &general_rows);

    let disclosure = unsafe {
        NSButton::buttonWithTitle_target_action(&widgets::ns(""), Some(kit.target), Some(sel!(toggleAdvanced:)), mtm)
    };
    disclosure.setBezelStyle(NSBezelStyle::Disclosure);
    disclosure.setButtonType(NSButtonType::PushOnPushOff);
    disclosure.setState(NSControlStateValueOff);
    let heading = tr("Advanced Colors");
    widgets::describe(&disclosure, &heading, &heading);
    let advanced_title = unsafe {
        NSButton::buttonWithTitle_target_action(
            &widgets::ns(&heading),
            Some(kit.target),
            Some(sel!(flipAdvanced:)),
            mtm,
        )
    };
    advanced_title.setBordered(false);
    advanced_title.setAlignment(NSTextAlignment::Left);
    advanced_title.setFont(Some(&metrics::font(Text::Headline)));
    let ink = kit.theme.color(Key::TextSecondary);
    advanced_title.setAttributedTitle(&widgets::styled(&heading, &metrics::font(Text::Headline), &ink));
    advanced_title.setContentTintColor(Some(&ink));
    kit.paint.add(&advanced_title, Key::TextSecondary, Slot::Title);
    let toggle = widgets::stack(mtm, &[&disclosure, &advanced_title], false, metrics::SPACE_4);
    let advanced = widgets::stack(mtm, &[], true, metrics::SPACE_20);
    advanced.setAlignment(NSLayoutAttribute::Leading);
    advanced.setHidden(true);

    let page = scrolling(mtm, &[&mode_card, &style_card, &general_card, &toggle, &advanced]);
    return Appearance {
        page,
        height,
        mode,
        circles,
        chosen,
        rename,
        delete,
        accent,
        tint,
        strength,
        strength_label,
        disclosure,
        advanced,
        built: Cell::new(false),
        keys: RefCell::new(Vec::new()),
    };
}
