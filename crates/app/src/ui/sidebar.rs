use std::cell::{Cell, RefCell};

use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, Bool, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBezelStyle, NSButton, NSColor, NSControl, NSControlSize, NSControlTextEditingDelegate, NSEvent, NSFont, NSImage,
    NSImageScaling, NSImageView, NSLayoutConstraintOrientation, NSLineBreakMode, NSMenu, NSMenuDelegate, NSMenuItem,
    NSScrollView, NSStackView, NSStackViewDistribution, NSTableCellView, NSTableColumn, NSTableView,
    NSTableViewDataSource, NSTableViewDelegate, NSTableViewStyle, NSTextField, NSTextFieldDelegate, NSTextView,
    NSUserInterfaceLayoutOrientation, NSView, NSViewController,
};
use objc2_app_kit::{NSDragOperation, NSDraggingInfo, NSTableViewDropOperation};
use objc2_foundation::{NSArray, NSIndexSet, NSNotification, NSObject, NSObjectProtocol};
use proto::Request;
use uuid::Uuid;

use crate::i18n::tr;
use crate::ui::state::privacy_title;
use crate::ui::view::{FolderLock, Library};
use crate::ui::{UI, commit, context, dialogs, folders, ns, picker, submit, update};

const NEW_TAG: isize = -1;
const RETURN: u16 = 36;
const ENTER: u16 = 76;

#[derive(Clone)]
struct FolderRow {
    index: usize,
    name: String,
    count: Option<usize>,
    image: Option<Retained<NSImage>>,
    tint: Option<Retained<NSColor>>,
    locked: bool,
    hidden: bool,
    sealed: bool,
}

#[derive(Clone)]
enum Row {
    Header(String),
    All(usize),
    Folder(FolderRow),
}

struct SourceIvars {
    table: Retained<NSTableView>,
    rows: RefCell<Vec<Row>>,
    syncing: Cell<bool>,
    has_locked: Cell<bool>,
    unlocked: Cell<bool>,
}

#[derive(Clone, Copy)]
enum DropTarget {
    Unfiled,
    Folder(usize),
}

impl DropTarget {
    fn folder(self) -> Option<usize> {
        return match self {
            Self::Unfiled => None,
            Self::Folder(index) => Some(index),
        };
    }
}

impl SidebarSource {
    fn drop_target(&self, row: isize) -> Option<DropTarget> {
        return match self.row_at(row)? {
            Row::All(_) => Some(DropTarget::Unfiled),
            Row::Folder(folder) if !folder.sealed => Some(DropTarget::Folder(folder.index)),
            _ => None,
        };
    }
}

fn folder(tag: isize) -> Option<(Uuid, FolderLock)> {
    return UI.with_borrow(|ui| {
        let ui = ui.as_ref()?;
        let index = usize::try_from(tag).ok()?;
        return Some((*ui.library.folder_ids.get(index)?, *ui.library.locks.get(index)?));
    });
}

fn set_lock(tag: isize, locked: Option<bool>, toggle_hidden: bool) {
    let Some((id, lock)) = folder(tag) else {
        return;
    };
    let hidden = toggle_hidden.then_some(!lock.hidden);
    submit(move |app| Some(app.set_folder_lock(id, locked, hidden)));
}

struct TableIvars {
    source: RefCell<Option<Weak<SidebarSource>>>,
}

