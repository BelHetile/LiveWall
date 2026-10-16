use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSColor, NSEvent, NSEventModifierFlags, NSMenu, NSScrollView,
    NSStackView, NSTextAlignment, NSTextField, NSTrackingArea, NSTrackingAreaOptions, NSView,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGColor;
use objc2_foundation::{NSArray, NSNumber};
use objc2_quartz_core::{
    CAGradientLayer, CALayer, CAMediaTiming, CAMediaTimingFunction, CATransaction, CATransform3D,
    CATransform3DIdentity, CATransition, kCAGravityResizeAspectFill, kCAMediaTimingFunctionEaseIn,
    kCAMediaTimingFunctionEaseInEaseOut, kCAMediaTimingFunctionEaseOut, kCATransitionFade,
};

use crate::model::{ItemId, ItemKind};
use crate::ui::palette::{Theme, is_dark};
use crate::ui::state::{Marker, Scope, markers};
use crate::ui::view::{Download, Item, Library, Stage};
use crate::ui::widgets::{self, Weight};
use crate::ui::{actions, menus};

pub const TILE_ASPECT: CGFloat = 10.0 / 16.0;
const PADDING: CGFloat = 16.0;
const MIN_TILE: CGFloat = 240.0;
const COLUMN_GAP: CGFloat = 16.0;
const ROW_GAP: CGFloat = 22.0;
const LABEL_HEIGHT: CGFloat = 42.0;
const RADIUS: CGFloat = 16.0;
const RING_GAP: CGFloat = 5.0;
const ROW_HEIGHT: CGFloat = 78.0;
const ROW_ART: CGFloat = 96.0;
const MORPH: f64 = 0.3;
const STATUS_WIDTH: CGFloat = 280.0;
const STATUS_BAR: CGFloat = 4.0;
const FADE_IN: f64 = 0.24;
const FADE_OUT: f64 = 0.18;
const SELECT: f64 = 0.22;
const CHIPS: f64 = 0.22;
const SCROLL_MIN: f64 = 0.16;
const SCROLL_MAX: f64 = 0.35;

fn ease_out() -> Retained<CAMediaTimingFunction> {
    return CAMediaTimingFunction::functionWithName(unsafe { kCAMediaTimingFunctionEaseOut });
}

fn ease_in() -> Retained<CAMediaTimingFunction> {
    return CAMediaTimingFunction::functionWithName(unsafe { kCAMediaTimingFunctionEaseIn });
}

fn ease_in_out() -> Retained<CAMediaTimingFunction> {
    return CAMediaTimingFunction::functionWithName(unsafe { kCAMediaTimingFunctionEaseInEaseOut });
}

fn chip_key(markers: &[Marker]) -> Vec<(&'static str, String, bool)> {
    return markers.iter().map(|marker| (marker.icon, marker.text.clone(), marker.active)).collect();
}

fn overall(downloads: &[Download]) -> f64 {
    if downloads.is_empty() {
        return 0.0;
    }
    let count = f64::from(u32::try_from(downloads.len()).unwrap_or(u32::MAX));
    return downloads.iter().map(|download| download.percent).sum::<f64>() / count;
}

fn download_line(download: &Download) -> String {
    return match download.stage {
        Stage::Preparing => "Getting video info…".to_string(),
        Stage::Merging => "Merging video and audio…".to_string(),
        Stage::Importing => "Adding to library…".to_string(),
        Stage::Downloading => {
            let mut parts = vec![format!("Downloading {:.0}%", download.percent * 100.0)];
            if let Some(speed) = download.speed.filter(|speed| *speed > 0.0) {
                parts.push(format!("{:.1} MB/s", speed / 1_048_576.0));
            }
            if let Some(eta) = download.eta.filter(|eta| eta.is_finite() && *eta >= 0.0) {
                parts.push(format!("{} left", clock(eta)));
            }
            parts.join(" · ")
        }
    };
}

fn clock(seconds: f64) -> String {
    let total = std::time::Duration::try_from_secs_f64(seconds.round()).map_or(0, |duration| duration.as_secs());
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        return format!("{hours}:{minutes:02}:{seconds:02}");
    }
    return format!("{minutes}:{seconds:02}");
}

pub struct TileIvars {
    index: Cell<usize>,
    ident: ItemId,
    plate: Retained<CALayer>,
    clip: Retained<CALayer>,
    image: Retained<CALayer>,
    bar: Retained<CALayer>,
    progress: Cell<Option<f64>>,
    ring: Retained<CALayer>,
    highlight: Retained<CALayer>,
    chips: Retained<NSStackView>,
    chip_views: RefCell<Vec<Retained<NSView>>>,
    name: Retained<NSTextField>,
    meta: Retained<NSTextField>,
    accent: RefCell<Retained<NSColor>>,
    chips_key: RefCell<Vec<(&'static str, String, bool)>>,
    selected: Cell<bool>,
    painted: Cell<bool>,
    hovered: Cell<bool>,
    list: Cell<bool>,
    fresh: Cell<bool>,
    stale: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = TileIvars]
    pub struct TileView;

