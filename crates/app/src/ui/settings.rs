use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBezelStyle, NSButton, NSButtonType, NSColor, NSColorWell, NSColorWellStyle, NSControl, NSControlSize,
    NSControlStateValueOff, NSControlStateValueOn, NSEvent, NSEventModifierFlags, NSLayoutAttribute, NSPopUpButton,
    NSScrollView, NSSegmentSwitchTracking, NSSegmentedControl, NSSlider, NSStackView, NSSwitch, NSTabViewController,
    NSTabViewControllerTabStyle, NSTabViewItem, NSTextAlignment, NSTextField, NSView, NSViewController,
    NSViewControllerTransitionOptions, NSWindow, NSWindowButton, NSWindowStyleMask, NSWindowToolbarStyle,
};
use objc2_core_foundation::{CGFloat, CGRect};
use objc2_foundation::{NSArray, NSString};
use proto::HotkeyAction;
use serde_json::{Value, json};

use crate::hotkey;
use crate::i18n::{self, Arg, tr, trf};
use crate::style::{self, ACCENT, Accent, Group, Key, Pair, Rgba, Style, TINT, TINT_STRENGTH, Variant};
use crate::ui::metrics::{self, Text};
use crate::ui::theme::{self, Look, Theme};
use crate::ui::widgets::{self, NumberField, Paint, Slot, Units};

const WIDTH: CGFloat = 540.0;
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
const STRENGTH: isize = 25;
pub const STYLE_ACCENT: isize = 1000;
pub const STYLE_TINT: isize = 1001;
pub const DARK_WELL: isize = 10_000;
const RECORDER_WIDTH: CGFloat = 128.0;
const TITLE_WIDTH: CGFloat = 170.0;
const WELL_WIDTH: CGFloat = 40.0;
const SWATCH: CGFloat = 30.0;
const SWATCH_CELL: CGFloat = 54.0;
const SWATCH_COLUMNS: usize = 6;
const KNOB_WIDTH: CGFloat = 150.0;
const VALUE_WIDTH: CGFloat = 62.0;

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
const ACCENT_MODES: [&str; 2] = [style::SYSTEM, style::CUSTOM];

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

pub fn style_field(tag: isize) -> Option<(&'static str, bool)> {
    let (base, dark) = if tag >= DARK_WELL { (tag - DARK_WELL, true) } else { (tag, false) };
    let field = match base {
        STYLE_ACCENT => ACCENT,
        STYLE_TINT => TINT,
        index => Key::ALL.get(usize::try_from(index).ok()?)?.id(),
    };
    return Some((field, dark));
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
    shown: RefCell<Option<proto::KeyCombo>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = RecorderIvars]
    struct RecorderView;

    impl RecorderView {
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

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            if let Some(window) = self.window() {
                window.makeFirstResponder(Some(self));
            }
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
            shown: RefCell::new(None),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setWantsLayer(true);
        if let Some(layer) = this.layer() {
            layer.setCornerRadius(metrics::RADIUS_CONTROL);
        }
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
        this.restyle(theme);
        return this;
    }

    fn restyle(&self, theme: &Theme) {
        let ink = RecorderInk::new(theme);
        if let Some(layer) = self.layer() {
            layer.setBackgroundColor(Some(&widgets::cg(self, &ink.fill)));
        }
        *self.ivars().ink.borrow_mut() = ink;
        let shown = self.ivars().shown.borrow().clone();
        self.show(shown.as_ref());
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

    fn paint(&self, text: &str, color: &NSColor, border: Option<&NSColor>) {
        let label = &self.ivars().label;
        label.setStringValue(&widgets::ns(text));
        label.setTextColor(Some(color));
        if let Some(layer) = self.layer() {
            layer.setBorderWidth(if border.is_some() { metrics::BORDER_EMPHASIS } else { 0.0 });
            if let Some(border) = border {
                layer.setBorderColor(Some(&widgets::cg(self, border)));
            }
        }
    }

    fn begin(&self) {
        self.ivars().recording.set(true);
        let (hint, active) = {
            let ink = self.ivars().ink.borrow();
            (ink.hint.clone(), ink.active.clone())
        };
        self.paint(&tr("Type Shortcut…"), &hint, Some(&active));
        crate::ui::recording(true);
    }

    fn end(&self) {
        self.ivars().recording.set(false);
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

pub struct SwatchIvars {
    index: usize,
    ring: Retained<NSView>,
    dot: Retained<NSView>,
    accent: Retained<NSColor>,
    tint: Retained<NSColor>,
    selection: RefCell<Retained<NSColor>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = SwatchIvars]
    struct SwatchView;

    impl SwatchView {
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            crate::ui::pick_style(self.ivars().index);
        }
    }
);

