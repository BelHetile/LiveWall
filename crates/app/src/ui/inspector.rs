use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{AllocAnyThread, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBox, NSBoxType, NSButton, NSCellImagePosition, NSColor, NSControl, NSControlSize, NSEvent,
    NSEventType, NSFont, NSForegroundColorAttributeName, NSFontTextStyleHeadline, NSFontWeightRegular, NSGridCellPlacement, NSGridView, NSImageView,
    NSControlStateValueOff, NSControlStateValueOn, NSLayoutAttribute, NSLayoutConstraintOrientation, NSMenu, NSSegmentDistribution, NSSegmentSwitchTracking, NSSegmentedControl, NSLineBreakMode, NSMenuItem, NSPopUpButton, NSScreen,
    NSScrollView, NSSlider, NSStackView, NSStackViewDistribution, NSStackViewGravity, NSTextAlignment, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView, NSViewController,
};
use objc2_av_foundation::{
    AVLayerVideoGravityResizeAspectFill, AVPlayerItem, AVPlayerLayer, AVPlayerLooper, AVQueuePlayer, AVURLAsset,
};
use objc2_core_foundation::{CGFloat, CGRect};
use objc2_core_media::CMTime;
use objc2_foundation::{
    NSAttributedString, NSAttributedStringKey,
    NSArray, NSDictionary, NSNumberFormatter, NSNumberFormatterStyle, NSObject, NSObjectProtocol, NSString, NSTimer,
};
use objc2_quartz_core::{CALayer, CATransaction, kCAGravityResizeAspectFill};
use proto::{ApplyTarget, FillMode, Geometry, Request};

use crate::cf::file_url;
use crate::geometry::{self, Size};
use crate::i18n::{self, Arg, tr, trf};
use crate::media::av::CM_TIMESCALE;
use crate::model::{ItemId, ItemKind};
use crate::platform::{cancel_timer, schedule_block};
use crate::ui::state::{Marker, Scope, TAG_ALL, desktop_name, duration, markers};
use crate::ui::view::{Item, Library};
use crate::ui::{Ui, folders, ns, submit, update};

const PREVIEW_ASPECT: CGFloat = 10.0 / 16.0;
const PREVIEW_RADIUS: CGFloat = 8.0;
const INSET: CGFloat = 16.0;
const VALUE_WIDTH: CGFloat = 56.0;
const VOLUME_ON: f64 = 0.6;
const VOLUME: isize = 1;
const SPEED: isize = 2;
const HORIZONTAL: isize = 3;
const VERTICAL: isize = 4;
const ZOOM: isize = 5;
const SPEED_MIN: f64 = 0.25;
const SPEED_MAX: f64 = 2.0;
const ZOOM_MIN: f64 = 1.0;
const ZOOM_MAX: f64 = 4.0;
const MODES: [(FillMode, &str); 5] = [
    (FillMode::Fill, i18n::key("Fill")),
    (FillMode::Fit, i18n::key("Fit")),
    (FillMode::Stretch, i18n::key("Stretch")),
    (FillMode::Center, i18n::key("Center")),
    (FillMode::Custom, i18n::key("Custom")),
];

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    struct Preview;

    unsafe impl NSObjectProtocol for Preview {}

    impl Preview {
        #[unsafe(method(layout))]
        fn layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            if let Some(host) = self.layer()
                && let Some(layers) = unsafe { host.sublayers() }
            {
                for layer in layers.iter() {
                    layer.setFrame(host.bounds());
                }
            }
            CATransaction::commit();
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            selected(|ui, index| ui.inspector.toggle_play(&ui.library, index));
        }
    }
);

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct Actions;

    unsafe impl NSObjectProtocol for Actions {}

    impl Actions {
        #[unsafe(method(play:))]
        fn play(&self, _sender: Option<&AnyObject>) {
            selected(|ui, index| ui.inspector.toggle_play(&ui.library, index));
        }

        #[unsafe(method(listen:))]
        fn listen(&self, _sender: Option<&AnyObject>) {
            selected(|ui, index| ui.inspector.toggle_listen(&ui.library, index));
        }

        #[unsafe(method(mute:))]
        fn mute(&self, _sender: Option<&AnyObject>) {
            selected(|ui, index| {
                let item = &mut ui.library.items[index];
                let settings = &mut item.settings;
                settings.muted = !settings.muted;
                if !settings.muted && settings.volume <= 0.0 {
                    settings.volume = VOLUME_ON;
                }
                commit(item.id, Some(settings.volume), None, Some(settings.muted));
                ui.inspector.sync(&ui.library, index);
            });
        }

        #[unsafe(method(slide:))]
        fn slide(&self, sender: &NSSlider) {
            let tag = sender.tag();
            let value = if tag == SPEED { speed_from_slider(sender.doubleValue()) } else { sender.doubleValue() };
            adjust(tag, value, dragging(self.mtm()));
        }

        #[unsafe(method(typed:))]
        fn typed(&self, sender: &NSTextField) {
            adjust(sender.tag(), sender.doubleValue(), false);
        }

        #[unsafe(method(fill:))]
        fn fill(&self, sender: &NSSegmentedControl) {
            let mode = usize::try_from(sender.selectedSegment()).ok().and_then(|index| MODES.get(index));
            let Some((mode, _)) = mode.copied() else {
                return;
            };
            selected(move |ui, index| {
                let item = &mut ui.library.items[index];
                let geometry = item.modes.get(&mode).copied().unwrap_or_default();
                item.settings.mode = mode;
                (item.settings.x, item.settings.y, item.settings.zoom) = (geometry.x, geometry.y, geometry.zoom);
                let request = Request::SetGeometry { item: item.id.to_string(), mode, geometry };
                submit(move |app| Some(app.dispatch(&request)));
                ui.inspector.sync(&ui.library, index);
            });
        }

        #[unsafe(method(destination:))]
        fn destination(&self, sender: &NSMenuItem) {
            let tag = sender.tag();
            selected(move |ui, index| {
                let chosen = &mut ui.inspector.desktops;
                match usize::try_from(tag) {
                    Ok(space) => match chosen.iter().position(|candidate| *candidate == space) {
                        Some(position) => {
                            chosen.remove(position);
                        }
                        None => {
                            chosen.push(space);
                            chosen.sort_unstable();
                        }
                    },
                    Err(_) => chosen.clear(),
                }
                ui.inspector.sync(&ui.library, index);
            });
        }

        #[unsafe(method(setWallpaper:))]
        fn set_wallpaper(&self, _sender: Option<&AnyObject>) {
            selected(|ui, index| {
                let id = ui.library.items[index].id.to_string();
                let spaces: Vec<String> = ui
                    .inspector
                    .desktops
                    .iter()
                    .filter_map(|space| ui.library.spaces.get(*space).cloned())
                    .collect();
                submit(move |app| {
                    if spaces.is_empty() {
                        return Some(app.apply(Some(&id), &ApplyTarget::Default, true, None));
                    }
                    let mut last = None;
                    for space in spaces {
                        last = Some(app.apply(Some(&id), &ApplyTarget::Space { space }, false, None));
                    }
                    return last;
                });
            });
        }

        #[unsafe(method(setLock:))]
        fn set_lock(&self, _sender: Option<&AnyObject>) {
            lock(false);
        }

        #[unsafe(method(lockFrame:))]
        fn lock_frame(&self, _sender: Option<&AnyObject>) {
            lock(true);
        }

        #[unsafe(method(scrub:))]
        fn scrub(&self, sender: &NSSlider) {
            let value = sender.doubleValue().max(0.0);
            update(move |ui, _| ui.inspector.seek(value));
        }
    }
);