    impl TileView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let index = self.ivars().index.get();
            crate::ui::focus();
            crate::ui::select(Some(index));
            if event.clickCount() >= 2 {
                actions::apply(index);
            }
        }

        #[unsafe(method_id(menuForEvent:))]
        fn menu_for_event(&self, _event: &NSEvent) -> Option<Retained<NSMenu>> {
            let index = self.ivars().index.get();
            crate::ui::select(Some(index));
            menus::context_menu(self.mtm(), index)
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.hover(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.hover(false);
        }
    }
);

impl TileView {
    fn new(mtm: MainThreadMarker, index: usize, library: &Library, scope: Scope, theme: &Theme) -> Retained<Self> {
        let item = &library.items[index];
        let dark = is_dark(&objc2_app_kit::NSApplication::sharedApplication(mtm).effectiveAppearance());

        let plate = CALayer::new();
        plate.setShadowColor(Some(&widgets::cg(mtm, &NSColor::blackColor())));
        plate.setShadowOpacity(if dark { 0.4 } else { 0.14 });
        plate.setShadowRadius(8.0);
        plate.setShadowOffset(CGSize::new(0.0, 3.0));
        plate.setBackgroundColor(Some(&widgets::cg(mtm, &NSColor::windowBackgroundColor())));
        let clip = CALayer::new();
        clip.setMasksToBounds(true);
        clip.setBorderWidth(0.5);
        clip.setBorderColor(Some(&widgets::cg(mtm, &theme.hairline)));
        clip.setBackgroundColor(Some(&widgets::cg(mtm, &NSColor::quaternaryLabelColor())));
        let image = CALayer::new();
        image.setContentsGravity(unsafe { kCAGravityResizeAspectFill });
        if let Some(thumb) = &item.image {
            unsafe { image.setContents(Some(thumb)) };
        }
        clip.addSublayer(&image);
        let bar = CALayer::new();
        bar.setBackgroundColor(Some(&widgets::cg(mtm, &theme.accent)));
        bar.setHidden(true);
        clip.addSublayer(&bar);
        plate.addSublayer(&clip);

        let ring = CALayer::new();
        ring.setBorderColor(Some(&widgets::cg(mtm, &theme.accent)));
        ring.setOpacity(0.0);
        let highlight = CALayer::new();
        highlight.setBackgroundColor(Some(&widgets::cg(mtm, &theme.accent_soft)));
        highlight.setCornerRadius(12.0);
        highlight.setOpacity(0.0);

        let marker_list = markers(library, index, scope);
        let chip_views: Vec<Retained<NSView>> =
            marker_list.iter().map(|marker| widgets::chip(mtm, marker, theme)).collect();
        let chip_refs: Vec<&NSView> = chip_views.iter().map(|view| &**view).collect();
        let chips = widgets::stack(mtm, &chip_refs, false, 5.0);
        chips.setWantsLayer(true);

        let name = widgets::label(mtm, &item.title, 13.0, &Weight::Medium, &NSColor::labelColor());
        let meta = widgets::label(mtm, &meta_line(item), 11.0, &Weight::Regular, &NSColor::tertiaryLabelColor());

        let this = Self::alloc(mtm).set_ivars(TileIvars {
            index: Cell::new(index),
            ident: item.id,
            plate,
            clip,
            image,
            bar,
            progress: Cell::new(item.progress),
            ring,
            highlight,
            chips,
            chip_views: RefCell::new(chip_views),
            name,
            meta,
            accent: RefCell::new(theme.accent.clone()),
            chips_key: RefCell::new(chip_key(&marker_list)),
            selected: Cell::new(false),
            painted: Cell::new(false),
            hovered: Cell::new(false),
            list: Cell::new(false),
            fresh: Cell::new(true),
            stale: Cell::new(false),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setWantsLayer(true);
        let ivars = this.ivars();
        if let Some(layer) = this.layer() {
            layer.addSublayer(&ivars.highlight);
            layer.addSublayer(&ivars.ring);
            layer.addSublayer(&ivars.plate);
        }
        this.addSubview(&ivars.chips);
        this.addSubview(&ivars.name);
        this.addSubview(&ivars.meta);
        let options = NSTrackingAreaOptions::MouseEnteredAndExited
            | NSTrackingAreaOptions::ActiveInKeyWindow
            | NSTrackingAreaOptions::InVisibleRect;
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                CGRect::ZERO,
                options,
                Some(&this),
                None,
            )
        };
        this.addTrackingArea(&area);
        return this;
    }

