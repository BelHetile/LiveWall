use std::path::Path;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSDraggingInfo, NSMenu, NSMenuItem, NSPasteboardItem,
    NSPasteboardType, NSPasteboardTypeFileURL, NSPasteboardWriting,
};
use objc2_foundation::{NSCopying, NSObject, NSObjectProtocol, NSString, NSURL};
use proto::{ApplyTarget, Request};

use crate::app;
use crate::i18n::{Arg, tr, trf};
use crate::model::ItemId;
use crate::ui::state::{Scope, TAG_ALL, desktop_name};
use crate::ui::view::Item;
use crate::ui::{UI, Ui, dialogs, ns, submit, update};

const ITEM_TYPE: &str = "app.livewall.item";

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    pub struct ContextActions;

    unsafe impl NSObjectProtocol for ContextActions {}

    impl ContextActions {
        #[unsafe(method(applyTo:))]
        fn apply_to(&self, sender: &NSMenuItem) {
            apply(Scope::from_tag(sender.tag()));
        }

        #[unsafe(method(setLock:))]
        fn set_lock(&self, _sender: Option<&AnyObject>) {
            selected(|_, id| submit(move |app| Some(app.set_lock(Some((id, None))))));
        }

        #[unsafe(method(optimize:))]
        fn optimize_item(&self, _sender: Option<&AnyObject>) {
            optimize();
        }

        #[unsafe(method(moveTo:))]
        fn move_to(&self, sender: &NSMenuItem) {
            let folder = usize::try_from(sender.tag()).ok();
            selected(move |ui, id| move_items(ui, vec![id], folder));
        }

        #[unsafe(method(delete:))]
        fn delete(&self, _sender: Option<&AnyObject>) {
            confirm_delete(self.mtm());
        }
    }
);

impl ContextActions {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        return unsafe { msg_send![Self::alloc(mtm), init] };
    }
}

fn selected(change: impl FnOnce(&mut Ui, ItemId)) {
    update(|ui, _| {
        if let Some(id) = ui.selected {
            change(ui, id);
        }
    });
}

pub fn apply(scope: Scope) {
    selected(move |ui, id| {
        let target = match scope {
            Scope::All => Some(ApplyTarget::Default),
            Scope::Space(space) => ui.library.spaces.get(space).cloned().map(|space| ApplyTarget::Space { space }),
        };
        if let Some(target) = target {
            let id = id.to_string();
            let everywhere = scope == Scope::All;
            submit(move |app| Some(app.apply(Some(&id), &target, everywhere, None)));
        }
    });
}

pub fn optimize() {
    selected(|_, id| {
        let request = Request::Optimize { id: id.to_string() };
        submit(move |app| Some(app.dispatch(&request)));
    });
}

pub fn reset() {
    selected(|_, id| submit(move |app| Some(app.reset_item(id))));
}

fn entry(mtm: MainThreadMarker, title: &str, action: Sel, tag: isize, target: &AnyObject) -> Retained<NSMenuItem> {
    let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &ns(title), Some(action), &ns("")) };
    item.setTag(tag);
    unsafe { item.setTarget(Some(target)) };
    return item;
}

fn submenu(mtm: MainThreadMarker, title: &str, menu: &NSMenu) -> Retained<NSMenuItem> {
    let item = NSMenuItem::new(mtm);
    item.setTitle(&ns(title));
    item.setSubmenu(Some(menu));
    return item;
}

pub fn menu(mtm: MainThreadMarker) -> Option<Retained<NSMenu>> {
    return UI.with_borrow(|ui| {
        let ui = ui.as_ref()?;
        let target: &AnyObject = ui.grid.actions();
        let index = ui.selected_index()?;
        let item = ui.library.items.get(index)?;
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        let destinations = NSMenu::new(mtm);
        destinations.addItem(&entry(mtm, &tr("All Desktops"), sel!(applyTo:), TAG_ALL, target));
        if ui.library.desktops > 0 {
            destinations.addItem(&NSMenuItem::separatorItem(mtm));
        }
        for space in 0..ui.library.desktops {
            let tag = isize::try_from(space).unwrap_or(TAG_ALL);
            destinations.addItem(&entry(mtm, &desktop_name(space), sel!(applyTo:), tag, target));
        }
        let wallpaper = submenu(mtm, &tr("Set as Wallpaper"), &destinations);
        wallpaper.setEnabled(item.playable);
        menu.addItem(&wallpaper);
        menu.addItem(&entry(mtm, &tr("Set as Lock Screen"), sel!(setLock:), 0, target));
        if item.optimizable {
            let optimize = entry(mtm, &tr("Optimize…"), sel!(optimize:), 0, target);
            optimize.setEnabled(item.progress.is_none());
            menu.addItem(&optimize);
        }
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let folders = NSMenu::new(mtm);
        folders.setAutoenablesItems(false);
        fill_folders(mtm, &folders, ui, item, target);
        menu.addItem(&submenu(mtm, &tr("Move to"), &folders));
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&entry(mtm, &tr("Remove from Library…"), sel!(delete:), 0, target));
        return Some(menu);
    });
}

