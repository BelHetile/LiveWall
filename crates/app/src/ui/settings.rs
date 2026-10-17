use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSColor, NSColorWell, NSColorWellStyle, NSControlSize, NSControlStateValueOff, NSControlStateValueOn, NSEvent,
    NSEventModifierFlags, NSLayoutAttribute, NSPopUpButton, NSSegmentSwitchTracking, NSSegmentedControl, NSSlider,
    NSSwitch, NSTabViewController, NSTabViewControllerTabStyle, NSTabViewItem, NSTextAlignment, NSTextField, NSView,
    NSViewController, NSViewControllerTransitionOptions, NSWindow, NSWindowButton, NSWindowStyleMask,
    NSWindowToolbarStyle,
};
use objc2_core_foundation::{CGFloat, CGRect};
use objc2_foundation::{NSArray, NSString};
use proto::HotkeyAction;
use serde_json::{Value, json};

use crate::hotkey;
use crate::i18n::{self, Arg, tr, trf};
use crate::ui::palette::{Look, Rgb, Theme};
use crate::ui::widgets::{self, NumberField, Units, Weight};

const WIDTH: CGFloat = 540.0;
const ESCAPE: u32 = 53;
const DELETE: u32 = 51;
const FORWARD_DELETE: u32 = 117;
pub const ROTATE: isize = 1;
pub const TRANSITION: isize = 2;
pub const PAUSE: isize = 3;
const CLOSE_INSET: CGFloat = 18.0;
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
const VOLUME: isize = 20;
const DURATION: isize = 21;
pub const ACCENT: isize = 1;
pub const CANVAS: isize = 2;

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
        _ => None,
    };
}

pub struct RecorderIvars {
    action: HotkeyAction,
    label: Retained<NSTextField>,
    recording: Cell<bool>,
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
    fn new(mtm: MainThreadMarker, action: HotkeyAction) -> Retained<Self> {
        let label = widgets::label(mtm, "", 12.0, &Weight::Medium, &NSColor::labelColor());
        label.setAlignment(NSTextAlignment::Center);
        let this = Self::alloc(mtm).set_ivars(RecorderIvars { action, label, recording: Cell::new(false) });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setWantsLayer(true);
        if let Some(layer) = this.layer() {
            layer.setCornerRadius(6.0);
            layer.setBackgroundColor(Some(&widgets::cg(mtm, &NSColor::quaternarySystemFillColor())));
        }
        let label = &this.ivars().label;
        this.addSubview(label);
        label.setTranslatesAutoresizingMaskIntoConstraints(false);
        label.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), 6.0).setActive(true);
        label.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -6.0).setActive(true);
        label.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()).setActive(true);
        this.widthAnchor().constraintEqualToConstant(128.0).setActive(true);
        this.heightAnchor().constraintEqualToConstant(24.0).setActive(true);
        this.setToolTip(Some(&widgets::ns(&tr("Click, then press a shortcut. ⌫ clears it, ⎋ cancels."))));
        return this;
    }

    fn show(&self, combo: Option<&proto::KeyCombo>) {
        if self.ivars().recording.get() {
            return;
        }
        let (text, color) = match combo {
            Some(combo) => (hotkey::display(combo), NSColor::labelColor()),
            None => (tr("None"), NSColor::tertiaryLabelColor()),
        };
        self.paint(&text, &color, None);
    }

    fn paint(&self, text: &str, color: &NSColor, border: Option<&NSColor>) {
        let label = &self.ivars().label;
        label.setStringValue(&widgets::ns(text));
        label.setTextColor(Some(color));
        if let Some(layer) = self.layer() {
            layer.setBorderWidth(if border.is_some() { 1.5 } else { 0.0 });
            if let Some(border) = border {
                layer.setBorderColor(Some(&widgets::cg(self.mtm(), border)));
            }
        }
    }

    fn begin(&self) {
        self.ivars().recording.set(true);
        let accent = NSColor::controlAccentColor();
        self.paint(&tr("Type Shortcut…"), &NSColor::secondaryLabelColor(), Some(&accent));
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
        let Some(name) = hotkey::key_name(code) else {
            return self.paint(&tr("Unsupported Key"), &NSColor::systemRedColor(), Some(&NSColor::systemRedColor()));
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
            Err(message) => self.paint(&message, &NSColor::systemRedColor(), Some(&NSColor::systemRedColor())),
        }
    }
}

