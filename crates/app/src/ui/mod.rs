mod fixture;
mod inspector;
mod palette;
mod toolbar;
mod wall;
mod widgets;

use std::cell::RefCell;
use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAppearance, NSAppearanceNameAqua, NSApplication, NSApplicationActivationPolicy,
    NSApplicationDelegate, NSBackingStoreType, NSMenu, NSMenuItem, NSSplitViewController, NSSplitViewItem,
    NSViewController, NSWindow, NSWindowStyleMask, NSWindowTitleVisibility, NSWindowToolbarStyle,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSString};

use crate::ui::fixture::Library;
use crate::ui::inspector::Inspector;
use crate::ui::palette::{Palette, Theme};
use crate::ui::toolbar::ToolbarDelegate;
use crate::ui::wall::WallView;

struct Ui {
    library: Library,
    theme: Theme,
    actions: Retained<Actions>,
    inspector_item: Retained<NSSplitViewItem>,
    wall: Retained<WallView>,
    inspector: Inspector,
    selected: Option<usize>,
    _window: Retained<NSWindow>,
    _split: Retained<NSSplitViewController>,
    _toolbar: Retained<ToolbarDelegate>,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct Actions;

    unsafe impl NSObjectProtocol for Actions {}

    unsafe impl NSApplicationDelegate for Actions {
        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn last_window(&self, _application: &NSApplication) -> bool {
            true
        }

        #[unsafe(method(applicationDidFinishLaunching:))]
        fn launched(&self, _notification: &NSNotification) {}
    }

    impl Actions {
        #[unsafe(method(noop:))]
        fn noop(&self, _sender: Option<&AnyObject>) {}
    }
);

impl Actions {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        return unsafe { msg_send![super(this), init] };
    }
}

pub fn select(index: Option<usize>) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    UI.with_borrow_mut(|ui| {
        let Some(ui) = ui.as_mut() else {
            return;
        };
        if ui.selected == index {
            return;
        }
        ui.selected = index;
        ui.wall.set_selected(index);
        ui.inspector.show(mtm, &ui.library, &ui.theme, index, &ui.actions);
        if index.is_some() && ui.inspector_item.isCollapsed() {
            ui.inspector_item.animator().setCollapsed(false);
        }
    });
}

pub fn apply(index: usize) {
    select(Some(index));
}

fn env(name: &str) -> Option<String> {
    return std::env::var(name).ok().filter(|value| !value.is_empty());
}

pub fn run() -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("lwui must run on the main thread")?;
    let application = NSApplication::sharedApplication(mtm);
    let _ = application.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    match env("LW_UI_APPEARANCE").as_deref() {
        Some("dark") => application.setAppearance(widgets::dark().as_deref()),
        Some("light") => {
            application.setAppearance(NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).as_deref());
        }
        _ => {}
    }
    let palette = match env("LW_UI_PALETTE") {
        Some(path) => Palette::load(std::path::Path::new(&path))?,
        None => Palette::default(),
    };
    let theme = Theme::new(palette);

    let root = env("LW_UI_FIXTURES").map_or_else(fixture::default_root, PathBuf::from);
    let thumbs = std::env::temp_dir().join("lwui-thumbs");
    std::fs::create_dir_all(&thumbs).map_err(|error| format!("{}: {error}", thumbs.display()))?;
    let library = fixture::load(&root, &thumbs);
    if library.items.is_empty() {
        return Err(format!("no media under {}", root.display()));
    }

    let actions = Actions::new(mtm);
    application.setDelegate(Some(ProtocolObject::from_ref(&*actions)));
    application.setMainMenu(Some(&main_menu(mtm)));

    let wall = WallView::new(mtm, &library, &theme);
    let wall_controller = NSViewController::new(mtm);
    wall_controller.setView(&wall);
    let mut inspector = Inspector::new(mtm, &theme);
    let inspector_controller = NSViewController::new(mtm);
    inspector_controller.setView(&inspector.scroll);

    let split = NSSplitViewController::new(mtm);
    let wall_item = NSSplitViewItem::splitViewItemWithViewController(&wall_controller);
    wall_item.setAutomaticallyAdjustsSafeAreaInsets(false);
    let inspector_item = NSSplitViewItem::inspectorWithViewController(&inspector_controller);
    inspector_item.setMinimumThickness(Inspector::width());
    inspector_item.setMaximumThickness(Inspector::width() + 60.0);
    inspector_item.setCanCollapse(true);
    split.addSplitViewItem(&wall_item);
    split.addSplitViewItem(&inspector_item);

    let size = env("LW_UI_SIZE")
        .and_then(|value| {
            let (width, height) = value.split_once('x')?;
            return Some(CGSize::new(width.parse().ok()?, height.parse().ok()?));
        })
        .unwrap_or(CGSize::new(1320.0, 840.0));
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable
        | NSWindowStyleMask::FullSizeContentView;
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            CGRect::new(CGPoint::ZERO, size),
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str("LiveWall"));
    window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
    window.setToolbarStyle(NSWindowToolbarStyle::Unified);
    window.setContentMinSize(CGSize::new(900.0, 560.0));
    window.setContentViewController(Some(&split));
    window.setContentSize(size);
    let toolbar = toolbar::delegate(mtm, &library, &theme, &actions);
    window.setToolbar(Some(&toolbar.toolbar(mtm)));
    window.center();

    let selected =
        env("LW_UI_SELECT").and_then(|value| value.parse::<usize>().ok()).filter(|index| *index < library.items.len());
    inspector.show(mtm, &library, &theme, selected, &actions);
    wall.set_selected(selected);
    inspector_item.setCollapsed(env("LW_UI_INSPECTOR").as_deref() == Some("0"));

    window.makeKeyAndOrderFront(None);
    #[allow(deprecated)]
    application.activateIgnoringOtherApps(true);
    if let Some(path) = env("LW_UI_WINDOW_FILE") {
        std::fs::write(&path, window.windowNumber().to_string()).map_err(|error| format!("{path}: {error}"))?;
    }

    UI.with_borrow_mut(|ui| {
        *ui = Some(Ui {
            library,
            theme,
            actions,
            inspector_item,
            wall,
            inspector,
            selected,
            _window: window,
            _split: split,
            _toolbar: toolbar,
        });
    });
    application.run();
    return Ok(());
}

fn main_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let bar = NSMenu::new(mtm);
    let app_item = NSMenuItem::new(mtm);
    let app_menu = NSMenu::new(mtm);
    let quit = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str("Quit LiveWall"),
            Some(sel!(terminate:)),
            &NSString::from_str("q"),
        )
    };
    app_menu.addItem(&quit);
    app_item.setSubmenu(Some(&app_menu));
    bar.addItem(&app_item);
    return bar;
}
