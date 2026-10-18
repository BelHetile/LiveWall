use std::cell::{Cell, RefCell};

use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, Message, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBezelStyle, NSButton, NSColor, NSControl, NSControlStateValueOff, NSControlStateValueOn, NSEvent,
    NSEventModifierFlags, NSFont, NSForegroundColorAttributeName, NSGridCell, NSGridCellPlacement, NSGridRowAlignment,
    NSGridView, NSImage, NSLineBreakMode, NSPopUpButton, NSResponder, NSSlider, NSStackView, NSTabViewController,
    NSTabViewControllerTabStyle, NSTabViewItem, NSTextField, NSUserInterfaceLayoutOrientation, NSView,
    NSViewController, NSWindow, NSWindowStyleMask, NSWindowToolbarStyle,
};
use objc2_core_foundation::{CGFloat, CGRect, CGSize};
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSDictionary, NSEdgeInsets, NSObject, NSObjectProtocol,
    NSString,
};
use proto::{HotkeyAction, Request, ResponseBody};
use serde_json::{Value, json};

use crate::app;
use crate::hotkey;
use crate::i18n::{self, Arg, tr, trf};
use crate::model::AppearanceMode;
use crate::ui::inspector::{dragging, value_field};
use crate::ui::{ns, submit, windows};

const INSET: CGFloat = 20.0;
const SECTION_GAP: CGFloat = 14.0;
const NOTE_WIDTH: CGFloat = 300.0;
const CHECKBOX_INDENT: CGFloat = 20.0;
const SLIDER_WIDTH: CGFloat = 160.0;
const ESCAPE: u16 = 53;
const DELETE: u16 = 51;
const FORWARD_DELETE: u16 = 117;

const LOGIN: isize = 1;
const OPEN_LIBRARY: isize = 2;
const PAUSE: isize = 3;
const FULLSCREEN: isize = 4;
const COVERED: isize = 5;
const HOT: isize = 6;
const RELEASE: isize = 7;
const OPTIMIZE: isize = 8;
const FRAME_RATE: isize = 9;
const SOUND: isize = 10;
const VOLUME: isize = 11;
const TRANSITION: isize = 12;
const STYLE: isize = 13;
const DURATION: isize = 14;
const ROTATE: isize = 15;
const EVERY: isize = 16;
const SHUFFLE: isize = 17;
const LOCK_SCREEN: isize = 18;
const LOCK_IDLE: isize = 19;
const IDLE_AFTER: isize = 20;
const HIDE_DESKTOP: isize = 21;
const APPEARANCE: isize = 22;

const MODES: [(AppearanceMode, &str, &str); 3] = [
    (AppearanceMode::System, "system", i18n::key("System")),
    (AppearanceMode::Light, "light", i18n::key("Light")),
    (AppearanceMode::Dark, "dark", i18n::key("Dark")),
];
const FRAME_RATES: [u32; 6] = [24, 30, 48, 60, 90, 120];
const RELEASES: [(&str, u32); 5] = [
    (i18n::key("Never"), 0),
    (i18n::key("After 15 Seconds"), 15),
    (i18n::key("After 30 Seconds"), 30),
    (i18n::key("After 1 Minute"), 60),
    (i18n::key("After 5 Minutes"), 300),
];
const STYLES: [(&str, &str); 4] =
    [(i18n::key("Fade"), "fade"), (i18n::key("Wipe"), "wipe"), (i18n::key("Grow"), "grow"), (i18n::key("Wave"), "wave")];
