use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityButtonRole, NSAnimatablePropertyContainer, NSAnimationContext, NSColor,
    NSDragOperation, NSDraggingContext, NSDraggingDestination, NSDraggingInfo, NSDraggingItem, NSDraggingSession,
    NSDraggingSource, NSEvent, NSEventModifierFlags, NSMenu, NSPasteboardItem, NSPasteboardTypeFileURL, NSScrollView,
    NSStackView, NSTextAlignment, NSTextField, NSTrackingArea, NSTrackingAreaOptions, NSView,
    NSViewLayerContentsRedrawPolicy,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGColor;
use objc2_foundation::{NSArray, NSCopying, NSNumber, NSObjectProtocol, NSURL};
use objc2_quartz_core::{
    CAGradientLayer, CALayer, CATransaction, CATransform3D, CATransform3DIdentity, kCAGravityResizeAspectFill,
};

use crate::i18n::{Arg, tr, trf};
use crate::model::{ItemId, ItemKind};
use crate::style::Key;
use crate::ui::controls;
use crate::ui::metrics::{self, Surface, Text, ease_in_out, ease_out};
use crate::ui::state::{Marker, Scope, markers};
use crate::ui::theme::Theme;
use crate::ui::view::{Download, Item, Library, Stage};
use crate::ui::widgets::{self, IconRole, Paint, Slot};
use crate::ui::{actions, menus, sidebar};

pub const TILE_ASPECT: CGFloat = 10.0 / 16.0;
const MIN_TILE: CGFloat = 240.0;
const COLUMN_GAP: CGFloat = metrics::SPACE_16;
const ROW_GAP: CGFloat = metrics::SPACE_24;
const NAME_HEIGHT: CGFloat = 18.0;
const META_HEIGHT: CGFloat = 15.0;
const LABEL_HEIGHT: CGFloat = metrics::SPACE_8 + NAME_HEIGHT + META_HEIGHT + 1.0;
const ROW_HEIGHT: CGFloat = 78.0;
const ROW_ART: CGFloat = 96.0;
const STATUS_WIDTH: CGFloat = 280.0;
const STATUS_BAR: CGFloat = metrics::SPACE_4;
const DRAG_WIDTH: CGFloat = 160.0;

thread_local! {
    static DRAGGED: RefCell<Option<Retained<TileView>>> = const { RefCell::new(None) };
}

