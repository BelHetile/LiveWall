use std::ptr::NonNull;
use std::sync::mpsc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSButton, NSColor, NSControlSize, NSEvent, NSFont, NSFontWeightRegular, NSImage, NSLayoutAttribute,
    NSLineBreakMode, NSMenu, NSMenuItem, NSPopUpButton, NSScreen, NSScrollView, NSSegmentSwitchTracking,
    NSSegmentedControl, NSSlider, NSStackView, NSStackViewGravity, NSTextAlignment, NSView,
};
use objc2_av_foundation::{
    AVAssetImageGenerator, AVLayerVideoGravityResizeAspectFill, AVPlayerItem, AVPlayerLayer, AVPlayerLooper,
    AVQueuePlayer, AVURLAsset,
};
use objc2_core_foundation::{CFRetained, CGFloat, CGRect, CGSize};
use objc2_core_graphics::CGImage;
use objc2_core_media::{CMTime, kCMTimeZero};
use objc2_foundation::{NSArray, NSError, NSString, NSTimer, NSURL};
use objc2_quartz_core::{CAAutoresizingMask, CALayer, kCAGravityResizeAspectFill};

use proto::{FillMode, Geometry};

use crate::geometry::{self, Size};
use crate::model::ItemKind;
use crate::ui::fixture::{Item, Library, Settings, Still};
use crate::ui::palette::Theme;
use crate::ui::state::{Filter, Scope, markers};
use crate::ui::toolbar::scope_menu;
use crate::ui::wall::{duration, kind_name, resolution};
use crate::ui::widgets::{self, NumberField, Units, Weight};

const WIDTH: CGFloat = 340.0;
const INSET: CGFloat = 20.0;
pub const VOLUME: isize = 1;
pub const SPEED: isize = 2;
pub const HORIZONTAL: isize = 3;
pub const VERTICAL: isize = 4;
pub const ZOOM: isize = 5;
pub const SCRUB: isize = 6;
pub const RESET: isize = -100;
const STILL_PX: CGFloat = 1280.0;
pub const FRAME_TO_LOCK: &str = "Set This Frame as Lock Screen";
const MODES: [&str; 5] = ["Fill", "Fit", "Stretch", "Center", "Custom"];

type Offset = (Retained<NSSlider>, Retained<NumberField>);

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    struct PreviewView;

    impl PreviewView {
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            crate::ui::toggle_play();
        }
    }
);

struct Controls {
    play: Retained<NSButton>,
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
}

pub struct Inspector {
    pub scroll: Retained<NSScrollView>,
    pub target: Option<Scope>,
    backdrop: Retained<objc2_app_kit::NSBox>,
    content: Retained<NSStackView>,
    shown: Option<usize>,
    player: Option<(Retained<AVQueuePlayer>, Retained<AVPlayerLooper>)>,
    timer: Option<Retained<NSTimer>>,
    playing: bool,
    controls: Option<Controls>,
}

impl Inspector {
    pub fn new(mtm: MainThreadMarker, theme: &Theme) -> Self {
        let scroll = NSScrollView::new(mtm);
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setAutomaticallyAdjustsContentInsets(true);
        let content = widgets::stack(mtm, &[], true, 14.0);
        content.setAlignment(NSLayoutAttribute::Leading);
        content.setEdgeInsets(widgets::insets(10.0, INSET, 20.0, INSET));
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
        let backdrop = widgets::card(mtm, &theme.chrome, 0.0);
        scroll.addSubview_positioned_relativeTo(&backdrop, objc2_app_kit::NSWindowOrderingMode::Below, None);
        widgets::pin(&backdrop, &scroll, 0.0);
        return Self {
            scroll,
            target: None,
            backdrop,
            content,
            shown: None,
            player: None,
            timer: None,
            playing: true,
            controls: None,
        };
    }

    pub fn width() -> CGFloat {
        return WIDTH;
    }