fn selected(change: impl FnOnce(&mut Ui, usize)) {
    update(|ui, _| {
        if let Some(index) = ui.selected_index() {
            change(ui, index);
        }
    });
}

#[allow(clippy::cast_possible_truncation)]
pub fn lock(frame: bool) {
    selected(|ui, index| {
        let id = ui.library.items[index].id;
        let time = frame.then(|| ui.inspector.time()).flatten().map(|time| time as f32);
        submit(move |app| Some(app.set_lock(Some((id, time)))));
    });
}

fn adjust(tag: isize, value: f64, live: bool) {
    selected(move |ui, index| {
        let item = &mut ui.library.items[index];
        let (width, height) = reach(item);
        let span = if tag == HORIZONTAL { width } else { height };
        let id = item.id;
        let mode = item.settings.mode;
        let settings = &mut item.settings;
        match tag {
            VOLUME => {
                settings.volume = (value / 100.0).clamp(0.0, 1.0);
                settings.muted = settings.volume <= 0.0;
            }
            SPEED => settings.speed = ((value * 20.0).round() / 20.0).max(0.05),
            HORIZONTAL | VERTICAL => {
                let value = if span > 0.0 { (value / span).clamp(-1.0, 1.0) } else { 0.0 };
                if tag == HORIZONTAL {
                    settings.x = value;
                } else {
                    settings.y = value;
                }
            }
            ZOOM => settings.zoom = value.max(0.1),
            _ => {}
        }
        if !live {
            match tag {
                VOLUME => commit(id, Some(settings.volume), None, Some(settings.muted)),
                SPEED => commit(id, None, Some(settings.speed), None),
                _ => {
                    let geometry = Geometry { x: settings.x, y: settings.y, zoom: settings.zoom };
                    let request = Request::SetGeometry { item: id.to_string(), mode, geometry };
                    submit(move |app| Some(app.dispatch(&request)));
                }
            }
        }
        ui.inspector.sync(&ui.library, index);
    });
}

fn speed_to_slider(speed: f64) -> f64 {
    if speed <= 1.0 {
        return (speed - SPEED_MIN) / (1.0 - SPEED_MIN) * 0.5;
    }
    return 0.5 + (speed - 1.0) / (SPEED_MAX - 1.0) * 0.5;
}

fn speed_from_slider(position: f64) -> f64 {
    if position <= 0.5 {
        return SPEED_MIN + position / 0.5 * (1.0 - SPEED_MIN);
    }
    return 1.0 + (position - 0.5) / 0.5 * (SPEED_MAX - 1.0);
}

fn tagged(slider: &NSSlider, field: &NSTextField, tag: isize, target: &AnyObject) {
    slider.setTag(tag);
    field.setTag(tag);
    wire(slider, target, sel!(slide:));
    wire(field, target, sel!(typed:));
}

