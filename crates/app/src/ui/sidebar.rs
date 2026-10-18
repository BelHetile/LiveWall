use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSColor, NSControlTextEditingDelegate, NSFont, NSImage, NSImageScaling, NSImageView, NSLayoutConstraintOrientation,
    NSLineBreakMode, NSMenu, NSMenuDelegate, NSMenuItem, NSScrollView, NSTableCellView, NSTableColumn, NSTableView,
    NSTableViewDataSource, NSTableViewDelegate, NSTableViewStyle, NSTextField, NSView, NSViewController, NSButton,
    NSBezelStyle, NSControlSize, NSStackView, NSStackViewDistribution, NSUserInterfaceLayoutOrientation,
};
use objc2_app_kit::{NSDragOperation, NSDraggingInfo, NSTableViewDropOperation};
use objc2_foundation::{NSArray, NSIndexSet, NSNotification, NSObject, NSObjectProtocol};
use uuid::Uuid;

use crate::i18n::tr;
use crate::ui::state::privacy_title;
use crate::ui::view::{FolderLock, Library};
use crate::ui::{UI, commit, context, dialogs, folders, ns, picker, submit, update};

const NEW_TAG: isize = -1;

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

impl SidebarSource {
    fn drop_target(&self, row: isize) -> Option<Option<usize>> {
        return match self.row_at(row)? {
            Row::All(_) => Some(None),
            Row::Folder(folder) if !folder.sealed => Some(Some(folder.index)),
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

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = SourceIvars]
    struct SidebarSource;

    unsafe impl NSObjectProtocol for SidebarSource {}

    unsafe impl NSControlTextEditingDelegate for SidebarSource {}

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
                Some(folder) => {
                    let items = context::dragged_items(info);
                    let files = if items.is_empty() { context::dragged_files(info) } else { Vec::new() };
                    let accepted = !items.is_empty() || !files.is_empty();
                    update(move |ui, _| {
                        if items.is_empty() {
                            context::import(ui, files, folder);
                        } else {
                            context::move_items(ui, items, folder);
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
                        crate::app::with(|app| app.request_unlock());
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
            if let Some((id, _)) = folder(sender.tag()) {
                dialogs::rename_folder(self.mtm(), id);
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
            menu.addItem(&self.entry("Rename Folder…", sel!(renameFolder:), tag, false));
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
    text.leadingAnchor().constraintEqualToSystemSpacingAfterAnchor_multiplier(&icon.trailingAnchor(), 1.0).setActive(true);
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

fn bar_button(
    mtm: MainThreadMarker,
    symbol: &str,
    tip: &str,
    target: &AnyObject,
    action: Sel,
) -> Retained<NSButton> {
    let image = folders::symbol(symbol).unwrap_or_else(|| NSImage::new());
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
        let table = NSTableView::new(mtm);
        table.setStyle(NSTableViewStyle::SourceList);
        table.setHeaderView(None);
        table.setAllowsEmptySelection(false);
        table.setFloatsGroupRows(false);
        let column = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &ns("sidebar"));
        table.addTableColumn(&column);
        table.registerForDraggedTypes(&NSArray::from_retained_slice(&context::dragged_types()));
        let source = SidebarSource::new(mtm, table.clone());
        let menu = NSMenu::new(mtm);
        unsafe {
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
        bar.topAnchor().constraintEqualToSystemSpacingBelowAnchor_multiplier(&scroll.bottomAnchor(), 1.0).setActive(true);
        view.bottomAnchor().constraintEqualToSystemSpacingBelowAnchor_multiplier(&bar.bottomAnchor(), 1.0).setActive(true);
        bar.leadingAnchor().constraintEqualToSystemSpacingAfterAnchor_multiplier(&view.leadingAnchor(), 1.0).setActive(true);
        view.trailingAnchor().constraintEqualToSystemSpacingAfterAnchor_multiplier(&bar.trailingAnchor(), 1.0).setActive(true);

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