    fn place(&self, frame: CGRect, list: bool, animate: bool) {
        let ivars = self.ivars();
        ivars.list.set(list);
        let animate = animate && !ivars.fresh.replace(false);
        let width = frame.size.width;
        let chips = ivars.chips.fittingSize();
        let (art, radius) = if list {
            (CGRect::new(CGPoint::new(10.0, 9.0), CGSize::new(ROW_ART, ROW_ART * TILE_ASPECT)), 8.0)
        } else {
            (CGRect::new(CGPoint::ZERO, CGSize::new(width, width * TILE_ASPECT)), RADIUS)
        };
        let ring = CGRect::new(
            CGPoint::new(art.origin.x - RING_GAP, art.origin.y - RING_GAP),
            CGSize::new(art.size.width + RING_GAP * 2.0, art.size.height + RING_GAP * 2.0),
        );
        let border = if list { 0.0 } else { 3.0 };
        let highlight = CGRect::new(CGPoint::ZERO, frame.size);
        let (name, meta, chip_frame) = if list {
            let text = ROW_ART + 24.0;
            let room = (width - text - chips.width - 28.0).max(80.0);
            (
                CGRect::new(CGPoint::new(text, 21.0), CGSize::new(room, 18.0)),
                CGRect::new(CGPoint::new(text, 41.0), CGSize::new(room, 15.0)),
                CGRect::new(CGPoint::new(width - chips.width - 14.0, (ROW_HEIGHT - chips.height) / 2.0), chips),
            )
        } else {
            let top = art.size.height + 9.0;
            (
                CGRect::new(CGPoint::new(4.0, top), CGSize::new(width - 8.0, 18.0)),
                CGRect::new(CGPoint::new(4.0, top + 18.0), CGSize::new(width - 8.0, 15.0)),
                CGRect::new(CGPoint::new(10.0, 10.0), chips),
            )
        };
        let layers = {
            let plate = ivars.plate.clone();
            let clip = ivars.clip.clone();
            let image = ivars.image.clone();
            let ring_layer = ivars.ring.clone();
            let highlight_layer = ivars.highlight.clone();
            move || {
                plate.setFrame(art);
                plate.setCornerRadius(radius);
                clip.setFrame(CGRect::new(CGPoint::ZERO, art.size));
                clip.setCornerRadius(radius);
                put(&image, CGRect::new(CGPoint::ZERO, art.size));
                put(&ring_layer, ring);
                ring_layer.setCornerRadius(radius + RING_GAP);
                ring_layer.setBorderWidth(border);
                highlight_layer.setFrame(highlight);
            }
        };
        if animate {
            let this = self.retain();
            let chips_view = ivars.chips.clone();
            let name_view = ivars.name.clone();
            let meta_view = ivars.meta.clone();
            let group = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
                let context = unsafe { context.as_ref() };
                context.setDuration(MORPH);
                context.setTimingFunction(Some(&ease_in_out()));
                context.setAllowsImplicitAnimation(true);
                this.animator().setFrame(frame);
                chips_view.setFrame(chip_frame);
                name_view.setFrame(name);
                meta_view.setFrame(meta);
                CATransaction::begin();
                CATransaction::setAnimationDuration(MORPH);
                CATransaction::setAnimationTimingFunction(Some(&ease_in_out()));
                layers();
                CATransaction::commit();
            });
            NSAnimationContext::runAnimationGroup(&group);
        } else {
            self.setFrame(frame);
            ivars.chips.setFrame(chip_frame);
            ivars.name.setFrame(name);
            ivars.meta.setFrame(meta);
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            layers();
            CATransaction::commit();
        }
        self.paint_selection();
        self.paint_progress();
    }

    fn paint_progress(&self) {
        let ivars = self.ivars();
        let Some(fraction) = ivars.progress.get() else {
            ivars.bar.setHidden(true);
            return;
        };
        let width = ivars.clip.bounds().size.width * fraction.clamp(0.0, 1.0);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        ivars.bar.setHidden(false);
        ivars.bar.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(width, 3.0)));
        CATransaction::commit();
    }

    fn hover(&self, on: bool) {
        let ivars = self.ivars();
        ivars.hovered.set(on);
        let scale = if on && !ivars.list.get() { 1.03 } else { 1.0 };
        CATransaction::begin();
        CATransaction::setAnimationDuration(0.22);
        CATransaction::setAnimationTimingFunction(Some(&CAMediaTimingFunction::functionWithName(unsafe {
            kCAMediaTimingFunctionEaseOut
        })));
        ivars.image.setTransform(CATransform3D {
            m11: scale,
            m22: scale,
            m33: 1.0,
            m44: 1.0,
            ..unsafe { CATransform3DIdentity }
        });
        ivars.plate.setShadowRadius(if on { 16.0 } else { 8.0 });
        ivars.plate.setShadowOffset(CGSize::new(0.0, if on { 8.0 } else { 3.0 }));
        CATransaction::commit();
        self.paint_selection();
    }

    fn set_selected(&self, selected: bool) {
        self.ivars().selected.set(selected);
        self.paint_selection();
    }

    fn invalidate(&self) {
        self.ivars().stale.set(true);
    }

    fn content(&self, library: &Library, index: usize, scope: Scope, theme: &Theme) {
        let ivars = self.ivars();
        ivars.index.set(index);
        let item = &library.items[index];
        if let Some(thumb) = &item.image {
            unsafe { ivars.image.setContents(Some(thumb)) };
        }
        ivars.name.setStringValue(&widgets::ns(&item.title));
        ivars.meta.setStringValue(&widgets::ns(&meta_line(item)));
        ivars.progress.set(item.progress);
        self.paint_progress();
        let marker_list = markers(library, index, scope);
        let key = chip_key(&marker_list);
        if !ivars.stale.replace(false) && *ivars.chips_key.borrow() == key {
            return;
        }
        *ivars.chips_key.borrow_mut() = key;
        *ivars.accent.borrow_mut() = theme.accent.clone();
        ivars.bar.setBackgroundColor(Some(&widgets::cg(self.mtm(), &theme.accent)));
        ivars.clip.setBorderColor(Some(&widgets::cg(self.mtm(), &theme.hairline)));
        ivars.ring.setBorderColor(Some(&widgets::cg(self.mtm(), &theme.accent)));
        ivars.highlight.setBackgroundColor(Some(&widgets::cg(self.mtm(), &theme.accent_soft)));
        if let Some(layer) = ivars.chips.layer() {
            let fade = CATransition::animation();
            fade.setType(unsafe { kCATransitionFade });
            fade.setDuration(CHIPS);
            fade.setTimingFunction(Some(&ease_in_out()));
            layer.addAnimation_forKey(&fade, Some(&widgets::ns("chips")));
        }
        for view in ivars.chip_views.borrow_mut().drain(..) {
            ivars.chips.removeArrangedSubview(&view);
            view.removeFromSuperview();
        }
        for marker in &marker_list {
            let chip = widgets::chip(self.mtm(), marker, theme);
            ivars.chips.addArrangedSubview(&chip);
            ivars.chip_views.borrow_mut().push(chip);
        }
        self.paint_selection();
    }

    fn fade(&self, entering: bool) {
        let context = if entering { ease_out() } else { ease_in() };
        let duration = if entering { FADE_IN } else { FADE_OUT };
        let target = if entering { 1.0 } else { 0.0 };
        let this = self.retain();
        let next = this.clone();
        let go = RcBlock::new(move |animation: NonNull<NSAnimationContext>| {
            let animation = unsafe { animation.as_ref() };
            animation.setDuration(duration);
            animation.setTimingFunction(Some(&context));
            this.animator().setAlphaValue(target);
        });
        if entering {
            NSAnimationContext::runAnimationGroup(&go);
            return;
        }
        let done = RcBlock::new(move || next.removeFromSuperview());
        NSAnimationContext::runAnimationGroup_completionHandler(&go, Some(&done));
    }

    fn paint_selection(&self) {
        let ivars = self.ivars();
        let selected = ivars.selected.get();
        let list = ivars.list.get();
        let scale = if selected { 1.0 } else { 0.94 };
        CATransaction::begin();
        CATransaction::setAnimationDuration(SELECT);
        CATransaction::setAnimationTimingFunction(Some(&ease_out()));
        ivars.ring.setOpacity(if selected && !list { 1.0 } else { 0.0 });
        ivars.ring.setTransform(CATransform3D {
            m11: scale,
            m22: scale,
            m33: 1.0,
            m44: 1.0,
            ..unsafe { CATransform3DIdentity }
        });
        let row = match (selected, ivars.hovered.get()) {
            (true, _) => 1.0,
            (false, true) => 0.45,
            (false, false) => 0.0,
        };
        ivars.highlight.setOpacity(if list { row } else { 0.0 });
        CATransaction::commit();
        if ivars.painted.replace(selected) != selected
            && let Some(layer) = ivars.name.layer()
        {
            let fade = CATransition::animation();
            fade.setType(unsafe { kCATransitionFade });
            fade.setDuration(SELECT);
            fade.setTimingFunction(Some(&ease_out()));
            layer.addAnimation_forKey(&fade, Some(&widgets::ns("tint")));
        }
        let color = if selected { ivars.accent.borrow().clone() } else { NSColor::labelColor() };
        ivars.name.setTextColor(Some(&color));
    }
}