pub fn chip_key(markers: &[Marker]) -> Vec<(&'static str, String, bool)> {
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
        Stage::Preparing => tr("Getting video info…"),
        Stage::Merging => tr("Merging video and audio…"),
        Stage::Importing => tr("Adding to library…"),
        Stage::Downloading => {
            let mut parts = vec![trf("Downloading %@", &[Arg::Text(crate::i18n::percent(download.percent))])];
            if let Some(speed) = download.speed.filter(|speed| *speed > 0.0) {
                #[allow(clippy::cast_possible_truncation)]
                let rate = crate::i18n::bytes(speed as i64);
                parts.push(trf("%@/s", &[Arg::Text(rate)]));
            }
            if let Some(eta) = download.eta.filter(|eta| eta.is_finite() && *eta >= 0.0) {
                parts.push(trf("%@ left", &[Arg::Text(clock(eta))]));
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
    path: String,
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
    titles: (Retained<NSColor>, Retained<NSColor>),
    theme: Theme,
    chips_key: RefCell<Vec<(&'static str, String, bool)>>,
    selected: Cell<bool>,
    look: Cell<Option<(bool, bool, bool)>>,
    chips_size: Cell<Option<CGSize>>,
    placed: Cell<Option<(CGRect, bool)>>,
    facts: RefCell<(String, String)>,
    art: Cell<usize>,
    hovered: Cell<bool>,
    list: Cell<bool>,
    fresh: Cell<bool>,
    dragging: Cell<bool>,
}

fn art_stamp(item: &Item) -> usize {
    return item.image.as_deref().map_or(0, |image| std::ptr::from_ref(image) as usize);
}

fn layered(body: impl FnOnce()) {
    let context = NSAnimationContext::currentContext();
    let duration = context.duration();
    CATransaction::begin();
    if context.allowsImplicitAnimation() && duration > 0.0 {
        CATransaction::setAnimationDuration(duration);
        CATransaction::setAnimationTimingFunction(context.timingFunction().as_deref());
    } else {
        CATransaction::setDisableActions(true);
    }
    body();
    CATransaction::commit();
}

fn still(body: impl FnOnce()) {
    NSAnimationContext::beginGrouping();
    let context = NSAnimationContext::currentContext();
    context.setAllowsImplicitAnimation(false);
    context.setDuration(0.0);
    body();
    NSAnimationContext::endGrouping();
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

        #[unsafe(method(wantsUpdateLayer))]
        fn wants_update_layer(&self) -> bool {
            true
        }

        #[unsafe(method(updateLayer))]
        fn update_layer(&self) {
            self.colorize();
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let index = self.ivars().index.get();
            self.ivars().dragging.set(false);
            crate::ui::focus();
            crate::ui::select(Some(index));
            if event.clickCount() >= 2 {
                actions::apply(index);
            }
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn accessibility_perform_press(&self) -> bool {
            crate::ui::select(Some(self.ivars().index.get()));
            true
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

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            self.drag(event);
        }
    }

    unsafe impl NSObjectProtocol for TileView {}

    unsafe impl NSDraggingSource for TileView {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn drag_mask(&self, _session: &NSDraggingSession, context: NSDraggingContext) -> NSDragOperation {
            return if context == NSDraggingContext::WithinApplication {
                NSDragOperation::Move
            } else {
                NSDragOperation::Copy
            };
        }

        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn drag_ended(&self, _session: &NSDraggingSession, _point: CGPoint, _operation: NSDragOperation) {
            DRAGGED.with_borrow_mut(Option::take);
        }
    }
);

impl TileView {
    fn drag(&self, event: &NSEvent) {
        let ivars = self.ivars();
        if ivars.dragging.replace(true) {
            return;
        }
        let index = ivars.index.get();
        let image = crate::ui::item_image(index);
        let item = NSPasteboardItem::new();
        item.setString_forType(&widgets::ns(&ivars.ident.to_string()), &widgets::ns(sidebar::ITEM_TYPE));
        if let Some(url) = NSURL::fileURLWithPath(&widgets::ns(&ivars.path)).absoluteString() {
            item.setString_forType(&url, unsafe { NSPasteboardTypeFileURL });
        }
        let dragged =
            NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), ProtocolObject::from_ref(&*item));
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let size = CGSize::new(DRAG_WIDTH, DRAG_WIDTH * TILE_ASPECT);
        let frame = CGRect::new(CGPoint::new(point.x - size.width / 2.0, point.y - size.height / 2.0), size);
        unsafe { dragged.setDraggingFrame_contents(frame, image.as_deref().map(|image| image as &AnyObject)) };
        let items = NSArray::from_retained_slice(&[dragged]);
        DRAGGED.with_borrow_mut(|slot| *slot = Some(self.retain()));
        self.beginDraggingSessionWithItems_event_source(&items, event, ProtocolObject::from_ref(self));
    }

    fn new(mtm: MainThreadMarker, index: usize, library: &Library, scope: Scope, theme: &Theme) -> Retained<Self> {
        let item = &library.items[index];

        let plate = CALayer::new();
        plate.setShadowOpacity(1.0);
        plate.setShadowRadius(metrics::SPACE_8);
        plate.setShadowOffset(CGSize::new(0.0, metrics::SPACE_4));
        let clip = CALayer::new();
        clip.setMasksToBounds(true);
        clip.setBorderWidth(metrics::BORDER_HAIRLINE);
        let image = CALayer::new();
        image.setContentsGravity(unsafe { kCAGravityResizeAspectFill });
        if let Some(thumb) = &item.image {
            unsafe { image.setContents(Some(thumb)) };
        }
        clip.addSublayer(&image);
        let bar = CALayer::new();
        bar.setHidden(true);
        clip.addSublayer(&bar);
        plate.addSublayer(&clip);

        let ring = CALayer::new();
        ring.setOpacity(0.0);
        let highlight = CALayer::new();
        highlight.setCornerRadius(metrics::RADIUS_TILE);
        highlight.setOpacity(0.0);

        let marker_list = markers(library, index, scope);
        let chip_views: Vec<Retained<NSView>> =
            marker_list.iter().map(|marker| widgets::chip(mtm, marker, theme)).collect();
        let chip_refs: Vec<&NSView> = chip_views.iter().map(|view| &**view).collect();
        let chips = widgets::stack(mtm, &chip_refs, false, metrics::SPACE_4);
        chips.setWantsLayer(true);

        let name = widgets::label(mtm, &item.title, Text::BodyMedium, &theme.color(Key::GridTitle));
        let meta = widgets::label(mtm, &meta_line(item), Text::Subheadline, &theme.color(Key::GridMeta));

        let this = Self::alloc(mtm).set_ivars(TileIvars {
            index: Cell::new(index),
            ident: item.id,
            path: item.path.to_string_lossy().into_owned(),
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
            titles: (theme.color(Key::GridTitle), theme.color(Key::GridTitleSelected)),
            theme: theme.clone(),
            chips_key: RefCell::new(chip_key(&marker_list)),
            selected: Cell::new(false),
            look: Cell::new(None),
            chips_size: Cell::new(None),
            placed: Cell::new(None),
            facts: RefCell::new((item.title.clone(), meta_line(item))),
            art: Cell::new(art_stamp(item)),
            hovered: Cell::new(false),
            list: Cell::new(false),
            fresh: Cell::new(true),
            dragging: Cell::new(false),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setWantsLayer(true);
        this.setLayerContentsRedrawPolicy(NSViewLayerContentsRedrawPolicy::OnSetNeedsDisplay);
        let ivars = this.ivars();
        if let Some(layer) = this.layer() {
            layer.addSublayer(&ivars.highlight);
            layer.addSublayer(&ivars.ring);
            layer.addSublayer(&ivars.plate);
        }
        this.addSubview(&ivars.chips);
        this.addSubview(&ivars.name);
        this.addSubview(&ivars.meta);
        controls::track(&this, &this);
        this.setAccessibilityElement(true);
        this.setAccessibilityRole(Some(unsafe { NSAccessibilityButtonRole }));
        widgets::describe(&this, &item.title, &item.title);
        this.paint_selection();
        this.setNeedsDisplay(true);
        return this;
    }

    fn colorize(&self) {
        let ivars = self.ivars();
        let theme = &ivars.theme;
        widgets::resolving(self, || {
            let color = |key: Key| theme.color(key).CGColor();
            ivars.plate.setShadowColor(Some(&color(Key::GridShadow)));
            ivars.plate.setBackgroundColor(Some(&color(Key::GridTile)));
            ivars.clip.setBorderColor(Some(&color(Key::GridTileBorder)));
            ivars.clip.setBackgroundColor(Some(&color(Key::GridTile)));
            ivars.bar.setBackgroundColor(Some(&color(Key::GridProgress)));
            ivars.ring.setBorderColor(Some(&color(Key::GridRing)));
            ivars.highlight.setBackgroundColor(Some(&color(Key::GridHighlight)));
        });
    }

    fn place(&self, frame: CGRect, list: bool) {
        if self.ivars().fresh.replace(false) {
            still(|| self.settle(frame, list));
            return;
        }
        self.settle(frame, list);
    }

    fn settle(&self, frame: CGRect, list: bool) {
        let ivars = self.ivars();
        ivars.list.set(list);
        ivars.placed.set(Some((frame, list)));
        let width = frame.size.width;
        let chips = ivars.chips_size.get().unwrap_or_else(|| {
            let size = ivars.chips.fittingSize();
            ivars.chips_size.set(Some(size));
            return size;
        });
        let art_height = ROW_ART * TILE_ASPECT;
        let (art, radius) = if list {
            let origin = CGPoint::new(metrics::SPACE_12, (ROW_HEIGHT - art_height) / 2.0);
            (CGRect::new(origin, CGSize::new(ROW_ART, art_height)), metrics::RADIUS_SMALL)
        } else {
            (CGRect::new(CGPoint::ZERO, CGSize::new(width, width * TILE_ASPECT)), metrics::RADIUS_TILE)
        };
        let gap = metrics::RING_GAP;
        let ring = CGRect::new(
            CGPoint::new(art.origin.x - gap, art.origin.y - gap),
            CGSize::new(art.size.width + gap * 2.0, art.size.height + gap * 2.0),
        );
        let border = if list { 0.0 } else { metrics::BORDER_RING };
        let highlight = CGRect::new(CGPoint::ZERO, frame.size);
        let (name, meta, chip_frame) = if list {
            let text = art.origin.x + ROW_ART + metrics::SPACE_12;
            let room = (width - text - chips.width - metrics::SPACE_12 * 2.0).max(80.0);
            let top = ((ROW_HEIGHT - NAME_HEIGHT - META_HEIGHT - metrics::SPACE_2) / 2.0).floor();
            (
                CGRect::new(CGPoint::new(text, top), CGSize::new(room, NAME_HEIGHT)),
                CGRect::new(CGPoint::new(text, top + NAME_HEIGHT + metrics::SPACE_2), CGSize::new(room, META_HEIGHT)),
                CGRect::new(
                    CGPoint::new(width - chips.width - metrics::SPACE_12, (ROW_HEIGHT - chips.height) / 2.0),
                    chips,
                ),
            )
        } else {
            let top = art.size.height + metrics::SPACE_8;
            let inset = metrics::SPACE_4;
            (
                CGRect::new(CGPoint::new(inset, top), CGSize::new(width - inset * 2.0, NAME_HEIGHT)),
                CGRect::new(CGPoint::new(inset, top + NAME_HEIGHT), CGSize::new(width - inset * 2.0, META_HEIGHT)),
                CGRect::new(CGPoint::new(metrics::SPACE_8, metrics::SPACE_8), chips),
            )
        };
        self.setFrame(frame);
        ivars.chips.setFrame(chip_frame);
        ivars.name.setFrame(name);
        ivars.meta.setFrame(meta);
        layered(|| {
            ivars.plate.setFrame(art);
            ivars.plate.setCornerRadius(radius);
            ivars.clip.setFrame(CGRect::new(CGPoint::ZERO, art.size));
            ivars.clip.setCornerRadius(radius);
            put(&ivars.image, CGRect::new(CGPoint::ZERO, art.size));
            put(&ivars.ring, ring);
            ivars.ring.setCornerRadius(metrics::ring_radius(radius));
            ivars.ring.setBorderWidth(border);
            ivars.highlight.setFrame(highlight);
        });
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
        ivars.bar.setHidden(false);
        layered(|| ivars.bar.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(width, STATUS_BAR))));
    }

    fn hover(&self, on: bool) {
        let ivars = self.ivars();
        ivars.hovered.set(on);
        let scale = if on && !ivars.list.get() { metrics::scale(metrics::HOVER_SCALE) } else { 1.0 };
        CATransaction::begin();
        CATransaction::setAnimationDuration(metrics::transform(metrics::MOTION_QUICK));
        CATransaction::setAnimationTimingFunction(Some(&ease_out()));
        ivars.image.setTransform(CATransform3D {
            m11: scale,
            m22: scale,
            m33: 1.0,
            m44: 1.0,
            ..unsafe { CATransform3DIdentity }
        });
        ivars.plate.setShadowRadius(if on { metrics::SPACE_16 } else { metrics::SPACE_8 });
        ivars.plate.setShadowOffset(CGSize::new(0.0, if on { metrics::SPACE_8 } else { metrics::SPACE_4 }));
        CATransaction::commit();
        self.paint_selection();
    }

    fn set_selected(&self, selected: bool) {
        self.ivars().selected.set(selected);
        self.setAccessibilitySelected(selected);
        self.paint_selection();
    }

    fn content(&self, library: &Library, index: usize, scope: Scope, theme: &Theme) {
        let ivars = self.ivars();
        ivars.index.set(index);
        let item = &library.items[index];
        if let Some(thumb) = &item.image
            && ivars.art.replace(art_stamp(item)) != art_stamp(item)
        {
            unsafe { ivars.image.setContents(Some(thumb)) };
        }
        let facts = (item.title.clone(), meta_line(item));
        if *ivars.facts.borrow() != facts {
            ivars.name.setStringValue(&widgets::ns(&facts.0));
            ivars.meta.setStringValue(&widgets::ns(&facts.1));
            widgets::describe(self, &facts.0, &facts.0);
            *ivars.facts.borrow_mut() = facts;
        }
        ivars.progress.set(item.progress);
        self.paint_progress();
        let marker_list = markers(library, index, scope);
        let key = chip_key(&marker_list);
        if *ivars.chips_key.borrow() == key {
            return;
        }
        *ivars.chips_key.borrow_mut() = key;
        for view in ivars.chip_views.borrow_mut().drain(..) {
            ivars.chips.removeArrangedSubview(&view);
            view.removeFromSuperview();
        }
        for marker in &marker_list {
            let chip = widgets::chip(self.mtm(), marker, theme);
            ivars.chips.addArrangedSubview(&chip);
            ivars.chip_views.borrow_mut().push(chip);
        }
        ivars.chips_size.set(None);
        if let Some((frame, list)) = ivars.placed.get() {
            self.settle(frame, list);
        }
        self.paint_selection();
    }

    fn paint_selection(&self) {
        let ivars = self.ivars();
        let selected = ivars.selected.get();
        let list = ivars.list.get();
        let look = Some((selected, list, list && ivars.hovered.get()));
        if ivars.look.replace(look) == look {
            return;
        }
        let scale = if selected { 1.0 } else { metrics::scale(metrics::SELECT_SCALE) };
        CATransaction::begin();
        CATransaction::setAnimationDuration(metrics::MOTION_STATE);
        CATransaction::setAnimationTimingFunction(Some(&ease_in_out()));
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
        let titles = &ivars.titles;
        ivars.name.setTextColor(Some(if selected { &titles.1 } else { &titles.0 }));
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
        ItemKind::Video => crate::i18n::key("Video"),
        ItemKind::Gif => crate::i18n::key("GIF"),
        ItemKind::Image => crate::i18n::key("Image"),
    };
}

pub fn meta_line(item: &Item) -> String {
    let mut parts = vec![tr(kind_name(item.kind))];
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

struct ShadeIvars {
    fade: Retained<CAGradientLayer>,
    theme: Theme,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ShadeIvars]
    struct ShadeView;

    impl ShadeView {
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: CGPoint) -> Option<Retained<NSView>> {
            None
        }

        #[unsafe(method_id(makeBackingLayer))]
        fn make_backing_layer(&self) -> Retained<CALayer> {
            Retained::into_super(self.ivars().fade.clone())
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

impl ShadeView {
    fn new(mtm: MainThreadMarker, theme: &Theme) -> Retained<Self> {
        let fade = CAGradientLayer::new();
        fade.setStartPoint(CGPoint::new(0.5, 1.0));
        fade.setEndPoint(CGPoint::new(0.5, 0.0));
        let locations: Vec<Retained<NSNumber>> =
            [0.0, 0.7, 1.0].iter().map(|location| NSNumber::new_f64(*location)).collect();
        fade.setLocations(Some(&NSArray::from_retained_slice(&locations)));
        let this = Self::alloc(mtm).set_ivars(ShadeIvars { fade, theme: theme.clone() });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setWantsLayer(true);
        this.setLayerContentsRedrawPolicy(NSViewLayerContentsRedrawPolicy::OnSetNeedsDisplay);
        this.setNeedsDisplay(true);
        return this;
    }

    fn colorize(&self) {
        let ivars = self.ivars();
        widgets::resolving(self, || {
            let shade = ivars.theme.color(Key::GridShade);
            let colors: Vec<Retained<CGColor>> =
                [0.4, 0.34, 0.0].iter().map(|alpha| shade.colorWithAlphaComponent(*alpha).CGColor()).collect();
            let objects: Vec<&AnyObject> = colors
                .iter()
                .map(|color| unsafe { &*std::ptr::from_ref::<CGColor>(color).cast::<AnyObject>() })
                .collect();
            unsafe { ivars.fade.setColors(Some(&NSArray::from_slice(&objects))) };
        });
    }
}

struct TrackIvars {
    fill: Retained<CALayer>,
    theme: Theme,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = TrackIvars]
    struct TrackView;

    impl TrackView {
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

impl TrackView {
    fn new(mtm: MainThreadMarker, fill: &CALayer, theme: &Theme) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TrackIvars { fill: fill.retain(), theme: theme.clone() });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setWantsLayer(true);
        this.setLayerContentsRedrawPolicy(NSViewLayerContentsRedrawPolicy::OnSetNeedsDisplay);
        if let Some(layer) = this.layer() {
            layer.setCornerRadius(metrics::capsule(STATUS_BAR));
            layer.setMasksToBounds(true);
            layer.addSublayer(fill);
        }
        this.setNeedsDisplay(true);
        return this;
    }

    fn colorize(&self) {
        let ivars = self.ivars();
        widgets::resolving(self, || {
            ivars.fill.setBackgroundColor(Some(&ivars.theme.color(Key::PillBar).CGColor()));
            if let Some(layer) = self.layer() {
                layer.setBackgroundColor(Some(&ivars.theme.color(Key::PillTrack).CGColor()));
            }
        });
    }
}

struct DropIvars {
    theme: Theme,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = DropIvars]
    struct DropView;

    impl DropView {
        #[unsafe(method_id(hitTest:))]
        fn hit_test(&self, _point: CGPoint) -> Option<Retained<NSView>> {
            None
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

impl DropView {
    fn new(mtm: MainThreadMarker, theme: &Theme) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DropIvars { theme: theme.clone() });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setWantsLayer(true);
        this.setLayerContentsRedrawPolicy(NSViewLayerContentsRedrawPolicy::OnSetNeedsDisplay);
        if let Some(layer) = this.layer() {
            layer.setBorderWidth(metrics::BORDER_RING);
            layer.setCornerRadius(metrics::RADIUS_TILE);
            layer.setOpacity(0.0);
        }
        this.setNeedsDisplay(true);
        return this;
    }

    fn colorize(&self) {
        let ivars = self.ivars();
        widgets::resolving(self, || {
            if let Some(layer) = self.layer() {
                layer.setBorderColor(Some(&ivars.theme.accent().CGColor()));
                layer.setBackgroundColor(Some(&ivars.theme.color(Key::SelectionSoft).CGColor()));
            }
        });
    }

    fn show(&self, on: bool) {
        let Some(layer) = self.layer() else {
            return;
        };
        if (layer.opacity() > 0.0) == on {
            return;
        }
        CATransaction::begin();
        CATransaction::setAnimationDuration(metrics::transform(metrics::MOTION_QUICK));
        CATransaction::setAnimationTimingFunction(Some(&ease_out()));
        layer.setOpacity(if on { 1.0 } else { 0.0 });
        CATransaction::commit();
    }
}

pub struct WallIvars {
    shade: Retained<ShadeView>,
    drop: Retained<DropView>,
    scroll: Retained<NSScrollView>,
    document: Retained<DocumentView>,
    empty: Retained<NSView>,
    status: Surface,
    status_text: Retained<NSTextField>,
    status_track: Retained<TrackView>,
    status_fill: Retained<CALayer>,
    tiles: RefCell<Vec<Retained<TileView>>>,
    shown: RefCell<(Vec<ItemId>, bool)>,
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
            self.relayout(false);
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn appearance_changed(&self) {
            let () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            let ivars = self.ivars();
            self.setNeedsDisplay(true);
            ivars.shade.setNeedsDisplay(true);
            ivars.drop.setNeedsDisplay(true);
            ivars.status_track.setNeedsDisplay(true);
            for tile in ivars.tiles.borrow().iter() {
                tile.setNeedsDisplay(true);
            }
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

    unsafe impl NSObjectProtocol for WallView {}

    unsafe impl NSDraggingDestination for WallView {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            return self.drop_target(info);
        }

        #[unsafe(method(draggingUpdated:))]
        fn dragging_updated(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            return self.drop_target(info);
        }

        #[unsafe(method(draggingExited:))]
        fn dragging_exited(&self, _info: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
            self.ivars().drop.show(false);
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            let files = sidebar::droppable(info);
            let accepted = !files.is_empty() && actions::import_here(&files);
            self.ivars().drop.show(false);
            return accepted.into();
        }

        #[unsafe(method(concludeDragOperation:))]
        fn conclude_drag(&self, _info: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
            self.ivars().drop.show(false);
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
    pub const DELETE: u16 = 51;
    pub const FORWARD_DELETE: u16 = 117;
    pub const DIGITS: [u16; 9] = [18, 19, 20, 21, 23, 22, 26, 28, 25];
}

struct Pill {
    surface: Surface,
    text: Retained<NSTextField>,
    track: Retained<TrackView>,
    fill: Retained<CALayer>,
}

fn download_pill(mtm: MainThreadMarker, theme: &Theme, paint: &mut Paint) -> Pill {
    let icon = widgets::symbol_view(mtm, "arrow.down.circle", IconRole::Inline, &theme.color(Key::TextSecondary));
    paint.add(&icon, Key::TextSecondary, Slot::Tint);
    let text = paint.label(mtm, "", Text::CalloutMedium, Key::Text, theme);
    let row = widgets::stack(mtm, &[&icon, &text], false, metrics::SPACE_8);
    let fill = CALayer::new();
    fill.setCornerRadius(metrics::capsule(STATUS_BAR));
    let track = TrackView::new(mtm, &fill, theme);
    track.widthAnchor().constraintEqualToConstant(STATUS_WIDTH).setActive(true);
    track.heightAnchor().constraintEqualToConstant(STATUS_BAR).setActive(true);
    let body = widgets::stack(mtm, &[&row, &track], true, metrics::SPACE_8);
    body.setAlignment(objc2_app_kit::NSLayoutAttribute::Leading);
    body.setEdgeInsets(widgets::insets(metrics::SPACE_8, metrics::SPACE_12, metrics::SPACE_12, metrics::SPACE_12));
    let surface = Surface::mount(mtm, &body, metrics::RADIUS_TILE, None, Key::PillFill, theme);
    surface.view().setHidden(true);
    return Pill { surface, text, track, fill };
}

impl WallView {
    pub fn new(mtm: MainThreadMarker, theme: &Theme) -> Retained<Self> {
        let mut paint = Paint::new();
        let canvas = paint.fill(mtm, Key::WindowCanvas, 0.0, theme);
        let scroll = NSScrollView::new(mtm);
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setAutomaticallyAdjustsContentInsets(true);
        let document: Retained<DocumentView> = unsafe { msg_send![DocumentView::alloc(mtm), init] };
        document.setWantsLayer(true);
        scroll.setDocumentView(Some(&document));
        let icon = widgets::symbol_view(mtm, "photo.on.rectangle.angled", IconRole::Hero, &theme.color(Key::GridEmpty));
        paint.add(&icon, Key::GridEmpty, Slot::Tint);
        let title = paint.label(mtm, &tr("No Wallpapers"), Text::Title2, Key::TextSecondary, theme);
        let hint = paint.label(
            mtm,
            &tr("Add wallpapers, or try a different search, filter or folder."),
            Text::Callout,
            Key::GridEmpty,
            theme,
        );
        hint.setAlignment(NSTextAlignment::Center);
        let empty = widgets::stack(mtm, &[&icon, &title, &hint], true, metrics::SPACE_8);
        empty.setHidden(true);
        let Pill { surface: status, text: status_text, track: status_track, fill: status_fill } =
            download_pill(mtm, theme, &mut paint);
        let shade = ShadeView::new(mtm, theme);
        let drop = DropView::new(mtm, theme);
        let this = Self::alloc(mtm).set_ivars(WallIvars {
            shade,
            drop,
            scroll,
            document,
            empty: widgets::view(&empty),
            status,
            status_text,
            status_track,
            status_fill,
            tiles: RefCell::new(Vec::new()),
            shown: RefCell::new((Vec::new(), false)),
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
        let status = ivars.status.view();
        this.addSubview(status);
        status.setTranslatesAutoresizingMaskIntoConstraints(false);
        status.centerXAnchor().constraintEqualToAnchor(&this.centerXAnchor()).setActive(true);
        status
            .bottomAnchor()
            .constraintEqualToAnchor_constant(&this.bottomAnchor(), -metrics::SPACE_20)
            .setActive(true);
        this.addSubview(&ivars.drop);
        widgets::pin(&ivars.drop, &this, metrics::SPACE_8);
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
        let types = [unsafe { NSPasteboardTypeFileURL }.copy()];
        this.registerForDraggedTypes(&NSArray::from_retained_slice(&types));
        return this;
    }

    fn drop_target(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
        let on = sidebar::dragged_items(info).is_empty() && !sidebar::droppable(info).is_empty();
        self.ivars().drop.show(on);
        return if on { NSDragOperation::Copy } else { NSDragOperation::None };
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
            keys::DELETE | keys::FORWARD_DELETE => crate::ui::dialogs::confirm_delete(self.mtm(), self.target()),
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

    pub fn accessibility_changed(&self) {
        self.ivars().status.refresh_material();
    }

    fn place_shade(&self) {
        let ivars = self.ivars();
        let bounds = self.bounds();
        let height = self.safeAreaInsets().top + metrics::SPACE_24;
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
        *ivars.shown.borrow_mut() = key;
        self.show_downloads(&library.downloads);
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
        for index in visible {
            if let Some(tile) = previous.remove(&library.items[*index].id) {
                tile.content(library, *index, scope, theme);
                tiles.push(tile);
                continue;
            }
            let tile = TileView::new(mtm, *index, library, scope, theme);
            ivars.document.addSubview(&tile);
            tiles.push(tile);
        }
        for tile in previous.into_values() {
            tile.removeFromSuperview();
        }
        *ivars.tiles.borrow_mut() = tiles;
        ivars.list.set(list);
        self.show_empty(visible.is_empty());
        self.set_selected(selected);
        let duration = metrics::transform(metrics::MOTION_LAYOUT);
        if morph && duration > 0.0 {
            let wall = self.retain();
            let group = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
                let context = unsafe { context.as_ref() };
                context.setDuration(duration);
                context.setTimingFunction(Some(&ease_in_out()));
                context.setAllowsImplicitAnimation(true);
                wall.relayout(true);
            });
            NSAnimationContext::runAnimationGroup(&group);
        } else {
            self.relayout(morph);
        }
        self.rehover();
    }

    fn show_downloads(&self, downloads: &[Download]) {
        let ivars = self.ivars();
        let text = match downloads {
            [] => None,
            [only] => Some(download_line(only)),
            many => Some(trf(
                "Downloading %ld videos · %@",
                &[Arg::Int(i64::try_from(many.len()).unwrap_or(0)), Arg::Text(crate::i18n::percent(overall(many)))],
            )),
        };
        if let Some(text) = &text {
            ivars.status_text.setStringValue(&widgets::ns(text));
            let width = STATUS_WIDTH * overall(downloads).clamp(0.0, 1.0);
            ivars.status_fill.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(width, STATUS_BAR)));
        }
        let hidden = text.is_none();
        ivars.status.view().setHidden(hidden);
    }

    fn show_empty(&self, show: bool) {
        self.ivars().empty.setHidden(!show);
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
            let padding = metrics::WALL_PADDING;
            let rect = CGRect::new(
                CGPoint::new(frame.origin.x, frame.origin.y - padding),
                CGSize::new(frame.size.width, frame.size.height + padding * 2.0),
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
                let range = metrics::SCROLL_MAX - metrics::SCROLL_MIN;
                let duration = metrics::transform(
                    metrics::SCROLL_MIN + ((origin - visible.origin.y).abs() / span).min(1.0) * range,
                );
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
    fn relayout(&self, force: bool) {
        let ivars = self.ivars();
        let width = ivars.scroll.contentSize().width;
        if !force && (width - ivars.last_width.get()).abs() < 0.5 {
            return;
        }
        ivars.last_width.set(width);
        let tiles = ivars.tiles.borrow();
        let padding = metrics::WALL_PADDING;
        let usable = (width - padding * 2.0).max(MIN_TILE);
        let list = ivars.list.get();
        let (columns, tile, height, gap) = if list {
            (1, usable, ROW_HEIGHT, metrics::SPACE_2)
        } else {
            let columns = ((usable + COLUMN_GAP) / (MIN_TILE + COLUMN_GAP)).floor().max(1.0);
            let tile = (usable - COLUMN_GAP * (columns - 1.0)) / columns;
            (columns as usize, tile, tile * TILE_ASPECT + LABEL_HEIGHT, ROW_GAP)
        };
        ivars.columns.set(columns);
        let top = metrics::SPACE_12;
        for (position, view) in tiles.iter().enumerate() {
            let column = (position % columns) as CGFloat;
            let row = (position / columns) as CGFloat;
            view.place(
                CGRect::new(
                    CGPoint::new(padding + column * (tile + COLUMN_GAP), top + row * (height + gap)),
                    CGSize::new(tile, if list { ROW_HEIGHT } else { height }),
                ),
                list,
            );
        }
        let rows = tiles.len().div_ceil(columns) as CGFloat;
        let total = top + rows * (height + gap) + padding;
        let visible = ivars.scroll.contentSize().height;
        ivars.document.setFrameSize(CGSize::new(width, total.max(visible)));
    }
}
