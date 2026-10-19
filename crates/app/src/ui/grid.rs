use std::cell::RefCell;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::runtime::{AnyObject, Bool};
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::NSIndexPathNSCollectionViewAdditions;
use objc2_app_kit::{
    NSBeep, NSBezierPath, NSCompositingOperation, NSControlSize, NSDraggingImageComponent,
    NSDraggingImageComponentIconKey, NSEventModifierFlags, NSFontTextStyleHeadline, NSImage, NSProgressIndicator,
    NSProgressIndicatorStyle,
};
use objc2_app_kit::{
    NSCollectionView, NSCollectionViewDataSource, NSCollectionViewDelegate, NSCollectionViewFlowLayout,
    NSCollectionViewItem, NSColor, NSFont, NSImageView, NSLayoutConstraintOrientation, NSLineBreakMode, NSResponder,
    NSScrollView, NSStackView, NSTextField, NSUserInterfaceLayoutOrientation, NSView, NSViewController,
};
use objc2_app_kit::{
    NSControlTextEditingDelegate, NSTableCellView, NSTableColumn, NSTableView, NSTableViewColumnAutoresizingStyle,
    NSTableViewDataSource, NSTableViewDelegate, NSTableViewStyle,
};
use objc2_app_kit::{NSDragOperation, NSDraggingDestination, NSDraggingInfo, NSEvent, NSMenu, NSPasteboardWriting};
use objc2_core_foundation::{CGFloat, CGSize};
use objc2_core_foundation::{CGPoint, CGRect};
use objc2_foundation::{
    NSArray, NSCopying, NSDictionary, NSEdgeInsets, NSIndexPath, NSIndexSet, NSNotification, NSObject,
    NSObjectProtocol, NSSet,
};
use objc2_quartz_core::{CALayer, CATransaction, kCAGravityResizeAspectFill};

use crate::i18n::{self, tr};
use crate::model::{ItemId, ItemKind};
use crate::ui::state::{Filter, Layout, Scope, duration, markers};
use crate::ui::view::{Item, Library};
use crate::ui::{context, folders, frame, inspector, ns, toggle_play, update};

const TILE: &str = "tile";
const LARGE: CGFloat = 240.0;
const SMALL: CGFloat = 140.0;
const GAP: CGFloat = 20.0;
const LINE_GAP: CGFloat = 24.0;
const INSET: CGFloat = 20.0;
const ASPECT: CGFloat = 10.0 / 16.0;
const RADIUS: CGFloat = 8.0;
const RING: CGFloat = 3.0;
const TEXT_GAP: CGFloat = 6.0;
const DASH: &str = "—";
const ROW_HEIGHT: CGFloat = 36.0;
const ROW_THUMB: CGSize = CGSize::new(48.0, 30.0);
const ROW_RADIUS: CGFloat = 4.0;
const NAME: &str = "name";
const BAR_INSET: CGFloat = 8.0;
const BAR_WIDTH: CGFloat = 80.0;
const BAR: usize = 4;
const EMPTY_GAP: CGFloat = 4.0;
const KEY_RETURN: u16 = 36;
const KEY_ENTER: u16 = 76;
const KEY_SPACE: u16 = 49;
const KEY_COMMA: u16 = 43;
const KEY_PERIOD: u16 = 47;
const KEY_GRAVE: u16 = 50;
const KEY_DELETE: u16 = 51;
const KEY_FORWARD_DELETE: u16 = 117;
const KEY_ESCAPE: u16 = 53;
const KEY_DIGITS: [u16; 9] = [18, 19, 20, 21, 23, 22, 26, 28, 25];
const COLUMNS: [(&str, &str, CGFloat); 5] = [
    ("kind", i18n::key("Kind"), 100.0),
    ("length", i18n::key("Duration"), 100.0),
    ("resolution", i18n::key("Resolution"), 100.0),
    ("size", i18n::key("Size"), 80.0),
    ("added", i18n::key("Date Added"), 130.0),
];

#[derive(Clone)]
struct Cell {
    title: String,
    meta: String,
    kind: String,
    length: String,
    resolution: String,
    size: String,
    added: String,
    progress: Option<f64>,
    image: Option<Retained<objc2_app_kit::NSImage>>,
    markers: Vec<Mark>,
}

#[derive(Clone, PartialEq)]
struct Mark {
    icon: &'static str,
    short: Option<String>,
    tip: String,
    active: bool,
}

impl Cell {
    fn same_shape(&self, other: &Cell) -> bool {
        return self.title == other.title
            && self.meta == other.meta
            && self.kind == other.kind
            && self.length == other.length
            && self.resolution == other.resolution
            && self.size == other.size
            && self.added == other.added
            && self.image == other.image
            && self.markers == other.markers
            && self.progress.is_some() == other.progress.is_some();
    }
}

define_class!(
    #[unsafe(super(NSCollectionViewItem, NSViewController, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    struct Tile;

    unsafe impl NSObjectProtocol for Tile {}

    impl Tile {
        #[unsafe(method(loadView))]
        fn load_view(&self) {
            self.build();
        }

        #[unsafe(method(setSelected:))]
        fn set_selected(&self, selected: bool) {
            let _: () = unsafe { msg_send![super(self), setSelected: selected] };
            self.paint();
        }

        #[unsafe(method_id(draggingImageComponents))]
        fn components(&self) -> Retained<NSArray<NSDraggingImageComponent>> {
            let base = unsafe { msg_send![super(self), draggingImageComponents] };
            with_thumbnail(&self.view(), base, RADIUS)
        }
    }
);

