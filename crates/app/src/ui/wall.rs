use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSColor, NSEvent, NSImage, NSScrollView, NSTrackingArea, NSTrackingAreaOptions, NSView};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_foundation::NSString;
use objc2_quartz_core::{CALayer, CATransaction, kCAGravityResizeAspectFill};

use crate::model::ItemKind;
use crate::ui::fixture::{Item, Library};
use crate::ui::palette::Theme;
use crate::ui::widgets::{self, Weight};

const PADDING: CGFloat = 20.0;
const MIN_TILE: CGFloat = 220.0;
const COLUMN_GAP: CGFloat = 20.0;
const ROW_GAP: CGFloat = 26.0;
const LABEL_HEIGHT: CGFloat = 44.0;
const RADIUS: CGFloat = 14.0;
const RING_GAP: CGFloat = 5.0;

pub struct TileIvars {
    index: usize,
    art: Retained<NSView>,
    image: Retained<CALayer>,
    ring: Retained<NSView>,
    chips: Retained<NSView>,
    name: Retained<NSView>,
    meta: Retained<NSView>,
    selected: Cell<bool>,
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
            let index = self.ivars().index;
            if event.clickCount() >= 2 {
                crate::ui::apply(index);
                return;
            }
            crate::ui::select(Some(index));
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
    fn new(mtm: MainThreadMarker, index: usize, item: &Item, library: &Library, theme: &Theme) -> Retained<Self> {
        let art = NSView::new(mtm);
        art.setWantsLayer(true);
        let image = CALayer::new();
        if let Some(layer) = art.layer() {
            layer.setCornerRadius(RADIUS);
            layer.setMasksToBounds(true);
            layer.setBorderWidth(0.5);
            layer.setBorderColor(Some(&widgets::cg(mtm, &NSColor::separatorColor())));
            layer.setBackgroundColor(Some(&widgets::cg(mtm, &NSColor::quaternarySystemFillColor())));
            layer.addSublayer(&image);
        }
        if let Some(path) = &item.thumb
            && let Some(thumb) =
                NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(&path.to_string_lossy()))
        {
            unsafe { image.setContents(Some(&thumb)) };
        }
        image.setContentsGravity(unsafe { kCAGravityResizeAspectFill });

        let ring = NSView::new(mtm);
        ring.setWantsLayer(true);
        if let Some(layer) = ring.layer() {
            layer.setCornerRadius(RADIUS + RING_GAP);
            layer.setBorderWidth(3.0);
            layer.setBorderColor(Some(&widgets::cg(mtm, &theme.accent)));
        }
        ring.setHidden(true);

        let chip_views: Vec<Retained<NSView>> = widgets::marker_text(library, &library.markers(index))
            .into_iter()
            .map(|(text, icon)| widgets::chip(mtm, &text, icon))
            .collect();
        let chip_refs: Vec<&NSView> = chip_views.iter().map(|view| &**view).collect();
        let chips = widgets::view(&widgets::stack(mtm, &chip_refs, false, 6.0));

        let name = widgets::label(mtm, &item.title, 13.0, &Weight::Medium, &NSColor::labelColor());
        let meta = widgets::label(mtm, &meta_line(item), 11.0, &Weight::Regular, &NSColor::tertiaryLabelColor());

