use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBackgroundStyle, NSColor, NSControl, NSControlTextEditingDelegate, NSDragOperation, NSDraggingInfo, NSEvent,
    NSEventModifierFlags, NSImage, NSImageView, NSMenu, NSMenuItem, NSPasteboardTypeFileURL, NSScrollView,
    NSTableCellView, NSTableColumn, NSTableColumnResizingOptions, NSTableView, NSTableViewColumnAutoresizingStyle,
    NSTableViewDataSource, NSTableViewDelegate, NSTableViewDropOperation, NSTableViewRowSizeStyle, NSTableViewStyle,
    NSTextField, NSTextFieldDelegate, NSTextView, NSView,
};
use objc2_core_foundation::CGRect;
use objc2_foundation::{
    NSArray, NSCopying, NSIndexSet, NSInteger, NSNotification, NSNumber, NSObject, NSObjectProtocol, NSString, NSURL,
};
use proto::Request;
use uuid::Uuid;

use crate::i18n::{key, tr};
use crate::style::Key;
use crate::ui::metrics::{self, Text};
use crate::ui::state::{
    Filter, TAG_DELETE_FOLDER, TAG_HIDE_FOLDER, TAG_LOCK_FOLDER, TAG_NEW_FOLDER, TAG_RENAME_FOLDER, TAG_STYLE_FOLDER,
    privacy_title,
};
use crate::ui::theme::{self, Theme};
use crate::ui::view::Library;
use crate::ui::widgets::{self, Slot, ns};
use crate::ui::{UI, folders, submit};

pub const ITEM_TYPE: &str = "app.livewall.item";
pub const WIDTH: f64 = 220.0;
const MIN_WIDTH: f64 = 180.0;
const MAX_WIDTH: f64 = 320.0;
const BAR_HEIGHT: f64 = metrics::BUTTON_HEIGHT + metrics::SPACE_8;
const RETURN: u16 = 36;
const ENTER: u16 = 76;
const DELETE: u16 = 51;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Row {
    All,
    Header,
    Folder(usize),
}

#[derive(Clone, PartialEq)]
struct Entry {
    row: Row,
    id: Option<Uuid>,
    title: String,
    icon: Option<String>,
    color: Option<String>,
    count: usize,
    locked: bool,
    sealed: bool,
    unlocked: bool,
}

#[derive(Clone)]
struct Ink {
    icon: Retained<NSColor>,
    count: Retained<NSColor>,
    header: Retained<NSColor>,
}