fn put(layer: &CALayer, frame: CGRect) {
    layer.setBounds(CGRect::new(CGPoint::ZERO, frame.size));
    layer.setPosition(CGPoint::new(frame.origin.x + frame.size.width / 2.0, frame.origin.y + frame.size.height / 2.0));
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

pub fn kind_name(kind: ItemKind) -> &'static str {
    return match kind {
        ItemKind::Video => "Video",
        ItemKind::Gif => "GIF",
        ItemKind::Image => "Image",
    };
}

pub fn meta_line(item: &Item) -> String {
    let mut parts = vec![kind_name(item.kind).to_string()];
    parts.extend(resolution(item));
    if item.kind == ItemKind::Video && item.duration > 0.0 {
        parts.push(duration(item.duration));
    }
    return parts.join(" · ");
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn duration(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    return format!("{}:{:02}", total / 60, total % 60);
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    struct DocumentView;

    impl DocumentView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            crate::ui::focus();
            crate::ui::select(None);
        }
    }
);

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    struct ShadeView;

    impl ShadeView {
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: CGPoint) -> Option<Retained<NSView>> {
            None
        }
    }
);

pub struct WallIvars {
    canvas: Retained<objc2_app_kit::NSBox>,
    shade: Retained<ShadeView>,
    fade: Retained<CAGradientLayer>,
    scroll: Retained<NSScrollView>,
    document: Retained<DocumentView>,
    empty: Retained<NSView>,
    status: Retained<NSView>,
    status_text: Retained<NSTextField>,
    status_fill: Retained<CALayer>,
    pause: Retained<NSView>,
    pause_text: Retained<NSTextField>,
    tiles: RefCell<Vec<Retained<TileView>>>,
    shown: RefCell<(Vec<ItemId>, bool)>,
    unlocked: Cell<bool>,
    list: Cell<bool>,
    columns: Cell<usize>,
    keyboard: Cell<bool>,
    last_width: Cell<CGFloat>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = WallIvars]
    pub struct WallView;

    impl WallView {
        #[unsafe(method(layout))]
        fn layout(&self) {
            let () = unsafe { msg_send![super(self), layout] };
            self.place_shade();
            self.relayout(false, false);
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if !self.key(event) {
                let () = unsafe { msg_send![super(self), keyDown: event] };
            }
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, _event: &NSEvent) {
            if self.ivars().keyboard.replace(false) {
                self.rehover();
            }
        }
    }
);