const INTERVALS: [(&str, u32); 4] = [
    (i18n::key("5 Minutes"), 5),
    (i18n::key("15 Minutes"), 15),
    (i18n::key("1 Hour"), 60),
    (i18n::key("Daily"), 1440),
];
const IDLE_MINUTES: [(&str, u32); 5] = [
    (i18n::key("1 Minute"), 1),
    (i18n::key("5 Minutes"), 5),
    (i18n::key("15 Minutes"), 15),
    (i18n::key("30 Minutes"), 30),
    (i18n::key("1 Hour"), 60),
];
const PAUSE_KEYS: [&str; 3] = ["pause_when_fullscreen", "pause_when_maximized", "pause_when_hot"];

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct SettingsTarget;

    unsafe impl NSObjectProtocol for SettingsTarget {}

    impl SettingsTarget {
        #[unsafe(method(toggle:))]
        fn toggle(&self, sender: &NSButton) {
            flag(sender.tag(), sender.state() == NSControlStateValueOn);
        }

        #[unsafe(method(choose:))]
        fn choose(&self, sender: &NSPopUpButton) {
            choose(self.mtm(), sender.tag(), usize::try_from(sender.indexOfSelectedItem()).unwrap_or(0));
        }

        #[unsafe(method(slide:))]
        fn slide(&self, sender: &NSSlider) {
            let tag = sender.tag();
            let value = sender.doubleValue().round();
            with_panel(|panel| panel.show_value(tag, value));
            if !dragging(self.mtm()) {
                number(tag, value);
            }
        }

        #[unsafe(method(typed:))]
        fn typed(&self, sender: &NSTextField) {
            let tag = sender.tag();
            let value = if tag == VOLUME { sender.doubleValue().clamp(0.0, 100.0) } else { sender.doubleValue().clamp(200.0, 3000.0) };
            with_panel(|panel| panel.show_value(tag, value));
            number(tag, value);
        }

        #[unsafe(method(restoreShortcuts:))]
        fn restore_shortcuts(&self, _sender: Option<&AnyObject>) {
            app::with(app::App::restore_hotkeys);
            sync();
        }
    }
);

struct RecorderIvars {
    action: HotkeyAction,
    recording: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSButton, NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = RecorderIvars]
    struct Recorder;

    unsafe impl NSObjectProtocol for Recorder {}

    impl Recorder {
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
            if self.ivars().recording.get() {
                self.key(event);
                true
            } else {
                false
            }
        }
    }
);

impl Recorder {
    fn new(mtm: MainThreadMarker, action: HotkeyAction) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RecorderIvars { action, recording: Cell::new(false) });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setBezelStyle(NSBezelStyle::Push);
        this.widthAnchor().constraintGreaterThanOrEqualToConstant(SLIDER_WIDTH).setActive(true);
        return this;
    }

    fn show(&self, combo: Option<&proto::KeyCombo>) {
        if self.ivars().recording.get() {
            return;
        }
        self.setHighlighted(false);
        self.setTitle(&ns(&combo.map_or_else(|| tr("None"), hotkey::display)));
    }

    fn warn(&self, message: &str) {
        let key: &NSAttributedStringKey = unsafe { NSForegroundColorAttributeName };
        let color = NSColor::systemRedColor();
        let attributes = NSDictionary::from_slices(&[key], &[&*color as &AnyObject]);
        let title =
            unsafe { NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &ns(message), Some(&attributes)) };
        self.setAttributedTitle(&title);
    }

    fn begin(&self) {
        self.ivars().recording.set(true);
        self.setHighlighted(true);
        self.setTitle(&ns(&tr("Type Shortcut…")));
        recording(true);
    }

    fn end(&self) {
        self.ivars().recording.set(false);
        recording(false);
    }

    fn finish(&self) {
        if let Some(window) = self.window() {
            window.makeFirstResponder(None);
        }
    }

    fn key(&self, event: &NSEvent) {
        let action = self.ivars().action;
        let code = event.keyCode();
        match code {
            ESCAPE => return self.finish(),
            DELETE | FORWARD_DELETE => {
                let _ = rebind(action, None);
                return self.finish();
            }
            _ => {}
        }
        let Some(name) = hotkey::key_name(u32::from(code)) else {
            return self.warn(&tr("Unsupported Key"));
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
        match rebind(action, Some(parts.join("+"))) {
            Ok(()) => self.finish(),
            Err(message) => self.warn(&message),
        }
    }
}

fn rebind(action: HotkeyAction, combo: Option<String>) -> Result<(), String> {
    let response = app::with(|app| app.set_hotkey(action, combo)).ok_or_else(|| tr("LiveWall is busy"))?;
    return match response.body {
        ResponseBody::Ok { .. } => Ok(()),
        ResponseBody::Err(body) => Err(body.message),
    };
}

fn recording(on: bool) {
    DispatchQueue::main().exec_async(move || {
        app::with(|app| app.record_hotkey(on));
        if !on {
            sync();
        }
    });
}