define_class!(
    #[unsafe(super(NSTableView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = TableIvars]
    struct SidebarTable;

    unsafe impl NSObjectProtocol for SidebarTable {}

    impl SidebarTable {
        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let source = self.ivars().source.borrow().as_ref().and_then(Weak::load);
            let handled = matches!(event.keyCode(), RETURN | ENTER)
                && source.is_some_and(|source| source.rename_selected());
            if !handled {
                let () = unsafe { msg_send![super(self), keyDown: event] };
            }
        }
    }
);

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = SourceIvars]
    struct SidebarSource;

    unsafe impl NSObjectProtocol for SidebarSource {}

    unsafe impl NSControlTextEditingDelegate for SidebarSource {
        #[unsafe(method(controlTextDidEndEditing:))]
        fn text_did_end_editing(&self, notification: &NSNotification) {
            if let Some(field) = notification.object().and_then(|object| object.downcast::<NSTextField>().ok()) {
                self.finish_rename(&field);
            }
        }

        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn do_command(&self, control: &NSControl, _text: &NSTextView, command: Sel) -> bool {
            let cancel = command == sel!(cancelOperation:);
            if cancel {
                self.cancel_rename(control);
            }
            cancel
        }
    }

    unsafe impl NSTextFieldDelegate for SidebarSource {}

    unsafe impl NSTableViewDataSource for SidebarSource {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn count(&self, _table: &NSTableView) -> isize {
            isize::try_from(self.ivars().rows.borrow().len()).unwrap_or(0)
        }

        #[unsafe(method(tableView:validateDrop:proposedRow:proposedDropOperation:))]
        fn validate_drop(
            &self,
            _table: &NSTableView,
            info: &ProtocolObject<dyn NSDraggingInfo>,
            row: isize,
            operation: NSTableViewDropOperation,
        ) -> NSDragOperation {
            if operation != NSTableViewDropOperation::On || self.drop_target(row).is_none() {
                NSDragOperation::None
            } else if !context::dragged_items(info).is_empty() {
                NSDragOperation::Move
            } else if !context::dragged_files(info).is_empty() {
                NSDragOperation::Copy
            } else {
                NSDragOperation::None
            }
        }

        #[unsafe(method(tableView:acceptDrop:row:dropOperation:))]
        fn accept_drop(
            &self,
            _table: &NSTableView,
            info: &ProtocolObject<dyn NSDraggingInfo>,
            row: isize,
            _operation: NSTableViewDropOperation,
        ) -> bool {
            match self.drop_target(row) {
                Some(target) => {
                    let folder = target.folder();
                    let items = context::dragged_items(info);
                    let files = if items.is_empty() { context::dragged_files(info) } else { Vec::new() };
                    let accepted = !items.is_empty() || !files.is_empty();
                    update(move |ui, _| {
                        if items.is_empty() {
                            context::import(ui, files, folder);
                        } else {
                            context::move_items(ui, &items, folder);
                        }
                    });
                    accepted
                }
                None => false,
            }
        }
    }

    unsafe impl NSTableViewDelegate for SidebarSource {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn view(&self, _table: &NSTableView, _column: Option<&NSTableColumn>, row: isize) -> Option<Retained<NSView>> {
            self.row_at(row).map(|row| cell(self.mtm(), &row))
        }

        #[unsafe(method(tableView:isGroupRow:))]
        fn is_group(&self, _table: &NSTableView, row: isize) -> Bool {
            Bool::new(matches!(self.row_at(row), Some(Row::Header(_))))
        }

        #[unsafe(method(tableView:shouldSelectRow:))]
        fn should_select(&self, _table: &NSTableView, row: isize) -> Bool {
            Bool::new(matches!(self.row_at(row), Some(Row::All(_) | Row::Folder(_))))
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection(&self, _notification: &NSNotification) {
            let ivars = self.ivars();
            if !ivars.syncing.get() {
                match self.row_at(ivars.table.selectedRow()) {
                    Some(Row::All(_)) => commit(|ui| ui.filter.folder = None),
                    Some(Row::Folder(row)) if row.sealed => {
                        crate::app::with(crate::app::App::request_unlock);
                        commit(|_| {});
                    }
                    Some(Row::Folder(row)) => commit(|ui| ui.filter.folder = Some(row.index)),
                    _ => {}
                }
            }
        }
    }

    unsafe impl NSMenuDelegate for SidebarSource {
        #[unsafe(method(menuNeedsUpdate:))]
        fn needs_update(&self, menu: &NSMenu) {
            let clicked = self.ivars().table.clickedRow();
            let index = match self.row_at(clicked) {
                Some(Row::Folder(row)) => Some(row.index),
                _ => None,
            };
            self.fill(menu, index);
        }
    }

    impl SidebarSource {
        #[unsafe(method(newFolder:))]
        fn new_folder(&self, _sender: Option<&AnyObject>) {
            dialogs::new_folder(self.mtm());
        }

        #[unsafe(method(renameFolder:))]
        fn rename_folder(&self, sender: &NSMenuItem) {
            if let Ok(index) = usize::try_from(sender.tag()) {
                self.begin_rename(index);
            }
        }

        #[unsafe(method(renameClicked:))]
        fn rename_clicked(&self, _sender: Option<&AnyObject>) {
            if let Some(row) = self.folder_at(self.ivars().table.clickedRow()) {
                self.begin_rename(row.index);
            }
        }

        #[unsafe(method(deleteFolder:))]
        fn delete_folder(&self, sender: &NSMenuItem) {
            if let Some((id, _)) = folder(sender.tag()) {
                dialogs::delete_folder(self.mtm(), id);
            }
        }

        #[unsafe(method(lockFolder:))]
        fn lock_folder(&self, sender: &NSMenuItem) {
            set_lock(sender.tag(), Some(true), false);
        }

        #[unsafe(method(unlockFolder:))]
        fn unlock_folder(&self, sender: &NSMenuItem) {
            set_lock(sender.tag(), Some(false), false);
        }

        #[unsafe(method(hideFolder:))]
        fn hide_folder(&self, sender: &NSMenuItem) {
            set_lock(sender.tag(), None, true);
        }

        #[unsafe(method(togglePrivacy:))]
        fn toggle_privacy(&self, _sender: Option<&AnyObject>) {
            crate::ui::toggle_privacy();
        }

        #[unsafe(method(styleFolder:))]
        fn style_folder(&self, sender: &NSMenuItem) {
            let ivars = self.ivars();
            let index = usize::try_from(sender.tag()).ok();
            let row = ivars
                .rows
                .borrow()
                .iter()
                .position(|row| matches!(row, Row::Folder(folder) if Some(folder.index) == index));
            if let (Some(index), Some(row)) = (index, row) {
                let rect = ivars.table.rectOfRow(isize::try_from(row).unwrap_or(0));
                picker::show(self.mtm(), index, &ivars.table, rect);
            }
        }
    }
);

