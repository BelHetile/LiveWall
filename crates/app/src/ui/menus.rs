use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem,
};
use objc2_foundation::NSObject;

use crate::i18n::tr;
use crate::ui::{ns, settings};

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

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(showSettings:))]
        fn show_settings(&self, _sender: Option<&AnyObject>) {
            settings::show(self.mtm(), None);
        }

        // !!! HARDCODED STUB !!! Does nothing yet. Real behaviour: select and scroll to the wallpaper
        // !!! assigned to this slot in the library (needs the Ui state and the grid).
        #[unsafe(method(showDesktopWallpaper:))]
        fn show_desktop_wallpaper(&self, _sender: Option<&AnyObject>) {
            eprintln!("STUB: show desktop wallpaper in library");
        }

        // !!! HARDCODED STUB !!! Does nothing yet. Same as above, for the lock screen slot.
        #[unsafe(method(showLockWallpaper:))]
        fn show_lock_wallpaper(&self, _sender: Option<&AnyObject>) {
            eprintln!("STUB: show lock screen wallpaper in library");
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
    _target: Retained<MenuTarget>,
}

impl Menus {
    pub fn target(&self) -> &AnyObject {
        return &self._target;
    }

    pub fn show_settings(&self, tab: isize) {
        settings::show(self._target.mtm(), Some(tab));
    }

    pub fn new(mtm: MainThreadMarker) -> Self {
        let command = NSEventModifierFlags::Command;
        let option = command | NSEventModifierFlags::Option;
        let control = command | NSEventModifierFlags::Control;
        let target = MenuTarget::new(mtm);
        let object: &AnyObject = &target;
        let bar = NSMenu::new(mtm);

        submenu(
            mtm,
            &bar,
            "LiveWall",
            &[
                item(mtm, &tr("About LiveWall"), Some(sel!(orderFrontStandardAboutPanel:)), None, None),
                separator(mtm),
                item(mtm, &tr("Settings…"), Some(sel!(showSettings:)), Some(object), Some((",", command))),
                separator(mtm),
                item(mtm, &tr("Hide LiveWall"), Some(sel!(hide:)), None, None),
                item(mtm, &tr("Hide others"), Some(sel!(hideOtherApplications:)), None, None),
                separator(mtm),
                item(mtm, &tr("Quit LiveWall"), Some(sel!(terminate:)), None, Some(("q", command))),
            ],
        );
        submenu(
            mtm,
            &bar,
            &tr("File"),
            &[
                item(mtm, &tr("Add Files…"), None, None, Some(("o", command))),
                item(mtm, &tr("Download from URL…"), None, None, None),
            ],
        );
        submenu(
            mtm,
            &bar,
            &tr("Edit"),
            &[
                item(mtm, &tr("Cut"), Some(sel!(cut:)), None, Some(("x", command))),
                item(mtm, &tr("Copy"), Some(sel!(copy:)), None, Some(("c", command))),
                item(mtm, &tr("Paste"), Some(sel!(paste:)), None, Some(("v", command))),
                item(mtm, &tr("Select All"), Some(sel!(selectAll:)), None, Some(("a", command))),
                separator(mtm),
                item(mtm, &tr("Find"), None, None, Some(("f", command))),
            ],
        );
        submenu(
            mtm,
            &bar,
            &tr("View"),
            &[
                item(mtm, &tr("as Large Grid"), None, None, Some(("1", command))),
                item(mtm, &tr("as Small Grid"), None, None, Some(("2", command))),
                item(mtm, &tr("as List"), None, None, Some(("3", command))),
                separator(mtm),
                item(
                    mtm,
                    &tr("Show Sidebar"),
                    Some(sel!(toggleSidebar:)),
                    None,
                    Some(("s", option)),
                ),
                item(
                    mtm,
                    &tr("Show Inspector"),
                    Some(sel!(toggleInspector:)),
                    None,
                    Some(("i", option)),
                ),
                separator(mtm),
                item(mtm, &tr("Enter Full Screen"), Some(sel!(toggleFullScreen:)), None, Some(("f", control))),
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
        NSApplication::sharedApplication(mtm).setWindowsMenu(Some(&window));
        return Self { bar, _target: target };
    }
}
