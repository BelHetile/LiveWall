use std::cell::Cell;
use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityButtonRole, NSButton, NSColor, NSControlSize, NSEvent, NSLayoutAttribute,
    NSLineBreakMode, NSMenu, NSMenuItem, NSPopUpButton, NSScreen, NSScrollView, NSSegmentedControl, NSSlider,
    NSStackView, NSStackViewGravity, NSTextAlignment, NSView, NSViewLayerContentsRedrawPolicy,
};
use objc2_av_foundation::{
    AVLayerVideoGravityResizeAspectFill, AVPlayerItem, AVPlayerLayer, AVPlayerLooper, AVQueuePlayer, AVURLAsset,
};
use objc2_core_foundation::CGFloat;
use objc2_core_media::CMTime;
use objc2_foundation::{NSArray, NSTimer};
use objc2_quartz_core::{CAAutoresizingMask, CALayer, kCAGravityResizeAspectFill};

use proto::{FillMode, Geometry};

use crate::cf::file_url;
use crate::geometry::{self, Size};
use crate::i18n::{self, Arg, tr, trf};
use crate::media::av::CM_TIMESCALE;
use crate::model::{ItemId, ItemKind};
use crate::platform::{cancel_timer, schedule_block};
use crate::style::Key;
use crate::ui::folders;
use crate::ui::metrics::{self, Text};
use crate::ui::state::{Filter, Scope, TAG_ALL, TAG_RESET, markers};
use crate::ui::theme::{self, Theme};
use crate::ui::toolbar::scope_menu;
use crate::ui::view::{Item, Library};
use crate::ui::wall::{TILE_ASPECT, chip_key, duration, kind_name, resolution};
use crate::ui::widgets::{self, IconRole, NumberField, Slot, Units};

const WIDTH: CGFloat = 340.0;
const INSET: CGFloat = metrics::INSPECTOR_INSET;
const LOCK_WIDTH: CGFloat = 118.0;
const VOLUME_WIDTH: CGFloat = 136.0;
const SLIDER_WIDTH: CGFloat = 164.0;
const VALUE_WIDTH: CGFloat = 50.0;
pub const VOLUME: isize = 1;
pub const SPEED: isize = 2;
pub const HORIZONTAL: isize = 3;
pub const VERTICAL: isize = 4;
pub const ZOOM: isize = 5;
pub const SCRUB: isize = 6;
pub const FRAME_TO_LOCK: &str = i18n::key("Set This Frame as Lock Screen");
const MODES: [&str; 5] =
    [i18n::key("Fill"), i18n::key("Fit"), i18n::key("Stretch"), i18n::key("Center"), i18n::key("Custom")];
pub const FILLS: [FillMode; 5] = [FillMode::Fill, FillMode::Fit, FillMode::Stretch, FillMode::Center, FillMode::Custom];

type Shown = (ItemId, PathBuf, bool, bool);

fn shown_key(library: &Library, selected: Option<usize>) -> Option<Shown> {
    let item = library.items.get(selected?)?;
    return Some((item.id, item.path.clone(), item.progress.is_some(), item.playable));
}

type Offset = (Retained<NSSlider>, Retained<NumberField>);

