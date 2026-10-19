use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSEventModifierFlags, NSMenu, NSMenuDelegate, NSMenuItem, NSMenuItemValidation, NSText, NSWindow,
};
use objc2_foundation::{NSObject, NSObjectProtocol};
use proto::HotkeyAction;

use crate::app;
use crate::i18n::{key, tr};
use crate::model::ItemKind;
use crate::ui::state::{Layout, privacy_title};
use crate::ui::{UI, Ui, context, dialogs, inspector, later, ns, settings, submit, update};

const PRIVACY_KEY: &str = "x";

fn item(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<Sel>,
    target: Option<&AnyObject>,
    key: Option<(&str, NSEventModifierFlags)>,
) -> Retained<NSMenuItem> {
    let item = NSMenuItem::new(mtm);
    item.setTitle(&ns(title));
    unsafe {
        item.setTarget(target);
        item.setAction(action);
    }
    if let Some((key, modifiers)) = key {
        item.setKeyEquivalent(&ns(key));
        item.setKeyEquivalentModifierMask(modifiers);
    }
    return item;
}

fn separator(mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    return NSMenuItem::separatorItem(mtm);
}

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

fn tagged(item: Retained<NSMenuItem>, tag: isize) -> Retained<NSMenuItem> {
    item.setTag(tag);
    return item;
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
    let title = tr(crate::hotkey::title(action));
    return tagged(item(mtm, &title, Some(sel!(run:)), Some(target), Some((key, modifiers))), tag);
}

fn with_ui<R>(read: impl FnOnce(&Ui) -> R) -> Option<R> {
    return UI.with(|ui| ui.try_borrow().ok().and_then(|ui| ui.as_ref().map(read)));
}

fn editing(mtm: MainThreadMarker) -> bool {
    return NSApplication::sharedApplication(mtm)
        .keyWindow()
        .and_then(|window| window.firstResponder())
        .is_some_and(|responder| responder.downcast::<NSText>().is_ok());
}

fn validate(ui: &Ui, mtm: MainThreadMarker, menu_item: &NSMenuItem) -> bool {
    let Some(action) = menu_item.action() else {
        return true;
    };
    let selected = ui.selected_index().and_then(|index| ui.library.items.get(index));
    let sheeted = ui.window.attachedSheet().is_some();
    if action == sel!(togglePrivacy:) {
        let unlocked = ui.library.unlocked;
        menu_item.setTitle(&ns(&tr(privacy_title(unlocked))));
        menu_item.setKeyEquivalent(&ns(if unlocked { PRIVACY_KEY } else { "" }));
        menu_item.setHidden(!ui.library.has_locked);
        return true;
    }
    if action == sel!(addFiles:) || action == sel!(downloadURL:) || action == sel!(newFolder:) {
        return !sheeted;
    }
    if action == sel!(find:) {
        return ui.window.isVisible();
    }
    if action == sel!(deselect:) {
        return selected.is_some() && !editing(mtm);
    }
    if action == sel!(setLayout:) {
        menu_item.setState(isize::from(ui.filter.layout.index() == menu_item.tag()));
        return true;
    }
    if action == sel!(setWallpaper:) {
        return selected.is_some_and(|item| item.playable);
    }
    if action == sel!(setLock:) {
        return selected.is_some() && !ui.library.lock_follows;
    }
    if action == sel!(lockFrame:) {
        return !ui.library.lock_follows && selected.is_some_and(|item| item.kind == ItemKind::Video);
    }
    if action == sel!(resetSettings:) {
        return selected.is_some_and(|item| item.changed);
    }
    if action == sel!(optimize:) {
        let optimizable = selected.is_some_and(|item| item.optimizable);
        menu_item.setHidden(!optimizable);
        return selected.is_some_and(|item| item.optimizable && item.progress.is_none());
    }
    if action == sel!(moveTo:) {
        return selected.is_some();
    }
    if action == sel!(delete:) {
        return selected.is_some() && !sheeted && !editing(mtm);
    }
    return true;
}