        let this = Self::alloc(mtm).set_ivars(TileIvars {
            index,
            art,
            image,
            ring,
            chips,
            name: widgets::view(&name),
            meta: widgets::view(&meta),
            selected: Cell::new(false),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setWantsLayer(true);
        let ivars = this.ivars();
        this.addSubview(&ivars.ring);
        this.addSubview(&ivars.art);
        this.addSubview(&ivars.chips);
        this.addSubview(&ivars.name);
        this.addSubview(&ivars.meta);
        if let Some(layer) = this.layer() {
            layer.setShadowOpacity(0.0);
        }
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

    fn place(&self, frame: CGRect) {
        self.setFrame(frame);
        let ivars = self.ivars();
        let width = frame.size.width;
        let height = width * 10.0 / 16.0;
        let art = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(width, height));
        ivars.art.setFrame(art);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        ivars.image.setFrame(CGRect::new(CGPoint::ZERO, art.size));
        CATransaction::commit();
        ivars.ring.setFrame(CGRect::new(
            CGPoint::new(-RING_GAP, -RING_GAP),
            CGSize::new(width + RING_GAP * 2.0, height + RING_GAP * 2.0),
        ));
        let chips = ivars.chips.fittingSize();
        ivars.chips.setFrame(CGRect::new(CGPoint::new(10.0, 10.0), chips));
        ivars.name.setFrame(CGRect::new(CGPoint::new(2.0, height + 8.0), CGSize::new(width - 4.0, 18.0)));
        ivars.meta.setFrame(CGRect::new(CGPoint::new(2.0, height + 26.0), CGSize::new(width - 4.0, 15.0)));
    }

    fn hover(&self, on: bool) {
        let scale = if on { 1.04 } else { 1.0 };
        let transform = objc2_quartz_core::CATransform3D {
            m11: scale,
            m22: scale,
            m33: 1.0,
            m44: 1.0,
            ..unsafe { objc2_quartz_core::CATransform3DIdentity }
        };
        self.ivars().image.setTransform(transform);
    }

    pub fn set_selected(&self, selected: bool) {
        self.ivars().selected.set(selected);
        self.ivars().ring.setHidden(!selected);
    }
}

pub fn meta_line(item: &Item) -> String {
    let kind = match item.kind {
        ItemKind::Video => "Video",
        ItemKind::Gif => "GIF",
        ItemKind::Image => "Image",
    };
    let mut parts = vec![kind.to_string()];
    if item.width > 0 {
        parts.push(format!("{}×{}", item.width, item.height));
    }
    if item.kind == ItemKind::Video && item.duration > 0.0 {
        parts.push(duration(item.duration));
    }
    return parts.join(" · ");
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn duration(seconds: f64) -> String {
    let total = seconds.round() as u64;
    return format!("{}:{:02}", total / 60, total % 60);
}

pub struct DocumentIvars;

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = DocumentIvars]
    struct DocumentView;

    impl DocumentView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            crate::ui::select(None);
        }
    }
);

pub struct WallIvars {
    scroll: Retained<NSScrollView>,
    document: Retained<DocumentView>,
    tiles: RefCell<Vec<Retained<TileView>>>,
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
            self.relayout(false);
        }
    }
);

impl WallView {
    pub fn new(mtm: MainThreadMarker, library: &Library, theme: &Theme) -> Retained<Self> {
        let scroll = NSScrollView::new(mtm);
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setAutomaticallyAdjustsContentInsets(true);
        let document = DocumentView::alloc(mtm).set_ivars(DocumentIvars);
        let document: Retained<DocumentView> = unsafe { msg_send![super(document), init] };
        scroll.setDocumentView(Some(&document));
        let tiles: Vec<Retained<TileView>> = library
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| TileView::new(mtm, index, item, library, theme))
            .collect();
        for tile in &tiles {
            document.addSubview(tile);
        }
        let this = Self::alloc(mtm).set_ivars(WallIvars {
            scroll,
            document,
            tiles: RefCell::new(tiles),
            last_width: Cell::new(0.0),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setWantsLayer(true);
        if let Some(layer) = this.layer() {
            layer.setBackgroundColor(Some(&widgets::cg(mtm, &theme.canvas)));
        }
        let scroll = &this.ivars().scroll;
        this.addSubview(scroll);
        widgets::pin(scroll, &this, 0.0);
        return this;
    }

    pub fn set_selected(&self, selected: Option<usize>) {
        for tile in self.ivars().tiles.borrow().iter() {
            tile.set_selected(Some(tile.ivars().index) == selected);
        }
    }

    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn relayout(&self, force: bool) {
        let ivars = self.ivars();
        let width = ivars.scroll.contentSize().width;
        if !force && (width - ivars.last_width.get()).abs() < 0.5 {
            return;
        }
        ivars.last_width.set(width);
        let usable = (width - PADDING * 2.0).max(MIN_TILE);
        let columns = ((usable + COLUMN_GAP) / (MIN_TILE + COLUMN_GAP)).floor().max(1.0);
        let tile = (usable - COLUMN_GAP * (columns - 1.0)) / columns;
        let height = tile * 10.0 / 16.0 + LABEL_HEIGHT;
        let tiles = ivars.tiles.borrow();
        let columns = columns as usize;
        for (position, view) in tiles.iter().enumerate() {
            let column = (position % columns) as CGFloat;
            let row = (position / columns) as CGFloat;
            view.place(CGRect::new(
                CGPoint::new(PADDING + column * (tile + COLUMN_GAP), 12.0 + row * (height + ROW_GAP)),
                CGSize::new(tile, height),
            ));
        }
        let rows = tiles.len().div_ceil(columns) as CGFloat;
        let total = 12.0 + rows * (height + ROW_GAP) + PADDING;
        let visible = ivars.scroll.contentSize().height;
        ivars.document.setFrameSize(CGSize::new(width, total.max(visible)));
    }
}