impl SidebarSource {
    fn new(mtm: MainThreadMarker, table: Retained<NSTableView>) -> Retained<Self> {
        let source = Self::alloc(mtm).set_ivars(SourceIvars {
            table,
            rows: RefCell::new(Vec::new()),
            syncing: Cell::new(false),
            has_locked: Cell::new(false),
            unlocked: Cell::new(false),
        });
        return unsafe { msg_send![super(source), init] };
    }

    fn row_at(&self, row: isize) -> Option<Row> {
        let row = usize::try_from(row).ok()?;
        return self.ivars().rows.borrow().get(row).cloned();
    }

    fn folder_at(&self, row: isize) -> Option<FolderRow> {
        return match self.row_at(row)? {
            Row::Folder(folder) if !folder.sealed => Some(folder),
            _ => None,
        };
    }

    fn rename_selected(&self) -> bool {
        let Some(row) = self.folder_at(self.ivars().table.selectedRow()) else {
            return false;
        };
        self.begin_rename(row.index);
        return true;
    }

    fn begin_rename(&self, index: usize) {
        let table = &self.ivars().table;
        let position = self
            .ivars()
            .rows
            .borrow()
            .iter()
            .position(|row| matches!(row, Row::Folder(folder) if folder.index == index && !folder.sealed));
        let Some(row) = position.and_then(|position| isize::try_from(position).ok()) else {
            return;
        };
        table.scrollRowToVisible(row);
        let Some(cell) = table.viewAtColumn_row_makeIfNecessary(0, row, true) else {
            return;
        };
        let Ok(cell) = cell.downcast::<NSTableCellView>() else {
            return;
        };
        let Some(field) = (unsafe { cell.textField() }) else {
            return;
        };
        field.setEditable(true);
        unsafe { field.setDelegate(Some(ProtocolObject::from_ref(self))) };
        unsafe { field.selectText(None) };
    }