impl Ink {
    fn new(theme: &Theme) -> Self {
        return Self {
            icon: theme.color(Key::SidebarIcon),
            count: theme.color(Key::SidebarCount),
            header: theme.color(Key::TextSecondary),
        };
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MoveTo {
    Root,
    Folder(usize),
}

pub struct SourceIvars {
    entries: RefCell<Vec<Entry>>,
    images: RefCell<Vec<Option<Retained<NSImage>>>>,
    ink: Ink,
    table: RefCell<Option<Retained<SidebarTable>>>,
    syncing: Cell<bool>,
    stale: Cell<bool>,
}

pub struct CellIvars {
    accent: Option<Retained<NSColor>>,
}

define_class!(
    #[unsafe(super(NSTableCellView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = CellIvars]
    pub struct FolderCell;

    impl FolderCell {
        #[unsafe(method(setBackgroundStyle:))]
        fn set_background_style(&self, style: NSBackgroundStyle) {
            let () = unsafe { msg_send![super(self), setBackgroundStyle: style] };
            self.tint(style);
        }
    }
);

impl FolderCell {
    fn new(mtm: MainThreadMarker, accent: Option<Retained<NSColor>>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(CellIvars { accent });
        return unsafe { msg_send![super(this), initWithFrame: CGRect::ZERO] };
    }

    fn tint(&self, style: NSBackgroundStyle) {
        let Some(icon) = (unsafe { self.imageView() }) else {
            return;
        };
        if !icon.image().is_some_and(|image| image.isTemplate()) {
            return;
        }
        let accent = self.ivars().accent.as_deref().filter(|_| style != NSBackgroundStyle::Emphasized);
        icon.setContentTintColor(accent);
    }
}

define_class!(
    #[unsafe(super(NSTableView))]
    #[thread_kind = MainThreadOnly]
    pub struct SidebarTable;

    impl SidebarTable {
        #[unsafe(method_id(menuForEvent:))]
        fn menu_for_event(&self, event: &NSEvent) -> Option<Retained<NSMenu>> {
            self.context(event)
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if !self.key(event) {
                let () = unsafe { msg_send![super(self), keyDown: event] };
            }
        }
    }
);

impl SidebarTable {
    fn context(&self, event: &NSEvent) -> Option<Retained<NSMenu>> {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let row = usize::try_from(self.rowAtPoint(point)).ok();
        return context_menu(self.mtm(), row.and_then(row_kind));
    }

    fn key(&self, event: &NSEvent) -> bool {
        let Some(Row::Folder(folder)) = usize::try_from(self.selectedRow()).ok().and_then(row_kind) else {
            return false;
        };
        let command = event.modifierFlags().contains(NSEventModifierFlags::Command);
        match event.keyCode() {
            RETURN | ENTER => crate::ui::rename_folder(folder),
            DELETE if command => crate::ui::actions::delete_folder(folder),
            _ => return false,
        }
        return true;
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = SourceIvars]
    pub struct SidebarSource;

    unsafe impl NSObjectProtocol for SidebarSource {}

    impl SidebarSource {
        #[unsafe(method(renameClicked:))]
        fn rename_clicked(&self, sender: &NSTableView) {
            self.clicked(sender.clickedRow());
        }
    }

    unsafe impl NSTableViewDataSource for SidebarSource {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn rows(&self, _table: &NSTableView) -> NSInteger {
            NSInteger::try_from(self.ivars().entries.borrow().len()).unwrap_or(0)
        }

        #[unsafe(method(tableView:validateDrop:proposedRow:proposedDropOperation:))]
        fn validate_drop(
            &self,
            table: &NSTableView,
            info: &ProtocolObject<dyn NSDraggingInfo>,
            row: NSInteger,
            operation: NSTableViewDropOperation,
        ) -> NSDragOperation {
            self.validate(table, info, row, operation)
        }

        #[unsafe(method(tableView:acceptDrop:row:dropOperation:))]
        fn accept_drop(
            &self,
            _table: &NSTableView,
            info: &ProtocolObject<dyn NSDraggingInfo>,
            row: NSInteger,
            _operation: NSTableViewDropOperation,
        ) -> bool {
            self.accept(info, row)
        }
    }

    unsafe impl NSControlTextEditingDelegate for SidebarSource {
        #[unsafe(method(controlTextDidEndEditing:))]
        fn text_did_end_editing(&self, notification: &NSNotification) {
            self.ended(notification);
        }

        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn do_command(&self, control: &NSControl, _text: &NSTextView, command: Sel) -> bool {
            self.command(control, command)
        }
    }

    unsafe impl NSTextFieldDelegate for SidebarSource {}

    unsafe impl NSTableViewDelegate for SidebarSource {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn view_for(
            &self,
            _table: &NSTableView,
            _column: Option<&NSTableColumn>,
            row: NSInteger,
        ) -> Option<Retained<NSView>> {
            self.view_at(row)
        }

        #[unsafe(method(tableView:isGroupRow:))]
        fn is_group(&self, _table: &NSTableView, row: NSInteger) -> bool {
            self.kind_at(row) == Some(Row::Header)
        }

        #[unsafe(method(tableView:shouldSelectRow:))]
        fn should_select(&self, _table: &NSTableView, row: NSInteger) -> bool {
            self.kind_at(row).is_some_and(|kind| kind != Row::Header)
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_did_change(&self, _notification: &NSNotification) {
            self.selected();
        }
    }
);

impl SidebarSource {
    fn kind_at(&self, row: NSInteger) -> Option<Row> {
        return usize::try_from(row).ok().and_then(|row| self.entry(row)).map(|entry| entry.row);
    }

    fn table(&self) -> Option<Retained<SidebarTable>> {
        return self.ivars().table.borrow().clone();
    }

    fn clicked(&self, row: NSInteger) {
        let target = usize::try_from(row).ok().and_then(|row| self.target(row));
        if let Some(folder) = target {
            crate::ui::rename_folder(folder);
        }
    }

    fn validate(
        &self,
        table: &NSTableView,
        info: &ProtocolObject<dyn NSDraggingInfo>,
        row: NSInteger,
        operation: NSTableViewDropOperation,
    ) -> NSDragOperation {
        let Some(index) = usize::try_from(row).ok() else {
            return NSDragOperation::None;
        };
        let own = !dragged_items(info).is_empty();
        let accepted = if own {
            self.move_target(index).is_some()
        } else {
            self.target(index).is_some() && !droppable(info).is_empty()
        };
        if !accepted {
            return NSDragOperation::None;
        }
        if operation != NSTableViewDropOperation::On {
            table.setDropRow_dropOperation(row, NSTableViewDropOperation::On);
        }
        return if own { NSDragOperation::Move } else { NSDragOperation::Copy };
    }

    fn accept(&self, info: &ProtocolObject<dyn NSDraggingInfo>, row: NSInteger) -> bool {
        let Some(row) = usize::try_from(row).ok() else {
            return false;
        };
        let ids = dragged_items(info);
        if !ids.is_empty() {
            let Some(target) = self.move_target(row) else {
                return false;
            };
            let folder = match target {
                MoveTo::Root => None,
                MoveTo::Folder(folder) => Some(folder),
            };
            crate::ui::actions::move_items(ids, folder);
            return true;
        }
        let Some(folder) = self.target(row) else {
            return false;
        };
        let files = droppable(info);
        if files.is_empty() {
            return false;
        }
        crate::ui::actions::import(&files, Some(folder));
        return true;
    }

    fn ended(&self, notification: &NSNotification) {
        if let Some(field) = notification.object().and_then(|object| object.downcast::<NSTextField>().ok()) {
            self.finish_rename(&field);
        }
    }

    fn command(&self, control: &NSControl, command: Sel) -> bool {
        if command != sel!(cancelOperation:) {
            return false;
        }
        let Some(table) = self.table() else {
            return true;
        };
        if let Some(entry) = usize::try_from(table.rowForView(control)).ok().and_then(|row| self.entry(row)) {
            control.setStringValue(&ns(&entry.title));
        }
        if let Some(window) = table.window() {
            window.makeFirstResponder(Some(&table));
        }
        return true;
    }

    fn view_at(&self, row: NSInteger) -> Option<Retained<NSView>> {
        let row = usize::try_from(row).ok()?;
        let entry = self.entry(row)?;
        let image = self.ivars().images.borrow().get(row).cloned().flatten();
        return Some(self.cell(&entry, image.as_deref()));
    }

    fn selected(&self) {
        if self.ivars().syncing.get() {
            return;
        }
        let Some(row) = self.table().map(|table| table.selectedRow()) else {
            return;
        };
        match self.kind_at(row) {
            Some(Row::All) => crate::ui::actions::open_folder(None),
            Some(Row::Folder(folder)) => crate::ui::actions::open_folder(Some(folder)),
            _ => {}
        }
    }
}

impl SidebarSource {
    fn entry(&self, row: usize) -> Option<Entry> {
        return self.ivars().entries.borrow().get(row).cloned();
    }

    fn target(&self, row: usize) -> Option<usize> {
        let entry = self.entry(row)?;
        return match entry.row {
            Row::Folder(folder) if !entry.sealed => Some(folder),
            _ => None,
        };
    }

    fn move_target(&self, row: usize) -> Option<MoveTo> {
        let entry = self.entry(row)?;
        return match entry.row {
            Row::Folder(folder) if !entry.sealed => Some(MoveTo::Folder(folder)),
            Row::All => Some(MoveTo::Root),
            _ => None,
        };
    }

    fn finish_rename(&self, field: &NSTextField) {
        let Some(table) = self.table() else {
            return;
        };
        let row = table.rowForView(field);
        if let Some(entry) = usize::try_from(row).ok().and_then(|row| self.entry(row)) {
            let name = field.stringValue().to_string().trim().to_string();
            if let Some(id) = entry.id.filter(|_| !name.is_empty() && name != entry.title) {
                let request = Request::FolderRename { id: id.to_string(), name };
                submit(move |app| Some(app.dispatch(&request)));
            } else {
                field.setStringValue(&ns(&entry.title));
            }
        }
        if self.ivars().stale.replace(false) {
            crate::ui::changed();
        }
    }

    fn cell(&self, entry: &Entry, image: Option<&NSImage>) -> Retained<NSView> {
        let mtm = self.mtm();
        if entry.row == Row::Header {
            let cell = NSTableCellView::new(mtm);
            let title = widgets::label(mtm, &entry.title, Text::Section, &self.ivars().ink.header);
            cell.addSubview(&title);
            title.setTranslatesAutoresizingMaskIntoConstraints(false);
            title
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&cell.leadingAnchor(), metrics::SPACE_2)
                .setActive(true);
            title.centerYAnchor().constraintEqualToAnchor(&cell.centerYAnchor()).setActive(true);
            unsafe { cell.setTextField(Some(&title)) };
            return Retained::into_super(cell);
        }
        let ink = self.ivars().ink.clone();
        let cell = FolderCell::new(mtm, Some(ink.icon.clone()));
        theme::register(&cell, Key::SidebarIcon, Slot::Tint);
        let icon = NSImageView::new(mtm);
        icon.setImage(image);
        let title = NSTextField::labelWithString(&ns(&entry.title), mtm);
        title.setFont(Some(&metrics::font(Text::Body)));
        title.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByTruncatingTail);
        title.setMaximumNumberOfLines(1);
        title.setContentCompressionResistancePriority_forOrientation(
            250.0,
            objc2_app_kit::NSLayoutConstraintOrientation::Horizontal,
        );
        let editable = matches!(entry.row, Row::Folder(_)) && !entry.sealed;
        title.setEditable(editable);
        if editable {
            unsafe { title.setDelegate(Some(ProtocolObject::from_ref(self))) };
        }
        let shown = if entry.count > 0 && !entry.sealed { entry.count.to_string() } else { String::new() };
        let count = widgets::label(mtm, &shown, Text::Subheadline, &ink.count);
        count.setFont(Some(&metrics::digits(Text::Subheadline)));
        count.setContentCompressionResistancePriority_forOrientation(
            750.0,
            objc2_app_kit::NSLayoutConstraintOrientation::Horizontal,
        );
        for view in [&*icon as &NSView, &title, &count] {
            cell.addSubview(view);
            view.setTranslatesAutoresizingMaskIntoConstraints(false);
            view.centerYAnchor().constraintEqualToAnchor(&cell.centerYAnchor()).setActive(true);
        }
        icon.leadingAnchor().constraintEqualToAnchor_constant(&cell.leadingAnchor(), metrics::SPACE_4).setActive(true);
        icon.widthAnchor().constraintEqualToConstant(metrics::ICON_ROW).setActive(true);
        icon.heightAnchor().constraintEqualToConstant(metrics::ICON_ROW).setActive(true);
        title
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&icon.trailingAnchor(), metrics::SPACE_8)
            .setActive(true);
        count
            .trailingAnchor()
            .constraintEqualToAnchor_constant(&cell.trailingAnchor(), -metrics::SPACE_8)
            .setActive(true);
        title
            .trailingAnchor()
            .constraintLessThanOrEqualToAnchor_constant(&count.leadingAnchor(), -metrics::SPACE_8)
            .setActive(true);
        unsafe {
            cell.setImageView(Some(&icon));
            cell.setTextField(Some(&title));
        }
        cell.tint(cell.backgroundStyle());
        if entry.sealed {
            cell.setToolTip(Some(&ns(&tr("Unlock private folders to open this folder"))));
        } else {
            cell.setToolTip(Some(&ns(&entry.title)));
        }
        return Retained::into_super(Retained::into_super(cell));
    }
}