impl SwatchView {
    fn new(mtm: MainThreadMarker, index: usize, style: &Style, theme: &Theme) -> Retained<Self> {
        let ring = NSView::new(mtm);
        ring.setWantsLayer(true);
        let dot = NSView::new(mtm);
        dot.setWantsLayer(true);
        let accent = match &style.accent {
            Accent::System => theme::system(style::SystemColor::ControlAccent),
            Accent::Fixed(pair) => theme::pair_color(pair),
        };
        let tint = style.tint.as_ref().map_or_else(NSColor::windowBackgroundColor, theme::pair_color);
        let name = if style::built_in(&style.name) { tr(&style.name) } else { style.name.clone() };
        let caption = widgets::label(mtm, &name, Text::Subheadline, &theme.color(Key::TextSecondary));
        caption.setAlignment(NSTextAlignment::Center);
        let this = SwatchView::alloc(mtm).set_ivars(SwatchIvars {
            index,
            ring: ring.clone(),
            dot: dot.clone(),
            accent,
            tint,
            selection: RefCell::new(theme.color(Key::Selection)),
        });
        let this: Retained<SwatchView> = unsafe { msg_send![super(this), init] };
        this.setToolTip(Some(&widgets::ns(&name)));
        for view in [&*ring, &*dot, &**caption] {
            this.addSubview(view);
            view.setTranslatesAutoresizingMaskIntoConstraints(false);
            view.centerXAnchor().constraintEqualToAnchor(&this.centerXAnchor()).setActive(true);
        }
        let outer = SWATCH + metrics::SPACE_8;
        ring.widthAnchor().constraintEqualToConstant(outer).setActive(true);
        ring.heightAnchor().constraintEqualToConstant(outer).setActive(true);
        ring.topAnchor().constraintEqualToAnchor(&this.topAnchor()).setActive(true);
        dot.widthAnchor().constraintEqualToConstant(SWATCH).setActive(true);
        dot.heightAnchor().constraintEqualToConstant(SWATCH).setActive(true);
        dot.centerYAnchor().constraintEqualToAnchor(&ring.centerYAnchor()).setActive(true);
        caption.topAnchor().constraintEqualToAnchor_constant(&ring.bottomAnchor(), metrics::SPACE_4).setActive(true);
        caption.bottomAnchor().constraintEqualToAnchor(&this.bottomAnchor()).setActive(true);
        caption.widthAnchor().constraintLessThanOrEqualToConstant(SWATCH_CELL + metrics::SPACE_8).setActive(true);
        this.widthAnchor().constraintEqualToConstant(SWATCH_CELL).setActive(true);
        this.paint();
        return this;
    }

    fn paint(&self) {
        let ivars = self.ivars();
        if let Some(layer) = ivars.ring.layer() {
            layer.setCornerRadius(metrics::capsule(SWATCH + metrics::SPACE_8));
            layer.setBorderWidth(metrics::BORDER_EMPHASIS);
            layer.setBorderColor(Some(&widgets::cg(&ivars.ring, &ivars.selection.borrow())));
        }
        if let Some(layer) = ivars.dot.layer() {
            layer.setCornerRadius(metrics::capsule(SWATCH));
            layer.setBackgroundColor(Some(&widgets::cg(&ivars.dot, &ivars.accent)));
            layer.setBorderWidth(metrics::SPACE_4);
            layer.setBorderColor(Some(&widgets::cg(&ivars.dot, &ivars.tint)));
        }
    }
}

struct Well {
    light: Retained<NSColorWell>,
    dark: Retained<NSColorWell>,
    auto: Retained<NSTextField>,
    reset: Retained<NSButton>,
}