struct Panel {
    window: Retained<NSWindow>,
    checks: Vec<Retained<NSButton>>,
    popups: Vec<Retained<NSPopUpButton>>,
    knobs: Vec<(Retained<NSSlider>, Retained<NSTextField>)>,
    groups: Vec<(isize, Vec<Retained<NSControl>>)>,
    recorders: Vec<Retained<Recorder>>,
}

thread_local! {
    static PANEL: RefCell<Option<Panel>> = const { RefCell::new(None) };
    static TARGET: RefCell<Option<Retained<SettingsTarget>>> = const { RefCell::new(None) };
}

fn with_panel(body: impl FnOnce(&Panel)) {
    PANEL.with(|panel| {
        if let Ok(panel) = panel.try_borrow()
            && let Some(panel) = panel.as_ref()
        {
            body(panel);
        }
    });
}

pub fn window() -> Option<Retained<NSWindow>> {
    return PANEL.with(|panel| {
        return panel.try_borrow().ok().and_then(|panel| panel.as_ref().map(|panel| panel.window.clone()));
    });
}

pub fn visible() -> bool {
    return window().is_some_and(|window| window.isVisible());
}

pub fn resigned(resigned: &NSWindow) {
    if let Some(own) = window()
        && *own == *resigned
    {
        own.makeFirstResponder(None);
    }
}

fn target(mtm: MainThreadMarker) -> Retained<SettingsTarget> {
    return TARGET.with(|cell| {
        return cell.borrow_mut().get_or_insert_with(|| unsafe { msg_send![SettingsTarget::alloc(mtm), init] }).clone();
    });
}

fn send(request: Request) {
    submit(move |app| Some(app.dispatch(&request)));
    DispatchQueue::main().exec_async(sync);
}

fn patch(value: Value) {
    send(Request::SettingsSet { patch: value });
}

fn flag(tag: isize, on: bool) {
    match tag {
        LOGIN => send(Request::LoginItemSet { enabled: on }),
        OPEN_LIBRARY => patch(json!({ "open_library_at_launch": on })),
        FULLSCREEN => patch(json!({ "pause_when_fullscreen": on })),
        COVERED => patch(json!({ "pause_when_maximized": on })),
        HOT => patch(json!({ "pause_when_hot": on })),
        OPTIMIZE => patch(json!({ "optimize_on_import": on })),
        SOUND => patch(json!({ "muted": !on })),
        SHUFFLE => patch(json!({ "shuffle": on })),
        ROTATE => patch(json!({ "auto_rotate": on })),
        LOCK_SCREEN => patch(json!({ "privacy": { "lock_on_screen_lock": on } })),
        LOCK_IDLE => patch(json!({ "privacy": { "lock_on_idle": on } })),
        HIDE_DESKTOP => patch(json!({ "privacy": { "hide_desktop": on } })),
        PAUSE => patch(Value::Object(PAUSE_KEYS.iter().map(|key| ((*key).to_string(), json!(on))).collect())),
        TRANSITION => patch(json!({ "transition": { "kind": if on { Value::Null } else { json!("none") } } })),
        _ => {}
    }
}

fn choose(mtm: MainThreadMarker, tag: isize, index: usize) {
    let value = match tag {
        APPEARANCE => MODES.get(index).map(|(mode, name, _)| {
            windows::set_appearance(mtm, *mode);
            return json!({ "appearance": { "mode": name } });
        }),
        FRAME_RATE => FRAME_RATES.get(index).map(|rate| json!({ "optimize_fps_limit": rate })),
        RELEASE => RELEASES.get(index).map(|(_, seconds)| json!({ "stop_after_seconds": seconds })),
        STYLE => STYLES.get(index).map(|(_, kind)| json!({ "transition": { "kind": kind } })),
        EVERY => INTERVALS.get(index).map(|(_, minutes)| json!({ "rotate_minutes": minutes })),
        IDLE_AFTER => IDLE_MINUTES.get(index).map(|(_, minutes)| json!({ "privacy": { "idle_minutes": minutes } })),
        _ => None,
    };
    match value {
        Some(value) => patch(value),
        None => DispatchQueue::main().exec_async(sync),
    }
}

fn number(tag: isize, value: f64) {
    match tag {
        VOLUME => patch(json!({ "volume": (value / 100.0).clamp(0.0, 1.0) })),
        DURATION => patch(json!({ "transition": { "duration": value.max(0.0) / 1000.0 } })),
        _ => {}
    }
}

