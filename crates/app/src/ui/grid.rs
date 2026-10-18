use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_app_kit::{
    NSBezierPath, NSCompositingOperation, NSDraggingImageComponent, NSDraggingImageComponentIconKey, NSImage,
};
use objc2_core_foundation::{CGPoint, CGRect};
use objc2_app_kit::{
    NSDragOperation, NSDraggingDestination, NSDraggingInfo, NSEvent, NSMenu, NSPasteboardWriting,
};
use objc2_app_kit::{
    NSTableCellView, NSTableColumn, NSTableView, NSTableViewColumnAutoresizingStyle, NSTableViewDataSource,
    NSTableViewDelegate, NSTableViewStyle, NSControlTextEditingDelegate,
};
use objc2_app_kit::{
    NSCollectionView, NSCollectionViewDataSource, NSCollectionViewDelegate,
    NSCollectionViewFlowLayout, NSCollectionViewItem, NSColor, NSFont, NSImageView,
    NSLayoutConstraintOrientation, NSLineBreakMode, NSResponder, NSScrollView, NSStackView, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView, NSViewController,
};
use objc2_core_foundation::{CGFloat, CGSize};
use objc2_app_kit::NSIndexPathNSCollectionViewAdditions;
use objc2_foundation::{NSCopying, NSArray, NSEdgeInsets, NSIndexPath, NSIndexSet, NSNotification, NSObject, NSObjectProtocol, NSSet};
use objc2_quartz_core::{CALayer, CATransaction, kCAGravityResizeAspectFill};

use crate::i18n::{self, tr};
use crate::model::{ItemId, ItemKind};
use crate::ui::state::{Filter, Layout, duration, markers};
use crate::ui::view::{Item, Library};
use crate::ui::{context, folders, inspector, ns, update};

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
const COLUMNS: [(&str, &str, CGFloat); 5] = [
    ("kind", i18n::key("Kind"), 70.0),
    ("length", i18n::key("Duration"), 70.0),
    ("resolution", i18n::key("Resolution"), 100.0),
    ("size", i18n::key("Size"), 80.0),
    ("added", i18n::key("Date Added"), 100.0),
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
    image: Option<Retained<objc2_app_kit::NSImage>>,
    markers: Vec<(&'static str, String, bool)>,
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
        let from = CGRect::new(
            CGPoint::new((source.width - crop.width) / 2.0, (source.height - crop.height) / 2.0),
            crop,
        );
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
                for layer in layers.iter() {
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

fn title_font() -> Retained<NSFont> {
    return NSFont::systemFontOfSize(NSFont::systemFontSize());
}

fn meta_font() -> Retained<NSFont> {
    return NSFont::systemFontOfSize(NSFont::smallSystemFontSize());
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
        for view in [&*thumb as &NSView, &title, &meta, &marks] {
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
            let views: Vec<Retained<NSImageView>> = cell
                .markers
                .iter()
                .filter_map(|(icon, tip, active)| {
                    let image = folders::symbol(icon)?;
                    let view = NSImageView::imageViewWithImage(&image, mtm);
                    let tint = if *active { NSColor::controlAccentColor() } else { NSColor::secondaryLabelColor() };
                    view.setContentTintColor(Some(&tint));
                    view.setToolTip(Some(&ns(tip)));
                    return Some(view);
                })
                .collect();
            let refs: Vec<&NSView> = views.iter().map(|view| view.as_ref()).collect();
            marks.setViews_inGravity(&NSArray::from_slice(&refs), objc2_app_kit::NSStackViewGravity::Trailing);
        }
        self.paint();
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
        if let Some(layout) = self.collectionViewLayout().and_then(|layout| layout.downcast::<NSCollectionViewFlowLayout>().ok())
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
        root.registerForDraggedTypes(&NSArray::from_retained_slice(&[unsafe { objc2_app_kit::NSPasteboardTypeFileURL }.copy()]));
        for pane in [&scroll, &list] {
            pane.setTranslatesAutoresizingMaskIntoConstraints(false);
            root.addSubview(pane);
            pane.leadingAnchor().constraintEqualToAnchor(&root.leadingAnchor()).setActive(true);
            pane.trailingAnchor().constraintEqualToAnchor(&root.trailingAnchor()).setActive(true);
            pane.topAnchor().constraintEqualToAnchor(&root.topAnchor()).setActive(true);
            pane.bottomAnchor().constraintEqualToAnchor(&root.bottomAnchor()).setActive(true);
        }
        let controller = NSViewController::new(mtm);
        controller.setView(&root);
        return Self { controller, source, collection, scroll, list, actions: context::ContextActions::new(mtm) };
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
                    image: item.image.clone(),
                    markers: markers(library, *index, filter.scope)
                        .into_iter()
                        .map(|marker| (marker.icon, marker.tip, marker.active))
                        .collect(),
                };
            })
            .collect();
        let ids: Vec<ItemId> = visible.iter().map(|index| library.items[*index].id).collect();
        let position = selected.and_then(|id| ids.iter().position(|other| *other == id));
        let ivars = self.source.ivars();
        *ivars.cells.borrow_mut() = cells;
        *ivars.ids.borrow_mut() = ids;
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
    for (icon, tip, active) in &cell.markers {
        if let Some(image) = folders::symbol(icon) {
            let glyph = NSImageView::imageViewWithImage(&image, mtm);
            let tint = if *active { NSColor::controlAccentColor() } else { NSColor::secondaryLabelColor() };
            glyph.setContentTintColor(Some(&tint));
            glyph.setToolTip(Some(&ns(tip)));
            marks.addArrangedSubview(&glyph);
        }
    }
    view.addSubview(&marks);
    thumb.leadingAnchor().constraintEqualToAnchor(&view.leadingAnchor()).setActive(true);
    thumb.centerYAnchor().constraintEqualToAnchor(&view.centerYAnchor()).setActive(true);
    thumb.widthAnchor().constraintEqualToConstant(ROW_THUMB.width).setActive(true);
    thumb.heightAnchor().constraintEqualToConstant(ROW_THUMB.height).setActive(true);
    title.leadingAnchor().constraintEqualToAnchor_constant(&thumb.trailingAnchor(), 8.0).setActive(true);
    title.centerYAnchor().constraintEqualToAnchor(&view.centerYAnchor()).setActive(true);
    title.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&marks.leadingAnchor(), -6.0).setActive(true);
    marks.trailingAnchor().constraintEqualToAnchor(&view.trailingAnchor()).setActive(true);
    marks.centerYAnchor().constraintEqualToAnchor(&view.centerYAnchor()).setActive(true);
    unsafe { view.setTextField(Some(&title)) };
    return Retained::into_super(view);
}