fn validate_command(menu_item: &NSMenuItem) -> bool {
    let action = usize::try_from(menu_item.tag()).ok().and_then(|index| HotkeyAction::ALL.get(index)).copied();
    let title = match action {
        Some(HotkeyAction::TogglePause) => {
            app::with(|app| app.paused()).map(|paused| if paused { key("Resume") } else { key("Pause") })
        }
        Some(HotkeyAction::ToggleMute) => {
            app::with(|app| app.settings().muted).map(|muted| if muted { key("Unmute") } else { key("Mute") })
        }
        _ => None,
    };
    if let Some(title) = title {
        menu_item.setTitle(&ns(&tr(title)));
    }
    return true;
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct MenuTarget;

    unsafe impl NSObjectProtocol for MenuTarget {}

    unsafe impl NSMenuItemValidation for MenuTarget {
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, menu_item: &NSMenuItem) -> bool {
            let mtm = self.mtm();
            if menu_item.action() == Some(sel!(run:)) {
                validate_command(menu_item)
            } else {
                with_ui(|ui| validate(ui, mtm, menu_item)).unwrap_or(true)
            }
        }
    }

    unsafe impl NSMenuDelegate for MenuTarget {
        #[unsafe(method(menuNeedsUpdate:))]
        fn needs_update(&self, menu: &NSMenu) {
            menu.removeAllItems();
            let target: &AnyObject = self;
            with_ui(|ui| {
                if let Some(item) = ui.selected_index().and_then(|index| ui.library.items.get(index)) {
                    context::fill_folders(self.mtm(), menu, ui, item, target);
                }
            });
        }
    }

    impl MenuTarget {
        #[unsafe(method(togglePrivacy:))]
        fn toggle_privacy(&self, _sender: Option<&AnyObject>) {
            crate::ui::toggle_privacy();
        }

        #[unsafe(method(addFiles:))]
        fn add_files(&self, _sender: Option<&AnyObject>) {
            dialogs::add_files(self.mtm());
        }

        #[unsafe(method(downloadURL:))]
        fn download_url(&self, _sender: Option<&AnyObject>) {
            dialogs::download_sheet(self.mtm());
        }

        #[unsafe(method(newFolder:))]
        fn new_folder(&self, _sender: Option<&AnyObject>) {
            dialogs::new_folder(self.mtm());
        }

        #[unsafe(method(find:))]
        fn find(&self, _sender: Option<&AnyObject>) {
            later(|ui, _| ui.toolbar.find());
        }

        #[unsafe(method(deselect:))]
        fn deselect(&self, _sender: Option<&AnyObject>) {
            crate::ui::select(None);
        }

        #[unsafe(method(setLayout:))]
        fn set_layout(&self, sender: &NSMenuItem) {
            crate::ui::set_layout(Layout::from_index(sender.tag()));
        }

        #[unsafe(method(setWallpaper:))]
        fn set_wallpaper(&self, _sender: Option<&AnyObject>) {
            let scope = with_ui(|ui| ui.filter.scope);
            if let Some(scope) = scope {
                context::apply(scope);
            }
        }

        #[unsafe(method(setLock:))]
        fn set_lock(&self, _sender: Option<&AnyObject>) {
            inspector::lock(false);
        }

        #[unsafe(method(lockFrame:))]
        fn lock_frame(&self, _sender: Option<&AnyObject>) {
            inspector::lock(true);
        }

        #[unsafe(method(resetSettings:))]
        fn reset_settings(&self, _sender: Option<&AnyObject>) {
            context::reset();
        }

        #[unsafe(method(optimize:))]
        fn optimize(&self, _sender: Option<&AnyObject>) {
            context::optimize();
        }

        #[unsafe(method(moveTo:))]
        fn move_to(&self, sender: &NSMenuItem) {
            if sender.submenu().is_none() {
                let folder = usize::try_from(sender.tag()).ok();
                update(move |ui, _| {
                    if let Some(id) = ui.selected {
                        context::move_items(ui, &[id], folder);
                    }
                });
            }
        }

        #[unsafe(method(delete:))]
        fn delete(&self, _sender: Option<&AnyObject>) {
            context::confirm_delete(self.mtm());
        }

        #[unsafe(method(run:))]
        fn run(&self, sender: &NSMenuItem) {
            if let Some(action) = usize::try_from(sender.tag()).ok().and_then(|index| HotkeyAction::ALL.get(index)) {
                let action = *action;
                submit(move |app| {
                    app.run_hotkey(action);
                    return None;
                });
            }
        }

        #[unsafe(method(showSettings:))]
        fn show_settings(&self, _sender: Option<&AnyObject>) {
            settings::show(self.mtm(), None);
        }

        #[unsafe(method(hideLibrary:))]
        fn hide_library(&self, _sender: Option<&AnyObject>) {
            later(Ui::hide);
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let target: Retained<MenuTarget> = unsafe { msg_send![MenuTarget::alloc(mtm), init] };
        return target;
    }
}