impl Panel {
    fn show_value(&self, tag: isize, value: f64) {
        if let Some((slider, field)) = self.knobs.iter().find(|(slider, _)| slider.tag() == tag) {
            slider.setDoubleValue(value);
            field.setDoubleValue(value);
        }
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn sync(&self, values: &Value, hotkeys: &proto::Hotkeys) {
        let get = |path: &[&str]| path.iter().try_fold(values, |value, key| value.get(*key));
        let on = |path: &[&str]| get(path).and_then(Value::as_bool).unwrap_or(false);
        let whole = |path: &[&str]| get(path).and_then(Value::as_u64).and_then(|value| u32::try_from(value).ok()).unwrap_or(0);
        let kind = get(&["transition", "kind"]).and_then(Value::as_str);
        let master = PAUSE_KEYS.iter().any(|key| on(&[key]));
        let animate = kind != Some("none");
        let rotate = on(&["auto_rotate"]);
        let idle = on(&["privacy", "lock_on_idle"]);
        for check in &self.checks {
            let state = match check.tag() {
                LOGIN => on(&["launch_at_login"]),
                OPEN_LIBRARY => on(&["open_library_at_launch"]),
                PAUSE => master,
                FULLSCREEN => on(&["pause_when_fullscreen"]),
                COVERED => on(&["pause_when_maximized"]),
                HOT => on(&["pause_when_hot"]),
                OPTIMIZE => on(&["optimize_on_import"]),
                SOUND => !on(&["muted"]),
                TRANSITION => animate,
                ROTATE => rotate,
                SHUFFLE => on(&["shuffle"]),
                LOCK_SCREEN => on(&["privacy", "lock_on_screen_lock"]),
                LOCK_IDLE => idle,
                HIDE_DESKTOP => on(&["privacy", "hide_desktop"]),
                _ => continue,
            };
            check.setState(if state { NSControlStateValueOn } else { NSControlStateValueOff });
        }
        let mode = get(&["appearance", "mode"]).and_then(Value::as_str).unwrap_or("system");
        let rate = whole(&["optimize_fps_limit"]);
        let release = whole(&["stop_after_seconds"]);
        let interval = whole(&["rotate_minutes"]);
        let idle_minutes = whole(&["privacy", "idle_minutes"]);
        for popup in &self.popups {
            match popup.tag() {
                APPEARANCE => pick(popup, MODES.len(), MODES.iter().position(|(_, name, _)| *name == mode), ""),
                FRAME_RATE => pick(popup, FRAME_RATES.len(), FRAME_RATES.iter().position(|value| *value == rate), &format!("{rate} fps")),
                RELEASE => pick(
                    popup,
                    RELEASES.len(),
                    RELEASES.iter().position(|(_, value)| *value == release),
                    &trf("After %ld Seconds", &[Arg::Int(i64::from(release))]),
                ),
                STYLE => pick(popup, STYLES.len(), STYLES.iter().position(|(_, name)| Some(*name) == kind).or(Some(0)), ""),
                EVERY => pick(
                    popup,
                    INTERVALS.len(),
                    INTERVALS.iter().position(|(_, value)| *value == interval),
                    &trf("%ld Minutes", &[Arg::Int(i64::from(interval))]),
                ),
                IDLE_AFTER => pick(
                    popup,
                    IDLE_MINUTES.len(),
                    IDLE_MINUTES.iter().position(|(_, value)| *value == idle_minutes),
                    &trf("%ld Minutes", &[Arg::Int(i64::from(idle_minutes))]),
                ),
                _ => {}
            }
        }
        for (slider, _) in &self.knobs {
            let value = match slider.tag() {
                VOLUME => get(&["volume"]).and_then(Value::as_f64).unwrap_or(1.0) * 100.0,
                DURATION => {
                    get(&["transition", "duration"]).and_then(Value::as_f64).unwrap_or(proto::Transition::DEFAULT_DURATION)
                        * 1000.0
                }
                _ => continue,
            };
            self.show_value(slider.tag(), value.round());
        }
        for (tag, controls) in &self.groups {
            let enabled = match *tag {
                PAUSE => master,
                TRANSITION => animate,
                ROTATE => rotate,
                LOCK_IDLE => idle,
                _ => true,
            };
            for control in controls {
                control.setEnabled(enabled);
            }
        }
        for recorder in &self.recorders {
            recorder.show(hotkeys.get(recorder.ivars().action));
        }
    }
}

fn pick(popup: &NSPopUpButton, base: usize, index: Option<usize>, extra: &str) {
    let base = isize::try_from(base).unwrap_or(0);
    while popup.numberOfItems() > base {
        popup.removeItemAtIndex(base);
    }
    match index.and_then(|index| isize::try_from(index).ok()) {
        Some(index) => popup.selectItemAtIndex(index),
        None => {
            popup.addItemWithTitle(&ns(extra));
            popup.selectItemAtIndex(base);
        }
    }
}

pub fn sync() {
    let Some((values, hotkeys)) =
        app::with(|app| (serde_json::to_value(app.settings()).unwrap_or(Value::Null), app.settings().hotkeys.clone()))
    else {
        return;
    };
    with_panel(|panel| panel.sync(&values, &hotkeys));
}

struct Builder {
    mtm: MainThreadMarker,
    target: Retained<SettingsTarget>,
    checks: Vec<Retained<NSButton>>,
    popups: Vec<Retained<NSPopUpButton>>,
    knobs: Vec<(Retained<NSSlider>, Retained<NSTextField>)>,
    groups: Vec<(isize, Vec<Retained<NSControl>>)>,
    recorders: Vec<Retained<Recorder>>,
}

fn control<T: AsRef<NSControl>>(view: &Retained<T>) -> Retained<NSControl> {
    let control: &NSControl = (**view).as_ref();
    return control.retain();
}

fn view<T: AsRef<NSView>>(view: &Retained<T>) -> Retained<NSView> {
    let view: &NSView = (**view).as_ref();
    return view.retain();
}

impl Builder {
    fn check(&mut self, title: &str, tag: isize) -> Retained<NSButton> {
        let button = unsafe {
            NSButton::checkboxWithTitle_target_action(&ns(title), Some(&self.target), Some(sel!(toggle:)), self.mtm)
        };
        button.setTag(tag);
        self.checks.push(button.clone());
        return button;
    }