impl Well {
    fn new(mtm: MainThreadMarker, tag: isize, target: &AnyObject, paint: &mut Paint, theme: &Theme) -> Self {
        let well = |tag: isize, tip: &str| {
            let well = NSColorWell::colorWellWithStyle(NSColorWellStyle::Minimal, mtm);
            well.setSupportsAlpha(true);
            well.setTag(tag);
            well.setToolTip(Some(&widgets::ns(tip)));
            unsafe {
                well.setTarget(Some(target));
                well.setAction(Some(sel!(styleColor:)));
            }
            well.widthAnchor().constraintEqualToConstant(WELL_WIDTH).setActive(true);
            return well;
        };
        let light = well(tag, &tr("Light appearance"));
        let dark = well(tag + DARK_WELL, &tr("Dark appearance"));
        let auto = paint.label(mtm, &tr("Auto"), Text::Subheadline, Key::TextTertiary, theme);
        auto.setToolTip(Some(&widgets::ns(&tr("The dark color follows the light one until you change it"))));
        let reset = widgets::icon_button(
            mtm,
            "arrow.counterclockwise",
            sel!(styleReset:),
            target,
            &theme.color(Key::TextSecondary),
        );
        paint.add(&reset, Key::TextSecondary, Slot::Tint);
        reset.setTag(tag);
        reset.setToolTip(Some(&widgets::ns(&tr("Reset"))));
        return Self { light, dark, auto, reset };
    }

    fn view(&self, mtm: MainThreadMarker) -> Retained<NSView> {
        let row = widgets::stack(mtm, &[&self.auto, &self.light, &self.dark, &self.reset], false, metrics::SPACE_8);
        return widgets::view(&row);
    }

    fn show(&self, light: Rgba, dark: Rgba, auto: bool, custom: bool) {
        set_well(&self.light, light);
        set_well(&self.dark, dark);
        self.auto.setHidden(!auto);
        self.reset.setEnabled(custom);
        widgets::alpha(&self.reset, if custom { 1.0 } else { metrics::DIMMED });
    }

    fn enable(&self, on: bool) {
        for well in [&self.light, &self.dark] {
            well.setEnabled(on);
            widgets::alpha(well, if on { 1.0 } else { metrics::DIMMED });
        }
    }
}

fn set_well(well: &NSColorWell, value: Rgba) {
    if theme::rgba(&well.color()) != Some(value) {
        well.setColor(&theme::ns_color(value));
    }
}

pub struct Appearance {
    page: Retained<NSView>,
    height: CGFloat,
    mode: Retained<NSSegmentedControl>,
    gallery: Retained<NSStackView>,
    swatches: RefCell<Vec<Retained<SwatchView>>>,
    shown: RefCell<Vec<Style>>,
    accent_mode: Retained<NSSegmentedControl>,
    accent: Well,
    tint: Well,
    strength: Knob,
    rename: Retained<NSButton>,
    delete: Retained<NSButton>,
    disclosure: Retained<NSButton>,
    advanced: Retained<NSView>,
    keys: Vec<(Key, Well)>,
}

pub struct Settings {
    pub window: Retained<NSWindow>,
    switches: Vec<Retained<NSSwitch>>,
    popups: Vec<Retained<NSPopUpButton>>,
    knobs: Vec<Knob>,
    rotate: Vec<Retained<NSView>>,
    transition: Vec<Retained<NSView>>,
    pause: Vec<Retained<NSView>>,
    idle: Vec<Retained<NSView>>,
    recorders: Vec<Retained<RecorderView>>,
    tabs: Retained<NSTabViewController>,
    paint: Paint,
    appearance: Appearance,
}

struct General {
    page: Retained<NSView>,
    pause: Vec<Retained<NSView>>,
    switches: Vec<Retained<NSSwitch>>,
    popups: Vec<Retained<NSPopUpButton>>,
}

struct Locking {
    page: Retained<NSView>,
    idle: Vec<Retained<NSView>>,
    switches: Vec<Retained<NSSwitch>>,
    popups: Vec<Retained<NSPopUpButton>>,
}

