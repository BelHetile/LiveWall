use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use proto::HotkeyAction;

use crate::i18n::tr;
use crate::ui::actions::{Actions, PRIVACY_KEY};
use crate::ui::state::{
    TAG_ALL, TAG_DELETE_FOLDER, TAG_HIDE_FOLDER, TAG_LOCK_FOLDER, TAG_NEW_FOLDER, TAG_RENAME_FOLDER, TAG_STYLE_FOLDER,
    privacy_title,
};
use crate::ui::widgets::{menu_item, ns};
use crate::ui::{UI, folders, inspector, toolbar};

fn submenu(mtm: MainThreadMarker, bar: &NSMenu, title: &str, items: &[Retained<NSMenuItem>]) -> Retained<NSMenu> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &ns(title));
    for item in items {
        menu.addItem(item);
    }
    let holder = NSMenuItem::new(mtm);
    holder.setSubmenu(Some(&menu));
    bar.addItem(&holder);
    return menu;
}

fn command_item(
    mtm: MainThreadMarker,
    action: HotkeyAction,
    key: &str,
    modifiers: NSEventModifierFlags,
    target: &AnyObject,
) -> Retained<NSMenuItem> {
    let tag = HotkeyAction::ALL
        .iter()
        .position(|other| *other == action)
        .and_then(|index| isize::try_from(index).ok())
        .unwrap_or(0);
    return menu_item(mtm, &tr(crate::hotkey::title(action)), sel!(run:), Some(target), tag, Some((key, modifiers)));
}

pub fn main_menu(mtm: MainThreadMarker, actions: &Actions) -> Retained<NSMenu> {
    let command = NSEventModifierFlags::Command;
    let shift = command | NSEventModifierFlags::Shift;
    let option = command | NSEventModifierFlags::Option;
    let target: &AnyObject = actions;
    let bar = NSMenu::new(mtm);
    submenu(
        mtm,
        &bar,
        "LiveWall",
        &[
            menu_item(mtm, &tr("About LiveWall"), sel!(orderFrontStandardAboutPanel:), None, 0, None),
            NSMenuItem::separatorItem(mtm),
            menu_item(mtm, &tr("Settings…"), sel!(showSettings:), Some(target), 0, Some((",", command))),
            menu_item(
                mtm,
                &tr(privacy_title(false)),
                sel!(togglePrivacy:),
                Some(target),
                0,
                Some((PRIVACY_KEY, option | NSEventModifierFlags::Control)),
            ),
            NSMenuItem::separatorItem(mtm),
            menu_item(mtm, &tr("Hide LiveWall"), sel!(hideLibrary:), Some(target), 0, Some(("h", command))),
            NSMenuItem::separatorItem(mtm),
            menu_item(mtm, &tr("Quit LiveWall"), sel!(quit:), Some(target), 0, Some(("q", command))),
        ],
    );
    submenu(
        mtm,
        &bar,
        &tr("File"),
        &[
            menu_item(mtm, &tr("Add Files…"), sel!(addFiles:), Some(target), 0, Some(("n", command))),
            menu_item(mtm, &tr("Add Folder…"), sel!(addFiles:), Some(target), 1, Some(("n", option))),
            menu_item(mtm, &tr("Download from URL…"), sel!(download:), Some(target), 0, Some(("l", command))),
            NSMenuItem::separatorItem(mtm),
            menu_item(mtm, &tr("New Folder…"), sel!(folder:), Some(target), TAG_NEW_FOLDER, Some(("n", shift))),
            menu_item(mtm, &tr("Folder Icon & Color…"), sel!(folderAction:), Some(target), TAG_STYLE_FOLDER, None),
            menu_item(mtm, &tr("Rename Folder"), sel!(folderAction:), Some(target), TAG_RENAME_FOLDER, None),
            menu_item(mtm, &tr("Lock Folder"), sel!(folderAction:), Some(target), TAG_LOCK_FOLDER, None),
            menu_item(mtm, &tr("Hide When Locked"), sel!(folderAction:), Some(target), TAG_HIDE_FOLDER, None),
            menu_item(mtm, &tr("Delete Folder…"), sel!(folderAction:), Some(target), TAG_DELETE_FOLDER, None),
            NSMenuItem::separatorItem(mtm),
            menu_item(mtm, &tr("Close Window"), sel!(performClose:), None, 0, Some(("w", command))),
        ],
    );
    submenu(
        mtm,
        &bar,
        &tr("Edit"),
        &[
            menu_item(mtm, &tr("Undo"), sel!(undo:), None, 0, Some(("z", command))),
            menu_item(mtm, &tr("Redo"), sel!(redo:), None, 0, Some(("z", shift))),
            NSMenuItem::separatorItem(mtm),
            menu_item(mtm, &tr("Cut"), sel!(cut:), None, 0, Some(("x", command))),
            menu_item(mtm, &tr("Copy"), sel!(copy:), None, 0, Some(("c", command))),
            menu_item(mtm, &tr("Paste"), sel!(paste:), None, 0, Some(("v", command))),
            menu_item(mtm, &tr("Select All"), sel!(selectAll:), None, 0, Some(("a", command))),
            NSMenuItem::separatorItem(mtm),
            menu_item(mtm, &tr("Find"), sel!(find:), Some(target), 0, Some(("f", command))),
            menu_item(
                mtm,
                &tr("Deselect"),
                sel!(deselect:),
                Some(target),
                0,
                Some(("\u{1b}", NSEventModifierFlags::empty())),
            ),
        ],
    );
    submenu(
        mtm,
        &bar,
        &tr("View"),
        &[
            menu_item(mtm, &tr("as Grid"), sel!(showGrid:), Some(target), 0, Some(("1", command))),
            menu_item(mtm, &tr("as List"), sel!(showList:), Some(target), 0, Some(("2", command))),
            NSMenuItem::separatorItem(mtm),
            menu_item(
                mtm,
                &tr("Show Sidebar"),
                sel!(toggleSidebar:),
                None,
                0,
                Some(("s", command | NSEventModifierFlags::Control)),
            ),
            menu_item(mtm, &tr("Show Inspector"), sel!(toggleInspector:), None, 0, Some(("i", option))),
            NSMenuItem::separatorItem(mtm),
            menu_item(
                mtm,
                &tr("Enter Full Screen"),
                sel!(toggleFullScreen:),
                None,
                0,
                Some(("f", command | NSEventModifierFlags::Control)),
            ),
        ],
    );
    wallpaper_menus(mtm, &bar, target);
    return bar;
}