define_class!(
    #[unsafe(super(NSTableCellView, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    struct NameCell;

    unsafe impl NSObjectProtocol for NameCell {}

    impl NameCell {
        #[unsafe(method_id(draggingImageComponents))]
        fn components(&self) -> Retained<NSArray<NSDraggingImageComponent>> {
            let base = unsafe { msg_send![super(self), draggingImageComponents] };
            with_thumbnail(self, base, ROW_RADIUS)
        }
    }
);

fn with_thumbnail(
    view: &NSView,
    base: Retained<NSArray<NSDraggingImageComponent>>,
    radius: CGFloat,
) -> Retained<NSArray<NSDraggingImageComponent>> {
    let Some(thumb) = view.subviews().iter().find_map(|sub| sub.downcast::<Thumb>().ok()) else {
        return base;
    };
    let image = thumb
        .layer()
        .and_then(|host| unsafe { host.sublayers() })
        .and_then(|layers| layers.firstObject())
        .and_then(|picture| unsafe { picture.contents() })
        .and_then(|contents| contents.downcast::<NSImage>().ok());
    let Some(image) = image else {
        return base;
    };
    let frame = thumb.frame();
    let component = NSDraggingImageComponent::draggingImageComponentWithKey(unsafe { NSDraggingImageComponentIconKey });
    unsafe { component.setContents(Some(&filled(image, frame.size, radius))) };
    component.setFrame(frame);
    let mut all: Vec<Retained<NSDraggingImageComponent>> = vec![component];
    all.extend(base.iter());
    return NSArray::from_retained_slice(&all);
}

fn filled(image: Retained<NSImage>, size: CGSize, radius: CGFloat) -> Retained<NSImage> {
    let draw = RcBlock::new(move |bounds: CGRect| -> Bool {
        let source = image.size();
        let scale = (bounds.size.width / source.width).max(bounds.size.height / source.height);
        let crop = CGSize::new(bounds.size.width / scale, bounds.size.height / scale);
        let from =
            CGRect::new(CGPoint::new((source.width - crop.width) / 2.0, (source.height - crop.height) / 2.0), crop);
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, radius, radius).addClip();
        image.drawInRect_fromRect_operation_fraction(bounds, from, NSCompositingOperation::SourceOver, 1.0);
        return Bool::YES;
    });
    return NSImage::imageWithSize_flipped_drawingHandler(size, false, &draw);
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    struct Thumb;

    unsafe impl NSObjectProtocol for Thumb {}

    impl Thumb {
        #[unsafe(method(layout))]
        fn layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            if let Some(host) = self.layer()
                && let Some(layers) = unsafe { host.sublayers() }
            {
                for layer in &layers {
                    layer.setFrame(host.bounds());
                }
            }
            CATransaction::commit();
        }
    }
);

fn label(mtm: MainThreadMarker, font: &NSFont, color: &NSColor) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&ns(""), mtm);
    label.setFont(Some(font));
    label.setTextColor(Some(color));
    label.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
    label.setAllowsExpansionToolTips(true);
    label.setContentCompressionResistancePriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
    label.setTranslatesAutoresizingMaskIntoConstraints(false);
    return label;
}

fn progress_bar(mtm: MainThreadMarker) -> Retained<NSProgressIndicator> {
    let bar = NSProgressIndicator::new(mtm);
    bar.setStyle(NSProgressIndicatorStyle::Bar);
    bar.setIndeterminate(false);
    bar.setMinValue(0.0);
    bar.setMaxValue(1.0);
    bar.setControlSize(NSControlSize::Small);
    bar.setTranslatesAutoresizingMaskIntoConstraints(false);
    return bar;
}

fn title_font() -> Retained<NSFont> {
    return NSFont::systemFontOfSize(NSFont::systemFontSize());
}

fn meta_font() -> Retained<NSFont> {
    return NSFont::systemFontOfSize(NSFont::smallSystemFontSize());
}

fn mark_view(mtm: MainThreadMarker, mark: &Mark) -> Option<Retained<NSView>> {
    let image = folders::symbol(mark.icon)?;
    let tint = if mark.active { NSColor::controlAccentColor() } else { NSColor::secondaryLabelColor() };
    let glyph = NSImageView::imageViewWithImage(&image, mtm);
    glyph.setContentTintColor(Some(&tint));
    let Some(short) = &mark.short else {
        glyph.setToolTip(Some(&ns(&mark.tip)));
        return Some(Retained::into_super(Retained::into_super(glyph)));
    };
    let text = NSTextField::labelWithString(&ns(short), mtm);
    text.setFont(Some(&meta_font()));
    text.setTextColor(Some(&tint));
    let row = NSStackView::stackViewWithViews(&NSArray::from_slice(&[&*glyph as &NSView, &text]), mtm);
    row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    row.setSpacing(2.0);
    row.setToolTip(Some(&ns(&mark.tip)));
    return Some(Retained::into_super(row));
}

