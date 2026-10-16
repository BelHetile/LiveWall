use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSButton, NSColor, NSColorWell, NSColorWellStyle, NSControlSize, NSControlStateValueOff, NSControlStateValueOn,
    NSEvent, NSEventModifierFlags, NSLayoutAttribute, NSPopUpButton, NSSegmentSwitchTracking, NSSegmentedControl,
    NSSlider, NSSwitch, NSTabViewController, NSTabViewControllerTabStyle, NSTabViewItem, NSTextAlignment, NSTextField,
    NSView, NSViewController, NSViewControllerTransitionOptions, NSWindow, NSWindowStyleMask, NSWindowToolbarStyle,
};
use objc2_core_foundation::{CGFloat, CGRect};
use objc2_foundation::{NSArray, NSString};

use proto::KeyCombo;

use crate::hotkey;
use crate::ui::hotkeys::{Command, Hotkeys};
use crate::ui::palette::{Look, Rgb, Theme};
use crate::ui::widgets::{self, NumberField, Units, Weight};

const WIDTH: CGFloat = 540.0;
pub const ROTATE: isize = 1;
pub const TRANSITION: isize = 2;
pub const PAUSE: isize = 3;
pub const ACCENT: isize = 1;
pub const CANVAS: isize = 2;
const VOLUME: isize = 20;
const DURATION: isize = 21;

type Knob = (Retained<NSSlider>, Retained<NumberField>);

pub struct RecorderIvars {
    command: Command,
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
    fn new(mtm: MainThreadMarker, command: Command) -> Retained<Self> {
        let label = widgets::label(mtm, "", 12.0, &Weight::Medium, &NSColor::labelColor());
        label.setAlignment(NSTextAlignment::Center);
        let this = Self::alloc(mtm).set_ivars(RecorderIvars { command, label, recording: Cell::new(false) });
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
        this.setToolTip(Some(&NSString::from_str("Click, then press a shortcut. ⌫ clears it, ⎋ cancels.")));
        return this;
    }

    fn show(&self, combo: Option<&KeyCombo>) {
        if self.ivars().recording.get() {
            return;
        }
        let (text, color) = match combo {
            Some(combo) => (hotkey::display(combo), NSColor::labelColor()),
            None => ("None".to_string(), NSColor::tertiaryLabelColor()),
        };
        self.paint(&text, &color, None);
    }