struct PreviewIvars {
    border: Retained<NSColor>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = PreviewIvars]
    struct PreviewView;

    impl PreviewView {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            crate::ui::toggle_play();
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_perform_press(&self) -> bool {
            crate::ui::toggle_play();
            true
        }

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

impl PreviewView {
    fn colorize(&self) {
        let Some(layer) = self.layer() else {
            return;
        };
        let border = &self.ivars().border;
        widgets::resolving(self, || layer.setBorderColor(Some(&border.CGColor())));
    }
}

struct Controls {
    play: Retained<NSButton>,
    listen: Retained<NSButton>,
    time: Retained<NumberField>,
    mute: Retained<NSButton>,
    volume: Retained<NSSlider>,
    volume_value: Retained<NumberField>,
    speed: Retained<NSSlider>,
    speed_value: Retained<NumberField>,
    modes: Retained<NSSegmentedControl>,
    offsets: [Offset; 3],
    markers: Retained<NSStackView>,
    destination: Retained<NSPopUpButton>,
    folder: Retained<NSPopUpButton>,
    reset: Retained<NSButton>,
    lit: Cell<bool>,
}

pub struct Inspector {
    pub scroll: Retained<NSScrollView>,
    pub target: Option<Scope>,
    theme: Theme,
    content: Retained<NSStackView>,
    empty: Retained<NSStackView>,
    shown: Option<Shown>,
    player: Option<(Retained<AVQueuePlayer>, Retained<AVPlayerLooper>)>,
    timer: Option<Retained<NSTimer>>,
    playing: bool,
    listening: Option<ItemId>,
    controls: Option<Controls>,
    markers_key: Vec<(&'static str, String, bool)>,
    folders_key: Option<FoldersKey>,
}

#[derive(PartialEq)]
struct FoldersKey {
    names: Vec<String>,
    icons: Vec<Option<String>>,
    colors: Vec<Option<String>>,
    sealed: Vec<bool>,
    current: Option<usize>,
}

impl Inspector {
    pub fn new(mtm: MainThreadMarker, theme: &Theme) -> Self {
        let scroll = NSScrollView::new(mtm);
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setAutomaticallyAdjustsContentInsets(true);
        let content = widgets::stack(mtm, &[], true, metrics::SPACE_12);
        content.setAlignment(NSLayoutAttribute::Leading);
        content.setEdgeInsets(widgets::insets(metrics::SPACE_8, INSET, metrics::SPACE_20, INSET));
        content.setWantsLayer(true);
        let document = widgets::Flipped::new(mtm);
        document.setTranslatesAutoresizingMaskIntoConstraints(false);
        document.addSubview(&content);
        widgets::pin(&content, &document, 0.0);
        scroll.setDocumentView(Some(&document));
        let clip = scroll.contentView();
        document.leadingAnchor().constraintEqualToAnchor(&clip.leadingAnchor()).setActive(true);
        document.trailingAnchor().constraintEqualToAnchor(&clip.trailingAnchor()).setActive(true);
        document.topAnchor().constraintEqualToAnchor(&clip.topAnchor()).setActive(true);
        let empty = empty_state(mtm, theme);
        document.addSubview(&empty);
        empty.setTranslatesAutoresizingMaskIntoConstraints(false);
        empty.leadingAnchor().constraintEqualToAnchor(&document.leadingAnchor()).setActive(true);
        empty.trailingAnchor().constraintEqualToAnchor(&document.trailingAnchor()).setActive(true);
        empty.topAnchor().constraintEqualToAnchor_constant(&document.topAnchor(), metrics::EMPTY_TOP).setActive(true);
        let backdrop = widgets::card(mtm, &theme.color(Key::InspectorBackdrop), 0.0);
        theme::register(&backdrop, Key::InspectorBackdrop, Slot::Fill);
        scroll.addSubview_positioned_relativeTo(&backdrop, objc2_app_kit::NSWindowOrderingMode::Below, None);
        widgets::pin(&backdrop, &scroll, 0.0);
        return Self {
            scroll,
            target: None,
            theme: theme.clone(),
            content,
            empty,
            shown: None,
            player: None,
            timer: None,
            playing: true,
            listening: None,
            controls: None,
            markers_key: Vec::new(),
            folders_key: None,
        };
    }

    pub fn width() -> CGFloat {
        return WIDTH;
    }

    pub fn refresh(
        &mut self,
        library: &Library,
        filter: &Filter,
        theme: &Theme,
        selected: Option<usize>,
        target: &AnyObject,
    ) {
        let mtm = self.scroll.mtm();
        let key = shown_key(library, selected);
        let rebuilt = key != self.shown || (selected.is_some() && self.controls.is_none());
        if rebuilt {
            self.show(library, theme, selected, target);
        }
        let (Some(index), Some(controls)) = (selected, &self.controls) else {
            return;
        };
        let marker_list = markers(library, index, filter.scope);
        let marker_key = chip_key(&marker_list);
        if rebuilt || self.markers_key != marker_key {
            let views: Vec<Retained<NSView>> =
                marker_list.iter().map(|marker| widgets::chip(mtm, marker, theme)).collect();
            let refs: Vec<&NSView> = views.iter().map(|view| &**view).collect();
            controls.markers.setViews_inGravity(&NSArray::from_slice(&refs), NSStackViewGravity::Leading);
            controls.markers.setHidden(refs.is_empty());
            self.markers_key = marker_key;
        }
        let scope = self.target.unwrap_or(filter.scope);
        let title = trf("On %@", &[Arg::Text(scope.title())]);
        let menu = scope_menu(mtm, library, scope, &title, sel!(target:), target);
        if self.target.is_some() {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            menu.addItem(&widgets::menu_item(mtm, &tr("Match Toolbar"), sel!(target:), Some(target), TAG_RESET, None));
        }
        controls.destination.setMenu(Some(&menu));
        widgets::alpha(&controls.destination, if self.target.is_some() { 1.0 } else { metrics::MUTED });
        let folders_key = FoldersKey {
            names: library.folders.clone(),
            icons: library.icons.clone(),
            colors: library.colors.clone(),
            sealed: (0..library.folders.len()).map(|folder| library.sealed(folder)).collect(),
            current: library.items[index].folder,
        };
        if rebuilt || self.folders_key.as_ref() != Some(&folders_key) {
            fill_folders(&controls.folder, library, library.items[index].folder);
            self.folders_key = Some(folders_key);
        }
        self.sync(&library.items[index]);
    }

    fn clear(&mut self) {
        let views = self.content.arrangedSubviews();
        for view in (0..views.count()).map(|position| views.objectAtIndex(position)) {
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

    pub fn toggle_listen(&mut self, item: &Item) {
        self.set_listening(if self.listening.is_some() { None } else { Some(item.id) });
        self.sync(item);
    }

    pub fn stop_listening(&mut self) {
        if let Some((player, _)) = &self.player {
            unsafe { player.setMuted(true) };
        }
        self.set_listening(None);
    }

    fn set_listening(&mut self, item: Option<ItemId>) {
        if self.listening == item {
            return;
        }
        self.listening = item;
        super::submit(move |app| {
            app.set_previewing(item);
            return None;
        });
    }

    fn show(&mut self, library: &Library, theme: &Theme, selected: Option<usize>, target: &AnyObject) {
        let mtm = self.scroll.mtm();
        self.clear();
        self.shown = shown_key(library, selected);
        self.target = None;
        self.empty.setHidden(selected.is_some());
        let Some(index) = selected else {
            return;
        };
        let item = &library.items[index];
        let video = item.kind == ItemKind::Video;

        let preview = self.preview(mtm, item, theme);
        if video {
            unsafe { preview.setMenu(Some(&frame_menu(mtm, target))) };
        }
        let play = widgets::icon_button(mtm, "pause.fill", sel!(play:), target, Key::TextSecondary, theme);
        widgets::describe(&play, &tr("Play / Pause"), &tr("Play / Pause"));
        let scrub = slider(mtm, 0.0, 0.0, item.duration.max(0.01), SCRUB, theme, target);
        let time = field(mtm, SCRUB, target, theme);
        let length = widgets::label(
            mtm,
            &format!("/ {}", duration(item.duration)),
            Text::Subheadline,
            &theme.color(Key::TextTertiary),
        );
        length.setFont(Some(&metrics::digits(Text::Subheadline)));
        length.setContentCompressionResistancePriority_forOrientation(
            1000.0,
            objc2_app_kit::NSLayoutConstraintOrientation::Horizontal,
        );
        let listen = widgets::icon_button(mtm, "headphones", sel!(listen:), target, Key::TextSecondary, theme);
        let transport = widgets::stack(mtm, &[&play, &scrub, &time, &length, &listen], false, metrics::SPACE_8);
        transport.setCustomSpacing_afterView(metrics::SPACE_4, &time);
        transport.setHidden(!video);

        let (header, marker_stack) = header(mtm, item, theme);

        let (actions, destination) = actions(mtm, theme, video, item, target);
        let (playback, mute, volume, volume_value, speed, speed_value) = playback(mtm, theme, target);
        let (placement, modes, offsets) = placement(mtm, theme, target);
        let (library_row, folder) = library_row(mtm, theme, target);
        let reset = reset_button(mtm, theme, target);

        let mut sections: Vec<Retained<NSView>> = vec![preview.clone()];
        if video {
            sections.push(widgets::view(&transport));
        }
        sections.extend([widgets::view(&header), widgets::view(&actions)]);
        if video {
            sections.push(Retained::into_super(widgets::divider(mtm, Key::Hairline, theme)));
            sections.push(widgets::view(&playback));
        }
        sections.extend([
            Retained::into_super(widgets::divider(mtm, Key::Hairline, theme)),
            widgets::view(&placement),
            Retained::into_super(widgets::divider(mtm, Key::Hairline, theme)),
            widgets::view(&library_row),
            widgets::view(&reset),
        ]);
        for view in &sections {
            self.content.addArrangedSubview(view);
            view.leadingAnchor().constraintEqualToAnchor_constant(&self.content.leadingAnchor(), INSET).setActive(true);
            view.trailingAnchor()
                .constraintEqualToAnchor_constant(&self.content.trailingAnchor(), -INSET)
                .setActive(true);
        }
        self.content.setCustomSpacing_afterView(if video { metrics::SPACE_4 } else { metrics::SPACE_12 }, &preview);
        if video {
            self.content.setCustomSpacing_afterView(metrics::SPACE_8, &transport);
        }
        preview.heightAnchor().constraintEqualToAnchor_multiplier(&preview.widthAnchor(), TILE_ASPECT).setActive(true);

        self.content.setCustomSpacing_afterView(metrics::SPACE_8, &library_row);
        self.start_clock(&scrub, &time, item.duration);
        time.show(0.0);
        self.controls = Some(Controls {
            play,
            listen,
            time,
            mute,
            volume,
            volume_value,
            speed,
            speed_value,
            modes,
            offsets,
            markers: marker_stack,
            destination,
            folder,
            reset,
            lit: Cell::new(false),
        });
    }

    fn start_clock(&mut self, scrub: &NSSlider, time: &NumberField, length: f64) {
        let Some((player, _)) = &self.player else {
            return;
        };
        let player = player.clone();
        let (scrub, time): (Retained<NSSlider>, Retained<NumberField>) = (scrub.into(), time.into());
        self.timer = Some(schedule_block(0.25, true, move || {
            let seconds = unsafe { player.currentTime().seconds() };
            if seconds.is_finite() && length > 0.0 && !time.editing() {
                scrub.setDoubleValue(seconds % length);
                time.show(seconds % length);
            }
        }));
    }

    pub fn sync(&self, item: &Item) {
        let Some(controls) = &self.controls else {
            return;
        };
        let settings = &item.settings;
        let silent = settings.muted || settings.volume <= 0.0;
        widgets::set_icon(&controls.mute, if silent { "speaker.slash.fill" } else { "speaker.wave.2.fill" });
        widgets::set_icon(&controls.play, if self.playing { "pause.fill" } else { "play.fill" });
        let listening = self.listening.is_some();
        if controls.lit.replace(listening) != listening {
            widgets::retint(&controls.listen, if listening { Key::Selection } else { Key::TextSecondary }, &self.theme);
        }
        let tip = if listening {
            tr("Stop listening; the desktop plays its own sound again")
        } else {
            tr("Listen to the preview; the desktop copy is muted meanwhile")
        };
        widgets::describe(&controls.listen, &tip, &tip);
        let mute = tr(if silent { i18n::key("Unmute") } else { i18n::key("Mute") });
        widgets::describe(&controls.mute, &mute, &mute);
        let volume = if settings.muted { 0.0 } else { settings.volume * 100.0 };
        controls.volume.setDoubleValue(volume);
        controls.volume_value.show(volume);
        controls.speed.setDoubleValue(settings.speed);
        controls.speed_value.show(settings.speed);
        controls.modes.setSelectedSegment(fill_index(settings.mode));
        let shifts = matches!(settings.mode, FillMode::Fill | FillMode::Center | FillMode::Custom);
        let (width, height) = reach(item);
        let values = [
            (settings.x * width, width, shifts && width > 0.0),
            (settings.y * height, height, shifts && height > 0.0),
            (settings.zoom, 0.0, settings.mode == FillMode::Custom),
        ];
        for ((control, value_field), (value, span, enabled)) in controls.offsets.iter().zip(values) {
            if control.tag() != ZOOM {
                let span = if span > 0.0 { span } else { 1.0 };
                control.setMinValue(-span);
                control.setMaxValue(span);
            }
            control.setDoubleValue(value);
            control.setEnabled(enabled);
            value_field.show(value);
            value_field.setEnabled(enabled);
            widgets::alpha(value_field, if enabled { 1.0 } else { metrics::DIMMED });
        }
        controls.reset.setEnabled(item.changed);
        widgets::alpha(&controls.reset, if item.changed { 1.0 } else { metrics::DIMMED });
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

    pub fn pause(&self) {
        if let Some((player, _)) = &self.player {
            unsafe { player.pause() };
        }
    }

    pub fn toggle_play(&mut self) {
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
    }

    pub fn frame(&mut self, count: isize) {
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
    }

    pub fn time(&self) -> Option<f64> {
        let (player, _) = self.player.as_ref()?;
        let item = unsafe { player.currentItem() }?;
        let seconds = unsafe { item.currentTime().seconds() };
        let length = unsafe { item.duration().seconds() };
        let time = if length.is_finite() && length > 0.0 { seconds % length } else { seconds };
        return time.is_finite().then_some(time);
    }

    pub fn seek(&self, seconds: f64) {
        if let Some((player, _)) = &self.player {
            unsafe { player.seekToTime(CMTime::with_seconds(seconds, CM_TIMESCALE)) };
        }
        if let Some(controls) = &self.controls {
            controls.time.show(seconds);
        }
    }

    fn preview(&mut self, mtm: MainThreadMarker, item: &Item, theme: &Theme) -> Retained<NSView> {
        let view = PreviewView::alloc(mtm).set_ivars(PreviewIvars { border: theme.color(Key::InspectorPreviewBorder) });
        let view: Retained<PreviewView> = unsafe { msg_send![super(view), init] };
        let layer: Retained<CALayer> = if item.kind == ItemKind::Video {
            let url = file_url(&item.path);
            let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) };
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
            Retained::into_super(layer)
        } else {
            let layer = CALayer::new();
            if let Some(image) = &item.image {
                unsafe { layer.setContents(Some(image)) };
            }
            layer.setContentsGravity(unsafe { kCAGravityResizeAspectFill });
            layer
        };
        layer.setAutoresizingMask(CAAutoresizingMask::LayerWidthSizable | CAAutoresizingMask::LayerHeightSizable);
        view.setWantsLayer(true);
        view.setLayerContentsRedrawPolicy(NSViewLayerContentsRedrawPolicy::OnSetNeedsDisplay);
        if item.kind == ItemKind::Video {
            widgets::describe(&view, &tr("Play / Pause"), &tr("Play / Pause"));
            view.setAccessibilityElement(true);
            view.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        }
        if let Some(host) = view.layer() {
            host.setCornerRadius(metrics::RADIUS_TILE);
            host.setMasksToBounds(true);
            host.setBorderWidth(metrics::BORDER_HAIRLINE);
            host.addSublayer(&layer);
        }
        view.setNeedsDisplay(true);
        return Retained::into_super(view);
    }
}

fn empty_state(mtm: MainThreadMarker, theme: &Theme) -> Retained<NSStackView> {
    let faint = theme.color(Key::TextTertiary);
    let icon = widgets::symbol_view(mtm, "sparkles.rectangle.stack", IconRole::Hero, &faint);
    theme::register(&icon, Key::TextTertiary, Slot::Tint);
    let title = widgets::label(mtm, &tr("No Selection"), Text::Title3, &theme.color(Key::TextSecondary));
    let hint =
        widgets::label(mtm, &tr("Select a wallpaper to preview it and\nchange how it plays."), Text::Callout, &faint);
    hint.setMaximumNumberOfLines(2);
    hint.setAlignment(NSTextAlignment::Center);
    let stack = widgets::stack(mtm, &[&icon, &title, &hint], true, metrics::SPACE_8);
    stack.setAlignment(NSLayoutAttribute::CenterX);
    return stack;
}

fn header(mtm: MainThreadMarker, item: &Item, theme: &Theme) -> (Retained<NSStackView>, Retained<NSStackView>) {
    let title = widgets::label(mtm, &item.title, Text::Title3, &theme.color(Key::Text));
    title.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
    title.setToolTip(Some(&widgets::ns(&item.title)));
    let (primary, secondary) = facts(item);
    let meta = widgets::label(mtm, &primary, Text::Subheadline, &theme.color(Key::TextSecondary));
    let more = widgets::label(mtm, &secondary, Text::Subheadline, &theme.color(Key::TextTertiary));
    let markers = widgets::stack(mtm, &[], false, metrics::SPACE_4);
    let header = widgets::stack(mtm, &[&title, &meta, &more, &markers], true, metrics::SPACE_2);
    header.setAlignment(NSLayoutAttribute::Leading);
    header.setCustomSpacing_afterView(metrics::SPACE_4, &title);
    header.setCustomSpacing_afterView(metrics::SPACE_8, &more);
    for view in [&*title, &*meta, &*more] {
        view.trailingAnchor().constraintLessThanOrEqualToAnchor(&header.trailingAnchor()).setActive(true);
    }
    return (header, markers);
}

fn frame_menu(mtm: MainThreadMarker, target: &AnyObject) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    menu.addItem(&widgets::menu_item(mtm, FRAME_TO_LOCK, sel!(lockFrame:), Some(target), 0, None));
    return menu;
}

fn actions(
    mtm: MainThreadMarker,
    theme: &Theme,
    video: bool,
    item: &Item,
    target: &AnyObject,
) -> (Retained<NSStackView>, Retained<NSPopUpButton>) {
    let (title, icon, action) = match (item.playable, item.progress) {
        (true, _) => (tr("Set as Wallpaper"), "photo.on.rectangle", sel!(setWallpaper:)),
        (false, None) => (tr("Optimize"), "wand.and.sparkles", sel!(optimize:)),
        (false, Some(_)) => (tr("Optimizing…"), "wand.and.sparkles", sel!(optimize:)),
    };
    let wallpaper = widgets::primary(mtm, &title, Some((icon, IconRole::Inline)), action, target, theme);
    wallpaper.setKeyEquivalent(&widgets::ns("\r"));
    wallpaper.setEnabled(item.playable || item.progress.is_none());
    let lock = widgets::secondary(
        mtm,
        &tr("Lock Screen"),
        Some(("lock", IconRole::Inline)),
        sel!(setLock:),
        target,
        theme,
    );
    lock.setToolTip(Some(&widgets::ns(&tr("Set as Lock Screen"))));
    lock.widthAnchor().constraintEqualToConstant(LOCK_WIDTH).setActive(true);
    wallpaper.setContentHuggingPriority_forOrientation(1.0, objc2_app_kit::NSLayoutConstraintOrientation::Horizontal);
    let buttons = widgets::stack(mtm, &[&wallpaper, &lock], false, metrics::SPACE_8);
    buttons.setDistribution(objc2_app_kit::NSStackViewDistribution::Fill);
    lock.heightAnchor().constraintEqualToAnchor(&wallpaper.heightAnchor()).setActive(true);
    let destination = widgets::popup(mtm, true, theme);
    destination.setToolTip(Some(&widgets::ns(&tr("Choose a specific desktop"))));
    let stack = widgets::stack(mtm, &[&buttons, &destination], true, metrics::SPACE_8);
    stack.setAlignment(NSLayoutAttribute::Leading);
    buttons.leadingAnchor().constraintEqualToAnchor(&stack.leadingAnchor()).setActive(true);
    buttons.trailingAnchor().constraintEqualToAnchor(&stack.trailingAnchor()).setActive(true);
    destination.leadingAnchor().constraintEqualToAnchor(&stack.leadingAnchor()).setActive(true);
    destination.trailingAnchor().constraintEqualToAnchor(&wallpaper.trailingAnchor()).setActive(true);
    if video {
        let frame = widgets::secondary(
            mtm,
            &tr("Use This Frame"),
            Some(("film", IconRole::Inline)),
            sel!(lockFrame:),
            target,
            theme,
        );
        frame.setToolTip(Some(&widgets::ns(&tr(FRAME_TO_LOCK))));
        frame.setTranslatesAutoresizingMaskIntoConstraints(false);
        stack.addSubview(&frame);
        frame.leadingAnchor().constraintEqualToAnchor(&lock.leadingAnchor()).setActive(true);
        frame.trailingAnchor().constraintEqualToAnchor(&lock.trailingAnchor()).setActive(true);
        frame.centerYAnchor().constraintEqualToAnchor(&destination.centerYAnchor()).setActive(true);
    }
    return (stack, destination);
}

type Playback = (
    Retained<NSStackView>,
    Retained<NSButton>,
    Retained<NSSlider>,
    Retained<NumberField>,
    Retained<NSSlider>,
    Retained<NumberField>,
);

fn playback(mtm: MainThreadMarker, theme: &Theme, target: &AnyObject) -> Playback {
    let mute = widgets::icon_button(mtm, "speaker.wave.2.fill", sel!(mute:), target, Key::TextSecondary, theme);
    let volume = slider(mtm, 0.0, 0.0, 100.0, VOLUME, theme, target);
    volume.widthAnchor().constraintEqualToConstant(VOLUME_WIDTH).setActive(true);
    let volume_value = field(mtm, VOLUME, target, theme);
    let volume_box = widgets::stack(mtm, &[&mute, &volume, &volume_value], false, metrics::SPACE_4);
    let speed = slider(mtm, 1.0, 0.25, 2.0, SPEED, theme, target);
    speed.widthAnchor().constraintEqualToConstant(SLIDER_WIDTH).setActive(true);
    let speed_value = field(mtm, SPEED, target, theme);
    let speed_box = widgets::stack(mtm, &[&speed, &speed_value], false, metrics::SPACE_4);
    let volume_row = widgets::row(mtm, &tr("Volume"), &volume_box, theme);
    let speed_row = widgets::row(mtm, &tr("Speed"), &speed_box, theme);
    let section = widgets::section(mtm, &tr("PLAYBACK"), &[&volume_row, &speed_row], theme);
    return (section, mute, volume, volume_value, speed, speed_value);
}

fn placement(
    mtm: MainThreadMarker,
    theme: &Theme,
    target: &AnyObject,
) -> (Retained<NSStackView>, Retained<NSSegmentedControl>, [Offset; 3]) {
    let labels: Vec<String> = MODES.iter().map(|text| tr(text)).collect();
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let modes = widgets::segmented(mtm, &refs, sel!(fill:), target, Key::InspectorSegment, theme);
    let offsets = [
        (slider(mtm, 0.0, -100.0, 100.0, HORIZONTAL, theme, target), field(mtm, HORIZONTAL, target, theme)),
        (slider(mtm, 0.0, -100.0, 100.0, VERTICAL, theme, target), field(mtm, VERTICAL, target, theme)),
        (slider(mtm, 1.0, 1.0, 4.0, ZOOM, theme, target), field(mtm, ZOOM, target, theme)),
    ];
    let rows: Vec<Retained<NSStackView>> = [tr("Horizontal"), tr("Vertical"), tr("Zoom")]
        .iter()
        .zip(&offsets)
        .map(|(name, (control, value))| {
            control.widthAnchor().constraintEqualToConstant(SLIDER_WIDTH).setActive(true);
            return widgets::row(mtm, name, &widgets::stack(mtm, &[control, value], false, metrics::SPACE_4), theme);
        })
        .collect();
    let gap = NSView::new(mtm);
    gap.heightAnchor().constraintEqualToConstant(metrics::SPACE_2).setActive(true);
    let mut views: Vec<&NSView> = vec![&modes, &gap];
    views.extend(rows.iter().map(|row| &***row));
    let section = widgets::section(mtm, &tr("PLACEMENT"), &views, theme);
    return (section, modes, offsets);
}

fn reset_button(mtm: MainThreadMarker, theme: &Theme, target: &AnyObject) -> Retained<NSButton> {
    let reset = widgets::secondary(
        mtm,
        "",
        Some(("arrow.counterclockwise", IconRole::Inline)),
        sel!(resetSettings:),
        target,
        theme,
    );
    widgets::quiet(&reset, Key::Destructive, theme);
    widgets::describe(
        &reset,
        &tr("Reset playback and placement for this wallpaper"),
        &tr("Reset to Defaults"),
    );
    return reset;
}

fn library_row(
    mtm: MainThreadMarker,
    theme: &Theme,
    target: &AnyObject,
) -> (Retained<NSStackView>, Retained<NSPopUpButton>) {
    let folder = widgets::popup(mtm, false, theme);
    unsafe {
        folder.setTarget(Some(target));
        folder.setAction(Some(sel!(moveFolder:)));
    }
    if let Some(menu) = folder.menu() {
        menu.setAutoenablesItems(false);
    }
    folder.setContentHuggingPriority_forOrientation(1.0, objc2_app_kit::NSLayoutConstraintOrientation::Horizontal);
    let icon = widgets::symbol_view(mtm, "folder", IconRole::Small, &theme.color(Key::TextSecondary));
    theme::register(&icon, Key::TextSecondary, Slot::Tint);
    icon.setContentCompressionResistancePriority_forOrientation(
        1000.0,
        objc2_app_kit::NSLayoutConstraintOrientation::Horizontal,
    );
    let delete = widgets::icon_button(mtm, "trash", sel!(delete:), target, Key::Destructive, theme);
    widgets::describe(&delete, &tr("Remove from Library"), &tr("Remove from Library"));
    let row = widgets::stack(mtm, &[&icon, &folder, &delete], false, metrics::SPACE_8);
    row.heightAnchor().constraintEqualToConstant(metrics::BUTTON_HEIGHT).setActive(true);
    return (row, folder);
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
    secondary.push(crate::i18n::bytes(i64::try_from(item.bytes).unwrap_or(i64::MAX)));
    secondary.push(trf("Added %@", &[Arg::Text(crate::i18n::date(item.added))]));
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
    return FILLS
        .iter()
        .position(|candidate| *candidate == mode)
        .and_then(|index| isize::try_from(index).ok())
        .unwrap_or(0);
}

pub fn stored(item: &Item, tag: isize, value: f64) -> f64 {
    let (width, height) = reach(item);
    let span = if tag == HORIZONTAL { width } else { height };
    return match tag {
        VOLUME => (value / 100.0).clamp(0.0, 1.0),
        SPEED => value.max(0.05),
        HORIZONTAL | VERTICAL if span > 0.0 => (value / span).clamp(-1.0, 1.0),
        HORIZONTAL | VERTICAL => 0.0,
        ZOOM => value.max(0.1),
        _ => value.max(0.0),
    };
}

fn field(mtm: MainThreadMarker, tag: isize, target: &AnyObject, theme: &Theme) -> Retained<NumberField> {
    let (step, format): (f64, fn(f64) -> String) = match tag {
        VOLUME => (5.0, |value| format!("{value:.0}%")),
        HORIZONTAL | VERTICAL => (10.0, |value| format!("{value:+.0} px")),
        SCRUB => (1.0, duration),
        _ => (0.05, |value| format!("{value:.2}×")),
    };
    let parse: fn(&str) -> Option<f64> = if tag == SCRUB { seconds } else { widgets::number };
    let field = NumberField::new(mtm, Units { step, format, parse }, tag, VALUE_WIDTH, sel!(typed:), target, theme);
    if let Some(title) = knob_title(tag) {
        field.setAccessibilityLabel(Some(&widgets::ns(&title)));
    }
    return field;
}

fn seconds(text: &str) -> Option<f64> {
    let Some((minutes, rest)) = text.split_once(':') else {
        return widgets::number(text);
    };
    return Some(widgets::number(minutes)? * 60.0 + widgets::number(rest)?);
}

fn fill_folders(popup: &NSPopUpButton, library: &Library, folder: Option<usize>) {
    popup.removeAllItems();
    let Some(menu) = popup.menu() else { return };
    let mtm = popup.mtm();
    let unfiled = NSMenuItem::new(mtm);
    unfiled.setTitle(&widgets::ns(&tr("Unfiled")));
    unfiled.setTag(TAG_ALL);
    menu.addItem(&unfiled);
    for (index, name) in library.folders.iter().enumerate() {
        let item = NSMenuItem::new(mtm);
        item.setTitle(&widgets::ns(name));
        item.setTag(isize::try_from(index).unwrap_or(TAG_ALL));
        item.setEnabled(!library.sealed(index));
        item.setImage(folders::image(library, index, metrics::ICON_ROW).as_deref());
        menu.addItem(&item);
    }
    popup.selectItemAtIndex(folder.map_or(0, |index| isize::try_from(index + 1).unwrap_or(0)));
}

fn slider(
    mtm: MainThreadMarker,
    value: f64,
    min: f64,
    max: f64,
    tag: isize,
    theme: &Theme,
    target: &AnyObject,
) -> Retained<NSSlider> {
    let action: Sel = sel!(slide:);
    let slider = unsafe {
        NSSlider::sliderWithValue_minValue_maxValue_target_action(value, min, max, Some(target), Some(action), mtm)
    };
    slider.setTrackFillColor(Some(&theme.color(Key::InspectorSlider)));
    theme::register(&slider, Key::InspectorSlider, Slot::Track);
    slider.setControlSize(NSControlSize::Small);
    slider.setTag(tag);
    if let Some(title) = knob_title(tag) {
        slider.setAccessibilityLabel(Some(&widgets::ns(&title)));
    }
    return slider;
}

fn knob_title(tag: isize) -> Option<String> {
    let title = match tag {
        VOLUME => i18n::key("Volume"),
        SPEED => i18n::key("Speed"),
        HORIZONTAL => i18n::key("Horizontal"),
        VERTICAL => i18n::key("Vertical"),
        ZOOM => i18n::key("Zoom"),
        _ => return None,
    };
    return Some(tr(title));
}