pub struct Sidebar {
    pub view: Retained<NSView>,
    table: Retained<SidebarTable>,
    source: Retained<SidebarSource>,
    privacy: Retained<objc2_app_kit::NSButton>,
}

impl Sidebar {
    pub fn new(mtm: MainThreadMarker, theme: &Theme, target: &AnyObject) -> Self {
        let source = SidebarSource::alloc(mtm).set_ivars(SourceIvars {
            entries: RefCell::new(Vec::new()),
            images: RefCell::new(Vec::new()),
            ink: Ink::new(theme),
            table: RefCell::new(None),
            syncing: Cell::new(false),
            stale: Cell::new(false),
        });
        let source: Retained<SidebarSource> = unsafe { msg_send![super(source), init] };
        let table: Retained<SidebarTable> = unsafe { msg_send![SidebarTable::alloc(mtm), initWithFrame: CGRect::ZERO] };
        let column = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &ns("folder"));
        column.setEditable(true);
        column.setResizingMask(NSTableColumnResizingOptions::AutoresizingMask);
        table.addTableColumn(&column);
        table.setHeaderView(None);
        table.setStyle(NSTableViewStyle::SourceList);
        table.setRowSizeStyle(NSTableViewRowSizeStyle::Custom);
        table.setRowHeight(metrics::ROW_LIST);
        table.setColumnAutoresizingStyle(NSTableViewColumnAutoresizingStyle::FirstColumnOnlyAutoresizingStyle);
        table.setFloatsGroupRows(false);
        table.setAllowsEmptySelection(false);
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(&*source)));
            table.setDelegate(Some(ProtocolObject::from_ref(&*source)));
            table.setTarget(Some(&source));
            table.setDoubleAction(Some(sel!(renameClicked:)));
        }
        let types = [ns(ITEM_TYPE), unsafe { NSPasteboardTypeFileURL }.copy()];
        table.registerForDraggedTypes(&NSArray::from_retained_slice(&types));
        *source.ivars().table.borrow_mut() = Some(table.clone());

        let scroll = NSScrollView::new(mtm);
        scroll.setDocumentView(Some(&table));
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);

        let add = widgets::icon_button(mtm, "plus", sel!(folder:), target, Key::TextSecondary, theme);
        add.setTag(TAG_NEW_FOLDER);
        widgets::describe(&add, &tr("New Folder…"), &tr("New Folder…"));
        let privacy = widgets::icon_button(mtm, "lock", sel!(togglePrivacy:), target, Key::TextSecondary, theme);
        let gap = widgets::spacer(mtm);
        let bar = widgets::stack(mtm, &[&add, &gap, &privacy], false, metrics::SPACE_8);
        bar.setEdgeInsets(widgets::insets(0.0, metrics::SPACE_8, 0.0, metrics::SPACE_8));

        let view = NSView::new(mtm);
        view.addSubview(&scroll);
        view.addSubview(&bar);
        for child in [&*scroll as &NSView, &bar] {
            child.setTranslatesAutoresizingMaskIntoConstraints(false);
            child.leadingAnchor().constraintEqualToAnchor(&view.leadingAnchor()).setActive(true);
            child.trailingAnchor().constraintEqualToAnchor(&view.trailingAnchor()).setActive(true);
        }
        scroll.topAnchor().constraintEqualToAnchor(&view.topAnchor()).setActive(true);
        scroll.bottomAnchor().constraintEqualToAnchor(&bar.topAnchor()).setActive(true);
        bar.bottomAnchor().constraintEqualToAnchor(&view.bottomAnchor()).setActive(true);
        bar.heightAnchor().constraintEqualToConstant(BAR_HEIGHT).setActive(true);
        return Self { view, table, source, privacy };
    }

    pub fn min_width() -> f64 {
        return MIN_WIDTH;
    }

    pub fn max_width() -> f64 {
        return MAX_WIDTH;
    }

    pub fn sync(&self, library: &Library, filter: &Filter) {
        let entries = entries(library);
        let ivars = self.source.ivars();
        if *ivars.entries.borrow() != entries {
            if self.table.editedRow() >= 0 {
                ivars.stale.set(true);
            } else {
                let images = entries
                    .iter()
                    .map(|entry| match entry.row {
                        Row::All => folders::painted("photo.on.rectangle.angled", None, metrics::ICON_ROW),
                        Row::Header => None,
                        Row::Folder(folder) => folders::image(library, folder, metrics::ICON_ROW),
                    })
                    .collect();
                *ivars.images.borrow_mut() = images;
                *ivars.entries.borrow_mut() = entries;
                ivars.syncing.set(true);
                self.table.reloadData();
                ivars.syncing.set(false);
            }
        }
        self.select(filter.folder);
        let open = library.unlocked && library.has_locked;
        widgets::set_icon(&self.privacy, if open { "lock.open" } else { "lock" });
        let privacy = tr(privacy_title(library.unlocked));
        widgets::describe(&self.privacy, &privacy, &privacy);
    }

    fn row_of(&self, folder: Option<usize>) -> Option<usize> {
        let wanted = folder.map_or(Row::All, Row::Folder);
        return self.source.ivars().entries.borrow().iter().position(|entry| entry.row == wanted);
    }

    fn select(&self, folder: Option<usize>) {
        let Some(row) = self.row_of(folder).and_then(|row| NSInteger::try_from(row).ok()) else {
            return;
        };
        if self.table.selectedRow() == row {
            return;
        }
        let ivars = self.source.ivars();
        ivars.syncing.set(true);
        let rows = NSIndexSet::indexSetWithIndex(row.unsigned_abs());
        self.table.selectRowIndexes_byExtendingSelection(&rows, false);
        ivars.syncing.set(false);
    }

    pub fn rename(&self, folder: usize) {
        let Some(row) = self.row_of(Some(folder)).and_then(|row| NSInteger::try_from(row).ok()) else {
            return;
        };
        self.table.scrollRowToVisible(row);
        self.table.editColumn_row_withEvent_select(0, row, None, true);
    }

    pub fn anchor(&self, folder: usize) -> Option<(Retained<NSView>, CGRect)> {
        let row = self.row_of(Some(folder)).and_then(|row| NSInteger::try_from(row).ok())?;
        self.table.scrollRowToVisible(row);
        let view: Retained<NSView> =
            Retained::into_super(Retained::into_super(Retained::into_super(self.table.clone())));
        return Some((view, self.table.rectOfRow(row)));
    }
}

