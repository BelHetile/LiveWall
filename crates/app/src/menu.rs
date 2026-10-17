use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSControlStateValueOff, NSControlStateValueOn, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength,
};
use objc2_foundation::{NSData, NSObject, NSObjectProtocol, NSSize, NSString};
use serde::Serialize;

use crate::app::{Event, post};
use crate::i18n::tr;

const STATUS_ICON: &[u8] = include_bytes!("../../../resources/status-icon@2x.png");
const STATUS_ICON_POINTS: f64 = 18.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(isize)]
pub enum MenuAction {
    TogglePause = 1,
    Next = 2,
    Previous = 3,
    Random = 4,
    ToggleMute = 5,
    ToggleLaunchAtLogin = 6,
    Quit = 7,
    OpenLibrary = 8,
}

impl MenuAction {
    const ALL: [Self; 8] = [
        Self::OpenLibrary,
        Self::TogglePause,
        Self::Next,
        Self::Previous,
        Self::Random,
        Self::ToggleMute,
        Self::ToggleLaunchAtLogin,
        Self::Quit,
    ];

    fn tag(self) -> isize {
        return self as isize;
    }

    fn from_tag(tag: isize) -> Option<Self> {
        return Self::ALL.into_iter().find(|action| action.tag() == tag);
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
    pub status: Option<String>,
    pub muted: bool,
    pub launch_at_login: bool,
}

pub struct Menu {
    status_item: Retained<NSStatusItem>,
    root: Retained<NSMenu>,
    _target: Retained<MenuTarget>,
    title: Retained<NSMenuItem>,
    library: Retained<NSMenuItem>,
    pause: Retained<NSMenuItem>,
    next: Retained<NSMenuItem>,
    previous: Retained<NSMenuItem>,
    random: Retained<NSMenuItem>,
    mute: Retained<NSMenuItem>,
    login: Retained<NSMenuItem>,
    quit: Retained<NSMenuItem>,
}

impl Menu {
    pub fn install(mtm: MainThreadMarker, state: &MenuState) -> Self {
        let status_item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
        set_status_icon(&status_item, mtm);

        let target = MenuTarget::new(mtm);
        let root = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("LiveWall"));
        root.setAutoenablesItems(false);

        let title = make_item(mtm, "", None, &target);
        title.setEnabled(false);
        root.addItem(&title);
        let library = make_item(mtm, "", Some(MenuAction::OpenLibrary), &target);
        root.addItem(&library);
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

        let menu = Self {
            status_item,
            root,
            _target: target,
            title,
            library,
            pause,
            next,
            previous,
            random,
            mute,
            login,
            quit,
        };
        menu.update(state);
        return menu;
    }

    pub fn update(&self, state: &MenuState) {
        let title = state.title.clone().unwrap_or_else(|| tr("No wallpaper"));
        self.title.setTitle(&NSString::from_str(&title));
        self.title.setSubtitle(state.status.as_deref().map(NSString::from_str).as_deref());
        self.library.setTitle(&NSString::from_str(&tr("Open library")));
        self.pause.setTitle(&NSString::from_str(&if state.paused { tr("Resume") } else { tr("Pause") }));
        self.next.setTitle(&NSString::from_str(&tr("Next")));
        self.previous.setTitle(&NSString::from_str(&tr("Previous")));
        self.random.setTitle(&NSString::from_str(&tr("Random")));
        self.mute.setTitle(&NSString::from_str(&if state.muted { tr("Unmute") } else { tr("Mute") }));
        self.login.setTitle(&NSString::from_str(&tr("Launch at login")));
        self.login.setState(if state.launch_at_login { NSControlStateValueOn } else { NSControlStateValueOff });
        self.quit.setTitle(&NSString::from_str(&tr("Quit LiveWall")));
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
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(STATUS_ICON)) else {
        return;
    };
    image.setSize(NSSize::new(STATUS_ICON_POINTS, STATUS_ICON_POINTS));
    image.setTemplate(true);
    if let Some(button) = status_item.button(mtm) {
        button.setImage(Some(&image));
    }
}