    fn choice(&mut self, items: &[String], tag: isize) -> Retained<NSPopUpButton> {
        let popup = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(self.mtm), CGRect::ZERO, false);
        let titles: Vec<Retained<NSString>> = items.iter().map(|title| ns(title)).collect();
        popup.addItemsWithTitles(&NSArray::from_retained_slice(&titles));
        popup.setTag(tag);
        unsafe {
            popup.setTarget(Some(&self.target));
            popup.setAction(Some(sel!(choose:)));
        }
        self.popups.push(popup.clone());
        return popup;
    }

    fn knob(&mut self, min: f64, max: f64, suffix: &str, tag: isize) -> (Retained<NSView>, Vec<Retained<NSControl>>) {
        let slider = unsafe {
            NSSlider::sliderWithValue_minValue_maxValue_target_action(min, min, max, Some(&self.target), Some(sel!(slide:)), self.mtm)
        };
        slider.setTag(tag);
        slider.widthAnchor().constraintEqualToConstant(SLIDER_WIDTH).setActive(true);
        let field = value_field(self.mtm, "", suffix, 0);
        field.setTag(tag);
        unsafe {
            field.setTarget(Some(&self.target));
            field.setAction(Some(sel!(typed:)));
        }
        let controls = vec![control(&slider), control(&field)];
        self.knobs.push((slider.clone(), field.clone()));
        return (row(self.mtm, &[&slider, &field]), controls);
    }

    fn group(&mut self, tag: isize, controls: Vec<Retained<NSControl>>) {
        self.groups.push((tag, controls));
    }
}

struct Form {
    grid: Retained<NSGridView>,
    sections: usize,
}

impl Form {
    fn new(mtm: MainThreadMarker) -> Self {
        let grid = NSGridView::new(mtm);
        grid.setRowAlignment(NSGridRowAlignment::FirstBaseline);
        grid.setColumnSpacing(8.0);
        grid.setRowSpacing(8.0);
        return Self { grid, sections: 0 };
    }