    pub fn restyle(&mut self, theme: &Theme) {
        self.backdrop.setFillColor(&theme.chrome);
        self.shown = None;
        self.controls = None;
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
        let rebuilt = selected != self.shown || (selected.is_some() && self.controls.is_none());
        if rebuilt {
            self.show(library, theme, selected, target);
        }
        let (Some(index), Some(controls)) = (selected, &self.controls) else {
            return;
        };
        let views: Vec<Retained<NSView>> =
            markers(library, index, filter.scope).iter().map(|marker| widgets::chip(mtm, marker, theme)).collect();
        let refs: Vec<&NSView> = views.iter().map(|view| &**view).collect();
        controls.markers.setViews_inGravity(&NSArray::from_slice(&refs), NSStackViewGravity::Leading);
        if rebuilt {
            controls.markers.setHidden(refs.is_empty());
        } else {
            widgets::reveal(&controls.markers, !refs.is_empty());
        }
        let scope = self.target.unwrap_or(filter.scope);
        let title = format!("On {}", scope.title());
        let menu = scope_menu(library, scope, &title, sel!(target:), target);
        if self.target.is_some() {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            let reset = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str("Match Toolbar"),
                    Some(sel!(target:)),
                    &NSString::new(),
                )
            };
            unsafe { reset.setTarget(Some(target)) };
            reset.setTag(RESET);
            menu.addItem(&reset);
        }
        controls.destination.setMenu(Some(&menu));
        widgets::alpha(&controls.destination, if self.target.is_some() { 1.0 } else { 0.85 });
        fill_folders(&controls.folder, library, library.items[index].folder);
        self.sync(&library.items[index]);
    }

    fn clear(&mut self) {
        let views = self.content.arrangedSubviews();
        for view in (0..views.count()).map(|position| views.objectAtIndex(position)) {
            self.content.removeArrangedSubview(&view);
            view.removeFromSuperview();
        }
        if let Some(timer) = self.timer.take() {
            timer.invalidate();
        }
        if let Some((player, _)) = self.player.take() {
            unsafe { player.pause() };
        }
        self.controls = None;
        self.playing = true;
    }

    fn show(&mut self, library: &Library, theme: &Theme, selected: Option<usize>, target: &AnyObject) {
        let mtm = self.scroll.mtm();
        widgets::fade(&self.content);
        self.clear();
        self.shown = selected;
        self.target = None;
        let Some(index) = selected else {
            self.empty(mtm);
            return;
        };
        let item = &library.items[index];
        let video = item.kind == ItemKind::Video;

        let preview = self.preview(mtm, item);
        if video {
            unsafe { preview.setMenu(Some(&frame_menu(mtm, target))) };
        }
        let play = widgets::icon_button(mtm, "pause.fill", sel!(play:), target);
        let scrub = slider(mtm, 0.0, 0.0, item.duration.max(0.01), SCRUB, theme, target);
        let time = field(mtm, SCRUB, target);
        let length = widgets::label(
            mtm,
            &format!("/ {}", duration(item.duration)),
            11.0,
            &Weight::Regular,
            &NSColor::tertiaryLabelColor(),
        );
        length.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(11.0, unsafe { NSFontWeightRegular })));
        length.setContentCompressionResistancePriority_forOrientation(
            1000.0,
            objc2_app_kit::NSLayoutConstraintOrientation::Horizontal,
        );
        let transport = widgets::stack(mtm, &[&play, &scrub, &time, &length], false, 8.0);
        transport.setCustomSpacing_afterView(4.0, &time);
        transport.setHidden(!video);

        let title = widgets::label(mtm, &item.title, 15.0, &Weight::Semibold, &NSColor::labelColor());
        title.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
        title.setToolTip(Some(&NSString::from_str(&item.title)));
        let (primary, secondary) = facts(item);
        let meta = widgets::label(mtm, &primary, 11.0, &Weight::Regular, &NSColor::secondaryLabelColor());
        let more = widgets::label(mtm, &secondary, 11.0, &Weight::Regular, &NSColor::tertiaryLabelColor());
        let marker_stack = widgets::stack(mtm, &[], false, 5.0);
        let header = widgets::stack(mtm, &[&title, &meta, &more, &marker_stack], true, 2.0);
        header.setAlignment(NSLayoutAttribute::Leading);
        header.setCustomSpacing_afterView(4.0, &title);
        header.setCustomSpacing_afterView(8.0, &more);
        for view in [&*title, &*meta, &*more] {
            view.trailingAnchor().constraintLessThanOrEqualToAnchor(&header.trailingAnchor()).setActive(true);
        }

        let (actions, destination) = actions(mtm, theme, video, target);
        let (playback, mute, volume, volume_value, speed, speed_value) = playback(mtm, theme, target);
        let (placement, modes, offsets) = placement(mtm, theme, target);
        let (library_row, folder) = library_row(mtm, target);
        let reset = reset_button(mtm, target);

        let mut sections: Vec<Retained<NSView>> = vec![preview.clone()];
        if video {
            sections.push(widgets::view(&transport));
        }
        sections.extend([widgets::view(&header), widgets::view(&actions)]);
        if video {
            sections.push(widgets::divider(mtm, theme));
            sections.push(widgets::view(&playback));
        }
        sections.extend([
            widgets::divider(mtm, theme),
            widgets::view(&placement),
            widgets::divider(mtm, theme),
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
        self.content.setCustomSpacing_afterView(if video { 4.0 } else { 12.0 }, &preview);
        if video {
            self.content.setCustomSpacing_afterView(8.0, &transport);
        }
        preview.heightAnchor().constraintEqualToAnchor_multiplier(&preview.widthAnchor(), 10.0 / 16.0).setActive(true);

        self.content.setCustomSpacing_afterView(8.0, &library_row);
        self.start_clock(&scrub, &time, item.duration);
        time.show(0.0);
        self.controls = Some(Controls {
            play,
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
        });
    }

    fn start_clock(&mut self, scrub: &NSSlider, time: &NumberField, length: f64) {
        let Some((player, _)) = &self.player else {
            return;
        };
        let player = player.clone();
        let (scrub, time): (Retained<NSSlider>, Retained<NumberField>) = (scrub.into(), time.into());
        let tick = RcBlock::new(move |_timer: NonNull<NSTimer>| {
            let seconds = unsafe { player.currentTime().seconds() };
            if seconds.is_finite() && length > 0.0 && !time.editing() {
                scrub.setDoubleValue(seconds % length);
                time.show(seconds % length);
            }
        });
        self.timer = Some(unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(0.25, true, &tick) });
    }

    pub fn sync(&self, item: &Item) {
        let Some(controls) = &self.controls else {
            return;
        };
        let settings = &item.settings;
        let silent = settings.muted || settings.volume <= 0.0;
        widgets::set_icon(&controls.mute, if silent { "speaker.slash.fill" } else { "speaker.wave.2.fill" });
        widgets::set_icon(&controls.play, if self.playing { "pause.fill" } else { "play.fill" });
        let volume = if settings.muted { 0.0 } else { settings.volume * 100.0 };
        controls.volume.setDoubleValue(volume);
        controls.volume_value.show(volume);
        controls.speed.setDoubleValue(settings.speed);
        controls.speed_value.show(settings.speed);
        controls.modes.setSelectedSegment(settings.fill);
        let shifts = matches!(settings.fill, 0 | 3 | 4);
        let (width, height) = reach(item);
        let values = [
            (settings.x * width, width, shifts && width > 0.0),
            (settings.y * height, height, shifts && height > 0.0),
            (settings.zoom, 0.0, settings.fill == 4),
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
            widgets::alpha(value_field, if enabled { 1.0 } else { 0.4 });
        }
        let changed = *settings != Settings::default();
        controls.reset.setEnabled(changed);
        widgets::alpha(&controls.reset, if changed { 1.0 } else { 0.45 });
        if let Some((player, _)) = &self.player {
            #[allow(clippy::cast_possible_truncation)]
            unsafe {
                player.setVolume(settings.volume as f32);
                player.setMuted(silent);
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

    pub fn still(&self) -> Option<Still> {
        let (player, _) = self.player.as_ref()?;
        let item = unsafe { player.currentItem() }?;
        let time = unsafe { item.currentTime() };
        let generator = unsafe { AVAssetImageGenerator::assetImageGeneratorWithAsset(&item.asset()) };
        unsafe {
            generator.setAppliesPreferredTrackTransform(true);
            generator.setMaximumSize(CGSize::new(STILL_PX, STILL_PX));
            generator.setRequestedTimeToleranceBefore(kCMTimeZero);
            generator.setRequestedTimeToleranceAfter(kCMTimeZero);
        }
        let (sender, receiver) = mpsc::channel();
        let block = RcBlock::new(move |image: *mut CGImage, _time: CMTime, _error: *mut NSError| {
            let _ = sender.send(NonNull::new(image).map(|image| unsafe { CFRetained::retain(image) }));
        });
        unsafe { generator.generateCGImageAsynchronouslyForTime_completionHandler(time, &block) };
        let image = receiver.recv().ok().flatten()?;
        let seconds = unsafe { time.seconds() };
        let length = unsafe { item.duration().seconds() };
        return Some(Still {
            time: if length.is_finite() && length > 0.0 { seconds % length } else { seconds },
            image: NSImage::initWithCGImage_size(NSImage::alloc(), &image, CGSize::ZERO),
        });
    }

    pub fn seek(&self, seconds: f64) {
        if let Some((player, _)) = &self.player {
            unsafe { player.seekToTime(CMTime::with_seconds(seconds, 600)) };
        }
        if let Some(controls) = &self.controls {
            controls.time.show(seconds);
        }
    }

    fn empty(&self, mtm: MainThreadMarker) {
        let icon = widgets::symbol_view(mtm, "sparkles.rectangle.stack", 30.0, &NSColor::tertiaryLabelColor());
        let title = widgets::label(mtm, "No Selection", 15.0, &Weight::Semibold, &NSColor::secondaryLabelColor());
        let hint = widgets::label(
            mtm,
            "Select a wallpaper to preview it and\nchange how it plays.",
            12.0,
            &Weight::Regular,
            &NSColor::tertiaryLabelColor(),
        );
        hint.setMaximumNumberOfLines(2);
        hint.setAlignment(NSTextAlignment::Center);
        let stack = widgets::stack(mtm, &[&icon, &title, &hint], true, 8.0);
        stack.setAlignment(NSLayoutAttribute::CenterX);
        self.content.addArrangedSubview(&stack);
        stack.leadingAnchor().constraintEqualToAnchor(&self.content.leadingAnchor()).setActive(true);
        stack.trailingAnchor().constraintEqualToAnchor(&self.content.trailingAnchor()).setActive(true);
        stack.topAnchor().constraintEqualToAnchor_constant(&self.content.topAnchor(), 200.0).setActive(true);
    }

    fn preview(&mut self, mtm: MainThreadMarker, item: &Item) -> Retained<NSView> {
        let view: Retained<PreviewView> = unsafe { msg_send![PreviewView::alloc(mtm), init] };
        let layer: Retained<CALayer> = if item.kind == ItemKind::Video {
            let url = NSURL::fileURLWithPath(&NSString::from_str(&item.path.to_string_lossy()));
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
        if let Some(host) = view.layer() {
            host.setCornerRadius(12.0);
            host.setMasksToBounds(true);
            host.setBorderWidth(0.5);
            host.setBorderColor(Some(&widgets::cg(mtm, &NSColor::separatorColor())));
            host.addSublayer(&layer);
        }
        return Retained::into_super(view);
    }
}

fn frame_menu(mtm: MainThreadMarker, target: &AnyObject) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(FRAME_TO_LOCK),
            Some(sel!(lockFrame:)),
            &NSString::new(),
        )
    };
    unsafe { item.setTarget(Some(target)) };
    menu.addItem(&item);
    return menu;
}