fn wallpaper_menus(mtm: MainThreadMarker, bar: &NSMenu, target: &AnyObject) {
    let command = NSEventModifierFlags::Command;
    let shift = command | NSEventModifierFlags::Shift;
    let option = command | NSEventModifierFlags::Option;
    submenu(
        mtm,
        bar,
        &tr("Wallpaper"),
        &[
            menu_item(mtm, &tr("Set as Wallpaper"), sel!(setWallpaper:), Some(target), 0, Some(("\r", command))),
            menu_item(mtm, &tr("Use for Lock Screen"), sel!(setLock:), Some(target), 0, Some(("l", shift))),
            menu_item(
                mtm,
                &tr(inspector::FRAME_TO_LOCK),
                sel!(lockFrame:),
                Some(target),
                0,
                Some(("l", shift | NSEventModifierFlags::Option)),
            ),
            menu_item(mtm, &tr("Reset to Defaults"), sel!(resetSettings:), Some(target), 0, Some(("r", option))),
            menu_item(mtm, &tr("Remove from Library…"), sel!(delete:), Some(target), 0, Some(("\u{8}", command))),
            NSMenuItem::separatorItem(mtm),
            command_item(mtm, HotkeyAction::Next, "]", command, target),
            command_item(mtm, HotkeyAction::Previous, "[", command, target),
            command_item(mtm, HotkeyAction::Random, "r", command, target),
            NSMenuItem::separatorItem(mtm),
            command_item(mtm, HotkeyAction::TogglePause, "p", option, target),
            command_item(mtm, HotkeyAction::VolumeUp, "\u{f700}", command, target),
            command_item(mtm, HotkeyAction::VolumeDown, "\u{f701}", command, target),
            command_item(mtm, HotkeyAction::ToggleMute, "m", shift, target),
        ],
    );
    let window = submenu(
        mtm,
        bar,
        &tr("Window"),
        &[
            menu_item(mtm, &tr("Minimize"), sel!(performMiniaturize:), None, 0, Some(("m", command))),
            menu_item(mtm, &tr("Zoom"), sel!(performZoom:), None, 0, None),
            NSMenuItem::separatorItem(mtm),
            menu_item(mtm, &tr("Bring All to Front"), sel!(arrangeInFront:), None, 0, None),
        ],
    );
    NSApplication::sharedApplication(mtm).setWindowsMenu(Some(&window));
}

pub fn context_menu(mtm: MainThreadMarker, index: usize) -> Option<Retained<NSMenu>> {
    return UI.with_borrow(|ui| {
        let ui = ui.as_ref()?;
        let target: &AnyObject = &ui.actions;
        let item = |title: &str, action: Sel, tag: isize| menu_item(mtm, title, action, Some(target), tag, None);
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        let destinations = toolbar::scope_menu(mtm, &ui.library, ui.filter.scope, "", sel!(applyTo:), target);
        destinations.removeItemAtIndex(0);
        let wallpaper = NSMenuItem::new(mtm);
        wallpaper.setTitle(&ns(&tr("Set as Wallpaper")));
        wallpaper.setSubmenu(Some(&destinations));
        wallpaper.setEnabled(ui.library.items.get(index).is_some_and(|entry| entry.playable));
        menu.addItem(&wallpaper);
        menu.addItem(&item(&tr("Set as Lock Screen"), sel!(setLock:), 0));
        if let Some(entry) = ui.library.items.get(index).filter(|entry| entry.optimizable) {
            let optimize = item(&tr("Optimize…"), sel!(optimize:), 0);
            optimize.setEnabled(entry.progress.is_none());
            menu.addItem(&optimize);
        }
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let folders = NSMenu::new(mtm);
        let current = ui.library.items.get(index).and_then(|item| item.folder);
        let unfiled = item(&tr("Unfiled"), sel!(moveTo:), TAG_ALL);
        unfiled.setState(isize::from(current.is_none()));
        folders.addItem(&unfiled);
        folders.addItem(&NSMenuItem::separatorItem(mtm));
        for (position, name) in ui.library.folders.iter().enumerate() {
            let entry = item(name, sel!(moveTo:), isize::try_from(position).unwrap_or(TAG_ALL));
            entry.setState(isize::from(current == Some(position)));
            entry.setImage(folders::image(&ui.library, position, folders::MENU).as_deref());
            folders.addItem(&entry);
        }
        folders.addItem(&NSMenuItem::separatorItem(mtm));
        folders.addItem(&item(&tr("New Folder…"), sel!(moveTo:), TAG_NEW_FOLDER));
        let move_to = NSMenuItem::new(mtm);
        move_to.setTitle(&ns(&tr("Move to")));
        move_to.setSubmenu(Some(&folders));
        menu.addItem(&move_to);
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&item(&tr("Remove from Library…"), sel!(delete:), 0));
        return Some(menu);
    });
}
