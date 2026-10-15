use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSControlStateValueOff, NSControlStateValueOn, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
use serde::Serialize;

use crate::app::{Event, post};
use crate::i18n::Strings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    TogglePause,
    Next,
    Previous,
    Random,
    ToggleMute,
    ToggleLaunchAtLogin,
    Quit,
}

impl MenuAction {
    fn tag(self) -> isize {
        return match self {
            MenuAction::TogglePause => 1,
            MenuAction::Next => 2,
            MenuAction::Previous => 3,
            MenuAction::Random => 4,
            MenuAction::ToggleMute => 5,
            MenuAction::ToggleLaunchAtLogin => 6,
            MenuAction::Quit => 7,
        };
    }

    fn from_tag(tag: isize) -> Option<Self> {
        return match tag {
            1 => Some(MenuAction::TogglePause),
            2 => Some(MenuAction::Next),
            3 => Some(MenuAction::Previous),
            4 => Some(MenuAction::Random),
            5 => Some(MenuAction::ToggleMute),
            6 => Some(MenuAction::ToggleLaunchAtLogin),
            7 => Some(MenuAction::Quit),
            _ => None,
        };
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct MenuTarget;

    unsafe impl NSObjectProtocol for MenuTarget {}

    impl MenuTarget {
        #[unsafe(method(menuAction:))]
        fn menu_action(&self, sender: Option<&NSMenuItem>) {
            if let Some(action) = sender.and_then(|item| MenuAction::from_tag(item.tag())) {
                post(Event::Menu(action));
            }
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        return unsafe { msg_send![super(this), init] };
    }
}

#[derive(Clone, Debug, Default)]
pub struct MenuState {
    pub title: Option<String>,
    pub paused: bool,
    pub muted: bool,
    pub launch_at_login: bool,
}

pub struct Menu {
    status_item: Retained<NSStatusItem>,
    root: Retained<NSMenu>,
    _target: Retained<MenuTarget>,
    title: Retained<NSMenuItem>,
    pause: Retained<NSMenuItem>,
    next: Retained<NSMenuItem>,
    previous: Retained<NSMenuItem>,
    random: Retained<NSMenuItem>,
    mute: Retained<NSMenuItem>,
    login: Retained<NSMenuItem>,
    quit: Retained<NSMenuItem>,
}

impl Menu {
    pub fn install(mtm: MainThreadMarker, state: &MenuState, strings: &Strings) -> Self {
        let status_item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        set_status_icon(&status_item, mtm);

        let target = MenuTarget::new(mtm);
        let root = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("LiveWall"));
        root.setAutoenablesItems(false);

        let title = make_item(mtm, "", None, &target);
        title.setEnabled(false);
        root.addItem(&title);
        root.addItem(&NSMenuItem::separatorItem(mtm));
        let pause = make_item(mtm, "", Some(MenuAction::TogglePause), &target);
        let next = make_item(mtm, "", Some(MenuAction::Next), &target);
        let previous = make_item(mtm, "", Some(MenuAction::Previous), &target);
        let random = make_item(mtm, "", Some(MenuAction::Random), &target);
        let mute = make_item(mtm, "", Some(MenuAction::ToggleMute), &target);
        root.addItem(&pause);
        root.addItem(&next);
        root.addItem(&previous);
        root.addItem(&random);
        root.addItem(&mute);
        root.addItem(&NSMenuItem::separatorItem(mtm));
        let login = make_item(mtm, "", Some(MenuAction::ToggleLaunchAtLogin), &target);
        root.addItem(&login);
        root.addItem(&NSMenuItem::separatorItem(mtm));
        let quit = make_quit_item(mtm, &target);
        root.addItem(&quit);

        status_item.setMenu(Some(&root));

        let menu = Self { status_item, root, _target: target, title, pause, next, previous, random, mute, login, quit };
        menu.update(state, strings);
        return menu;
    }

    pub fn update(&self, state: &MenuState, strings: &Strings) {
        let title = state.title.clone().unwrap_or_else(|| strings.get("No wallpaper"));
        self.title.setTitle(&NSString::from_str(&title));
        self.pause.setTitle(&NSString::from_str(&if state.paused {
            strings.get("Resume")
        } else {
            strings.get("Pause")
        }));
        self.next.setTitle(&NSString::from_str(&strings.get("Next")));
        self.previous.setTitle(&NSString::from_str(&strings.get("Previous")));
        self.random.setTitle(&NSString::from_str(&strings.get("Random")));
        self.mute.setTitle(&NSString::from_str(&if state.muted { strings.get("Unmute") } else { strings.get("Mute") }));
        self.login.setTitle(&NSString::from_str(&strings.get("Launch at login")));
        self.login.setState(if state.launch_at_login { NSControlStateValueOn } else { NSControlStateValueOff });
        self.quit.setTitle(&NSString::from_str(&strings.get("Quit LiveWall")));
    }

    pub fn report(&self) -> MenuReport {
        let mut items = Vec::new();
        for item in &self.root.itemArray().to_vec() {
            if item.isSeparatorItem() {
                continue;
            }
            items.push(MenuItemReport {
                title: item.title().to_string(),
                enabled: item.isEnabled(),
                checked: item.state() == NSControlStateValueOn,
            });
        }
        return MenuReport { items };
    }
}

impl Drop for Menu {
    fn drop(&mut self) {
        NSStatusBar::systemStatusBar().removeStatusItem(&self.status_item);
    }
}

#[derive(Serialize)]
pub struct MenuReport {
    pub items: Vec<MenuItemReport>,
}

#[derive(Serialize)]
pub struct MenuItemReport {
    pub title: String,
    pub enabled: bool,
    pub checked: bool,
}

fn make_item(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<MenuAction>,
    target: &MenuTarget,
) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            None,
            &NSString::from_str(""),
        )
    };
    if let Some(action) = action {
        item.setTag(action.tag());
        unsafe {
            item.setTarget(Some(AsRef::<AnyObject>::as_ref(target)));
            item.setAction(Some(sel!(menuAction:)));
        }
    }
    return item;
}

fn make_quit_item(mtm: MainThreadMarker, target: &MenuTarget) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(""),
            Some(sel!(menuAction:)),
            &NSString::from_str("q"),
        )
    };
    item.setTag(MenuAction::Quit.tag());
    unsafe { item.setTarget(Some(AsRef::<AnyObject>::as_ref(target))) };
    return item;
}

fn set_status_icon(status_item: &NSStatusItem, mtm: MainThreadMarker) {
    let image = NSImage::imageNamed(&NSString::from_str("status-icon")).or_else(|| {
        NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str("photo.on.rectangle"),
            Some(&NSString::from_str("LiveWall")),
        )
    });
    let Some(image) = image else { return };
    image.setTemplate(true);
    if let Some(button) = status_item.button(mtm) {
        button.setImage(Some(&image));
    }
}