fn actions(
    mtm: MainThreadMarker,
    theme: &Theme,
    video: bool,
    target: &AnyObject,
) -> (Retained<NSStackView>, Retained<NSPopUpButton>) {
    let wallpaper = widgets::button(mtm, "Set as Wallpaper", Some("photo.on.rectangle"), sel!(setWallpaper:), target);
    widgets::prominent(&wallpaper, theme);
    wallpaper.setKeyEquivalent(&NSString::from_str("\r"));
    let lock = widgets::button(mtm, "Lock Screen", Some("lock"), sel!(setLock:), target);
    lock.setToolTip(Some(&NSString::from_str("Set as Lock Screen")));
    lock.setAttributedTitle(&widgets::styled(
        "Lock Screen",
        &NSFont::systemFontOfSize(12.0),
        &NSColor::secondaryLabelColor(),
    ));
    lock.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
    lock.widthAnchor().constraintEqualToConstant(118.0).setActive(true);
    wallpaper.setContentHuggingPriority_forOrientation(1.0, objc2_app_kit::NSLayoutConstraintOrientation::Horizontal);
    let buttons = widgets::stack(mtm, &[&wallpaper, &lock], false, 8.0);
    buttons.setDistribution(objc2_app_kit::NSStackViewDistribution::Fill);
    lock.heightAnchor().constraintEqualToAnchor(&wallpaper.heightAnchor()).setActive(true);
    let destination = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), CGRect::ZERO, true);
    widgets::secondary(&destination);
    destination.setToolTip(Some(&NSString::from_str("Choose a specific desktop")));
    let stack = widgets::stack(mtm, &[&buttons, &destination], true, 8.0);
    stack.setAlignment(NSLayoutAttribute::Leading);
    buttons.leadingAnchor().constraintEqualToAnchor(&stack.leadingAnchor()).setActive(true);
    buttons.trailingAnchor().constraintEqualToAnchor(&stack.trailingAnchor()).setActive(true);
    destination.leadingAnchor().constraintEqualToAnchor(&stack.leadingAnchor()).setActive(true);
    destination.trailingAnchor().constraintEqualToAnchor(&wallpaper.trailingAnchor()).setActive(true);
    if video {
        let frame = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Use This Frame"),
                Some(target),
                Some(sel!(lockFrame:)),
                mtm,
            )
        };
        widgets::secondary(&frame);
        frame.setAttributedTitle(&widgets::styled(
            "Use This Frame",
            &NSFont::systemFontOfSize(12.0),
            &NSColor::secondaryLabelColor(),
        ));
        frame.setToolTip(Some(&NSString::from_str(FRAME_TO_LOCK)));
        frame.setImage(widgets::symbol("film", 11.0).as_deref());
        frame.setImagePosition(objc2_app_kit::NSCellImagePosition::ImageLeading);
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
    let mute = widgets::icon_button(mtm, "speaker.wave.2.fill", sel!(mute:), target);
    let volume = slider(mtm, 0.0, 0.0, 100.0, VOLUME, theme, target);
    volume.widthAnchor().constraintEqualToConstant(136.0).setActive(true);
    let volume_value = field(mtm, VOLUME, target);
    let volume_box = widgets::stack(mtm, &[&mute, &volume, &volume_value], false, 6.0);
    let speed = slider(mtm, 1.0, 0.25, 2.0, SPEED, theme, target);
    speed.widthAnchor().constraintEqualToConstant(164.0).setActive(true);
    let speed_value = field(mtm, SPEED, target);
    let speed_box = widgets::stack(mtm, &[&speed, &speed_value], false, 6.0);
    let volume_row = widgets::row(mtm, "Volume", &volume_box);
    let speed_row = widgets::row(mtm, "Speed", &speed_box);
    let section = widgets::section(mtm, "PLAYBACK", &[&volume_row, &speed_row]);
    return (section, mute, volume, volume_value, speed, speed_value);
}