pub fn dragging(mtm: MainThreadMarker) -> bool {
    let event = NSApplication::sharedApplication(mtm).currentEvent();
    return event
        .is_some_and(|event| matches!(event.r#type(), NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged));
}

#[allow(clippy::cast_possible_truncation)]
fn commit(id: ItemId, volume: Option<f64>, speed: Option<f64>, muted: Option<bool>) {
    let request = Request::SetItem {
        item: id.to_string(),
        volume: volume.map(|value| value as f32),
        speed: speed.map(|value| value as f32),
        muted,
        still_time: None,
    };
    submit(move |app| Some(app.dispatch(&request)));
}

fn wire(control: &NSControl, target: &AnyObject, action: Sel) {
    unsafe {
        control.setTarget(Some(target));
        control.setAction(Some(action));
    }
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    struct Flipped;

    unsafe impl NSObjectProtocol for Flipped {}

    impl Flipped {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

type Shown = (ItemId, bool, bool, usize);

struct Controls {
    play: Retained<NSButton>,
    listen: Retained<NSButton>,
    markers: Retained<NSStackView>,
    destination: Retained<NSPopUpButton>,
    scrub: Retained<NSSlider>,
    time: Retained<NSTextField>,
    mute: Retained<NSButton>,
    volume: Retained<NSSlider>,
    volume_value: Retained<NSTextField>,
    speed: Retained<NSSlider>,
    speed_value: Retained<NSTextField>,
    mode: Retained<NSSegmentedControl>,
    offsets: [(Retained<NSSlider>, Retained<NSTextField>); 3],
    folder: Retained<NSPopUpButton>,
    reset: Retained<NSButton>,
}

pub struct Inspector {
    pub controller: Retained<NSViewController>,
    content: Retained<NSStackView>,
    empty: Retained<NSTextField>,
    scroll: Retained<NSScrollView>,
    target: Retained<Actions>,
    shown: Option<Shown>,
    controls: Option<Controls>,
    scope: Scope,
    desktops: Vec<usize>,
    player: Option<(Retained<AVQueuePlayer>, Retained<AVPlayerLooper>)>,
    timer: Option<Retained<NSTimer>>,
    playing: bool,
    listening: Option<ItemId>,
    visible: bool,
}

fn up<T: Message + AsRef<NSView>>(view: &Retained<T>) -> Retained<NSView> {
    let view: &NSView = (**view).as_ref();
    return view.retain();
}

fn font(size: CGFloat) -> Retained<NSFont> {
    return NSFont::systemFontOfSize(size);
}

fn label(mtm: MainThreadMarker, text: &str, font: &NSFont, color: &NSColor) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&ns(text), mtm);
    label.setFont(Some(font));
    label.setTextColor(Some(color));
    label.setAllowsExpansionToolTips(true);
    return label;
}

fn stack(mtm: MainThreadMarker, views: &[&NSView], vertical: bool) -> Retained<NSStackView> {
    let stack = NSStackView::new(mtm);
    stack.setOrientation(if vertical {
        NSUserInterfaceLayoutOrientation::Vertical
    } else {
        NSUserInterfaceLayoutOrientation::Horizontal
    });
    stack.setAlignment(if vertical { NSLayoutAttribute::Leading } else { NSLayoutAttribute::CenterY });
    for view in views {
        stack.addArrangedSubview(view);
    }
    return stack;
}

fn separator(mtm: MainThreadMarker) -> Retained<NSBox> {
    let line = NSBox::new(mtm);
    line.setBoxType(NSBoxType::Separator);
    return line;
}

fn pin_width(parent: &NSStackView, view: &NSView) {
    view.leadingAnchor().constraintEqualToAnchor(&parent.leadingAnchor()).setActive(true);
    view.trailingAnchor().constraintEqualToAnchor(&parent.trailingAnchor()).setActive(true);
}

pub fn value_field(mtm: MainThreadMarker, prefix: &str, suffix: &str, digits: usize) -> Retained<NSTextField> {
    let formatter = NSNumberFormatter::new();
    formatter.setNumberStyle(NSNumberFormatterStyle::DecimalStyle);
    formatter.setUsesGroupingSeparator(false);
    formatter.setMinimumFractionDigits(digits);
    formatter.setMaximumFractionDigits(digits);
    if !prefix.is_empty() {
        formatter.setPositivePrefix(Some(&ns(prefix)));
    }
    formatter.setPositiveSuffix(Some(&ns(suffix)));
    formatter.setNegativeSuffix(Some(&ns(suffix)));
    let field = NSTextField::textFieldWithString(&ns(""), mtm);
    field.setFormatter(Some(&formatter));
    field.setControlSize(NSControlSize::Small);
    let digits_font = NSFont::monospacedDigitSystemFontOfSize_weight(NSFont::smallSystemFontSize(), unsafe {
        NSFontWeightRegular
    });
    field.setFont(Some(&digits_font));
    field.setAlignment(NSTextAlignment::Right);
    field.widthAnchor().constraintEqualToConstant(VALUE_WIDTH).setActive(true);
    return field;
}

fn slider(mtm: MainThreadMarker, min: f64, max: f64) -> Retained<NSSlider> {
    let slider = unsafe { NSSlider::sliderWithValue_minValue_maxValue_target_action(min, min, max, None, None, mtm) };
    slider.setControlSize(NSControlSize::Small);
    slider.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
    return slider;
}

fn symbol_button(mtm: MainThreadMarker, name: &str) -> Retained<NSButton> {
    let image = folders::symbol(name);
    let button = match image {
        Some(image) => unsafe { NSButton::buttonWithImage_target_action(&image, None, None, mtm) },
        None => NSButton::new(mtm),
    };
    button.setBordered(false);
    return button;
}

type Row<'a> = (&'a str, Vec<&'a NSView>);

fn form(mtm: MainThreadMarker, rows: &[Row], fill: isize) -> Retained<NSGridView> {
    let grid = NSGridView::new(mtm);
    let color = NSColor::secondaryLabelColor();
    let small_font = font(NSFont::smallSystemFontSize());
    let columns = rows.first().map_or(0, |(_, cells)| isize::try_from(cells.len()).unwrap_or(0));
    for (name, cells) in rows {
        let name = label(mtm, name, &small_font, &color);
        let mut views: Vec<&NSView> = vec![&name];
        views.extend(cells.iter().copied());
        grid.addRowWithViews(&NSArray::from_slice(&views)).setYPlacement(NSGridCellPlacement::Center);
    }
    grid.columnAtIndex(0).setXPlacement(NSGridCellPlacement::Trailing);
    for column in 1..=columns {
        let placement = if column == fill {
            NSGridCellPlacement::Fill
        } else if column == columns {
            NSGridCellPlacement::Trailing
        } else {
            NSGridCellPlacement::Leading
        };
        grid.columnAtIndex(column).setXPlacement(placement);
    }
    return grid;
}

fn section(mtm: MainThreadMarker, title: &str, body: &NSView) -> Retained<NSStackView> {
    let header = label(
        mtm,
        title,
        &NSFont::boldSystemFontOfSize(NSFont::smallSystemFontSize()),
        &NSColor::labelColor(),
    );
    let section = stack(mtm, &[&header, body], true);
    body.leadingAnchor().constraintEqualToAnchor(&section.leadingAnchor()).setActive(true);
    body.trailingAnchor().constraintEqualToAnchor(&section.trailingAnchor()).setActive(true);
    return section;
}

impl Inspector {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let content = stack(mtm, &[], true);
        content.setEdgeInsets(objc2_foundation::NSEdgeInsets { top: INSET, left: INSET, bottom: INSET, right: INSET });
        content.setTranslatesAutoresizingMaskIntoConstraints(false);
        let document: Retained<Flipped> = unsafe { msg_send![Flipped::alloc(mtm), init] };
        document.setTranslatesAutoresizingMaskIntoConstraints(false);
        document.addSubview(&content);
        content.leadingAnchor().constraintEqualToAnchor(&document.leadingAnchor()).setActive(true);
        content.trailingAnchor().constraintEqualToAnchor(&document.trailingAnchor()).setActive(true);
        content.topAnchor().constraintEqualToAnchor(&document.topAnchor()).setActive(true);
        content.bottomAnchor().constraintEqualToAnchor(&document.bottomAnchor()).setActive(true);
        let scroll = NSScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);
        scroll.setDocumentView(Some(&document));
        let clip = scroll.contentView();
        document.leadingAnchor().constraintEqualToAnchor(&clip.leadingAnchor()).setActive(true);
        document.trailingAnchor().constraintEqualToAnchor(&clip.trailingAnchor()).setActive(true);
        document.topAnchor().constraintEqualToAnchor(&clip.topAnchor()).setActive(true);

        let empty = label(mtm, &tr("No Selection"), &font(NSFont::systemFontSize()), &NSColor::secondaryLabelColor());
        let root = NSView::new(mtm);
        for view in [&*scroll as &NSView, &empty] {
            view.setTranslatesAutoresizingMaskIntoConstraints(false);
            root.addSubview(view);
        }
        scroll.leadingAnchor().constraintEqualToAnchor(&root.leadingAnchor()).setActive(true);
        scroll.trailingAnchor().constraintEqualToAnchor(&root.trailingAnchor()).setActive(true);
        scroll.topAnchor().constraintEqualToAnchor(&root.topAnchor()).setActive(true);
        scroll.bottomAnchor().constraintEqualToAnchor(&root.bottomAnchor()).setActive(true);
        empty.centerXAnchor().constraintEqualToAnchor(&root.centerXAnchor()).setActive(true);
        empty.centerYAnchor().constraintEqualToAnchor(&root.centerYAnchor()).setActive(true);
        let controller = NSViewController::new(mtm);
        controller.setView(&root);
        let target: Retained<Actions> = unsafe { msg_send![Actions::alloc(mtm), init] };
        return Self {
            controller,
            content,
            empty,
            scroll,
            target,
            shown: None,
            controls: None,
            scope: Scope::All,
            desktops: Vec::new(),
            player: None,
            timer: None,
            playing: true,
            listening: None,
            visible: false,
        };
    }

    pub fn set_visible(&mut self, visible: bool, library: &Library, selected: Option<usize>, scope: Scope) {
        if self.visible != visible {
            self.visible = visible;
            self.show(library, selected, scope);
        }
    }

    pub fn show(&mut self, library: &Library, selected: Option<usize>, scope: Scope) {
        if !self.visible {
            self.shown = None;
            self.clear();
            return;
        }
        let item = selected.and_then(|index| library.items.get(index));
        self.empty.setHidden(item.is_some());
        self.scroll.setHidden(item.is_none());
        self.scope = scope;
        let (Some(index), Some(item)) = (selected, item) else {
            self.shown = None;
            self.clear();
            return;
        };
        let key = (item.id, item.progress.is_some(), item.playable, library.folders.len());
        if self.shown != Some(key) || self.controls.is_none() {
            self.desktops = match scope {
                Scope::Space(space) => vec![space],
                Scope::All => Vec::new(),
            };
            self.build(item);
            self.shown = Some(key);
        }
        self.sync(library, index);
    }

    fn clear(&mut self) {
        let views = self.content.arrangedSubviews();
        for view in views.iter() {
            self.content.removeArrangedSubview(&view);
            view.removeFromSuperview();
        }
        cancel_timer(self.timer.take());
        if let Some((player, _)) = self.player.take() {
            unsafe { player.pause() };
        }
        self.controls = None;
        self.playing = true;
        self.set_listening(None);
    }

    fn set_listening(&mut self, item: Option<ItemId>) {
        if self.listening == item {
            return;
        }
        self.listening = item;
        submit(move |app| {
            app.set_previewing(item);
            return None;
        });
    }

    fn toggle_listen(&mut self, library: &Library, index: usize) {
        let item = library.items.get(index).map(|item| item.id);
        self.set_listening(if self.listening.is_some() { None } else { item });
        self.sync(library, index);
    }

    pub fn toggle_play(&mut self, library: &Library, index: usize) {
        self.playing = !self.playing;
        if let Some((player, _)) = &self.player {
            unsafe {
                if self.playing {
                    player.play();
                } else {
                    player.pause();
                }
            }
        }
        self.sync(library, index);
    }

    pub fn frame(&mut self, library: &Library, index: usize, count: isize) {
        let Some((player, _)) = &self.player else {
            return;
        };
        self.playing = false;
        unsafe {
            player.pause();
            if let Some(item) = player.currentItem() {
                item.stepByCount(count);
            }
        }
        self.sync(library, index);
    }

    fn time(&self) -> Option<f64> {
        let (player, _) = self.player.as_ref()?;
        let seconds = unsafe { player.currentTime().seconds() };
        return seconds.is_finite().then_some(seconds);
    }

    fn seek(&self, seconds: f64) {
        if let Some((player, _)) = &self.player {
            unsafe { player.seekToTime(CMTime::with_seconds(seconds, CM_TIMESCALE)) };
        }
    }

    fn start_clock(&mut self, scrub: &NSSlider, time: &NSTextField, length: f64) {
        let Some((player, _)) = &self.player else {
            return;
        };
        let player = player.clone();
        let (scrub, time): (Retained<NSSlider>, Retained<NSTextField>) = (scrub.retain(), time.retain());
        self.timer = Some(schedule_block(0.25, true, move || {
            let seconds = unsafe { player.currentTime().seconds() };
            if seconds.is_finite() && length > 0.0 && !dragging(scrub.mtm()) {
                let seconds = seconds % length;
                scrub.setDoubleValue(seconds);
                time.setStringValue(&ns(&format!("{} / {}", duration(seconds), duration(length))));
            }
        }));
    }

    fn build(&mut self, item: &Item) {
        let mtm = self.controller.mtm();
        self.clear();
        let video = item.kind == ItemKind::Video;
        let small_font = font(NSFont::smallSystemFontSize());
        let secondary = NSColor::secondaryLabelColor();

        let preview = self.preview(mtm, item);
        preview.heightAnchor().constraintEqualToAnchor_multiplier(&preview.widthAnchor(), PREVIEW_ASPECT).setActive(true);

        let target: &AnyObject = &self.target;
        let play = symbol_button(mtm, "pause.fill");
        wire(&play, target, sel!(play:));
        let scrub = slider(mtm, 0.0, item.duration.max(0.01));
        wire(&scrub, target, sel!(scrub:));
        let time = label(mtm, "", &small_font, &secondary);
        time.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(NSFont::smallSystemFontSize(), unsafe {
            NSFontWeightRegular
        })));
        time.setStringValue(&ns(&format!("{} / {}", duration(0.0), duration(item.duration))));
        time.setContentCompressionResistancePriority_forOrientation(1000.0, NSLayoutConstraintOrientation::Horizontal);
        let listen = symbol_button(mtm, "headphones");
        wire(&listen, target, sel!(listen:));
        let transport = stack(mtm, &[&play, &scrub, &time, &listen], false);

        let headline = unsafe {
            NSFont::preferredFontForTextStyle_options(NSFontTextStyleHeadline, &NSDictionary::new())
        };
        let title = label(mtm, &item.title, &headline, &NSColor::labelColor());
        title.setSelectable(true);
        let (primary, details) = facts(item);
        let primary = label(mtm, &primary, &small_font, &secondary);
        let details = label(mtm, &details, &small_font, &secondary);
        for text in [&title, &primary, &details] {
            text.setContentCompressionResistancePriority_forOrientation(250.0, NSLayoutConstraintOrientation::Horizontal);
        }
        primary.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        details.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        title.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
        let marker_row = stack(mtm, &[], false);
        let header = stack(mtm, &[&title, &primary, &details, &marker_row], true);
        header.setCustomSpacing_afterView(8.0, &details);

        let (title_text, icon) = match (item.playable, item.progress) {
            (true, _) => (tr("Set as Wallpaper"), "photo.on.rectangle"),
            (false, None) => (tr("Optimize"), "wand.and.sparkles"),
            (false, Some(_)) => (tr("Optimizing…"), "wand.and.sparkles"),
        };
        let wallpaper = unsafe { NSButton::buttonWithTitle_target_action(&ns(&title_text), None, None, mtm) };
        wallpaper.setImage(folders::symbol(icon).as_deref());
        wallpaper.setImagePosition(NSCellImagePosition::ImageLeading);
        wallpaper.setKeyEquivalent(&ns("\r"));
        wallpaper.setEnabled(item.playable || item.progress.is_none());
        wallpaper.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
        if item.playable {
            wire(&wallpaper, target, sel!(setWallpaper:));
        }
        let lock = unsafe { NSButton::buttonWithTitle_target_action(&ns(&tr("Lock Screen")), None, None, mtm) };
        lock.setImage(folders::symbol("lock").as_deref());
        lock.setImagePosition(NSCellImagePosition::ImageLeading);
        wire(&lock, target, sel!(setLock:));
        let destination = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), CGRect::ZERO, true);
        destination
            .setContentCompressionResistancePriority_forOrientation(250.0, NSLayoutConstraintOrientation::Horizontal);
        if let Some(menu) = destination.menu() {
            menu.setAutoenablesItems(false);
        }
        let top = stack(mtm, &[&wallpaper, &lock], false);
        top.setDistribution(NSStackViewDistribution::Fill);
        let frame = unsafe { NSButton::buttonWithTitle_target_action(&ns(&tr("Use This Frame")), None, None, mtm) };
        frame.setImage(folders::symbol("film").as_deref());
        frame.setImagePosition(NSCellImagePosition::ImageLeading);
        wire(&frame, target, sel!(lockFrame:));
        let bottom = if video {
            let bottom = stack(mtm, &[&destination, &frame], false);
            bottom.setDistribution(NSStackViewDistribution::Fill);
            bottom
        } else {
            stack(mtm, &[&destination], false)
        };
        let actions = stack(mtm, &[&top, &bottom], true);
        destination.widthAnchor().constraintEqualToAnchor(&wallpaper.widthAnchor()).setActive(true);
        if video {
            frame.widthAnchor().constraintEqualToAnchor(&lock.widthAnchor()).setActive(true);
        }
        pin_width(&actions, &top);
        pin_width(&actions, &bottom);

        let mute = symbol_button(mtm, "speaker.wave.2.fill");
        let widest = ["speaker.wave.2.fill", "speaker.slash.fill"]
            .iter()
            .filter_map(|name| folders::symbol(name))
            .map(|image| image.size().width)
            .fold(0.0, CGFloat::max);
        mute.widthAnchor().constraintEqualToConstant(widest).setActive(true);
        wire(&mute, target, sel!(mute:));
        let volume = slider(mtm, 0.0, 100.0);
        let volume_value = value_field(mtm, "", "%", 0);
        tagged(&volume, &volume_value, VOLUME, target);
        let speed = slider(mtm, 0.0, 1.0);
        let speed_value = value_field(mtm, "", "×", 2);
        tagged(&speed, &speed_value, SPEED, target);
        let blank = NSView::new(mtm);
        let playback = form(
            mtm,
            &[
                (&tr("Volume"), vec![&mute, &volume, &volume_value]),
                (&tr("Speed"), vec![&blank, &speed, &speed_value]),
            ],
            2,
        );

        let labels: Vec<Retained<NSString>> = MODES.iter().map(|(_, name)| ns(&tr(name))).collect();
        let mode = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &NSArray::from_retained_slice(&labels),
                NSSegmentSwitchTracking::SelectOne,
                None,
                None,
                mtm,
            )
        };
        mode.setSegmentDistribution(NSSegmentDistribution::FillProportionally);
        mode.setControlSize(NSControlSize::Small);
        wire(&mode, target, sel!(fill:));
        let offsets = [
            (slider(mtm, -100.0, 100.0), value_field(mtm, "+", " px", 0)),
            (slider(mtm, -100.0, 100.0), value_field(mtm, "+", " px", 0)),
            (slider(mtm, ZOOM_MIN, ZOOM_MAX), value_field(mtm, "", "×", 2)),
        ];
        for ((control, field), tag) in offsets.iter().zip([HORIZONTAL, VERTICAL, ZOOM]) {
            tagged(control, field, tag, target);
        }
        let offset_rows = form(
            mtm,
            &[
                (&tr("Horizontal"), vec![&offsets[0].0, &offsets[0].1]),
                (&tr("Vertical"), vec![&offsets[1].0, &offsets[1].1]),
                (&tr("Scale"), vec![&offsets[2].0, &offsets[2].1]),
            ],
            1,
        );
        let placement = stack(mtm, &[&mode, &offset_rows], true);
        pin_width(&placement, &mode);
        pin_width(&placement, &offset_rows);

        let folder = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), CGRect::ZERO, false);
        folder.setControlSize(NSControlSize::Small);
        if let Some(menu) = folder.menu() {
            menu.setAutoenablesItems(false);
        }
        folder.setContentCompressionResistancePriority_forOrientation(250.0, NSLayoutConstraintOrientation::Horizontal);
        let library_form = folder.clone();

        let reset = unsafe { NSButton::buttonWithTitle_target_action(&ns(&tr("Reset to Defaults")), None, None, mtm) };
        let remove = unsafe { NSButton::buttonWithTitle_target_action(&ns(&tr("Remove from Library")), None, None, mtm) };
        remove.setHasDestructiveAction(true);
        let keys: [&NSAttributedStringKey; 1] = unsafe { [NSForegroundColorAttributeName] };
        let values: [&AnyObject; 1] = [&NSColor::systemRedColor()];
        let red = NSDictionary::from_slices(&keys, &values);
        let title = unsafe {
            NSAttributedString::initWithString_attributes(
                NSAttributedString::alloc(),
                &ns(&tr("Remove from Library")),
                Some(&red),
            )
        };
        remove.setAttributedTitle(&title);
        let buttons = stack(mtm, &[&reset, &remove], false);
        buttons.setDistribution(NSStackViewDistribution::FillEqually);

        let mut sections: Vec<Retained<NSView>> = vec![up(&preview)];
        if video {
            sections.push(up(&transport));
        }
        sections.push(up(&header));
        sections.push(up(&actions));
        if video {
            sections.push(up(&separator(mtm)));
            sections.push(up(&section(mtm, &tr("Playback"), &playback)));
        }
        sections.push(up(&separator(mtm)));
        sections.push(up(&section(mtm, &tr("Placement"), &placement)));
        sections.push(up(&separator(mtm)));
        sections.push(up(&section(mtm, &tr("Library"), &library_form)));
        sections.push(up(&buttons));
        for view in &sections {
            self.content.addArrangedSubview(view);
            view.leadingAnchor()
                .constraintEqualToAnchor_constant(&self.content.leadingAnchor(), INSET)
                .setActive(true);
            view.trailingAnchor()
                .constraintEqualToAnchor_constant(&self.content.trailingAnchor(), -INSET)
                .setActive(true);
        }

        self.start_clock(&scrub, &time, item.duration);
        self.controls = Some(Controls {
            play,
            listen,
            markers: marker_row,
            destination,
            scrub,
            time,
            mute,
            volume,
            volume_value,
            speed,
            speed_value,
            mode,
            offsets,
            folder,
            reset,
        });
    }

    fn sync(&self, library: &Library, index: usize) {
        let (Some(controls), Some(item)) = (&self.controls, library.items.get(index)) else {
            return;
        };
        let settings = &item.settings;
        let silent = settings.muted || settings.volume <= 0.0;
        if let Some(image) = folders::symbol(if silent { "speaker.slash.fill" } else { "speaker.wave.2.fill" }) {
            controls.mute.setImage(Some(&image));
        }
        if let Some(image) = folders::symbol(if self.playing { "pause.fill" } else { "play.fill" }) {
            controls.play.setImage(Some(&image));
        }
        let listening = self.listening.is_some();
        let tint = if listening { NSColor::controlAccentColor() } else { NSColor::secondaryLabelColor() };
        controls.listen.setContentTintColor(Some(&tint));
        let tip = if listening {
            tr("Stop listening; the desktop plays its own sound again")
        } else {
            tr("Listen to the preview; the desktop copy is muted meanwhile")
        };
        controls.listen.setToolTip(Some(&ns(&tip)));
        let volume = if settings.muted { 0.0 } else { settings.volume * 100.0 };
        controls.volume.setDoubleValue(volume);
        controls.volume_value.setDoubleValue(volume);
        controls.speed.setDoubleValue(speed_to_slider(settings.speed));
        controls.speed_value.setDoubleValue(settings.speed);
        controls.mode.setSelectedSegment(fill_index(settings.mode));
        fill_destination(&controls.destination, library.desktops, &self.desktops, &self.target);
        let shifts = matches!(settings.mode, FillMode::Fill | FillMode::Center | FillMode::Custom);
        let (width, height) = reach(item);
        let values = [
            (settings.x * width, width, shifts && width > 0.0),
            (settings.y * height, height, shifts && height > 0.0),
            (settings.zoom, 0.0, settings.mode == FillMode::Custom),
        ];
        for (position, ((control, field), (value, span, enabled))) in controls.offsets.iter().zip(values).enumerate() {
            if position < 2 {
                let span = if span > 0.0 { span } else { 1.0 };
                control.setMinValue(-span);
                control.setMaxValue(span);
            }
            control.setDoubleValue(value);
            control.setEnabled(enabled);
            field.setDoubleValue(value);
            field.setEnabled(enabled);
        }
        controls.reset.setEnabled(item.changed);
        fill_folders(&controls.folder, library, item.folder);

        let mtm = self.controller.mtm();
        let views: Vec<Retained<NSView>> =
            markers(library, index, self.scope).iter().map(|marker| marker_view(mtm, marker)).collect();
        let refs: Vec<&NSView> = views.iter().map(|view| &**view).collect();
        controls.markers.setViews_inGravity(&NSArray::from_slice(&refs), NSStackViewGravity::Leading);
        controls.markers.setHidden(refs.is_empty());

        if let Some((player, _)) = &self.player {
            #[allow(clippy::cast_possible_truncation)]
            unsafe {
                player.setVolume(if settings.volume > 0.0 { settings.volume as f32 } else { 1.0 });
                player.setMuted(!listening);
                if self.playing {
                    player.setRate(settings.speed as f32);
                }
            }
        }
    }

    fn preview(&mut self, mtm: MainThreadMarker, item: &Item) -> Retained<NSView> {
        let view: Retained<Preview> = unsafe { msg_send![Preview::alloc(mtm), init] };
        let still = CALayer::new();
        if let Some(image) = &item.image {
            unsafe { still.setContents(Some(image)) };
        }
        still.setContentsGravity(unsafe { kCAGravityResizeAspectFill });
        let mut layers = vec![still];
        if item.kind == ItemKind::Video {
            let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&file_url(&item.path), None) };
            let player = unsafe { AVQueuePlayer::new(mtm) };
            let template = unsafe { AVPlayerItem::playerItemWithAsset(&asset, mtm) };
            let looper = unsafe { AVPlayerLooper::playerLooperWithPlayer_templateItem(&player, &template) };
            unsafe {
                player.setMuted(true);
                player.play();
            }
            let layer = unsafe { AVPlayerLayer::playerLayerWithPlayer(Some(&player)) };
            if let Some(gravity) = unsafe { AVLayerVideoGravityResizeAspectFill } {
                unsafe { layer.setVideoGravity(gravity) };
            }
            self.player = Some((player, looper));
            layers.push(Retained::into_super(layer));
        }
        view.setWantsLayer(true);
        if let Some(host) = view.layer() {
            host.setCornerRadius(PREVIEW_RADIUS);
            host.setMasksToBounds(true);
            for layer in &layers {
                host.addSublayer(layer);
            }
        }
        return Retained::into_super(view);
    }
}