struct Kit<'a> {
    mtm: MainThreadMarker,
    target: &'a AnyObject,
    theme: &'a Theme,
    paint: Paint,
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
        let appearance = appearance(&mut kit, look, height);
        knobs.push(appearance.strength.clone());
        let tabs = NSTabViewController::new(mtm);
        tabs.setTabStyle(NSTabViewControllerTabStyle::Toolbar);
        tabs.setCanPropagateSelectedChildViewControllerTitle(true);
        tabs.setTransitionOptions(NSViewControllerTransitionOptions::Crossfade);
        let pages = [
            (tr("General"), "gearshape", &general),
            (tr("Appearance"), "paintpalette", &appearance.page),
            (tr("Playback"), "play.rectangle", &playback),
            (tr("Shortcuts"), "keyboard", &shortcuts),
            (tr("Privacy"), "lock", &privacy),
        ];
        let size = objc2_foundation::NSSize::new(WIDTH, appearance.height);
        for (title, icon, view) in pages {
            let controller = NSViewController::new(mtm);
            controller.setView(view);
            controller.setTitle(Some(&widgets::ns(&title)));
            controller.setPreferredContentSize(size);
            let item = NSTabViewItem::tabViewItemWithViewController(&controller);
            item.setLabel(&widgets::ns(&title));
            item.setImage(widgets::symbol(icon, metrics::ICON_ROW).as_deref());
            tabs.addTabViewItem(&item);
        }
        let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;
        let window = NSWindow::windowWithContentViewController(&tabs);
        window.setStyleMask(style);
        window.setToolbarStyle(NSWindowToolbarStyle::Preference);
        unsafe { window.setReleasedWhenClosed(false) };
        window.setPreventsApplicationTerminationWhenModal(false);
        window.setContentSize(size);
        add_close(mtm, &window, style, target);
        return Self {
            window,
            switches,
            popups,
            knobs,
            rotate,
            transition,
            pause,
            idle,
            recorders,
            tabs,
            paint: kit.paint,
            appearance,
        };
    }

    pub fn show(&self, parent: &NSWindow, tab: Option<isize>) {
        if let Some(tab) = tab {
            self.tabs.setSelectedTabViewItemIndex(tab);
        }
        if self.window.sheetParent().is_none() {
            parent.beginSheet_completionHandler(&self.window, None);
        }
        self.window.makeFirstResponder(None);
    }

    pub fn close(&self) {
        if let Some(parent) = self.window.sheetParent() {
            parent.endSheet(&self.window);
        }
    }

    pub fn restyle(&self, look: &Look) {
        let theme = &look.theme;
        self.paint.apply(theme);
        for recorder in &self.recorders {
            recorder.restyle(theme);
        }
        for (_, field) in &self.knobs {
            field.paint(theme);
        }
        self.appearance.shown.borrow_mut().clear();
        self.sync_look(look);
    }

    pub fn sync_look(&self, look: &Look) {
        let page = &self.appearance;
        let theme = &look.theme;
        let style = look.style();
        page.mode.setSelectedSegment(isize::try_from(look.mode).unwrap_or(0));
        if *page.shown.borrow() != look.styles {
            fill_gallery(&page.gallery, &mut page.swatches.borrow_mut(), look);
            page.shown.borrow_mut().clone_from(&look.styles);
        }
        for swatch in page.swatches.borrow().iter() {
            widgets::alpha(&swatch.ivars().ring, if swatch.ivars().index == look.active { 1.0 } else { 0.0 });
        }
        let custom_accent = matches!(style.accent, Accent::Fixed(_));
        page.accent_mode.setSelectedSegment(isize::from(custom_accent));
        let accent_auto = !matches!(style.accent, Accent::Fixed(Pair { dark: Some(_), .. }));
        page.accent.show(
            theme.accent_value(Variant::Light),
            theme.accent_value(Variant::Dark),
            custom_accent && accent_auto,
            custom_accent,
        );
        page.accent.enable(custom_accent);
        let tint = style.tint.unwrap_or_else(|| {
            let fallback = theme.value(Key::Chrome, Variant::Light);
            return Pair::new(fallback);
        });
        page.tint.show(tint.light, tint.dark(), tint.dark.is_none(), style.tint.is_some());
        let percent = (style.tint_strength * 100.0).round();
        page.strength.0.setDoubleValue(percent);
        page.strength.1.show(percent);
        let user = !style::built_in(&style.name);
        page.rename.setEnabled(user);
        page.delete.setEnabled(user);
        for (key, well) in &page.keys {
            let pair = style.pair(*key);
            well.show(
                theme.value(*key, Variant::Light),
                theme.value(*key, Variant::Dark),
                pair.is_none_or(|pair| pair.dark.is_none()),
                pair.is_some(),
            );
        }
    }

    pub fn toggle_advanced(&self) {
        let page = &self.appearance;
        let open = page.disclosure.state() == NSControlStateValueOn;
        widgets::reveal(&page.advanced, open);
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
            switch.setState(if on { NSControlStateValueOn } else { NSControlStateValueOff });
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

fn column(mtm: MainThreadMarker, sections: &[&NSView]) -> Retained<NSStackView> {
    let stack = widgets::stack(mtm, sections, true, metrics::SPACE_20);
    stack.setAlignment(NSLayoutAttribute::Leading);
    stack.setEdgeInsets(widgets::insets(metrics::SPACE_20, metrics::SPACE_24, metrics::SPACE_24, metrics::SPACE_24));
    for view in sections {
        view.leadingAnchor()
            .constraintEqualToAnchor_constant(&stack.leadingAnchor(), metrics::SPACE_24)
            .setActive(true);
        view.trailingAnchor()
            .constraintEqualToAnchor_constant(&stack.trailingAnchor(), -metrics::SPACE_24)
            .setActive(true);
    }
    stack.widthAnchor().constraintEqualToConstant(WIDTH).setActive(true);
    return stack;
}

fn page(mtm: MainThreadMarker, sections: &[&NSView]) -> Retained<NSView> {
    let stack = column(mtm, sections);
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
    let stack = column(mtm, sections);
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
    let mtm = kit.mtm;
    let card = kit.paint.fill(mtm, Key::SettingsCard, metrics::RADIUS_CARD, kit.theme);
    let mut views: Vec<Retained<NSView>> = Vec::new();
    for (position, row) in rows.iter().enumerate() {
        if position > 0 {
            let line = widgets::divider(mtm, &kit.theme.color(Key::SettingsSeparator));
            kit.paint.add(&line, Key::SettingsSeparator, Slot::Fill);
            views.push(Retained::into_super(line));
        }
        views.push(row.clone());
    }
    let refs: Vec<&NSView> = views.iter().map(|view| &**view).collect();
    let body = widgets::stack(mtm, &refs, true, 0.0);
    for view in &refs {
        view.leadingAnchor().constraintEqualToAnchor(&body.leadingAnchor()).setActive(true);
        view.trailingAnchor().constraintEqualToAnchor(&body.trailingAnchor()).setActive(true);
    }
    card.addSubview(&body);
    widgets::pin(&body, &card, 0.0);
    let header = kit.paint.label(mtm, title, Text::Headline, Key::Text, kit.theme);
    let section = widgets::stack(mtm, &[&header, &card], true, metrics::SPACE_8);
    section.setAlignment(NSLayoutAttribute::Leading);
    card.leadingAnchor().constraintEqualToAnchor(&section.leadingAnchor()).setActive(true);
    card.trailingAnchor().constraintEqualToAnchor(&section.trailingAnchor()).setActive(true);
    header.leadingAnchor().constraintEqualToAnchor_constant(&section.leadingAnchor(), metrics::SPACE_2).setActive(true);
    return widgets::view(&section);
}

fn line(kit: &mut Kit, title: &str, note: Option<&str>, control: &NSView) -> Retained<NSView> {
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
    row.setEdgeInsets(widgets::insets(0.0, metrics::SPACE_12, 0.0, metrics::SPACE_12));
    let height = if note.is_some() { metrics::ROW_SETTINGS_HINT } else { metrics::ROW_SETTINGS };
    row.heightAnchor().constraintGreaterThanOrEqualToConstant(height).setActive(true);
    return widgets::view(&row);
}

fn switch(kit: &Kit, tag: isize) -> Retained<NSSwitch> {
    let control = NSSwitch::new(kit.mtm);
    control.setControlSize(NSControlSize::Small);
    control.setTag(tag);
    unsafe {
        control.setTarget(Some(kit.target));
        control.setAction(Some(sel!(toggle:)));
    }
    return control;
}

fn fixed_switch(mtm: MainThreadMarker) -> Retained<NSSwitch> {
    let control = NSSwitch::new(mtm);
    control.setControlSize(NSControlSize::Small);
    control.setState(NSControlStateValueOn);
    control.setEnabled(false);
    return control;
}

fn choice(kit: &Kit, items: &[&str], tag: isize) -> Retained<NSPopUpButton> {
    let popup = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(kit.mtm), CGRect::ZERO, false);
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
    slider.widthAnchor().constraintEqualToConstant(KNOB_WIDTH).setActive(true);
    let (step, format): (f64, fn(f64) -> String) =
        if tag == DURATION { (50.0, |value| format!("{value:.0} ms")) } else { (5.0, |value| format!("{value:.0}%")) };
    let units = Units { step, format, parse: widgets::number };
    let field = NumberField::new(kit.mtm, units, tag, VALUE_WIDTH, sel!(settingTyped:), kit.target, kit.theme);
    field.show(value);
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
        line(kit, &tr("Copy Added Files into Library"), None, &fixed_switch(mtm)),
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
    Vec<Retained<NSSwitch>>,
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

fn segmented(kit: &mut Kit, labels: &[&str], action: Sel) -> Retained<NSSegmentedControl> {
    let labels: Vec<Retained<NSString>> = labels.iter().map(|title| widgets::ns(&tr(title))).collect();
    let control = unsafe {
        NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
            &NSArray::from_retained_slice(&labels),
            NSSegmentSwitchTracking::SelectOne,
            Some(kit.target),
            Some(action),
            kit.mtm,
        )
    };
    control.setSelectedSegmentBezelColor(Some(&kit.theme.color(Key::Selection)));
    kit.paint.add(&control, Key::Selection, Slot::Bezel);
    return control;
}