pub fn fill_folders(mtm: MainThreadMarker, menu: &NSMenu, ui: &Ui, item: &Item, target: &AnyObject) {
    let unfiled = entry(mtm, &tr("Unfiled"), sel!(moveTo:), TAG_ALL, target);
    unfiled.setState(isize::from(item.folder.is_none()));
    menu.addItem(&unfiled);
    if !ui.library.folders.is_empty() {
        menu.addItem(&NSMenuItem::separatorItem(mtm));
    }
    for (position, name) in ui.library.folders.iter().enumerate() {
        let folder = entry(mtm, name, sel!(moveTo:), isize::try_from(position).unwrap_or(TAG_ALL), target);
        folder.setState(isize::from(item.folder == Some(position)));
        folder.setEnabled(!ui.library.sealed(position));
        menu.addItem(&folder);
    }
}

pub fn confirm_delete(mtm: MainThreadMarker) {
    let Some((id, title)) = UI.with_borrow(|ui| {
        let ui = ui.as_ref()?;
        let item = ui.library.items.get(ui.selected_index()?)?;
        return Some((item.id, item.title.clone()));
    }) else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    alert.setMessageText(&ns(&trf("Remove “%@” from the library?", &[Arg::Text(title)])));
    alert.setInformativeText(&ns(&tr("LiveWall deletes its copy. Your original file isn’t affected.")));
    let remove = alert.addButtonWithTitle(&ns(&tr("Remove")));
    remove.setHasDestructiveAction(true);
    alert.addButtonWithTitle(&ns(&tr("Cancel")));
    dialogs::sheet(&alert, move |response| {
        if response == NSAlertFirstButtonReturn {
            let request = Request::LibraryRemove { ids: vec![id.to_string()] };
            submit(move |app| Some(app.dispatch(&request)));
        }
    });
}

pub fn move_items(ui: &Ui, ids: Vec<ItemId>, folder: Option<usize>) {
    let folder = match folder {
        Some(index) if ui.library.sealed(index) => return,
        Some(index) => match ui.library.folder_ids.get(index) {
            Some(id) => Some(id.to_string()),
            None => return,
        },
        None => None,
    };
    let request = Request::LibraryMove { ids: ids.iter().map(ToString::to_string).collect(), folder };
    submit(move |app| Some(app.dispatch(&request)));
}

pub fn import(ui: &Ui, paths: Vec<String>, folder: Option<usize>) {
    if paths.is_empty() || folder.is_some_and(|index| ui.library.sealed(index)) {
        return;
    }
    let folder = folder.and_then(|index| ui.library.folder_ids.get(index)).map(ToString::to_string);
    let request = Request::LibraryAdd { paths, folder };
    dispatch2::DispatchQueue::main().exec_async(move || {
        app::with(|app| app.submit(request));
    });
}

fn item_type() -> Retained<NSPasteboardType> {
    return NSString::from_str(ITEM_TYPE);
}

pub fn dragged_types() -> [Retained<NSPasteboardType>; 2] {
    return [item_type(), unsafe { NSPasteboardTypeFileURL }.copy()];
}

pub fn writer(id: ItemId) -> Retained<ProtocolObject<dyn NSPasteboardWriting>> {
    let item = NSPasteboardItem::new();
    item.setString_forType(&ns(&id.to_string()), &item_type());
    return ProtocolObject::from_retained(item);
}

pub fn dragged_items(info: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<ItemId> {
    let kind = item_type();
    let Some(items) = info.draggingPasteboard().pasteboardItems() else {
        return Vec::new();
    };
    return items
        .iter()
        .filter_map(|item| item.stringForType(&kind))
        .filter_map(|value| value.to_string().parse::<ItemId>().ok())
        .collect();
}

pub fn dragged_files(info: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<String> {
    let kind = unsafe { NSPasteboardTypeFileURL };
    let Some(items) = info.draggingPasteboard().pasteboardItems() else {
        return Vec::new();
    };
    return items
        .iter()
        .filter_map(|item| item.stringForType(kind))
        .filter_map(|value| NSURL::URLWithString(&value))
        .filter_map(|url| url.filePathURL())
        .filter_map(|url| url.path())
        .flat_map(|path| media_in(Path::new(&path.to_string())))
        .collect();
}

pub fn media_in(path: &Path) -> Vec<String> {
    if !path.is_dir() {
        return vec![path.to_string_lossy().into_owned()];
    }
    let mut files: Vec<String> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|file| file.extension().and_then(|value| value.to_str()).and_then(crate::import::kind_for).is_some())
        .map(|file| file.to_string_lossy().into_owned())
        .collect();
    files.sort();
    return files;
}