pub struct SwatchIvars {
    index: usize,
    ring: Retained<NSView>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = SwatchIvars]
    struct SwatchView;

    impl SwatchView {
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            crate::ui::pick_theme(self.ivars().index);
        }
    }
);

pub struct Settings {
    pub window: Retained<NSWindow>,
    mode: Retained<NSSegmentedControl>,
    swatches: Vec<Retained<SwatchView>>,
    accent: Retained<NSColorWell>,
    canvas: Retained<NSColorWell>,
    switches: Vec<Retained<NSSwitch>>,
    popups: Vec<Retained<NSPopUpButton>>,
    knobs: Vec<Knob>,
    rotate: Vec<Retained<NSView>>,
    transition: Vec<Retained<NSView>>,
    pause: Vec<Retained<NSView>>,
    idle: Vec<Retained<NSView>>,
    recorders: Vec<Retained<RecorderView>>,
    tabs: Retained<NSTabViewController>,
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

impl Settings {
    pub fn new(mtm: MainThreadMarker, look: &Look, location: &str, target: &AnyObject) -> Self {
        let General { page: general, pause, mut switches, mut popups } = general(mtm, location, target);
        let (playback, rotate, transition, knobs, more_switches, more_popups) = playback(mtm, target);
        switches.extend(more_switches);
        popups.extend(more_popups);
        let (appearance, mode, swatches, accent, canvas) = appearance(mtm, look, target);
        let (shortcuts, recorders) = shortcuts(mtm, target);
        let Locking { page: privacy, idle, switches: lock_switches, popups: lock_popups } = locking(mtm, target);
        switches.extend(lock_switches);
        popups.extend(lock_popups);
        let tabs = NSTabViewController::new(mtm);
        tabs.setTabStyle(NSTabViewControllerTabStyle::Toolbar);
        tabs.setCanPropagateSelectedChildViewControllerTitle(true);
        tabs.setTransitionOptions(NSViewControllerTransitionOptions::Crossfade);
        let pages = [
            (tr("General"), "gearshape", &general),
            (tr("Appearance"), "paintpalette", &appearance),
            (tr("Playback"), "play.rectangle", &playback),
            (tr("Shortcuts"), "keyboard", &shortcuts),
            (tr("Privacy"), "lock", &privacy),
        ];
        let height = pages
            .iter()
            .filter_map(|(_, _, view)| view.subviews().firstObject())
            .map(|stack| stack.fittingSize().height)
            .fold(0.0, CGFloat::max);
        let size = objc2_foundation::NSSize::new(WIDTH, height);
        for (title, icon, view) in pages {
            let controller = NSViewController::new(mtm);
            controller.setView(view);
            controller.setTitle(Some(&widgets::ns(&title)));
            controller.setPreferredContentSize(size);
            let item = NSTabViewItem::tabViewItemWithViewController(&controller);
            item.setLabel(&widgets::ns(&title));
            item.setImage(widgets::symbol(icon, 15.0).as_deref());
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
            mode,
            swatches,
            accent,
            canvas,
            switches,
            popups,
            knobs,
            rotate,
            transition,
            pause,
            idle,
            recorders,
            tabs,
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

    pub fn sync_look(&self, look: &Look) {
        self.mode.setSelectedSegmentBezelColor(Some(&look.colors.accent));
        self.mode.setSelectedSegment(isize::try_from(look.mode).unwrap_or(0));
        for swatch in &self.swatches {
            widgets::alpha(&swatch.ivars().ring, if swatch.ivars().index == look.theme { 1.0 } else { 0.0 });
        }
        let palette = look.palette();
        self.accent.setColor(&palette.accent.color(1.0));
        self.canvas.setColor(&palette.canvas.color(1.0));
        let custom = look.theme == look.custom_index();
        self.accent.setEnabled(custom);
        self.canvas.setEnabled(custom);
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
            widgets::alpha(view, if on { 1.0 } else { 0.4 });
            if let Some(control) = view.downcast_ref::<objc2_app_kit::NSControl>() {
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
        let value = if tag == VOLUME { value.clamp(0.0, 100.0) } else { value.clamp(200.0, 3000.0) };
        if let Some((slider, field)) = self.knobs.iter().find(|(slider, _)| slider.tag() == tag) {
            slider.setDoubleValue(value);
            field.show(value);
        }
        return value;
    }

    pub fn custom_colors(&self) -> Option<(Rgb, Rgb)> {
        return Some((Rgb::from_color(&self.accent.color())?, Rgb::from_color(&self.canvas.color())?));
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

fn page(mtm: MainThreadMarker, sections: &[&NSView]) -> Retained<NSView> {
    let stack = widgets::stack(mtm, sections, true, 18.0);
    stack.setAlignment(NSLayoutAttribute::Leading);
    stack.setEdgeInsets(widgets::insets(20.0, 24.0, 24.0, 24.0));
    for view in sections {
        view.leadingAnchor().constraintEqualToAnchor_constant(&stack.leadingAnchor(), 24.0).setActive(true);
        view.trailingAnchor().constraintEqualToAnchor_constant(&stack.trailingAnchor(), -24.0).setActive(true);
    }
    stack.widthAnchor().constraintEqualToConstant(WIDTH).setActive(true);
    let container = widgets::Flipped::new(mtm);
    container.addSubview(&stack);
    stack.setTranslatesAutoresizingMaskIntoConstraints(false);
    stack.topAnchor().constraintEqualToAnchor(&container.topAnchor()).setActive(true);
    stack.leadingAnchor().constraintEqualToAnchor(&container.leadingAnchor()).setActive(true);
    stack.trailingAnchor().constraintEqualToAnchor(&container.trailingAnchor()).setActive(true);
    stack.bottomAnchor().constraintLessThanOrEqualToAnchor(&container.bottomAnchor()).setActive(true);
    return Retained::into_super(container);
}

fn form(mtm: MainThreadMarker, title: &str, rows: &[Retained<NSView>]) -> Retained<NSView> {
    let card = widgets::card(mtm, &NSColor::quaternarySystemFillColor(), 10.0);
    let mut views: Vec<Retained<NSView>> = Vec::new();
    for (position, row) in rows.iter().enumerate() {
        if position > 0 {
            let line = widgets::card(mtm, &NSColor::separatorColor(), 0.0);
            line.heightAnchor().constraintEqualToConstant(0.5).setActive(true);
            views.push(Retained::into_super(line));
        }
        views.push(row.clone());
    }
    let refs: Vec<&NSView> = views.iter().map(|view| &**view).collect();
    let body = widgets::stack(mtm, &refs, true, 0.0);
    for view in &refs {
        view.leadingAnchor().constraintEqualToAnchor_constant(&body.leadingAnchor(), 0.0).setActive(true);
        view.trailingAnchor().constraintEqualToAnchor(&body.trailingAnchor()).setActive(true);
    }
    card.addSubview(&body);
    widgets::pin(&body, &card, 0.0);
    let header = widgets::label(mtm, title, 13.0, &Weight::Semibold, &NSColor::labelColor());
    let section = widgets::stack(mtm, &[&header, &card], true, 8.0);
    section.setAlignment(NSLayoutAttribute::Leading);
    card.leadingAnchor().constraintEqualToAnchor(&section.leadingAnchor()).setActive(true);
    card.trailingAnchor().constraintEqualToAnchor(&section.trailingAnchor()).setActive(true);
    header.leadingAnchor().constraintEqualToAnchor_constant(&section.leadingAnchor(), 2.0).setActive(true);
    return widgets::view(&section);
}

fn line(mtm: MainThreadMarker, title: &str, note: Option<&str>, control: &NSView) -> Retained<NSView> {
    let name = widgets::label(mtm, title, 13.0, &Weight::Regular, &NSColor::labelColor());
    let text: Retained<NSView> = match note {
        Some(note) => {
            let hint = widgets::label(mtm, note, 11.0, &Weight::Regular, &NSColor::secondaryLabelColor());
            let stack = widgets::stack(mtm, &[&name, &hint], true, 2.0);
            stack.setAlignment(NSLayoutAttribute::Leading);
            widgets::view(&stack)
        }
        None => widgets::view(&name),
    };
    let gap = widgets::spacer(mtm);
    let row = widgets::stack(mtm, &[&text, &gap, control], false, 12.0);
    row.setEdgeInsets(widgets::insets(0.0, 12.0, 0.0, 12.0));
    row.heightAnchor().constraintGreaterThanOrEqualToConstant(if note.is_some() { 50.0 } else { 40.0 }).setActive(true);
    return widgets::view(&row);
}

fn switch(mtm: MainThreadMarker, tag: isize, target: &AnyObject) -> Retained<NSSwitch> {
    let control = NSSwitch::new(mtm);
    control.setControlSize(NSControlSize::Small);
    control.setTag(tag);
    unsafe {
        control.setTarget(Some(target));
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

fn choice(mtm: MainThreadMarker, items: &[&str], tag: isize, target: &AnyObject) -> Retained<NSPopUpButton> {
    let popup = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), CGRect::ZERO, false);
    if let Some(menu) = popup.menu() {
        menu.setAutoenablesItems(false);
    }
    let titles: Vec<Retained<NSString>> = items.iter().map(|title| widgets::ns(title)).collect();
    popup.addItemsWithTitles(&NSArray::from_retained_slice(&titles));
    popup.setTag(tag);
    unsafe {
        popup.setTarget(Some(target));
        popup.setAction(Some(sel!(choose:)));
    }
    return popup;
}

fn knob(mtm: MainThreadMarker, value: f64, min: f64, max: f64, tag: isize, target: &AnyObject) -> Knob {
    let action: Sel = sel!(settingSlide:);
    let slider = unsafe {
        NSSlider::sliderWithValue_minValue_maxValue_target_action(value, min, max, Some(target), Some(action), mtm)
    };
    slider.setControlSize(NSControlSize::Small);
    slider.setTag(tag);
    slider.widthAnchor().constraintEqualToConstant(150.0).setActive(true);
    let (step, format): (f64, fn(f64) -> String) =
        if tag == DURATION { (50.0, |value| format!("{value:.0} ms")) } else { (5.0, |value| format!("{value:.0}%")) };
    let field =
        NumberField::new(mtm, Units { step, format, parse: widgets::number }, tag, 62.0, sel!(settingTyped:), target);
    field.show(value);
    return (slider, field);
}

fn pair(mtm: MainThreadMarker, knob: &Knob) -> Retained<NSView> {
    return widgets::view(&widgets::stack(mtm, &[&knob.0, &knob.1], false, 8.0));
}

fn general(mtm: MainThreadMarker, location: &str, target: &AnyObject) -> General {
    let login = switch(mtm, LOGIN, target);
    let open_library = switch(mtm, OPEN_LIBRARY, target);
    let startup = form(
        mtm,
        &tr("Startup"),
        &[
            line(mtm, &tr("Open at Login"), None, &login),
            line(mtm, &tr("Open Library at Launch"), Some(&tr("Not when LiveWall starts at login")), &open_library),
        ],
    );
    let master = switch(mtm, PAUSE, target);
    let fullscreen = switch(mtm, FULLSCREEN, target);
    let covered = switch(mtm, COVERED, target);
    let hot = switch(mtm, HOT, target);
    let release = choice(mtm, &RELEASES.map(|(title, _)| tr(title)).each_ref().map(String::as_str), RELEASE, target);
    let pause = vec![widgets::view(&fullscreen), widgets::view(&covered), widgets::view(&hot)];
    let power = form(
        mtm,
        &tr("Pause Playback"),
        &[
            line(mtm, &tr("Pause Automatically"), None, &master),
            line(mtm, &tr("When an App Is Full Screen"), None, &fullscreen),
            line(
                mtm,
                &tr("When the Desktop Is Covered"),
                Some(&tr("Uses more battery to keep track of windows")),
                &covered,
            ),
            line(mtm, &tr("When the Mac Gets Hot"), None, &hot),
            line(mtm, &tr("Free Memory When Paused"), Some(&tr("Swaps a paused video for a still frame")), &release),
        ],
    );
    let place = widgets::label(mtm, location, 12.0, &Weight::Regular, &NSColor::secondaryLabelColor());
    let change = widgets::compact(mtm, &tr("Change…"), None, None);
    change.setEnabled(false);
    let library_row = widgets::stack(mtm, &[&place, &change], false, 8.0);
    let optimize = switch(mtm, OPTIMIZE, target);
    let frame_rate =
        choice(mtm, &FRAME_RATES.map(|rate| format!("{rate} fps")).each_ref().map(String::as_str), FRAME_RATE, target);
    let library = form(
        mtm,
        &tr("Library"),
        &[
            line(mtm, &tr("Location"), None, &library_row),
            line(mtm, &tr("Copy Added Files into Library"), None, &fixed_switch(mtm)),
            line(
                mtm,
                &tr("Optimize on Import"),
                Some(&tr("Convert videos that need it for smooth playback")),
                &optimize,
            ),
            line(mtm, &tr("Frame Rate Limit"), None, &frame_rate),
        ],
    );
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

fn playback(mtm: MainThreadMarker, target: &AnyObject) -> PlaybackPage {
    let volume = knob(mtm, 100.0, 0.0, 100.0, VOLUME, target);
    let sound = switch(mtm, SOUND, target);
    let sound_card = form(
        mtm,
        &tr("Sound"),
        &[
            line(mtm, &tr("Play Wallpaper Sound"), None, &sound),
            line(mtm, &tr("Default Volume"), None, &pair(mtm, &volume)),
        ],
    );
    let animate = switch(mtm, TRANSITION, target);
    let style = choice(mtm, &STYLES.map(|(title, _)| tr(title)).each_ref().map(String::as_str), STYLE, target);
    let length = knob(mtm, 1000.0, 200.0, 3000.0, DURATION, target);
    let transition_group = vec![widgets::view(&style), widgets::view(&length.0), widgets::view(&length.1)];
    let transitions = form(
        mtm,
        &tr("Transitions"),
        &[
            line(mtm, &tr("Animate Wallpaper Changes"), None, &animate),
            line(mtm, &tr("Style"), None, &style),
            line(mtm, &tr("Duration"), None, &pair(mtm, &length)),
        ],
    );
    let every = choice(mtm, &INTERVALS.map(|(title, _)| tr(title)).each_ref().map(String::as_str), EVERY, target);
    if let Some(item) = every.itemAtIndex(4) {
        item.setEnabled(false);
    }
    let shuffle = switch(mtm, SHUFFLE, target);
    let rotate = switch(mtm, ROTATE, target);
    let rotate_group = vec![widgets::view(&every), widgets::view(&shuffle)];
    let rotation = form(
        mtm,
        &tr("Rotation"),
        &[
            line(mtm, &tr("Change Wallpaper"), Some(&tr("Cycles through the current folder")), &rotate),
            line(mtm, &tr("Every"), None, &every),
            line(mtm, &tr("Shuffle"), None, &shuffle),
        ],
    );
    return (
        page(mtm, &[&sound_card, &transitions, &rotation]),
        rotate_group,
        transition_group,
        vec![volume, length],
        vec![sound, animate, shuffle, rotate],
        vec![style, every],
    );
}

fn shortcuts(mtm: MainThreadMarker, target: &AnyObject) -> (Retained<NSView>, Vec<Retained<RecorderView>>) {
    let recorders: Vec<Retained<RecorderView>> =
        HotkeyAction::ALL.iter().map(|action| RecorderView::new(mtm, *action)).collect();
    let rows: Vec<Retained<NSView>> = recorders
        .iter()
        .map(|recorder| line(mtm, &tr(crate::hotkey::title(recorder.ivars().action)), None, recorder))
        .collect();
    let global = form(mtm, &tr("Global"), &rows);
    let restore = widgets::compact(mtm, &tr("Restore Defaults"), Some(sel!(restoreShortcuts:)), Some(target));
    let hint = widgets::label(
        mtm,
        &tr("Work in any app, on the current desktop."),
        11.0,
        &Weight::Regular,
        &NSColor::secondaryLabelColor(),
    );
    let gap = widgets::spacer(mtm);
    let footer = widgets::view(&widgets::stack(mtm, &[&hint, &gap, &restore], false, 8.0));
    let keys = |text: &str| widgets::label(mtm, text, 12.0, &Weight::Medium, &NSColor::secondaryLabelColor());
    let library = form(
        mtm,
        &tr("In the Library"),
        &[
            line(mtm, &tr("Set on Desktop"), None, &keys("1 – 9")),
            line(mtm, &tr("Set on All Desktops"), None, &keys("`")),
        ],
    );
    return (page(mtm, &[&global, &footer, &library]), recorders);
}

fn locking(mtm: MainThreadMarker, target: &AnyObject) -> Locking {
    let screen = switch(mtm, LOCK_SCREEN, target);
    let display = switch(mtm, LOCK_DISPLAY, target);
    let idle = switch(mtm, LOCK_IDLE, target);
    let after =
        choice(mtm, &IDLE_MINUTES.map(|(title, _)| tr(title)).each_ref().map(String::as_str), IDLE_AFTER, target);
    let automatic = form(
        mtm,
        &tr("Lock Automatically"),
        &[
            line(mtm, &tr("Screen Locks or Sleeps"), Some(&tr("Includes the screen saver")), &screen),
            line(mtm, &tr("External Display Connects"), Some(&tr("Includes AirPlay and mirroring")), &display),
            line(mtm, &tr("After Inactivity"), None, &idle),
            line(mtm, &tr("Inactive For"), None, &after),
        ],
    );
    let hint = widgets::label(
        mtm,
        &tr("Always locked at launch and quit. Unlocking needs Touch ID or your password."),
        11.0,
        &Weight::Regular,
        &NSColor::secondaryLabelColor(),
    );
    return Locking {
        page: page(mtm, &[&automatic, &hint]),
        idle: vec![widgets::view(&after)],
        switches: vec![screen, display, idle],
        popups: vec![after],
    };
}

type AppearancePage = (
    Retained<NSView>,
    Retained<NSSegmentedControl>,
    Vec<Retained<SwatchView>>,
    Retained<NSColorWell>,
    Retained<NSColorWell>,
);

fn appearance(mtm: MainThreadMarker, look: &Look, target: &AnyObject) -> AppearancePage {
    let labels: Vec<Retained<NSString>> = Look::MODES.iter().map(|title| widgets::ns(&tr(title))).collect();
    let mode = unsafe {
        NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
            &NSArray::from_retained_slice(&labels),
            NSSegmentSwitchTracking::SelectOne,
            Some(target),
            Some(sel!(mode:)),
            mtm,
        )
    };
    let swatches: Vec<Retained<SwatchView>> = look
        .themes
        .iter()
        .enumerate()
        .map(|(index, (name, palette))| swatch(mtm, index, name, &Theme::new(*palette)))
        .collect();
    let refs: Vec<&NSView> = swatches.iter().map(|view| &***view).collect();
    let row = widgets::stack(mtm, &refs, false, 14.0);
    row.setEdgeInsets(widgets::insets(14.0, 12.0, 12.0, 12.0));
    mode.setSelectedSegmentBezelColor(Some(&look.colors.accent));
    let well = |tag: isize| {
        let well = NSColorWell::colorWellWithStyle(NSColorWellStyle::Minimal, mtm);
        well.setSupportsAlpha(false);
        well.setTag(tag);
        unsafe {
            well.setTarget(Some(target));
            well.setAction(Some(sel!(customColor:)));
        }
        well.widthAnchor().constraintEqualToConstant(44.0).setActive(true);
        return well;
    };
    let accent = well(ACCENT);
    let canvas = well(CANVAS);
    let window = form(mtm, &tr("Appearance"), &[line(mtm, &tr("Mode"), None, &mode)]);
    let theme = form(
        mtm,
        &tr("Theme"),
        &[
            widgets::view(&row),
            line(mtm, &tr("Accent"), Some(&tr("Selection, buttons and sliders")), &accent),
            line(mtm, &tr("Canvas"), Some(&tr("Tint behind the wallpaper wall")), &canvas),
        ],
    );
    return (page(mtm, &[&window, &theme]), mode, swatches, accent, canvas);
}

fn swatch(mtm: MainThreadMarker, index: usize, name: &str, theme: &Theme) -> Retained<SwatchView> {
    let size = 30.0;
    let ring = NSView::new(mtm);
    ring.setWantsLayer(true);
    if let Some(layer) = ring.layer() {
        layer.setCornerRadius(size / 2.0 + 4.0);
        layer.setBorderWidth(2.5);
        layer.setBorderColor(Some(&widgets::cg(mtm, &theme.accent)));
    }
    let dot = NSView::new(mtm);
    dot.setWantsLayer(true);
    if let Some(layer) = dot.layer() {
        layer.setCornerRadius(size / 2.0);
        layer.setBackgroundColor(Some(&widgets::cg(mtm, &theme.accent)));
        layer.setBorderWidth(4.0);
        layer.setBorderColor(Some(&widgets::cg(mtm, &theme.canvas)));
    }
    let caption = widgets::label(mtm, &tr(name), 11.0, &Weight::Regular, &NSColor::secondaryLabelColor());
    caption.setAlignment(NSTextAlignment::Center);
    let this = SwatchView::alloc(mtm).set_ivars(SwatchIvars { index, ring: ring.clone() });
    let this: Retained<SwatchView> = unsafe { msg_send![super(this), init] };
    for view in [&*ring, &*dot, &**caption] {
        this.addSubview(view);
        view.setTranslatesAutoresizingMaskIntoConstraints(false);
        view.centerXAnchor().constraintEqualToAnchor(&this.centerXAnchor()).setActive(true);
    }
    ring.widthAnchor().constraintEqualToConstant(size + 8.0).setActive(true);
    ring.heightAnchor().constraintEqualToConstant(size + 8.0).setActive(true);
    ring.topAnchor().constraintEqualToAnchor(&this.topAnchor()).setActive(true);
    dot.widthAnchor().constraintEqualToConstant(size).setActive(true);
    dot.heightAnchor().constraintEqualToConstant(size).setActive(true);
    dot.centerYAnchor().constraintEqualToAnchor(&ring.centerYAnchor()).setActive(true);
    caption.topAnchor().constraintEqualToAnchor_constant(&ring.bottomAnchor(), 4.0).setActive(true);
    caption.bottomAnchor().constraintEqualToAnchor(&this.bottomAnchor()).setActive(true);
    this.widthAnchor().constraintEqualToConstant(54.0).setActive(true);
    return this;
}