    fn add(&mut self, title: Option<&str>, control: &NSView) {
        let mtm = self.grid.mtm();
        let label: Retained<NSView> = match title {
            Some(title) => view(&NSTextField::labelWithString(&ns(&format!("{title}:")), mtm)),
            None => NSGridCell::emptyContentView(mtm),
        };
        let views: [&NSView; 2] = [&label, control];
        self.grid.addRowWithViews(&NSArray::from_slice(&views));
    }

    fn last_row_padding(&self, padding: CGFloat) {
        self.grid.rowAtIndex(self.grid.numberOfRows() - 1).setTopPadding(padding);
    }

    fn section(&mut self, title: &str, control: &NSView) {
        self.add(Some(title), control);
        if self.sections > 0 {
            self.last_row_padding(SECTION_GAP);
        }
        self.sections += 1;
    }

    fn line(&mut self, title: &str, control: &NSView) {
        self.add(Some(title), control);
    }

    fn row(&mut self, control: &NSView) {
        self.add(None, control);
    }

    fn indented(&mut self, control: &NSView) {
        self.row(&inset(self.grid.mtm(), control, CHECKBOX_INDENT));
    }

    fn note(&mut self, text: &str, indent: CGFloat) {
        let mtm = self.grid.mtm();
        let label = NSTextField::wrappingLabelWithString(&ns(text), mtm);
        label.setFont(Some(&NSFont::systemFontOfSize(NSFont::smallSystemFontSize())));
        label.setTextColor(Some(&NSColor::secondaryLabelColor()));
        label.setPreferredMaxLayoutWidth(NOTE_WIDTH);
        self.row(&inset(mtm, &label, indent));
        self.last_row_padding(-4.0);
    }

    fn page(self, title: &str, symbol: &str) -> Retained<NSTabViewItem> {
        let mtm = self.grid.mtm();
        if self.grid.numberOfColumns() > 0 {
            self.grid.columnAtIndex(0).setXPlacement(NSGridCellPlacement::Trailing);
        }
        let root = NSView::new(mtm);
        self.grid.setTranslatesAutoresizingMaskIntoConstraints(false);
        root.addSubview(&self.grid);
        self.grid.topAnchor().constraintEqualToAnchor_constant(&root.topAnchor(), INSET).setActive(true);
        self.grid.bottomAnchor().constraintEqualToAnchor_constant(&root.bottomAnchor(), -INSET).setActive(true);
        self.grid.leadingAnchor().constraintEqualToAnchor_constant(&root.leadingAnchor(), INSET).setActive(true);
        self.grid.trailingAnchor().constraintEqualToAnchor_constant(&root.trailingAnchor(), -INSET).setActive(true);
        let controller = NSViewController::new(mtm);
        controller.setView(&root);
        controller.setTitle(Some(&ns(title)));
        let item = NSTabViewItem::tabViewItemWithViewController(&controller);
        item.setLabel(&ns(title));
        item.setImage(NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(symbol), None).as_deref());
        return item;
    }
}

fn inset(mtm: MainThreadMarker, view: &NSView, indent: CGFloat) -> Retained<NSView> {
    let holder = NSStackView::stackViewWithViews(&NSArray::from_slice(&[view]), mtm);
    holder.setEdgeInsets(NSEdgeInsets { top: 0.0, left: indent, bottom: 0.0, right: 0.0 });
    return Retained::into_super(holder);
}

fn row(mtm: MainThreadMarker, views: &[&NSView]) -> Retained<NSView> {
    let stack = NSStackView::stackViewWithViews(&NSArray::from_slice(views), mtm);
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    stack.setSpacing(8.0);
    return Retained::into_super(stack);
}

fn titles<T>(items: &[(&str, T)]) -> Vec<String> {
    return items.iter().map(|(title, _)| tr(title)).collect();
}