fn placement(
    mtm: MainThreadMarker,
    theme: &Theme,
    target: &AnyObject,
) -> (Retained<NSStackView>, Retained<NSSegmentedControl>, [Offset; 3]) {
    let labels: Vec<Retained<NSString>> = MODES.iter().map(|text| NSString::from_str(text)).collect();
    let modes = unsafe {
        NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
            &NSArray::from_retained_slice(&labels),
            NSSegmentSwitchTracking::SelectOne,
            Some(target),
            Some(sel!(fill:)),
            mtm,
        )
    };
    modes.setSelectedSegmentBezelColor(Some(&theme.accent));
    modes.setSegmentDistribution(objc2_app_kit::NSSegmentDistribution::FillEqually);
    modes.setControlSize(NSControlSize::Small);
    let offsets = [
        (slider(mtm, 0.0, -100.0, 100.0, HORIZONTAL, theme, target), field(mtm, HORIZONTAL, target)),
        (slider(mtm, 0.0, -100.0, 100.0, VERTICAL, theme, target), field(mtm, VERTICAL, target)),
        (slider(mtm, 1.0, 1.0, 4.0, ZOOM, theme, target), field(mtm, ZOOM, target)),
    ];
    let rows: Vec<Retained<NSStackView>> = ["Horizontal", "Vertical", "Zoom"]
        .iter()
        .zip(&offsets)
        .map(|(name, (control, value))| {
            control.widthAnchor().constraintEqualToConstant(164.0).setActive(true);
            return widgets::row(mtm, name, &widgets::stack(mtm, &[control, value], false, 6.0));
        })
        .collect();
    let gap = NSView::new(mtm);
    gap.heightAnchor().constraintEqualToConstant(2.0).setActive(true);
    let mut views: Vec<&NSView> = vec![&modes, &gap];
    views.extend(rows.iter().map(|row| &***row));
    let section = widgets::section(mtm, "PLACEMENT", &views);
    return (section, modes, offsets);
}