impl Tile {
    fn build(&self) {
        let mtm = self.mtm();
        let root = NSView::new(mtm);
        let thumb: Retained<Thumb> = unsafe { msg_send![Thumb::alloc(mtm), init] };
        thumb.setWantsLayer(true);
        if let Some(host) = thumb.layer() {
            host.setCornerRadius(RADIUS);
            host.setMasksToBounds(true);
            host.setBackgroundColor(Some(&NSColor::quaternaryLabelColor().CGColor()));
            let picture = CALayer::new();
            picture.setContentsGravity(unsafe { kCAGravityResizeAspectFill });
            host.addSublayer(&picture);
        }
        thumb.setTranslatesAutoresizingMaskIntoConstraints(false);
        let title = label(mtm, &title_font(), &NSColor::labelColor());
        let meta = label(mtm, &meta_font(), &NSColor::secondaryLabelColor());
        let marks = NSStackView::new(mtm);
        marks.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        marks.setSpacing(4.0);
        marks.setTranslatesAutoresizingMaskIntoConstraints(false);
        marks.setContentHuggingPriority_forOrientation(750.0, NSLayoutConstraintOrientation::Horizontal);
        marks.setContentCompressionResistancePriority_forOrientation(1000.0, NSLayoutConstraintOrientation::Horizontal);
        let bar = progress_bar(mtm);
        bar.setHidden(true);
        for view in [&*thumb as &NSView, &title, &meta, &marks, &bar] {
            root.addSubview(view);
        }
        thumb.topAnchor().constraintEqualToAnchor(&root.topAnchor()).setActive(true);
        thumb.leadingAnchor().constraintEqualToAnchor(&root.leadingAnchor()).setActive(true);
        thumb.trailingAnchor().constraintEqualToAnchor(&root.trailingAnchor()).setActive(true);
        thumb.heightAnchor().constraintEqualToAnchor_multiplier(&thumb.widthAnchor(), ASPECT).setActive(true);
        title.topAnchor().constraintEqualToAnchor_constant(&thumb.bottomAnchor(), TEXT_GAP).setActive(true);
        title.leadingAnchor().constraintEqualToAnchor(&root.leadingAnchor()).setActive(true);
        title.trailingAnchor().constraintLessThanOrEqualToAnchor(&root.trailingAnchor()).setActive(true);
        meta.topAnchor().constraintEqualToAnchor(&title.bottomAnchor()).setActive(true);
        meta.leadingAnchor().constraintEqualToAnchor(&root.leadingAnchor()).setActive(true);
        meta.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&marks.leadingAnchor(), -6.0).setActive(true);
        marks.trailingAnchor().constraintEqualToAnchor(&root.trailingAnchor()).setActive(true);
        marks.centerYAnchor().constraintEqualToAnchor(&meta.centerYAnchor()).setActive(true);
        bar.leadingAnchor().constraintEqualToAnchor_constant(&thumb.leadingAnchor(), BAR_INSET).setActive(true);
        bar.trailingAnchor().constraintEqualToAnchor_constant(&thumb.trailingAnchor(), -BAR_INSET).setActive(true);
        bar.bottomAnchor().constraintEqualToAnchor_constant(&thumb.bottomAnchor(), -BAR_INSET).setActive(true);
        self.setView(&root);
        self.setTextField(Some(&title));
    }

    fn part(&self, index: usize) -> Option<Retained<NSView>> {
        let views = self.view().subviews();
        return (index < views.count()).then(|| views.objectAtIndex(index));
    }

    fn fill(&self, cell: &Cell) {
        let mtm = self.mtm();
        if let Some(title) = self.textField() {
            title.setStringValue(&ns(&cell.title));
        }
        if let Some(meta) = self.part(2).and_then(|view| view.downcast::<NSTextField>().ok()) {
            meta.setStringValue(&ns(&cell.meta));
        }
        if let Some(thumb) = self.part(0)
            && let Some(host) = thumb.layer()
            && let Some(layers) = unsafe { host.sublayers() }
            && let Some(picture) = layers.firstObject()
        {
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            unsafe { picture.setContents(cell.image.as_deref().map(|image| image as &objc2::runtime::AnyObject)) };
            CATransaction::commit();
        }
        if let Some(marks) = self.part(3).and_then(|view| view.downcast::<NSStackView>().ok()) {
            let views: Vec<Retained<NSView>> = cell.markers.iter().filter_map(|mark| mark_view(mtm, mark)).collect();
            let refs: Vec<&NSView> = views.iter().map(AsRef::as_ref).collect();
            marks.setViews_inGravity(&NSArray::from_slice(&refs), objc2_app_kit::NSStackViewGravity::Trailing);
        }
        self.show_progress(cell.progress);
        self.paint();
    }

    fn show_progress(&self, progress: Option<f64>) {
        if let Some(bar) = self.part(BAR).and_then(|view| view.downcast::<NSProgressIndicator>().ok()) {
            bar.setHidden(progress.is_none());
            bar.setDoubleValue(progress.unwrap_or(0.0));
        }
    }

    fn paint(&self) {
        if let Some(thumb) = self.part(0)
            && let Some(host) = thumb.layer()
        {
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            if self.isSelected() {
                host.setBorderWidth(RING);
                host.setBorderColor(Some(&NSColor::controlAccentColor().CGColor()));
            } else {
                host.setBorderWidth(0.0);
            }
            CATransaction::commit();
        }
    }
}