fn general(build: &mut Builder, location: &str) -> Retained<NSTabViewItem> {
    let mtm = build.mtm;
    let mut form = Form::new(mtm);
    let modes: Vec<String> = MODES.iter().map(|(_, _, title)| tr(title)).collect();
    form.section(&tr("Appearance"), &build.choice(&modes, APPEARANCE));
    form.section(&tr("Startup"), &build.check(&tr("Open at Login"), LOGIN));
    form.row(&build.check(&tr("Open Library at Launch"), OPEN_LIBRARY));
    form.note(&tr("Not when LiveWall starts at login"), CHECKBOX_INDENT);
    form.section(&tr("Pause Playback"), &build.check(&tr("Pause Automatically"), PAUSE));
    let fullscreen = build.check(&tr("When an App Is Full Screen"), FULLSCREEN);
    let covered = build.check(&tr("When the Desktop Is Covered"), COVERED);
    let hot = build.check(&tr("When the Mac Gets Hot"), HOT);
    form.indented(&fullscreen);
    form.indented(&covered);
    form.note(&tr("Uses more battery to keep track of windows"), CHECKBOX_INDENT * 2.0);
    form.indented(&hot);
    build.group(PAUSE, vec![control(&fullscreen), control(&covered), control(&hot)]);
    form.line(&tr("Free Memory When Paused"), &build.choice(&titles(&RELEASES), RELEASE));
    form.note(&tr("Swaps a paused video for a still frame"), 0.0);
    let place = NSTextField::labelWithString(&ns(location), mtm);
    place.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
    place.setAllowsExpansionToolTips(true);
    place.widthAnchor().constraintLessThanOrEqualToConstant(NOTE_WIDTH - 80.0).setActive(true);
    let change = unsafe { NSButton::buttonWithTitle_target_action(&ns(&tr("Change…")), None, None, mtm) };
    change.setEnabled(false);
    form.section(&tr("Location"), &row(mtm, &[&place, &change]));
    let copy = unsafe { NSButton::checkboxWithTitle_target_action(&ns(&tr("Copy Added Files into Library")), None, None, mtm) };
    copy.setState(NSControlStateValueOn);
    copy.setEnabled(false);
    form.line(&tr("Library"), &copy);
    form.row(&build.check(&tr("Optimize on Import"), OPTIMIZE));
    form.note(&tr("Convert videos that need it for smooth playback"), CHECKBOX_INDENT);
    let rates: Vec<String> = FRAME_RATES.iter().map(|rate| format!("{rate} fps")).collect();
    form.line(&tr("Frame Rate Limit"), &build.choice(&rates, FRAME_RATE));
    return form.page(&tr("General"), "gearshape");
}

fn playback(build: &mut Builder) -> Retained<NSTabViewItem> {
    let mut form = Form::new(build.mtm);
    form.section(&tr("Sound"), &build.check(&tr("Play Wallpaper Sound"), SOUND));
    let (volume, _) = build.knob(0.0, 100.0, "%", VOLUME);
    form.line(&tr("Default Volume"), &volume);
    form.section(&tr("Transitions"), &build.check(&tr("Animate Wallpaper Changes"), TRANSITION));
    let style = build.choice(&titles(&STYLES), STYLE);
    form.line(&tr("Style"), &style);
    let (length, mut controls) = build.knob(200.0, 3000.0, &format!("\u{a0}{}", tr("ms")), DURATION);
    form.line(&tr("Duration"), &length);
    controls.push(control(&style));
    build.group(TRANSITION, controls);
    form.section(&tr("Rotation"), &build.check(&tr("Change Wallpaper"), ROTATE));
    form.note(&tr("Cycles through the current folder"), CHECKBOX_INDENT);
    let every = build.choice(&titles(&INTERVALS), EVERY);
    form.line(&tr("Every"), &every);
    let shuffle = build.check(&tr("Shuffle"), SHUFFLE);
    form.row(&shuffle);
    build.group(ROTATE, vec![control(&every), control(&shuffle)]);
    return form.page(&tr("Playback"), "play.rectangle");
}

fn shortcuts(build: &mut Builder) -> Retained<NSTabViewItem> {
    let mtm = build.mtm;
    let mut form = Form::new(mtm);
    for (position, action) in HotkeyAction::ALL.iter().enumerate() {
        let recorder = Recorder::new(mtm, *action);
        let title = tr(hotkey::title(*action));
        if position == 0 {
            form.section(&title, &recorder);
        } else {
            form.line(&title, &recorder);
        }
        build.recorders.push(recorder);
    }
    form.note(&tr("Click, then press a shortcut. ⌫ clears it, ⎋ cancels."), 0.0);
    form.note(&tr("Work in any app, on the current desktop."), 0.0);
    let restore = unsafe {
        NSButton::buttonWithTitle_target_action(
            &ns(&tr("Restore Defaults")),
            Some(&build.target),
            Some(sel!(restoreShortcuts:)),
            mtm,
        )
    };
    form.row(&restore);
    let keys = |text: &str| -> Retained<NSView> {
        let label = NSTextField::labelWithString(&ns(text), mtm);
        label.setTextColor(Some(&NSColor::secondaryLabelColor()));
        return view(&label);
    };
    form.section(&tr("Set on Desktop"), &keys("1 – 9"));
    form.line(&tr("Set on All Desktops"), &keys("`"));
    return form.page(&tr("Shortcuts"), "keyboard");
}