pub struct Menus {
    pub bar: Retained<NSMenu>,
    target: Retained<MenuTarget>,
}

impl Menus {
    pub fn show_settings(&self, tab: isize) {
        settings::show(self.target.mtm(), Some(tab));
    }

    #[allow(clippy::too_many_lines)]
    pub fn new(mtm: MainThreadMarker) -> Self {
        let command = NSEventModifierFlags::Command;
        let shift = command | NSEventModifierFlags::Shift;
        let option = command | NSEventModifierFlags::Option;
        let control = command | NSEventModifierFlags::Control;
        let target = MenuTarget::new(mtm);
        let object: &AnyObject = &target;
        let bar = NSMenu::new(mtm);
        let application = NSApplication::sharedApplication(mtm);
        NSWindow::setAllowsAutomaticWindowTabbing(false, mtm);

        let services = NSMenu::initWithTitle(NSMenu::alloc(mtm), &ns(&tr("Services")));
        let services_item = item(mtm, &tr("Services"), None, None, None);
        services_item.setSubmenu(Some(&services));
        application.setServicesMenu(Some(&services));
        submenu(
            mtm,
            &bar,
            "LiveWall",
            &[
                item(mtm, &tr("About LiveWall"), Some(sel!(orderFrontStandardAboutPanel:)), None, None),
                separator(mtm),
                item(mtm, &tr("Settings…"), Some(sel!(showSettings:)), Some(object), Some((",", command))),
                item(
                    mtm,
                    &tr(privacy_title(false)),
                    Some(sel!(togglePrivacy:)),
                    Some(object),
                    Some((PRIVACY_KEY, option | NSEventModifierFlags::Control)),
                ),
                separator(mtm),
                services_item,
                separator(mtm),
                item(mtm, &tr("Hide LiveWall"), Some(sel!(hideLibrary:)), Some(object), Some(("h", command))),
                item(mtm, &tr("Hide Others"), Some(sel!(hideOtherApplications:)), None, Some(("h", option))),
                item(mtm, &tr("Show All"), Some(sel!(unhideAllApplications:)), None, None),
                separator(mtm),
                item(mtm, &tr("Quit LiveWall"), Some(sel!(terminate:)), None, Some(("q", command))),
            ],
        );
        submenu(
            mtm,
            &bar,
            &tr("File"),
            &[
                item(mtm, &tr("Add Files…"), Some(sel!(addFiles:)), Some(object), Some(("o", command))),
                item(mtm, &tr("Download from URL…"), Some(sel!(downloadURL:)), Some(object), Some(("l", command))),
                separator(mtm),
                item(mtm, &tr("New Folder…"), Some(sel!(newFolder:)), Some(object), Some(("n", shift))),
                separator(mtm),
                item(mtm, &tr("Close Window"), Some(sel!(performClose:)), None, Some(("w", command))),
            ],
        );
        submenu(
            mtm,
            &bar,
            &tr("Edit"),
            &[
                item(mtm, &tr("Undo"), Some(sel!(undo:)), None, Some(("z", command))),
                item(mtm, &tr("Redo"), Some(sel!(redo:)), None, Some(("z", shift))),
                separator(mtm),
                item(mtm, &tr("Cut"), Some(sel!(cut:)), None, Some(("x", command))),
                item(mtm, &tr("Copy"), Some(sel!(copy:)), None, Some(("c", command))),
                item(mtm, &tr("Paste"), Some(sel!(paste:)), None, Some(("v", command))),
                item(mtm, &tr("Select All"), Some(sel!(selectAll:)), None, Some(("a", command))),
                separator(mtm),
                item(mtm, &tr("Find"), Some(sel!(find:)), Some(object), Some(("f", command))),
                item(
                    mtm,
                    &tr("Deselect"),
                    Some(sel!(deselect:)),
                    Some(object),
                    Some(("\u{1b}", NSEventModifierFlags::empty())),
                ),
            ],
        );
        submenu(
            mtm,
            &bar,
            &tr("View"),
            &[
                tagged(item(mtm, &tr("as Large Grid"), Some(sel!(setLayout:)), Some(object), Some(("1", command))), 0),
                tagged(item(mtm, &tr("as Small Grid"), Some(sel!(setLayout:)), Some(object), Some(("2", command))), 1),
                tagged(item(mtm, &tr("as List"), Some(sel!(setLayout:)), Some(object), Some(("3", command))), 2),
                separator(mtm),
                item(mtm, &tr("Show Sidebar"), Some(sel!(toggleSidebar:)), None, Some(("s", option))),
                item(mtm, &tr("Show Inspector"), Some(sel!(toggleInspector:)), None, Some(("i", option))),
                separator(mtm),
                item(mtm, &tr("Enter Full Screen"), Some(sel!(toggleFullScreen:)), None, Some(("f", control))),
            ],
        );
        let folders = NSMenu::initWithTitle(NSMenu::alloc(mtm), &ns(&tr("Move to")));
        folders.setAutoenablesItems(false);
        folders.setDelegate(Some(ProtocolObject::from_ref(&*target)));
        let move_to = item(mtm, &tr("Move to"), Some(sel!(moveTo:)), Some(object), None);
        move_to.setSubmenu(Some(&folders));
        submenu(
            mtm,
            &bar,
            &tr("Wallpaper"),
            &[
                item(mtm, &tr("Set as Wallpaper"), Some(sel!(setWallpaper:)), Some(object), Some(("\r", command))),
                item(mtm, &tr("Set as Lock Screen"), Some(sel!(setLock:)), Some(object), Some(("l", shift))),
                item(
                    mtm,
                    &tr("Set This Frame as Lock Screen"),
                    Some(sel!(lockFrame:)),
                    Some(object),
                    Some(("l", shift | NSEventModifierFlags::Option)),
                ),
                item(mtm, &tr("Reset to Defaults"), Some(sel!(resetSettings:)), Some(object), Some(("r", option))),
                item(mtm, &tr("Optimize…"), Some(sel!(optimize:)), Some(object), None),
                move_to,
                item(mtm, &tr("Remove from Library…"), Some(sel!(delete:)), Some(object), Some(("\u{8}", command))),
                separator(mtm),
                command_item(mtm, HotkeyAction::Next, "]", command, object),
                command_item(mtm, HotkeyAction::Previous, "[", command, object),
                command_item(mtm, HotkeyAction::Random, "r", command, object),
                separator(mtm),
                command_item(mtm, HotkeyAction::TogglePause, "p", option, object),
                command_item(mtm, HotkeyAction::VolumeUp, "\u{f700}", command, object),
                command_item(mtm, HotkeyAction::VolumeDown, "\u{f701}", command, object),
                command_item(mtm, HotkeyAction::ToggleMute, "\u{f701}", option, object),
            ],
        );
        let window = submenu(
            mtm,
            &bar,
            &tr("Window"),
            &[
                item(mtm, &tr("Minimize"), Some(sel!(performMiniaturize:)), None, Some(("m", command))),
                item(mtm, &tr("Zoom"), Some(sel!(performZoom:)), None, None),
                separator(mtm),
                item(mtm, &tr("Bring All to Front"), Some(sel!(arrangeInFront:)), None, None),
            ],
        );
        application.setWindowsMenu(Some(&window));
        return Self { bar, target };
    }
}