struct SourceIvars {
    view: Retained<NSCollectionView>,
    table: Retained<NSTableView>,
    syncing: std::cell::Cell<bool>,
    cells: RefCell<Vec<Cell>>,
    ids: RefCell<Vec<ItemId>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = SourceIvars]
    struct GridSource;

    unsafe impl NSObjectProtocol for GridSource {}

    unsafe impl NSCollectionViewDataSource for GridSource {
        #[unsafe(method(collectionView:numberOfItemsInSection:))]
        fn count(&self, _view: &NSCollectionView, _section: isize) -> isize {
            isize::try_from(self.ivars().cells.borrow().len()).unwrap_or(0)
        }

        #[unsafe(method_id(collectionView:itemForRepresentedObjectAtIndexPath:))]
        fn item(&self, view: &NSCollectionView, path: &NSIndexPath) -> Retained<NSCollectionViewItem> {
            let item = view.makeItemWithIdentifier_forIndexPath(&ns(TILE), path);
            let cell = usize::try_from(path.item()).ok().and_then(|index| self.ivars().cells.borrow().get(index).cloned());
            if let (Some(cell), Ok(tile)) = (cell, item.clone().downcast::<Tile>()) {
                tile.fill(&cell);
            }
            item
        }
    }

    unsafe impl NSControlTextEditingDelegate for GridSource {}

    impl GridSource {
        #[unsafe(method(open:))]
        fn open(&self, _sender: Option<&AnyObject>) {
            if self.ivars().table.clickedRow() >= 0 {
                place(None);
            }
        }
    }

    unsafe impl NSTableViewDataSource for GridSource {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn rows(&self, _table: &NSTableView) -> isize {
            isize::try_from(self.ivars().cells.borrow().len()).unwrap_or(0)
        }

        #[unsafe(method_id(tableView:pasteboardWriterForRow:))]
        fn list_writer(&self, _table: &NSTableView, row: isize) -> Option<Retained<ProtocolObject<dyn NSPasteboardWriting>>> {
            self.writer(row)
        }
    }

    unsafe impl NSTableViewDelegate for GridSource {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn row_view(&self, _table: &NSTableView, column: Option<&NSTableColumn>, row: isize) -> Option<Retained<NSView>> {
            let cell = usize::try_from(row).ok().and_then(|row| self.ivars().cells.borrow().get(row).cloned());
            let identifier = column.map(|column| column.identifier().to_string()).unwrap_or_default();
            cell.map(|cell| list_cell(self.mtm(), &cell, &identifier))
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn table_selection(&self, _notification: &NSNotification) {
            if !self.ivars().syncing.get() {
                let row = usize::try_from(self.ivars().table.selectedRow()).ok();
                self.select(row);
            }
        }
    }

    unsafe impl NSCollectionViewDelegate for GridSource {
        #[unsafe(method(collectionView:didSelectItemsAtIndexPaths:))]
        fn did_select(&self, _view: &NSCollectionView, _paths: &NSSet<NSIndexPath>) {
            self.selection_changed();
        }

        #[unsafe(method(collectionView:didDeselectItemsAtIndexPaths:))]
        fn did_deselect(&self, _view: &NSCollectionView, _paths: &NSSet<NSIndexPath>) {
            self.selection_changed();
        }

        #[unsafe(method(collectionView:canDragItemsAtIndexPaths:withEvent:))]
        fn can_drag(&self, _view: &NSCollectionView, _paths: &NSSet<NSIndexPath>, _event: &NSEvent) -> bool {
            true
        }

        #[unsafe(method_id(collectionView:pasteboardWriterForItemAtIndexPath:))]
        fn grid_writer(&self, _view: &NSCollectionView, path: &NSIndexPath) -> Option<Retained<ProtocolObject<dyn NSPasteboardWriting>>> {
            self.writer(path.item())
        }
    }
);

define_class!(
    #[unsafe(super(NSCollectionView, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = std::cell::Cell<(CGFloat, CGFloat)>]
    struct Collection;

    unsafe impl NSObjectProtocol for Collection {}

    impl Collection {
        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, size: CGSize) {
            let _: () = unsafe { msg_send![super(self), setFrameSize: size] };
            self.resize(size.width);
        }

        #[unsafe(method_id(menuForEvent:))]
        fn menu_for_event(&self, event: &NSEvent) -> Option<Retained<NSMenu>> {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            let index = self.indexPathForItemAtPoint(point).and_then(|path| usize::try_from(path.item()).ok());
            context_menu(self.mtm(), index)
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if !handle_key(self.mtm(), event) {
                let _: () = unsafe { msg_send![super(self), keyDown: event] };
            }
        }

        #[unsafe(method(cancelOperation:))]
        fn cancel_operation(&self, _sender: Option<&AnyObject>) {
            crate::ui::select(None);
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let _: () = unsafe { msg_send![super(self), mouseDown: event] };
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            match self.indexPathForItemAtPoint(point) {
                Some(_) if event.clickCount() == 2 => place(None),
                Some(_) => {}
                None => crate::ui::select(None),
            }
        }
    }
);