fn button(kit: &Kit, title: &str, action: Sel) -> Retained<NSButton> {
    return widgets::compact(kit.mtm, title, Some(action), Some(kit.target));
}

fn titled(kit: &mut Kit, title: &str, control: &NSView) -> Retained<NSView> {
    let mtm = kit.mtm;
    let name = kit.paint.label(mtm, title, Text::Body, Key::Text, kit.theme);
    name.widthAnchor().constraintEqualToConstant(TITLE_WIDTH).setActive(true);
    let gap = widgets::spacer(mtm);
    let row = widgets::stack(mtm, &[&name, &gap, control], false, metrics::SPACE_8);
    row.setEdgeInsets(widgets::insets(0.0, metrics::SPACE_12, 0.0, metrics::SPACE_12));
    row.heightAnchor().constraintGreaterThanOrEqualToConstant(metrics::ROW_SETTINGS).setActive(true);
    return widgets::view(&row);
}

fn appearance(kit: &mut Kit, look: &Look, height: CGFloat) -> Appearance {
    let mtm = kit.mtm;
    let mode = segmented(kit, &theme::Look::MODES, sel!(mode:));
    let gallery = widgets::stack(mtm, &[], true, metrics::SPACE_12);
    gallery.setAlignment(NSLayoutAttribute::Leading);
    gallery.setEdgeInsets(widgets::insets(metrics::SPACE_16, metrics::SPACE_12, metrics::SPACE_12, metrics::SPACE_12));
    let accent_mode = segmented(kit, &ACCENT_MODES, sel!(accentMode:));
    accent_mode.setControlSize(NSControlSize::Small);
    let accent = Well::new(mtm, STYLE_ACCENT, kit.target, &mut kit.paint, kit.theme);
    let accent_controls = widgets::stack(mtm, &[&accent_mode, &accent.view(mtm)], false, metrics::SPACE_8);
    let tint = Well::new(mtm, STYLE_TINT, kit.target, &mut kit.paint, kit.theme);
    let strength = knob(kit, 50.0, 0.0, 100.0, STRENGTH);
    let rename = button(kit, &tr("Rename…"), sel!(renameStyle:));
    let delete = button(kit, &tr("Delete…"), sel!(deleteStyle:));
    let export = button(kit, &tr("Export…"), sel!(exportStyle:));
    let import = button(kit, &tr("Import…"), sel!(importStyle:));
    let actions = widgets::stack(mtm, &[&rename, &delete, &export, &import], false, metrics::SPACE_8);
    let rows = [line(kit, &tr("Mode"), None, &mode)];
    let window = form(kit, &tr("Appearance"), &rows);
    let rows = [
        widgets::view(&gallery),
        line(kit, &tr("Accent"), Some(&tr("Buttons, selection and sliders")), &accent_controls),
        line(kit, &tr("Tint"), Some(&tr("Colors the window, the sidebar and cards")), &tint.view(mtm)),
        line(kit, &tr("Tint Strength"), None, &pair(mtm, &strength)),
        line(kit, &tr("Style File"), None, &actions),
    ];
    let general = form(kit, &tr("Style"), &rows);
    let disclosure = unsafe {
        NSButton::buttonWithTitle_target_action(&widgets::ns(""), Some(kit.target), Some(sel!(toggleAdvanced:)), mtm)
    };
    disclosure.setBezelStyle(NSBezelStyle::Disclosure);
    disclosure.setButtonType(NSButtonType::PushOnPushOff);
    disclosure.setState(NSControlStateValueOff);
    let advanced_title = kit.paint.label(mtm, &tr("Advanced"), Text::Headline, Key::Text, kit.theme);
    let advanced_hint = kit.paint.label(
        mtm,
        &tr("Fine-tune single parts. Dark colors follow the light ones until you change them."),
        Text::Subheadline,
        Key::TextSecondary,
        kit.theme,
    );
    let toggle = widgets::stack(mtm, &[&disclosure, &advanced_title], false, metrics::SPACE_4);
    let mut keys = Vec::new();
    let mut groups: Vec<Retained<NSView>> = vec![widgets::view(&advanced_hint)];
    for group in Group::ALL {
        let mut rows = Vec::new();
        for key in group.keys() {
            let well = Well::new(mtm, key_tag(key), kit.target, &mut kit.paint, kit.theme);
            rows.push(titled(kit, &tr(key.title()), &well.view(mtm)));
            keys.push((key, well));
        }
        groups.push(form(kit, &tr(group.title()), &rows));
    }
    let group_refs: Vec<&NSView> = groups.iter().map(|view| &**view).collect();
    let advanced = widgets::stack(mtm, &group_refs, true, metrics::SPACE_20);
    advanced.setAlignment(NSLayoutAttribute::Leading);
    for view in &group_refs {
        view.leadingAnchor().constraintEqualToAnchor(&advanced.leadingAnchor()).setActive(true);
        view.trailingAnchor().constraintEqualToAnchor(&advanced.trailingAnchor()).setActive(true);
    }
    advanced.setHidden(true);
    let advanced = widgets::view(&advanced);
    let page = scrolling(mtm, &[&window, &general, &toggle, &advanced]);
    let appearance = Appearance {
        page,
        height,
        mode,
        gallery,
        swatches: RefCell::new(Vec::new()),
        shown: RefCell::new(Vec::new()),
        accent_mode,
        accent,
        tint,
        strength,
        rename,
        delete,
        disclosure,
        advanced,
        keys,
    };
    fill_gallery(&appearance.gallery, &mut appearance.swatches.borrow_mut(), look);
    appearance.shown.borrow_mut().clone_from(&look.styles);
    return appearance;
}

fn fill_gallery(gallery: &NSStackView, swatches: &mut Vec<Retained<SwatchView>>, look: &Look) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let arranged = gallery.arrangedSubviews();
    for index in 0..arranged.count() {
        let view = arranged.objectAtIndex(index);
        gallery.removeArrangedSubview(&view);
        view.removeFromSuperview();
    }
    swatches.clear();
    for (row, chunk) in look.styles.chunks(SWATCH_COLUMNS).enumerate() {
        let cells: Vec<Retained<SwatchView>> = chunk
            .iter()
            .enumerate()
            .map(|(column, style)| SwatchView::new(mtm, row * SWATCH_COLUMNS + column, style, &look.theme))
            .collect();
        let refs: Vec<&NSView> = cells.iter().map(|view| &***view).collect();
        let line = widgets::stack(mtm, &refs, false, metrics::SPACE_16);
        line.setAlignment(NSLayoutAttribute::Top);
        gallery.addArrangedSubview(&line);
        swatches.extend(cells);
    }
}