fn privacy(build: &mut Builder) -> Retained<NSTabViewItem> {
    let mut form = Form::new(build.mtm);
    form.section(&tr("Lock Automatically"), &build.check(&tr("Screen Locks or Sleeps"), LOCK_SCREEN));
    form.note(&tr("Includes the screen saver"), CHECKBOX_INDENT);
    form.row(&build.check(&tr("After Inactivity"), LOCK_IDLE));
    let after = build.choice(&titles(&IDLE_MINUTES), IDLE_AFTER);
    form.line(&tr("Inactive For"), &after);
    build.group(LOCK_IDLE, vec![control(&after)]);
    form.section(&tr("While Locked"), &build.check(&tr("Take Private Wallpapers off the Desktop"), HIDE_DESKTOP));
    form.note(
        &tr("Off: the desktop and the lock screen keep showing them; the library and the menu bar hide them"),
        CHECKBOX_INDENT,
    );
    form.note(&tr("Always locked at launch and quit. Unlocking needs Touch ID or your password."), 0.0);
    return form.page(&tr("Privacy"), "lock");
}

fn location() -> String {
    return app::with(|app| app.paths().wallpapers_dir())
        .and_then(|path| path.parent().map(|parent| parent.to_string_lossy().into_owned()))
        .map(|path| match std::env::var("HOME") {
            Ok(home) if path.starts_with(&home) => path.replacen(&home, "~", 1),
            _ => path,
        })
        .unwrap_or_default();
}

fn build(mtm: MainThreadMarker) -> Panel {
    let mut build = Builder {
        mtm,
        target: target(mtm),
        checks: Vec::new(),
        popups: Vec::new(),
        knobs: Vec::new(),
        groups: Vec::new(),
        recorders: Vec::new(),
    };
    let items = [general(&mut build, &location()), playback(&mut build), shortcuts(&mut build), privacy(&mut build)];
    let pages: Vec<Retained<NSViewController>> = items.iter().filter_map(|item| item.viewController(mtm)).collect();
    let width = pages.iter().map(|page| page.view().fittingSize().width).fold(0.0, CGFloat::max);
    for page in &pages {
        page.setPreferredContentSize(CGSize::new(width, page.view().fittingSize().height));
    }
    let tabs = NSTabViewController::new(mtm);
    tabs.setTabStyle(NSTabViewControllerTabStyle::Toolbar);
    for item in &items {
        tabs.addTabViewItem(item);
    }
    let window = NSWindow::windowWithContentViewController(&tabs);
    window.setStyleMask(NSWindowStyleMask::Titled | NSWindowStyleMask::Closable);
    window.setToolbarStyle(NSWindowToolbarStyle::Preference);
    unsafe { window.setReleasedWhenClosed(false) };
    window.setDelegate(Some(ProtocolObject::from_ref(&*windows::delegate(mtm))));
    window.center();
    return Panel {
        window,
        checks: build.checks,
        popups: build.popups,
        knobs: build.knobs,
        groups: build.groups,
        recorders: build.recorders,
    };
}

#[allow(deprecated)]
pub fn show(mtm: MainThreadMarker, tab: Option<isize>) {
    PANEL.with(|panel| {
        let mut panel = panel.borrow_mut();
        let panel = panel.get_or_insert_with(|| build(mtm));
        if let Some(tab) = tab
            && let Some(tabs) = panel.window.contentViewController().and_then(|controller| controller.downcast::<NSTabViewController>().ok())
        {
            tabs.setSelectedTabViewItemIndex(tab);
        }
        panel.window.makeKeyAndOrderFront(None);
        objc2_app_kit::NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    });
    sync();
}