define_class!(
    #[unsafe(super(NSTableView, objc2_app_kit::NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    struct List;

    unsafe impl NSObjectProtocol for List {}

    impl List {
        #[unsafe(method_id(menuForEvent:))]
        fn menu_for_event(&self, event: &NSEvent) -> Option<Retained<NSMenu>> {
            let point = self.convertPoint_fromView(event.locationInWindow(), None);
            context_menu(self.mtm(), usize::try_from(self.rowAtPoint(point)).ok())
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if !handle_key(self.mtm(), event) {
                let _: () = unsafe { msg_send![super(self), keyDown: event] };
            }
        }

        #[unsafe(method(cancelOperation:))]
        fn cancel_operation(&self, _sender: Option<&AnyObject>) {
            crate::ui::select(None);
        }
    }
);

define_class!(
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    struct Drop;

    unsafe impl NSObjectProtocol for Drop {}

    unsafe impl NSDraggingDestination for Drop {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            accepts(info)
        }

        #[unsafe(method(draggingUpdated:))]
        fn dragging_updated(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            accepts(info)
        }

        #[unsafe(method(performDragOperation:))]
        fn perform(&self, info: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            let files = context::dragged_files(info);
            let accepted = !files.is_empty();
            update(move |ui, _| context::import(ui, files, ui.filter.folder));
            accepted
        }
    }
);

fn accepts(info: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
    if context::dragged_items(info).is_empty() && !context::dragged_files(info).is_empty() {
        return NSDragOperation::Copy;
    }
    return NSDragOperation::None;
}

fn handle_key(mtm: MainThreadMarker, event: &NSEvent) -> bool {
    let blocked = NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option;
    if event.modifierFlags().intersects(blocked) {
        return false;
    }
    let code = event.keyCode();
    match code {
        KEY_RETURN | KEY_ENTER => place(None),
        KEY_SPACE => toggle_play(),
        KEY_COMMA => frame(-1),
        KEY_PERIOD => frame(1),
        KEY_GRAVE => place(Some(Scope::All)),
        KEY_DELETE | KEY_FORWARD_DELETE => context::confirm_delete(mtm),
        KEY_ESCAPE => crate::ui::select(None),
        _ => match KEY_DIGITS.iter().position(|digit| *digit == code) {
            Some(space) => place(Some(Scope::Space(space))),
            None => return false,
        },
    }
    return true;
}

fn place(scope: Option<Scope>) {
    let mut chosen = None;
    update(|ui, mtm| {
        let Some(item) = ui.selected_index().and_then(|index| ui.library.items.get(index)) else {
            return;
        };
        let scope = scope.unwrap_or(ui.filter.scope);
        let exists = match scope {
            Scope::All => true,
            Scope::Space(space) => space < ui.library.desktops,
        };
        if !exists || !item.playable {
            NSBeep();
            return;
        }
        if ui.filter.scope != scope {
            ui.filter.scope = scope;
            ui.refresh(mtm);
        }
        chosen = Some(scope);
    });
    if let Some(scope) = chosen {
        context::apply(scope);
    }
}

fn context_menu(mtm: MainThreadMarker, index: Option<usize>) -> Option<Retained<NSMenu>> {
    let index = index?;
    update(|ui, _| {
        if let Some(id) = ui.grid.pick(index)
            && ui.selected != Some(id)
        {
            ui.selected = Some(id);
            ui.inspector.show(&ui.library, ui.selected_index(), ui.filter.scope);
        }
    });
    return context::menu(mtm);
}

impl Collection {
    fn resize(&self, width: CGFloat) {
        let (text, minimum) = self.ivars().get();
        let tile = tile_size(width, minimum, text);
        if let Some(layout) =
            self.collectionViewLayout().and_then(|layout| layout.downcast::<NSCollectionViewFlowLayout>().ok())
            && layout.itemSize() != tile
        {
            layout.setItemSize(tile);
        }
    }
}

fn tile_size(available: CGFloat, minimum: CGFloat, text: CGFloat) -> CGSize {
    let width = (available - 2.0 * INSET).max(minimum);
    let columns = ((width + GAP) / (minimum + GAP)).floor().max(1.0);
    let tile = ((width - GAP * (columns - 1.0)) / columns).floor();
    return CGSize::new(tile, (tile * ASPECT).round() + TEXT_GAP + text);
}

impl GridSource {
    fn id(&self, index: isize) -> Option<ItemId> {
        return usize::try_from(index).ok().and_then(|index| self.ivars().ids.borrow().get(index).copied());
    }

    fn writer(&self, index: isize) -> Option<Retained<ProtocolObject<dyn NSPasteboardWriting>>> {
        return self.id(index).map(context::writer);
    }

    fn selection_changed(&self) {
        let paths = self.ivars().view.selectionIndexPaths();
        let index = paths.anyObject().and_then(|path| usize::try_from(path.item()).ok());
        self.select(index);
    }

    fn select(&self, index: Option<usize>) {
        let id = index.and_then(|index| self.ivars().ids.borrow().get(index).copied());
        update(move |ui, _| {
            if ui.selected != id {
                ui.selected = id;
                ui.inspector.show(&ui.library, ui.selected_index(), ui.filter.scope);
            }
        });
    }
}

pub struct Grid {
    pub controller: Retained<NSViewController>,
    source: Retained<GridSource>,
    collection: Retained<Collection>,
    scroll: Retained<NSScrollView>,
    list: Retained<NSScrollView>,
    empty: Retained<NSStackView>,
    actions: Retained<context::ContextActions>,
}