fn marker_view(mtm: MainThreadMarker, marker: &Marker) -> Retained<NSView> {
    let color = if marker.active { NSColor::controlAccentColor() } else { NSColor::secondaryLabelColor() };
    let glyph = match folders::symbol(marker.icon) {
        Some(image) => NSImageView::imageViewWithImage(&image, mtm),
        None => NSImageView::new(mtm),
    };
    glyph.setContentTintColor(Some(&color));
    let text = label(mtm, &marker.text, &font(NSFont::smallSystemFontSize()), &color);
    let row = stack(mtm, &[&glyph, &text], false);
    row.setSpacing(4.0);
    row.setHuggingPriority_forOrientation(750.0, NSLayoutConstraintOrientation::Horizontal);
    return up(&row);
}

fn fill_destination(popup: &NSPopUpButton, desktops: usize, chosen: &[usize], target: &AnyObject) {
    let mtm = popup.mtm();
    let place = match chosen {
        [] => tr("All Desktops"),
        [space] => desktop_name(*space),
        _ => {
            let numbers: Vec<String> = chosen.iter().map(|space| (space + 1).to_string()).collect();
            trf("Desktops %@", &[Arg::Text(numbers.join(", "))])
        }
    };
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    let entry = |title: &str, tag: isize, on: bool| {
        let item = NSMenuItem::new(mtm);
        item.setTitle(&ns(title));
        unsafe {
            item.setTarget(Some(target));
            item.setAction(Some(sel!(destination:)));
        }
        item.setTag(tag);
        item.setState(if on { NSControlStateValueOn } else { NSControlStateValueOff });
        menu.addItem(&item);
    };
    entry(&trf("On %@", &[Arg::Text(place)]), TAG_ALL, false);
    entry(&tr("All Desktops"), TAG_ALL, chosen.is_empty());
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    for space in 0..desktops {
        entry(&desktop_name(space), isize::try_from(space).unwrap_or(TAG_ALL), chosen.contains(&space));
    }
    popup.setMenu(Some(&menu));
}