fn entries(library: &Library) -> Vec<Entry> {
    let base = |row: Row, title: String| Entry {
        row,
        id: None,
        title,
        icon: None,
        color: None,
        count: 0,
        locked: false,
        sealed: false,
        unlocked: library.unlocked,
    };
    let mut out = vec![Entry { count: library.items.len(), ..base(Row::All, tr("All Wallpapers")) }];
    if library.folders.is_empty() {
        return out;
    }
    out.push(base(Row::Header, tr("Folders")));
    for (folder, name) in library.folders.iter().enumerate() {
        out.push(Entry {
            id: library.folder_ids.get(folder).copied(),
            icon: library.icons.get(folder).cloned().flatten(),
            color: library.colors.get(folder).cloned().flatten(),
            count: library.counts.get(folder).copied().unwrap_or(0),
            locked: library.locks.get(folder).is_some_and(|lock| lock.locked),
            sealed: library.sealed(folder),
            ..base(Row::Folder(folder), name.clone())
        });
    }
    return out;
}

fn row_kind(row: usize) -> Option<Row> {
    return UI.with(|ui| {
        let ui = ui.try_borrow().ok()?;
        let ui = ui.as_ref()?;
        return ui.sidebar.source.ivars().entries.borrow().get(row).map(|entry| entry.row);
    });
}