    fn cancel_rename(&self, control: &NSControl) {
        if let Some(row) = self.folder_at(self.ivars().table.rowForView(control)) {
            control.setStringValue(&ns(&row.name));
        }
        if let Some(window) = control.window() {
            window.makeFirstResponder(Some(&self.ivars().table));
        }
    }

    fn finish_rename(&self, field: &NSTextField) {
        field.setEditable(false);
        let Some(row) = self.folder_at(self.ivars().table.rowForView(field)) else {
            return;
        };
        let name = field.stringValue().to_string().trim().to_string();
        if name.is_empty() || name == row.name {
            field.setStringValue(&ns(&row.name));
            return;
        }
        let Some((id, _)) = folder(isize::try_from(row.index).unwrap_or(NEW_TAG)) else {
            return;
        };
        let request = Request::FolderRename { id: id.to_string(), name };
        submit(move |app| Some(app.dispatch(&request)));
    }

    fn entry(&self, title: &str, action: Sel, tag: isize, checked: bool) -> Retained<NSMenuItem> {
        let item = NSMenuItem::new(self.mtm());
        item.setTitle(&ns(&tr(title)));
        unsafe {
            item.setTarget(Some(self));
            item.setAction(Some(action));
        }
        item.setTag(tag);
        item.setState(isize::from(checked));
        return item;
    }

    fn fill(&self, menu: &NSMenu, index: Option<usize>) {
        menu.removeAllItems();
        let ivars = self.ivars();
        menu.addItem(&self.entry("New Folder…", sel!(newFolder:), NEW_TAG, false));
        let row = index.and_then(|index| {
            return ivars.rows.borrow().iter().find_map(|row| match row {
                Row::Folder(folder) if folder.index == index => Some(folder.clone()),
                _ => None,
            });
        });
        if let Some(row) = row.filter(|row| !row.sealed) {
            let tag = isize::try_from(row.index).unwrap_or(NEW_TAG);
            menu.addItem(&NSMenuItem::separatorItem(self.mtm()));
            menu.addItem(&self.entry(crate::i18n::key("Rename"), sel!(renameFolder:), tag, false));
            menu.addItem(&self.entry("Folder Icon & Color…", sel!(styleFolder:), tag, false));
            menu.addItem(&self.entry("Delete Folder…", sel!(deleteFolder:), tag, false));
            menu.addItem(&NSMenuItem::separatorItem(self.mtm()));
            if row.locked {
                menu.addItem(&self.entry("Remove Lock", sel!(unlockFolder:), tag, false));
                menu.addItem(&self.entry("Hide When Locked", sel!(hideFolder:), tag, row.hidden));
            } else {
                menu.addItem(&self.entry("Lock Folder", sel!(lockFolder:), tag, false));
            }
        }
        if ivars.has_locked.get() {
            menu.addItem(&NSMenuItem::separatorItem(self.mtm()));
            menu.addItem(&self.entry(privacy_title(ivars.unlocked.get()), sel!(togglePrivacy:), 0, false));
        }
    }
}

fn label(mtm: MainThreadMarker, text: &str) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&ns(text), mtm);
    field.setTranslatesAutoresizingMaskIntoConstraints(false);
    field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    field.setAllowsExpansionToolTips(true);
    return field;
}

fn cell(mtm: MainThreadMarker, row: &Row) -> Retained<NSView> {
    let cell = NSTableCellView::new(mtm);
    match row {
        Row::Header(title) => {
            let text = label(mtm, title);
            cell.addSubview(&text);
            unsafe { cell.setTextField(Some(&text)) };
            text.leadingAnchor().constraintEqualToAnchor(&cell.leadingAnchor()).setActive(true);
            text.trailingAnchor().constraintEqualToAnchor(&cell.trailingAnchor()).setActive(true);
            text.centerYAnchor().constraintEqualToAnchor(&cell.centerYAnchor()).setActive(true);
        }
        Row::All(count) => {
            let image = folders::symbol("photo.on.rectangle.angled");
            item_cell(mtm, &cell, &tr("All Wallpapers"), image.as_deref(), None, Some(*count));
        }
        Row::Folder(row) => {
            item_cell(mtm, &cell, &row.name, row.image.as_deref(), row.tint.as_deref(), row.count);
        }
    }
    return Retained::into_super(cell);
}