fn meta(item: &Item) -> String {
    let kind = tr(inspector::kind_name(item.kind));
    let detail = match item.kind {
        ItemKind::Video | ItemKind::Gif if item.duration > 0.0 => Some(duration(item.duration)),
        _ => inspector::resolution(item),
    };
    return match detail {
        Some(detail) => format!("{kind} · {detail}"),
        None => kind,
    };
}

impl Grid {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let layout = NSCollectionViewFlowLayout::new(mtm);
        layout.setMinimumInteritemSpacing(GAP);
        layout.setMinimumLineSpacing(LINE_GAP);
        layout.setSectionInset(NSEdgeInsets { top: INSET, left: INSET, bottom: INSET, right: INSET });
        let probe = |font: &NSFont| {
            let label = NSTextField::labelWithString(&ns("Ag"), mtm);
            label.setFont(Some(font));
            return label.fittingSize().height;
        };
        let text = (probe(&title_font()) + probe(&meta_font())).ceil();
        layout.setItemSize(tile_size(0.0, LARGE, text));
        let collection = Collection::alloc(mtm).set_ivars(std::cell::Cell::new((text, LARGE)));
        let collection: Retained<Collection> = unsafe { msg_send![super(collection), init] };
        let view: Retained<NSCollectionView> = Retained::into_super(collection.clone());
        view.setCollectionViewLayout(Some(&layout));
        view.setSelectable(true);
        view.setAllowsEmptySelection(true);
        view.setDraggingSourceOperationMask_forLocal(NSDragOperation::Move, true);
        view.setDraggingSourceOperationMask_forLocal(NSDragOperation::None, false);
        unsafe { view.registerClass_forItemWithIdentifier(Some(Tile::class()), &ns(TILE)) };
        let table = list_table(mtm);
        let source = GridSource::alloc(mtm).set_ivars(SourceIvars {
            view: view.clone(),
            table: table.clone(),
            syncing: std::cell::Cell::new(false),
            cells: RefCell::new(Vec::new()),
            ids: RefCell::new(Vec::new()),
        });
        let source: Retained<GridSource> = unsafe { msg_send![super(source), init] };
        unsafe {
            view.setDataSource(Some(ProtocolObject::from_ref(&*source)));
            view.setDelegate(Some(ProtocolObject::from_ref(&*source)));
            table.setDataSource(Some(ProtocolObject::from_ref(&*source)));
            table.setDelegate(Some(ProtocolObject::from_ref(&*source)));
        }
        let scroll = NSScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        scroll.setDrawsBackground(false);
        scroll.setDocumentView(Some(&view));
        let list = NSScrollView::new(mtm);
        list.setHasVerticalScroller(true);
        list.setDocumentView(Some(&table));
        list.setHidden(true);
        table.setDraggingSourceOperationMask_forLocal(NSDragOperation::Move, true);
        table.setDraggingSourceOperationMask_forLocal(NSDragOperation::None, false);
        let root: Retained<Drop> = unsafe { msg_send![Drop::alloc(mtm), init] };
        root.registerForDraggedTypes(&NSArray::from_retained_slice(&[unsafe {
            objc2_app_kit::NSPasteboardTypeFileURL
        }
        .copy()]));
        for pane in [&scroll, &list] {
            pane.setTranslatesAutoresizingMaskIntoConstraints(false);
            root.addSubview(pane);
            pane.leadingAnchor().constraintEqualToAnchor(&root.leadingAnchor()).setActive(true);
            pane.trailingAnchor().constraintEqualToAnchor(&root.trailingAnchor()).setActive(true);
            pane.topAnchor().constraintEqualToAnchor(&root.topAnchor()).setActive(true);
            pane.bottomAnchor().constraintEqualToAnchor(&root.bottomAnchor()).setActive(true);
        }
        let headline =
            unsafe { NSFont::preferredFontForTextStyle_options(NSFontTextStyleHeadline, &NSDictionary::new()) };
        let secondary = NSColor::secondaryLabelColor();
        let heading = NSTextField::labelWithString(&ns(&tr("No Wallpapers")), mtm);
        heading.setFont(Some(&headline));
        heading.setTextColor(Some(&secondary));
        let hint =
            NSTextField::labelWithString(&ns(&tr("Add wallpapers, or try a different search, filter or folder.")), mtm);
        hint.setTextColor(Some(&secondary));
        let empty = NSStackView::new(mtm);
        empty.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        empty.setSpacing(EMPTY_GAP);
        empty.addArrangedSubview(&heading);
        empty.addArrangedSubview(&hint);
        empty.setHidden(true);
        empty.setTranslatesAutoresizingMaskIntoConstraints(false);
        root.addSubview(&empty);
        empty.centerXAnchor().constraintEqualToAnchor(&root.centerXAnchor()).setActive(true);
        empty.centerYAnchor().constraintEqualToAnchor(&root.centerYAnchor()).setActive(true);
        let object: &AnyObject = &source;
        unsafe {
            table.setTarget(Some(object));
            table.setDoubleAction(Some(sel!(open:)));
        }
        let controller = NSViewController::new(mtm);
        controller.setView(&root);
        return Self {
            controller,
            source,
            collection,
            scroll,
            list,
            empty,
            actions: context::ContextActions::new(mtm),
        };
    }

    pub fn actions(&self) -> &objc2::runtime::AnyObject {
        return &self.actions;
    }

    fn pick(&self, index: usize) -> Option<ItemId> {
        let position = isize::try_from(index).ok()?;
        let id = self.source.id(position)?;
        let ivars = self.source.ivars();
        ivars.syncing.set(true);
        let path = NSIndexPath::indexPathForItem_inSection(position, 0);
        ivars.view.setSelectionIndexPaths(&NSSet::from_retained_slice(&[path]));
        ivars.table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(index), false);
        ivars.syncing.set(false);
        return Some(id);
    }

    pub fn select(&self, id: Option<ItemId>, reveal: bool) {
        let ivars = self.source.ivars();
        let position = id.and_then(|id| ivars.ids.borrow().iter().position(|other| *other == id));
        ivars.syncing.set(true);
        if let Some((row, item)) =
            position.and_then(|position| isize::try_from(position).ok().map(|item| (position, item)))
        {
            let path = NSIndexPath::indexPathForItem_inSection(item, 0);
            ivars.view.setSelectionIndexPaths(&NSSet::from_slice(&[&*path]));
            ivars.table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row), false);
            if reveal {
                if let Some(attributes) = ivars
                    .view
                    .collectionViewLayout()
                    .and_then(|layout| layout.layoutAttributesForItemAtIndexPath(&path))
                {
                    ivars.view.scrollRectToVisible(attributes.frame());
                }
                ivars.table.scrollRowToVisible(item);
            }
        } else {
            ivars.view.setSelectionIndexPaths(&NSSet::new());
            ivars.table.selectRowIndexes_byExtendingSelection(&NSIndexSet::new(), false);
        }
        ivars.syncing.set(false);
    }

    fn update_progress(&self) {
        let ivars = self.source.ivars();
        let cells = ivars.cells.borrow();
        for item in &ivars.view.visibleItems() {
            let progress = ivars
                .view
                .indexPathForItem(&item)
                .and_then(|path| usize::try_from(path.item()).ok())
                .and_then(|index| cells.get(index))
                .and_then(|cell| cell.progress);
            if let Ok(tile) = item.downcast::<Tile>() {
                tile.show_progress(progress);
            }
        }
        let rows = ivars.table.rowsInRect(ivars.table.visibleRect());
        for row in rows.location..rows.location + rows.length {
            let progress = cells.get(row).and_then(|cell| cell.progress);
            let bar = isize::try_from(row)
                .ok()
                .and_then(|row| ivars.table.viewAtColumn_row_makeIfNecessary(0, row, false))
                .and_then(|view| view.subviews().iter().find_map(|sub| sub.downcast::<NSProgressIndicator>().ok()));
            if let (Some(bar), Some(progress)) = (bar, progress) {
                bar.setDoubleValue(progress);
            }
        }
    }

    pub fn responder(&self, layout: Layout) -> Retained<NSView> {
        let ivars = self.source.ivars();
        if layout == Layout::List {
            return Retained::into_super(Retained::into_super(ivars.table.clone()));
        }
        return Retained::into_super(ivars.view.clone());
    }

    pub fn set_layout(&self, layout: Layout) {
        self.scroll.setHidden(layout == Layout::List);
        self.list.setHidden(layout != Layout::List);
        let minimum = if layout == Layout::Small { SMALL } else { LARGE };
        let (text, _) = self.collection.ivars().get();
        self.collection.ivars().set((text, minimum));
        self.collection.resize(self.collection.frame().size.width);
    }

    pub fn reload(&self, library: &Library, filter: &Filter, selected: Option<ItemId>) {
        let visible = filter.visible(library);
        let cells: Vec<Cell> = visible
            .iter()
            .map(|index| {
                let item = &library.items[*index];
                return Cell {
                    title: item.title.clone(),
                    meta: meta(item),
                    kind: tr(inspector::kind_name(item.kind)),
                    length: if item.duration > 0.0 { duration(item.duration) } else { DASH.to_string() },
                    resolution: if item.width > 0 {
                        format!("{} × {}", item.width, item.height)
                    } else {
                        DASH.to_string()
                    },
                    size: i18n::bytes(i64::try_from(item.bytes).unwrap_or(i64::MAX)),
                    added: i18n::date(item.added),
                    progress: item.progress,
                    image: item.image.clone(),
                    markers: markers(library, *index, filter.scope)
                        .into_iter()
                        .map(|marker| Mark {
                            icon: marker.icon,
                            short: marker.short,
                            tip: marker.tip,
                            active: marker.active,
                        })
                        .collect(),
                };
            })
            .collect();
        self.empty.setHidden(!visible.is_empty());
        let ids: Vec<ItemId> = visible.iter().map(|index| library.items[*index].id).collect();
        let position = selected.and_then(|id| ids.iter().position(|other| *other == id));
        let ivars = self.source.ivars();
        let in_place =
            *ivars.ids.borrow() == ids && ivars.cells.borrow().iter().zip(&cells).all(|(old, new)| old.same_shape(new));
        *ivars.cells.borrow_mut() = cells;
        *ivars.ids.borrow_mut() = ids;
        if in_place {
            self.update_progress();
            return;
        }
        ivars.view.reloadData();
        ivars.syncing.set(true);
        ivars.table.reloadData();
        if let Some(position) = position.and_then(|position| isize::try_from(position).ok()) {
            let path = NSIndexPath::indexPathForItem_inSection(position, 0);
            ivars.view.setSelectionIndexPaths(&NSSet::from_retained_slice(&[path]));
            if let Ok(row) = usize::try_from(position) {
                ivars.table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row), false);
            }
        }
        ivars.syncing.set(false);
    }
}