pub fn dragged_items(info: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<String> {
    return items_of_type(info, &ns(ITEM_TYPE)).into_iter().map(|text| text.to_string()).collect();
}

pub fn dropped_files(info: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<String> {
    let kind = unsafe { NSPasteboardTypeFileURL };
    return items_of_type(info, kind)
        .into_iter()
        .filter_map(|text| NSURL::URLWithString(&text))
        .filter_map(|url| url.path())
        .map(|path| path.to_string())
        .collect();
}

pub fn droppable(info: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<String> {
    return dropped_files(info)
        .into_iter()
        .filter(|path| crate::ui::dialogs::media(std::path::Path::new(path)))
        .collect();
}

fn items_of_type(info: &ProtocolObject<dyn NSDraggingInfo>, kind: &NSString) -> Vec<Retained<NSString>> {
    let Some(items) = info.draggingPasteboard().pasteboardItems() else {
        return Vec::new();
    };
    return items.iter().filter_map(|item| item.stringForType(kind)).collect();
}

fn context_menu(mtm: MainThreadMarker, row: Option<Row>) -> Option<Retained<NSMenu>> {
    return UI.with_borrow(|ui| {
        let ui = ui.as_ref()?;
        let library = &ui.library;
        let target: &AnyObject = &ui.actions;
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        let action = |title: &str, tag: isize, folder: usize| {
            let item = widgets::menu_item(mtm, title, sel!(folderAction:), Some(target), tag, None);
            let index = NSNumber::new_usize(folder);
            unsafe { item.setRepresentedObject(Some(&index)) };
            return item;
        };
        if let Some(Row::Folder(folder)) = row {
            if library.sealed(folder) {
                menu.addItem(&widgets::menu_item(
                    mtm,
                    &tr(privacy_title(false)),
                    sel!(togglePrivacy:),
                    Some(target),
                    0,
                    None,
                ));
                return Some(menu);
            }
            let lock = library.locks.get(folder).copied()?;
            menu.addItem(&action(&tr("Icon & Color…"), TAG_STYLE_FOLDER, folder));
            menu.addItem(&action(&tr("Rename"), TAG_RENAME_FOLDER, folder));
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            let title = if lock.locked { key("Remove Lock") } else { key("Lock Folder") };
            menu.addItem(&action(&tr(title), TAG_LOCK_FOLDER, folder));
            let hide = action(&tr("Hide When Locked"), TAG_HIDE_FOLDER, folder);
            hide.setState(isize::from(lock.hidden));
            hide.setEnabled(lock.locked);
            menu.addItem(&hide);
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            menu.addItem(&action(&tr("Delete Folder…"), TAG_DELETE_FOLDER, folder));
            menu.addItem(&NSMenuItem::separatorItem(mtm));
        }
        menu.addItem(&widgets::menu_item(mtm, &tr("New Folder…"), sel!(folder:), Some(target), TAG_NEW_FOLDER, None));
        if library.has_locked {
            menu.addItem(&widgets::menu_item(
                mtm,
                &tr(privacy_title(library.unlocked)),
                sel!(togglePrivacy:),
                Some(target),
                0,
                None,
            ));
        }
        return Some(menu);
    });
}