fn item_cell(
    mtm: MainThreadMarker,
    cell: &NSTableCellView,
    title: &str,
    image: Option<&NSImage>,
    tint: Option<&NSColor>,
    count: Option<usize>,
) {
    let icon = NSImageView::new(mtm);
    icon.setTranslatesAutoresizingMaskIntoConstraints(false);
    icon.setImage(image);
    icon.setImageScaling(NSImageScaling::ScaleProportionallyDown);
    icon.setContentTintColor(tint);
    let text = label(mtm, title);
    text.setContentCompressionResistancePriority_forOrientation(250.0, NSLayoutConstraintOrientation::Horizontal);
    cell.addSubview(&icon);
    cell.addSubview(&text);
    unsafe {
        cell.setImageView(Some(&icon));
        cell.setTextField(Some(&text));
    }
    icon.leadingAnchor().constraintEqualToAnchor(&cell.leadingAnchor()).setActive(true);
    icon.centerYAnchor().constraintEqualToAnchor(&cell.centerYAnchor()).setActive(true);
    icon.heightAnchor().constraintEqualToAnchor(&text.heightAnchor()).setActive(true);
    icon.widthAnchor().constraintEqualToAnchor(&icon.heightAnchor()).setActive(true);
    text.leadingAnchor()
        .constraintEqualToSystemSpacingAfterAnchor_multiplier(&icon.trailingAnchor(), 1.0)
        .setActive(true);
    text.centerYAnchor().constraintEqualToAnchor(&cell.centerYAnchor()).setActive(true);
    match count {
        Some(count) => {
            let badge = label(mtm, &count.to_string());
            badge.setTextColor(Some(&NSColor::secondaryLabelColor()));
            badge.setFont(Some(&NSFont::systemFontOfSize(NSFont::smallSystemFontSize())));
            cell.addSubview(&badge);
            badge.trailingAnchor().constraintEqualToAnchor(&cell.trailingAnchor()).setActive(true);
            badge.centerYAnchor().constraintEqualToAnchor(&cell.centerYAnchor()).setActive(true);
            badge
                .leadingAnchor()
                .constraintGreaterThanOrEqualToSystemSpacingAfterAnchor_multiplier(&text.trailingAnchor(), 1.0)
                .setActive(true);
        }
        None => {
            text.trailingAnchor().constraintLessThanOrEqualToAnchor(&cell.trailingAnchor()).setActive(true);
        }
    }
}

fn bar_button(mtm: MainThreadMarker, symbol: &str, tip: &str, target: &AnyObject, action: Sel) -> Retained<NSButton> {
    let image = folders::symbol(symbol).unwrap_or_default();
    let button = unsafe { NSButton::buttonWithImage_target_action(&image, Some(target), Some(action), mtm) };
    button.setBezelStyle(NSBezelStyle::Push);
    button.setControlSize(NSControlSize::Large);
    button.setToolTip(Some(&ns(tip)));
    return button;
}

pub struct Sidebar {
    pub controller: Retained<NSViewController>,
    source: Retained<SidebarSource>,
    privacy: Retained<NSButton>,
}