mod keys {
    pub const LEFT: u16 = 123;
    pub const RIGHT: u16 = 124;
    pub const DOWN: u16 = 125;
    pub const UP: u16 = 126;
    pub const RETURN: u16 = 36;
    pub const ENTER: u16 = 76;
    pub const SPACE: u16 = 49;
    pub const COMMA: u16 = 43;
    pub const PERIOD: u16 = 47;
    pub const GRAVE: u16 = 50;
    pub const DIGITS: [u16; 9] = [18, 19, 20, 21, 23, 22, 26, 28, 25];
}

fn download_pill(mtm: MainThreadMarker, theme: &Theme) -> (Retained<NSView>, Retained<NSTextField>, Retained<CALayer>) {
    let status_icon = widgets::symbol_view(mtm, "arrow.down.circle", 13.0, &NSColor::secondaryLabelColor());
    let status_text = widgets::label(mtm, "", 12.0, &Weight::Medium, &NSColor::labelColor());
    let status_row = widgets::stack(mtm, &[&status_icon, &status_text], false, 6.0);
    let status_track = NSView::new(mtm);
    status_track.setWantsLayer(true);
    status_track.widthAnchor().constraintEqualToConstant(STATUS_WIDTH).setActive(true);
    status_track.heightAnchor().constraintEqualToConstant(STATUS_BAR).setActive(true);
    let status_fill = CALayer::new();
    status_fill.setBackgroundColor(Some(&widgets::cg(mtm, &theme.accent)));
    status_fill.setCornerRadius(STATUS_BAR / 2.0);
    if let Some(layer) = status_track.layer() {
        layer.setBackgroundColor(Some(&widgets::cg(mtm, &NSColor::quaternaryLabelColor())));
        layer.setCornerRadius(STATUS_BAR / 2.0);
        layer.setMasksToBounds(true);
        layer.addSublayer(&status_fill);
    }
    let status_body = widgets::stack(mtm, &[&status_row, &status_track], true, 7.0);
    status_body.setAlignment(objc2_app_kit::NSLayoutAttribute::Leading);
    status_body.setEdgeInsets(widgets::insets(8.0, 14.0, 10.0, 14.0));
    let status = widgets::glass(mtm, &status_body, 16.0, None);
    status.setHidden(true);
    return (status, status_text, status_fill);
}

fn pause_pill(mtm: MainThreadMarker) -> (Retained<NSView>, Retained<NSTextField>) {
    let pause_icon = widgets::symbol_view(mtm, "pause.circle", 13.0, &NSColor::secondaryLabelColor());
    let pause_text = widgets::label(mtm, "", 12.0, &Weight::Medium, &NSColor::labelColor());
    let pause_row = widgets::stack(mtm, &[&pause_icon, &pause_text], false, 6.0);
    pause_row.setEdgeInsets(widgets::insets(6.0, 12.0, 6.0, 12.0));
    let pause = widgets::glass(mtm, &pause_row, 16.0, None);
    pause.setHidden(true);
    return (pause, pause_text);
}