fn reset_button(mtm: MainThreadMarker, target: &AnyObject) -> Retained<NSButton> {
    let reset = widgets::button(mtm, "", Some("arrow.counterclockwise"), sel!(resetSettings:), target);
    let font = NSFont::systemFontOfSize(12.0);
    reset.setAttributedTitle(&widgets::styled("Reset to Defaults", &font, &NSColor::systemRedColor()));
    reset.setImagePosition(objc2_app_kit::NSCellImagePosition::ImageLeading);
    reset.setContentTintColor(Some(&NSColor::systemRedColor()));
    reset.setToolTip(Some(&NSString::from_str("Reset playback and placement for this wallpaper")));
    return reset;
}

fn library_row(mtm: MainThreadMarker, target: &AnyObject) -> (Retained<NSStackView>, Retained<NSPopUpButton>) {
    let folder = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), CGRect::ZERO, false);
    unsafe {
        folder.setTarget(Some(target));
        folder.setAction(Some(sel!(moveFolder:)));
    }
    folder.setControlSize(NSControlSize::Small);
    folder.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    folder.setContentHuggingPriority_forOrientation(1.0, objc2_app_kit::NSLayoutConstraintOrientation::Horizontal);
    let icon = widgets::symbol_view(mtm, "folder", 12.0, &NSColor::secondaryLabelColor());
    icon.setContentCompressionResistancePriority_forOrientation(
        1000.0,
        objc2_app_kit::NSLayoutConstraintOrientation::Horizontal,
    );
    let delete = widgets::icon_button(mtm, "trash", sel!(delete:), target);
    delete.setContentTintColor(Some(&NSColor::systemRedColor()));
    delete.setToolTip(Some(&NSString::from_str("Remove from Library")));
    let row = widgets::stack(mtm, &[&icon, &folder, &delete], false, 8.0);
    row.heightAnchor().constraintEqualToConstant(26.0).setActive(true);
    return (row, folder);
}