fn fill_folders(popup: &NSPopUpButton, library: &Library, folder: Option<usize>) {
    popup.removeAllItems();
    let Some(menu) = popup.menu() else {
        return;
    };
    let mtm = popup.mtm();
    let unfiled = NSMenuItem::new(mtm);
    unfiled.setTitle(&ns(&tr("Unfiled")));
    unfiled.setTag(TAG_ALL);
    menu.addItem(&unfiled);
    for (index, name) in library.folders.iter().enumerate() {
        let entry = NSMenuItem::new(mtm);
        entry.setTitle(&ns(name));
        entry.setTag(isize::try_from(index).unwrap_or(TAG_ALL));
        entry.setEnabled(!library.sealed(index));
        entry.setImage(folders::image(library, index).as_deref());
        menu.addItem(&entry);
    }
    popup.selectItemAtIndex(folder.map_or(0, |index| isize::try_from(index + 1).unwrap_or(0)));
}

pub fn kind_name(kind: ItemKind) -> &'static str {
    return match kind {
        ItemKind::Video => i18n::key("Video"),
        ItemKind::Gif => i18n::key("GIF"),
        ItemKind::Image => i18n::key("Image"),
    };
}

pub fn resolution(item: &Item) -> Option<String> {
    let short = item.width.min(item.height);
    return match short {
        0 => None,
        2160.. => Some("4K".to_string()),
        1440.. => Some("1440p".to_string()),
        1080.. => Some("1080p".to_string()),
        720.. => Some("720p".to_string()),
        _ => Some(format!("{}×{}", item.width, item.height)),
    };
}