impl WallView {
    pub fn new(mtm: MainThreadMarker, theme: &Theme) -> Retained<Self> {
        let canvas = widgets::card(mtm, &theme.canvas, 0.0);
        let scroll = NSScrollView::new(mtm);
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setAutomaticallyAdjustsContentInsets(true);
        let document: Retained<DocumentView> = unsafe { msg_send![DocumentView::alloc(mtm), init] };
        document.setWantsLayer(true);
        scroll.setDocumentView(Some(&document));
        let icon = widgets::symbol_view(mtm, "photo.on.rectangle.angled", 34.0, &NSColor::tertiaryLabelColor());
        let title = widgets::label(mtm, "No Wallpapers", 17.0, &Weight::Semibold, &NSColor::secondaryLabelColor());
        let hint = widgets::label(
            mtm,
            "Add wallpapers, or try a different search, filter or folder.",
            12.0,
            &Weight::Regular,
            &NSColor::tertiaryLabelColor(),
        );
        hint.setAlignment(NSTextAlignment::Center);
        let empty = widgets::stack(mtm, &[&icon, &title, &hint], true, 8.0);
        empty.setHidden(true);
        empty.setAlphaValue(0.0);
        let (status, status_text, status_fill) = download_pill(mtm, theme);
        let (pause, pause_text) = pause_pill(mtm);
        let pills = widgets::stack(mtm, &[&pause, &status], true, 8.0);
        let shade: Retained<ShadeView> = unsafe { msg_send![ShadeView::alloc(mtm), init] };
        let fade = CAGradientLayer::new();
        fade.setStartPoint(CGPoint::new(0.5, 1.0));
        fade.setEndPoint(CGPoint::new(0.5, 0.0));
        shade.setLayer(Some(&fade));
        shade.setWantsLayer(true);
        let this = Self::alloc(mtm).set_ivars(WallIvars {
            canvas: canvas.clone(),
            shade,
            fade,
            scroll,
            document,
            empty: widgets::view(&empty),
            status,
            status_text,
            status_fill,
            pause,
            pause_text,
            tiles: RefCell::new(Vec::new()),
            shown: RefCell::new((Vec::new(), false)),
            unlocked: Cell::new(false),
            list: Cell::new(false),
            columns: Cell::new(1),
            keyboard: Cell::new(false),
            last_width: Cell::new(0.0),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        let ivars = this.ivars();
        this.addSubview(&canvas);
        this.addSubview(&ivars.scroll);
        this.addSubview(&ivars.empty);
        this.addSubview(&ivars.shade);
        this.addSubview(&pills);
        this.paint(theme);
        pills.setTranslatesAutoresizingMaskIntoConstraints(false);
        pills.centerXAnchor().constraintEqualToAnchor(&this.centerXAnchor()).setActive(true);
        pills.bottomAnchor().constraintEqualToAnchor_constant(&this.bottomAnchor(), -18.0).setActive(true);
        widgets::pin(&canvas, &this, 0.0);
        widgets::pin(&ivars.scroll, &this, 0.0);
        ivars.empty.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.empty.centerXAnchor().constraintEqualToAnchor(&this.centerXAnchor()).setActive(true);
        ivars.empty.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()).setActive(true);
        let options = NSTrackingAreaOptions::MouseMoved
            | NSTrackingAreaOptions::ActiveInKeyWindow
            | NSTrackingAreaOptions::InVisibleRect;
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                CGRect::ZERO,
                options,
                Some(&this),
                None,
            )
        };
        this.addTrackingArea(&area);
        return this;
    }

    fn key(&self, event: &NSEvent) -> bool {
        let flags = event.modifierFlags();
        if flags
            .intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option)
        {
            return false;
        }
        let columns = isize::try_from(self.ivars().columns.get()).unwrap_or(1);
        let code = event.keyCode();
        match code {
            keys::LEFT => self.step(-1),
            keys::RIGHT => self.step(1),
            keys::DOWN => self.step(columns),
            keys::UP => self.step(-columns),
            keys::RETURN | keys::ENTER => actions::apply_selection(),
            keys::SPACE => crate::ui::toggle_play(),
            keys::COMMA => crate::ui::frame(-1),
            keys::PERIOD => crate::ui::frame(1),
            keys::GRAVE => actions::place(self.target(), Scope::All),
            _ => match keys::DIGITS.iter().position(|digit| *digit == code) {
                Some(space) => actions::place(self.target(), Scope::Space(space)),
                None => return false,
            },
        }
        return true;
    }

    fn target(&self) -> Option<usize> {
        let ivars = self.ivars();
        let tiles = ivars.tiles.borrow();
        let hovered = tiles.iter().find(|tile| !ivars.keyboard.get() && tile.ivars().hovered.get());
        return hovered
            .or_else(|| tiles.iter().find(|tile| tile.ivars().selected.get()))
            .map(|tile| tile.ivars().index.get());
    }

    fn step(&self, delta: isize) {
        let ivars = self.ivars();
        ivars.keyboard.set(true);
        let index = {
            let tiles = ivars.tiles.borrow();
            for tile in tiles.iter().filter(|tile| tile.ivars().hovered.get()) {
                tile.hover(false);
            }
            let next = match tiles.iter().position(|tile| tile.ivars().selected.get()) {
                Some(position) => {
                    position.checked_add_signed(delta).filter(|next| *next < tiles.len()).unwrap_or(position)
                }
                None => 0,
            };
            tiles.get(next).map(|tile| tile.ivars().index.get())
        };
        if let Some(index) = index {
            crate::ui::reveal(index);
        }
    }

    pub fn paint(&self, theme: &Theme) {
        let ivars = self.ivars();
        ivars.canvas.setFillColor(&theme.canvas);
        ivars.status_fill.setBackgroundColor(Some(&widgets::cg(self.mtm(), &theme.accent)));
        let dark = is_dark(&objc2_app_kit::NSApplication::sharedApplication(self.mtm()).effectiveAppearance());
        let (base, alpha) = if dark { (NSColor::blackColor(), 0.4) } else { (theme.canvas.clone(), 0.4) };
        let colors: Vec<Retained<CGColor>> = [alpha, alpha * 0.85, 0.0]
            .iter()
            .map(|alpha| widgets::cg(self.mtm(), &base.colorWithAlphaComponent(*alpha)))
            .collect();
        let locations: Vec<Retained<NSNumber>> =
            [0.0, 0.7, 1.0].iter().map(|location| NSNumber::new_f64(*location)).collect();
        let objects: Vec<&AnyObject> =
            colors.iter().map(|color| unsafe { &*std::ptr::from_ref::<CGColor>(color).cast::<AnyObject>() }).collect();
        unsafe {
            ivars.fade.setColors(Some(&NSArray::from_slice(&objects)));
            ivars.fade.setLocations(Some(&NSArray::from_retained_slice(&locations)));
        }
        for tile in ivars.tiles.borrow().iter() {
            tile.invalidate();
        }
    }

    fn place_shade(&self) {
        let ivars = self.ivars();
        let bounds = self.bounds();
        let height = self.safeAreaInsets().top + 22.0;
        ivars.shade.setFrame(CGRect::new(
            CGPoint::new(0.0, bounds.size.height - height),
            CGSize::new(bounds.size.width, height),
        ));
    }

    pub fn reload(
        &self,
        library: &Library,
        visible: &[usize],
        selected: Option<usize>,
        scope: Scope,
        list: bool,
        theme: &Theme,
    ) {
        let mtm = self.mtm();
        let ivars = self.ivars();
        let key = (visible.iter().map(|index| library.items[*index].id).collect::<Vec<_>>(), list);
        let morph = *ivars.shown.borrow() != key;
        let sealing = ivars.unlocked.replace(library.unlocked) && !library.unlocked;
        *ivars.shown.borrow_mut() = key;
        self.show_downloads(&library.downloads);
        self.show_pause(library.pause.as_deref());
        let mut previous: HashMap<ItemId, Retained<TileView>> = ivars
            .tiles
            .borrow_mut()
            .drain(..)
            .map(|tile| {
                let ident = tile.ivars().ident;
                return (ident, tile);
            })
            .collect();
        let mut tiles: Vec<Retained<TileView>> = Vec::with_capacity(visible.len());
        let mut entering: Vec<Retained<TileView>> = Vec::new();
        for index in visible {
            if let Some(tile) = previous.remove(&library.items[*index].id) {
                tile.content(library, *index, scope, theme);
                tiles.push(tile);
                continue;
            }
            let tile = TileView::new(mtm, *index, library, scope, theme);
            tile.setAlphaValue(0.0);
            ivars.document.addSubview(&tile);
            entering.push(tile.clone());
            tiles.push(tile);
        }
        for tile in previous.into_values() {
            if sealing {
                tile.removeFromSuperview();
            } else {
                tile.fade(false);
            }
        }
        *ivars.tiles.borrow_mut() = tiles;
        ivars.list.set(list);
        self.show_empty(visible.is_empty());
        self.set_selected(selected);
        self.relayout(morph, morph);
        for tile in entering {
            tile.fade(true);
        }
        self.rehover();
    }

    fn show_downloads(&self, downloads: &[Download]) {
        let ivars = self.ivars();
        let text = match downloads {
            [] => None,
            [only] => Some(download_line(only)),
            many => Some(format!("Downloading {} videos · {:.0}%", many.len(), overall(many) * 100.0)),
        };
        if let Some(text) = &text {
            ivars.status_text.setStringValue(&widgets::ns(text));
            let width = STATUS_WIDTH * overall(downloads).clamp(0.0, 1.0);
            ivars.status_fill.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(width, STATUS_BAR)));
        }
        let hidden = text.is_none();
        if ivars.status.isHidden() != hidden {
            widgets::reveal(&ivars.status, !hidden);
        }
    }

    fn show_pause(&self, reason: Option<&str>) {
        let ivars = self.ivars();
        if let Some(reason) = reason {
            ivars.pause_text.setStringValue(&widgets::ns(&format!("Paused: {reason}")));
        }
        if ivars.pause.isHidden() != reason.is_none() {
            widgets::reveal(&ivars.pause, reason.is_some());
        }
    }

    fn show_empty(&self, show: bool) {
        let empty = self.ivars().empty.clone();
        if show {
            if !empty.isHidden() && empty.alphaValue() >= 1.0 {
                return;
            }
            empty.setHidden(false);
            let fade = RcBlock::new(move |animation: NonNull<NSAnimationContext>| {
                let animation = unsafe { animation.as_ref() };
                animation.setDuration(FADE_IN);
                animation.setTimingFunction(Some(&ease_out()));
                empty.animator().setAlphaValue(1.0);
            });
            NSAnimationContext::runAnimationGroup(&fade);
            return;
        }
        if empty.isHidden() {
            return;
        }
        let next = empty.clone();
        let done = RcBlock::new(move || {
            if next.alphaValue() == 0.0 {
                next.setHidden(true);
            }
        });
        let fade = RcBlock::new(move |animation: NonNull<NSAnimationContext>| {
            let animation = unsafe { animation.as_ref() };
            animation.setDuration(FADE_OUT);
            animation.setTimingFunction(Some(&ease_in()));
            empty.animator().setAlphaValue(0.0);
        });
        NSAnimationContext::runAnimationGroup_completionHandler(&fade, Some(&done));
    }

    fn rehover(&self) {
        if self.ivars().keyboard.get() {
            return;
        }
        let Some(window) = self.window() else {
            return;
        };
        let point = window.mouseLocationOutsideOfEventStream();
        for tile in self.ivars().tiles.borrow().iter() {
            let local = tile.convertPoint_fromView(point, None);
            let under = tile.visibleRect().size.width > 0.0 && objc2_foundation::NSPointInRect(local, tile.bounds());
            if under != tile.ivars().hovered.get() {
                tile.hover(under);
            }
        }
    }

    pub fn set_selected(&self, selected: Option<usize>) {
        for tile in self.ivars().tiles.borrow().iter() {
            tile.set_selected(Some(tile.ivars().index.get()) == selected);
        }
    }

    pub fn reveal(&self, index: usize) {
        let ivars = self.ivars();
        let frame = ivars.tiles.borrow().iter().find(|tile| tile.ivars().index.get() == index).map(|tile| tile.frame());
        if let Some(frame) = frame {
            let clip = ivars.scroll.contentView();
            let visible = clip.bounds();
            let rect = CGRect::new(
                CGPoint::new(frame.origin.x, frame.origin.y - PADDING),
                CGSize::new(frame.size.width, frame.size.height + PADDING * 2.0),
            );
            if rect.origin.y < visible.origin.y
                || rect.origin.y + rect.size.height > visible.origin.y + visible.size.height
            {
                let limit = (ivars.document.frame().size.height - visible.size.height).max(0.0);
                let origin = if rect.origin.y < visible.origin.y {
                    rect.origin.y
                } else {
                    rect.origin.y + rect.size.height - visible.size.height
                }
                .clamp(0.0, limit);
                let span = visible.size.height.max(1.0);
                let duration =
                    SCROLL_MIN + ((origin - visible.origin.y).abs() / span).min(1.0) * (SCROLL_MAX - SCROLL_MIN);
                let scroll = ivars.scroll.clone();
                let target = CGPoint::new(visible.origin.x, origin);
                let wall = self.retain();
                let group = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
                    let context = unsafe { context.as_ref() };
                    context.setDuration(duration);
                    context.setTimingFunction(Some(&ease_in_out()));
                    clip.animator().setBoundsOrigin(target);
                    scroll.reflectScrolledClipView(&clip);
                });
                let done = RcBlock::new(move || wall.rehover());
                NSAnimationContext::runAnimationGroup_completionHandler(&group, Some(&done));
            }
        }
        self.rehover();
    }

    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn relayout(&self, force: bool, animate: bool) {
        let ivars = self.ivars();
        let width = ivars.scroll.contentSize().width;
        if !force && (width - ivars.last_width.get()).abs() < 0.5 {
            return;
        }
        ivars.last_width.set(width);
        let tiles = ivars.tiles.borrow();
        let usable = (width - PADDING * 2.0).max(MIN_TILE);
        let list = ivars.list.get();
        let (columns, tile, height, gap) = if list {
            (1, usable, ROW_HEIGHT, 2.0)
        } else {
            let columns = ((usable + COLUMN_GAP) / (MIN_TILE + COLUMN_GAP)).floor().max(1.0);
            let tile = (usable - COLUMN_GAP * (columns - 1.0)) / columns;
            (columns as usize, tile, tile * TILE_ASPECT + LABEL_HEIGHT, ROW_GAP)
        };
        ivars.columns.set(columns);
        let top = 14.0;
        for (position, view) in tiles.iter().enumerate() {
            let column = (position % columns) as CGFloat;
            let row = (position / columns) as CGFloat;
            view.place(
                CGRect::new(
                    CGPoint::new(PADDING + column * (tile + COLUMN_GAP), top + row * (height + gap)),
                    CGSize::new(tile, if list { ROW_HEIGHT } else { height }),
                ),
                list,
                animate,
            );
        }
        let rows = tiles.len().div_ceil(columns) as CGFloat;
        let total = top + rows * (height + gap) + PADDING;
        let visible = ivars.scroll.contentSize().height;
        ivars.document.setFrameSize(CGSize::new(width, total.max(visible)));
    }
}