fn facts(item: &Item) -> (String, String) {
    let mut primary = vec![kind_name(item.kind).to_string()];
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
    #[allow(clippy::cast_precision_loss)]
    secondary.push(format!("{:.1} MB", item.bytes as f64 / 1_048_576.0));
    secondary.push(format!("Added {}", added(item.added)));
    return (primary.join(" · "), secondary.join(" · "));
}

fn added(order: usize) -> String {
    let days = i64::try_from(order).unwrap_or(0);
    let base = jiff::civil::date(2026, 9, 1);
    return base
        .checked_add(jiff::Span::new().days(days * 2))
        .map_or_else(|_| base.strftime("%-d %b %Y").to_string(), |day| day.strftime("%-d %b %Y").to_string());
}

fn reach(item: &Item) -> (f64, f64) {
    let screen = MainThreadMarker::new().and_then(NSScreen::mainScreen).map_or(Size::new(3024.0, 1964.0), |screen| {
        let frame = screen.frame();
        let scale = screen.backingScaleFactor();
        return Size::new(frame.size.width * scale, frame.size.height * scale);
    });
    let settings = &item.settings;
    let mode = match settings.fill {
        1 => FillMode::Fit,
        2 => FillMode::Stretch,
        3 => FillMode::Center,
        4 => FillMode::Custom,
        _ => FillMode::Fill,
    };
    let zoom = if settings.fill == 4 { settings.zoom } else { 1.0 };
    let media = Size::new(f64::from(item.width), f64::from(item.height));
    let rect = geometry::resolve(mode, Geometry { x: 0.0, y: 0.0, zoom }, media, screen).rect;
    return (((rect.w - screen.w) / 2.0).max(0.0).round(), ((rect.h - screen.h) / 2.0).max(0.0).round());
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

fn field(mtm: MainThreadMarker, tag: isize, target: &AnyObject) -> Retained<NumberField> {
    let (step, format): (f64, fn(f64) -> String) = match tag {
        VOLUME => (5.0, |value| format!("{value:.0}%")),
        HORIZONTAL | VERTICAL => (10.0, |value| format!("{value:+.0} px")),
        SCRUB => (1.0, duration),
        _ => (0.05, |value| format!("{value:.2}×")),
    };
    let parse: fn(&str) -> Option<f64> = if tag == SCRUB { seconds } else { widgets::number };
    return NumberField::new(mtm, Units { step, format, parse }, tag, 50.0, sel!(typed:), target);
}

fn seconds(text: &str) -> Option<f64> {
    let Some((minutes, rest)) = text.split_once(':') else {
        return widgets::number(text);
    };
    return Some(widgets::number(minutes)? * 60.0 + widgets::number(rest)?);
}

fn fill_folders(popup: &NSPopUpButton, library: &Library, folder: Option<usize>) {
    popup.removeAllItems();
    let mut titles: Vec<Retained<NSString>> = vec![NSString::from_str("Unfiled")];
    titles.extend(library.folders.iter().map(|name| NSString::from_str(name)));
    popup.addItemsWithTitles(&NSArray::from_retained_slice(&titles));
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
    slider.setTrackFillColor(Some(&theme.accent));
    slider.setControlSize(NSControlSize::Small);
    slider.setTag(tag);
    return slider;
}