    fn paint(&self, text: &str, color: &NSColor, border: Option<&NSColor>) {
        let label = &self.ivars().label;
        label.setStringValue(&NSString::from_str(text));
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
        self.paint("Type Shortcut…", &NSColor::secondaryLabelColor(), Some(&accent));
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
        let command = self.ivars().command;
        let code = u32::from(event.keyCode());
        match code {
            53 => return self.finish(),
            51 | 117 => {
                let _ = crate::ui::rebind(command, None);
                return self.finish();
            }
            _ => {}
        }
        let Some(name) = hotkey::key_name(code) else {
            return self.paint("Unsupported Key", &NSColor::systemRedColor(), Some(&NSColor::systemRedColor()));
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
        let Ok(combo) = hotkey::parse(&parts.join("+")) else {
            return;
        };
        match crate::ui::rebind(command, Some(combo)) {
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
    rotate: Vec<Retained<NSView>>,
    transition: Vec<Retained<NSView>>,
    pause: Vec<Retained<NSView>>,
    knobs: Vec<Knob>,
    recorders: Vec<Retained<RecorderView>>,
    tabs: Retained<NSTabViewController>,
}

impl Settings {
    pub fn new(mtm: MainThreadMarker, look: &Look, hotkeys: &Hotkeys, target: &AnyObject) -> Self {
        let (general, pause) = general(mtm, target);
        let (playback, rotate, transition, knobs) = playback(mtm, target);
        let (appearance, mode, swatches, accent, canvas) = appearance(mtm, look, target);
        let (shortcuts, recorders) = shortcuts(mtm, target);
        let tabs = NSTabViewController::new(mtm);
        tabs.setTabStyle(NSTabViewControllerTabStyle::Toolbar);
        tabs.setCanPropagateSelectedChildViewControllerTitle(true);
        tabs.setTransitionOptions(NSViewControllerTransitionOptions::Crossfade);
        let pages = [
            ("General", "gearshape", &general),
            ("Appearance", "paintpalette", &appearance),
            ("Playback", "play.rectangle", &playback),
            ("Shortcuts", "keyboard", &shortcuts),
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
            controller.setTitle(Some(&NSString::from_str(title)));
            controller.setPreferredContentSize(size);
            let item = NSTabViewItem::tabViewItemWithViewController(&controller);
            item.setLabel(&NSString::from_str(title));
            item.setImage(widgets::symbol(icon, 15.0).as_deref());
            tabs.addTabViewItem(&item);
        }
        let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable;
        let window = NSWindow::windowWithContentViewController(&tabs);
        window.setStyleMask(style);
        window.setToolbarStyle(NSWindowToolbarStyle::Preference);
        unsafe { window.setReleasedWhenClosed(false) };
        window.setContentSize(size);
        let this = Self { window, mode, swatches, accent, canvas, rotate, transition, pause, knobs, recorders, tabs };
        this.sync(look);
        this.sync_shortcuts(hotkeys);
        this.toggled(ROTATE, false);
        return this;
    }

    pub fn show(&self, tab: Option<isize>) {
        if let Some(tab) = tab {
            self.tabs.setSelectedTabViewItemIndex(tab);
        }
        if !self.window.isVisible() {
            self.window.center();
        }
        self.window.makeKeyAndOrderFront(None);
        self.window.makeFirstResponder(None);
    }

    pub fn sync(&self, look: &Look) {
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

    pub fn toggled(&self, tag: isize, on: bool) {
        let group = match tag {
            ROTATE => &self.rotate,
            TRANSITION => &self.transition,
            PAUSE => &self.pause,
            _ => return,
        };
        for view in group {
            widgets::alpha(view, if on { 1.0 } else { 0.4 });
            if let Some(control) = view.downcast_ref::<objc2_app_kit::NSControl>() {
                control.setEnabled(on);
            }
        }
    }

    pub fn sync_shortcuts(&self, hotkeys: &Hotkeys) {
        for recorder in &self.recorders {
            recorder.show(hotkeys.combo(recorder.ivars().command));
        }
    }

    pub fn slid(&self, tag: isize, value: f64) {
        if let Some((_, field)) = self.knobs.iter().find(|(slider, _)| slider.tag() == tag) {
            field.show(value.round());
        }
    }

    pub fn typed(&self, tag: isize, value: f64) {
        let value = if tag == VOLUME { value.clamp(0.0, 100.0) } else { value.max(0.0) };
        if let Some((slider, field)) = self.knobs.iter().find(|(slider, _)| slider.tag() == tag) {
            slider.setDoubleValue(value);
            field.show(value);
        }
    }

    pub fn custom_colors(&self) -> Option<(Rgb, Rgb)> {
        return Some((Rgb::from_color(&self.accent.color())?, Rgb::from_color(&self.canvas.color())?));
    }
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

fn switch(mtm: MainThreadMarker, on: bool, tag: isize, target: &AnyObject) -> Retained<NSSwitch> {
    let control = NSSwitch::new(mtm);
    control.setState(if on { NSControlStateValueOn } else { NSControlStateValueOff });
    control.setControlSize(NSControlSize::Small);
    control.setTag(tag);
    unsafe {
        control.setTarget(Some(target));
        control.setAction(Some(sel!(toggle:)));
    }
    return control;
}

fn choice(mtm: MainThreadMarker, items: &[&str], selected: isize, target: &AnyObject) -> Retained<NSPopUpButton> {
    let popup = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), CGRect::ZERO, false);
    let titles: Vec<Retained<NSString>> = items.iter().map(|title| NSString::from_str(title)).collect();
    popup.addItemsWithTitles(&NSArray::from_retained_slice(&titles));
    popup.selectItemAtIndex(selected);
    unsafe {
        popup.setTarget(Some(target));
        popup.setAction(Some(sel!(noop:)));
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

fn general(mtm: MainThreadMarker, target: &AnyObject) -> (Retained<NSView>, Vec<Retained<NSView>>) {
    let startup = form(
        mtm,
        "Startup",
        &[
            line(mtm, "Open at Login", None, &switch(mtm, true, 0, target)),
            line(mtm, "Show in Menu Bar", None, &switch(mtm, true, 0, target)),
            line(
                mtm,
                "Show in Dock",
                Some("Hide to keep LiveWall in the menu bar only"),
                &switch(mtm, false, 0, target),
            ),
        ],
    );
    let battery = switch(mtm, true, 0, target);
    let fullscreen = switch(mtm, true, 0, target);
    let covered = switch(mtm, false, 0, target);
    let pause = vec![widgets::view(&battery), widgets::view(&fullscreen), widgets::view(&covered)];
    let power = form(
        mtm,
        "Pause Playback",
        &[
            line(mtm, "Pause Automatically", None, &switch(mtm, true, PAUSE, target)),
            line(mtm, "On Battery Power", None, &battery),
            line(mtm, "When an App Is Full Screen", None, &fullscreen),
            line(mtm, "When the Desktop Is Covered", Some("Uses more battery to keep track of windows"), &covered),
        ],
    );
    let location = widgets::label(mtm, "~/Movies/LiveWall", 12.0, &Weight::Regular, &NSColor::secondaryLabelColor());
    let change = unsafe {
        NSButton::buttonWithTitle_target_action(&NSString::from_str("Change…"), Some(target), Some(sel!(noop:)), mtm)
    };
    change.setControlSize(NSControlSize::Small);
    let library_row = widgets::stack(mtm, &[&location, &change], false, 8.0);
    let library = form(
        mtm,
        "Library",
        &[
            line(mtm, "Location", None, &library_row),
            line(mtm, "Copy Added Files into Library", None, &switch(mtm, true, 0, target)),
        ],
    );
    return (page(mtm, &[&startup, &power, &library]), pause);
}

type PlaybackPage = (Retained<NSView>, Vec<Retained<NSView>>, Vec<Retained<NSView>>, Vec<Knob>);

fn playback(mtm: MainThreadMarker, target: &AnyObject) -> PlaybackPage {
    let volume = knob(mtm, 60.0, 0.0, 100.0, VOLUME, target);
    let sound = form(
        mtm,
        "Sound",
        &[
            line(mtm, "Play Wallpaper Sound", None, &switch(mtm, false, 0, target)),
            line(mtm, "Default Volume", None, &pair(mtm, &volume)),
        ],
    );
    let style = choice(mtm, &["Fade", "Wipe", "Grow", "Wave"], 0, target);
    let length = knob(mtm, 800.0, 200.0, 3000.0, DURATION, target);
    let transition_group = vec![widgets::view(&style), widgets::view(&length.0), widgets::view(&length.1)];
    let transitions = form(
        mtm,
        "Transitions",
        &[
            line(mtm, "Animate Wallpaper Changes", None, &switch(mtm, true, TRANSITION, target)),
            line(mtm, "Style", None, &style),
            line(mtm, "Duration", None, &pair(mtm, &length)),
        ],
    );
    let every = choice(mtm, &["5 Minutes", "15 Minutes", "1 Hour", "Daily", "On Login"], 2, target);
    let shuffle = switch(mtm, true, 0, target);
    let rotate_group = vec![widgets::view(&every), widgets::view(&shuffle)];
    let rotation = form(
        mtm,
        "Rotation",
        &[
            line(
                mtm,
                "Change Wallpaper",
                Some("Cycles through the current folder"),
                &switch(mtm, false, ROTATE, target),
            ),
            line(mtm, "Every", None, &every),
            line(mtm, "Shuffle", None, &shuffle),
        ],
    );
    return (page(mtm, &[&sound, &transitions, &rotation]), rotate_group, transition_group, vec![volume, length]);
}

fn shortcuts(mtm: MainThreadMarker, target: &AnyObject) -> (Retained<NSView>, Vec<Retained<RecorderView>>) {
    let recorders: Vec<Retained<RecorderView>> =
        Command::ALL.iter().map(|command| RecorderView::new(mtm, *command)).collect();
    let rows: Vec<Retained<NSView>> =
        recorders.iter().map(|recorder| line(mtm, recorder.ivars().command.title(), None, recorder)).collect();
    let global = form(mtm, "Global", &rows);
    let restore = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str("Restore Defaults"),
            Some(target),
            Some(sel!(restoreShortcuts:)),
            mtm,
        )
    };
    restore.setControlSize(NSControlSize::Small);
    let hint = widgets::label(
        mtm,
        "Work in any app, on the current desktop.",
        11.0,
        &Weight::Regular,
        &NSColor::secondaryLabelColor(),
    );
    let gap = widgets::spacer(mtm);
    let footer = widgets::view(&widgets::stack(mtm, &[&hint, &gap, &restore], false, 8.0));
    let keys = |text: &str| widgets::label(mtm, text, 12.0, &Weight::Medium, &NSColor::secondaryLabelColor());
    let library = form(
        mtm,
        "In the Library",
        &[line(mtm, "Set on Desktop", None, &keys("1 – 9")), line(mtm, "Set on All Desktops", None, &keys("`"))],
    );
    return (page(mtm, &[&global, &footer, &library]), recorders);
}

type AppearancePage = (
    Retained<NSView>,
    Retained<NSSegmentedControl>,
    Vec<Retained<SwatchView>>,
    Retained<NSColorWell>,
    Retained<NSColorWell>,
);

fn appearance(mtm: MainThreadMarker, look: &Look, target: &AnyObject) -> AppearancePage {
    let labels: Vec<Retained<NSString>> = Look::MODES.iter().map(|title| NSString::from_str(title)).collect();
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
    let window = form(mtm, "Appearance", &[line(mtm, "Mode", None, &mode)]);
    let theme = form(
        mtm,
        "Theme",
        &[
            widgets::view(&row),
            line(mtm, "Accent", Some("Selection, buttons and sliders"), &accent),
            line(mtm, "Canvas", Some("Tint behind the wallpaper wall"), &canvas),
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
    let caption = widgets::label(mtm, name, 11.0, &Weight::Regular, &NSColor::secondaryLabelColor());
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