fn list_table(mtm: MainThreadMarker) -> Retained<NSTableView> {
    let table: Retained<List> = unsafe { msg_send![List::alloc(mtm), init] };
    let table: Retained<NSTableView> = Retained::into_super(table);
    table.setStyle(NSTableViewStyle::Inset);
    table.setRowHeight(ROW_HEIGHT);
    table.setUsesAlternatingRowBackgroundColors(true);
    table.setColumnAutoresizingStyle(NSTableViewColumnAutoresizingStyle::FirstColumnOnlyAutoresizingStyle);
    let name = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &ns(NAME));
    name.setTitle(&ns(&tr("Name")));
    name.setMinWidth(160.0);
    name.setWidth(260.0);
    table.addTableColumn(&name);
    for (identifier, title, width) in COLUMNS {
        let column = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &ns(identifier));
        column.setTitle(&ns(&tr(title)));
        column.setWidth(width);
        column.setMinWidth(50.0);
        table.addTableColumn(&column);
    }
    return table;
}

fn list_cell(mtm: MainThreadMarker, cell: &Cell, identifier: &str) -> Retained<NSView> {
    let view: Retained<NSTableCellView> = if identifier == NAME {
        let named: Retained<NameCell> = unsafe { msg_send![NameCell::alloc(mtm), init] };
        Retained::into_super(named)
    } else {
        NSTableCellView::new(mtm)
    };
    let secondary = NSColor::secondaryLabelColor();
    let font = NSFont::systemFontOfSize(NSFont::systemFontSize());
    let text = |value: &str, color: &NSColor| {
        let text = label(mtm, &font, color);
        text.setStringValue(&ns(value));
        text.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        view.addSubview(&text);
        return text;
    };
    if identifier != NAME {
        let value = match identifier {
            "kind" => &cell.kind,
            "length" => &cell.length,
            "resolution" => &cell.resolution,
            "size" => &cell.size,
            _ => &cell.added,
        };
        let field = text(value, &secondary);
        field.leadingAnchor().constraintEqualToAnchor(&view.leadingAnchor()).setActive(true);
        field.trailingAnchor().constraintLessThanOrEqualToAnchor(&view.trailingAnchor()).setActive(true);
        field.centerYAnchor().constraintEqualToAnchor(&view.centerYAnchor()).setActive(true);
        unsafe { view.setTextField(Some(&field)) };
        return Retained::into_super(view);
    }
    let thumb: Retained<Thumb> = unsafe { msg_send![Thumb::alloc(mtm), init] };
    thumb.setWantsLayer(true);
    if let Some(host) = thumb.layer() {
        host.setCornerRadius(ROW_RADIUS);
        host.setMasksToBounds(true);
        host.setBackgroundColor(Some(&NSColor::quaternaryLabelColor().CGColor()));
        let picture = CALayer::new();
        picture.setContentsGravity(unsafe { kCAGravityResizeAspectFill });
        unsafe { picture.setContents(cell.image.as_deref().map(|image| image as &objc2::runtime::AnyObject)) };
        host.addSublayer(&picture);
    }
    thumb.setTranslatesAutoresizingMaskIntoConstraints(false);
    view.addSubview(&thumb);
    let title = text(&cell.title, &NSColor::labelColor());
    title.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
    let marks = NSStackView::new(mtm);
    marks.setSpacing(4.0);
    marks.setTranslatesAutoresizingMaskIntoConstraints(false);
    for view in cell.markers.iter().filter_map(|mark| mark_view(mtm, mark)) {
        marks.addArrangedSubview(&view);
    }
    view.addSubview(&marks);
    let bar = cell.progress.map(|value| {
        let bar = progress_bar(mtm);
        bar.setDoubleValue(value);
        view.addSubview(&bar);
        bar.trailingAnchor().constraintEqualToAnchor_constant(&marks.leadingAnchor(), -6.0).setActive(true);
        bar.centerYAnchor().constraintEqualToAnchor(&view.centerYAnchor()).setActive(true);
        bar.widthAnchor().constraintEqualToConstant(BAR_WIDTH).setActive(true);
        return bar;
    });
    thumb.leadingAnchor().constraintEqualToAnchor(&view.leadingAnchor()).setActive(true);
    thumb.centerYAnchor().constraintEqualToAnchor(&view.centerYAnchor()).setActive(true);
    thumb.widthAnchor().constraintEqualToConstant(ROW_THUMB.width).setActive(true);
    thumb.heightAnchor().constraintEqualToConstant(ROW_THUMB.height).setActive(true);
    title.leadingAnchor().constraintEqualToAnchor_constant(&thumb.trailingAnchor(), 8.0).setActive(true);
    title.centerYAnchor().constraintEqualToAnchor(&view.centerYAnchor()).setActive(true);
    match &bar {
        Some(bar) => title.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&bar.leadingAnchor(), -6.0),
        None => title.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&marks.leadingAnchor(), -6.0),
    }
    .setActive(true);
    marks.trailingAnchor().constraintEqualToAnchor(&view.trailingAnchor()).setActive(true);
    marks.centerYAnchor().constraintEqualToAnchor(&view.centerYAnchor()).setActive(true);
    unsafe { view.setTextField(Some(&title)) };
    return Retained::into_super(view);
}