fn facts(item: &Item) -> (String, String) {
    let mut primary = vec![tr(kind_name(item.kind))];
    if item.width > 0 {
        let size = format!("{} × {}", item.width, item.height);
        primary.push(match resolution(item).filter(|short| short.ends_with('p') || short == "4K") {
            Some(short) => format!("{size} ({short})"),
            None => size,
        });
    }
    if item.kind == ItemKind::Video && item.duration > 0.0 {
        primary.push(duration(item.duration));
    }
    let mut secondary: Vec<String> = Vec::new();
    secondary.extend(item.codec.as_ref().map(|codec| match codec.to_lowercase().as_str() {
        "hvc1" | "hev1" | "hevc" => "HEVC".to_string(),
        "avc1" | "avc3" | "h264" => "H.264".to_string(),
        other => other.to_uppercase(),
    }));
    if item.fps > 0.0 {
        secondary.push(format!("{:.0} fps", item.fps));
    }
    secondary.push(i18n::bytes(i64::try_from(item.bytes).unwrap_or(i64::MAX)));
    secondary.push(trf("Added %@", &[Arg::Text(i18n::date(item.added))]));
    return (primary.join(" · "), secondary.join(" · "));
}

fn reach(item: &Item) -> (f64, f64) {
    let screen = MainThreadMarker::new().and_then(NSScreen::mainScreen).map_or(Size::new(3024.0, 1964.0), |screen| {
        let frame = screen.frame();
        let scale = screen.backingScaleFactor();
        return Size::new(frame.size.width * scale, frame.size.height * scale);
    });
    let settings = &item.settings;
    let media = Size::new(f64::from(item.width), f64::from(item.height));
    let rect = geometry::resolve(settings.mode, Geometry { x: 0.0, y: 0.0, zoom: settings.zoom }, media, screen).rect;
    return (((rect.w - screen.w) / 2.0).max(0.0).round(), ((rect.h - screen.h) / 2.0).max(0.0).round());
}

fn fill_index(mode: FillMode) -> isize {
    return MODES
        .iter()
        .position(|(candidate, _)| *candidate == mode)
        .and_then(|index| isize::try_from(index).ok())
        .unwrap_or(0);
}