impl Sidebar {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let table = SidebarTable::alloc(mtm).set_ivars(TableIvars { source: RefCell::new(None) });
        let table: Retained<SidebarTable> = unsafe { msg_send![super(table), init] };
        table.setStyle(NSTableViewStyle::SourceList);
        table.setHeaderView(None);
        table.setAllowsEmptySelection(false);
        table.setFloatsGroupRows(false);
        let column = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &ns("sidebar"));
        table.addTableColumn(&column);
        table.registerForDraggedTypes(&NSArray::from_retained_slice(&context::dragged_types()));
        let source = SidebarSource::new(mtm, Retained::into_super(table.clone()));
        *table.ivars().source.borrow_mut() = Some(Weak::new(&source));
        let owner: &AnyObject = &source;
        let menu = NSMenu::new(mtm);
        unsafe {
            table.setTarget(Some(owner));
            table.setDoubleAction(Some(sel!(renameClicked:)));
            table.setDataSource(Some(ProtocolObject::from_ref(&*source)));
            table.setDelegate(Some(ProtocolObject::from_ref(&*source)));
            menu.setDelegate(Some(ProtocolObject::from_ref(&*source)));
        }
        unsafe { table.setMenu(Some(&menu)) };

        let scroll = NSScrollView::new(mtm);
        scroll.setDocumentView(Some(&table));
        scroll.setHasVerticalScroller(true);
        scroll.setDrawsBackground(false);
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);

        let target: &AnyObject = &source;
        let add = bar_button(mtm, "plus", &tr("New Folder"), target, sel!(newFolder:));
        let privacy = bar_button(mtm, "lock", &tr(privacy_title(false)), target, sel!(togglePrivacy:));
        privacy.setHidden(true);

        let bar = NSStackView::stackViewWithViews(&NSArray::from_slice(&[&*add as &NSView, &privacy]), mtm);
        bar.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        bar.setDistribution(NSStackViewDistribution::FillEqually);
        bar.setTranslatesAutoresizingMaskIntoConstraints(false);

        let view = NSView::new(mtm);
        view.addSubview(&scroll);
        view.addSubview(&bar);
        scroll.topAnchor().constraintEqualToAnchor(&view.topAnchor()).setActive(true);
        scroll.leadingAnchor().constraintEqualToAnchor(&view.leadingAnchor()).setActive(true);
        scroll.trailingAnchor().constraintEqualToAnchor(&view.trailingAnchor()).setActive(true);
        bar.topAnchor()
            .constraintEqualToSystemSpacingBelowAnchor_multiplier(&scroll.bottomAnchor(), 1.0)
            .setActive(true);
        view.bottomAnchor()
            .constraintEqualToSystemSpacingBelowAnchor_multiplier(&bar.bottomAnchor(), 1.0)
            .setActive(true);
        bar.leadingAnchor()
            .constraintEqualToSystemSpacingAfterAnchor_multiplier(&view.leadingAnchor(), 1.0)
            .setActive(true);
        view.trailingAnchor()
            .constraintEqualToSystemSpacingAfterAnchor_multiplier(&bar.trailingAnchor(), 1.0)
            .setActive(true);

        let controller = NSViewController::new(mtm);
        controller.setView(&view);
        return Self { controller, source, privacy };
    }

    pub fn reload(&self, library: &Library, selected: Option<usize>) {
        let ivars = self.source.ivars();
        let mut rows = vec![Row::Header(tr("Library")), Row::All(library.items.len())];
        if !library.folders.is_empty() {
            rows.push(Row::Header(tr("Folders")));
        }
        for (index, name) in library.folders.iter().enumerate() {
            let sealed = library.sealed(index);
            let lock = library.locks.get(index).copied().unwrap_or(FolderLock { locked: false, hidden: false });
            rows.push(Row::Folder(FolderRow {
                index,
                name: name.clone(),
                count: (!sealed).then(|| library.counts.get(index).copied()).flatten(),
                image: folders::image(library, index),
                tint: folders::tint(library, index),
                locked: lock.locked,
                hidden: lock.hidden,
                sealed,
            }));
        }
        let position = rows.iter().position(|row| match row {
            Row::All(_) => selected.is_none(),
            Row::Folder(folder) => Some(folder.index) == selected,
            Row::Header(_) => false,
        });
        ivars.syncing.set(true);
        *ivars.rows.borrow_mut() = rows;
        ivars.has_locked.set(library.has_locked);
        ivars.unlocked.set(library.unlocked);
        ivars.table.reloadData();
        if let Some(position) = position {
            ivars.table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(position), false);
        }
        ivars.syncing.set(false);
        self.privacy.setHidden(!library.has_locked);
        let glyph = if library.unlocked { "lock.open" } else { "lock" };
        self.privacy.setImage(folders::symbol(glyph).as_deref());
        self.privacy.setToolTip(Some(&ns(&tr(privacy_title(library.unlocked)))));
    }
}
