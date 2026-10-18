use std::cell::OnceCell;
use std::path::Path;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSBackingStoreType, NSImage, NSScreen,
    NSSplitViewController, NSSplitViewItem, NSViewController, NSWindow, NSWindowDelegate, NSWindowStyleMask,
    NSWindowTitleVisibility, NSWindowToolbarStyle,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_foundation::{NSBundle, NSData, NSNotification, NSObject, NSObjectProtocol};

use crate::model::AppearanceMode;
use crate::ui::{Ui, env, later, ns, settings};

const ICON: &[u8] = include_bytes!("../../../../resources/AppIcon.iconset/icon_512x512@2x.png");
const WINDOW_SIZE: CGSize = CGSize::new(1500.0, 860.0);
const MIN_SIZE: CGSize = CGSize::new(1180.0, 600.0);
const SCREEN_SHARE: CGFloat = 0.85;
const SIDEBAR_MIN: CGFloat = 200.0;
const SIDEBAR_MAX: CGFloat = 320.0;
const INSPECTOR_MIN: CGFloat = 250.0;
const INSPECTOR_MAX: CGFloat = 400.0;
const AUTOSAVE: &str = "LiveWallMain";
const AUTOSAVE_DEV: &str = "LiveWallDev";
const SPLIT_AUTOSAVE: &str = "LiveWallSplit";
const SPLIT_AUTOSAVE_DEV: &str = "LiveWallSplitDev";

pub struct Panes {
    pub split: Retained<NSSplitViewController>,
    pub sidebar: Retained<NSSplitViewItem>,
    pub inspector: Retained<NSSplitViewItem>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    pub struct WindowDelegate;

    unsafe impl NSObjectProtocol for WindowDelegate {}

    unsafe impl NSWindowDelegate for WindowDelegate {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            later(Ui::closed);
        }

        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, notification: &NSNotification) {
            if let Some(window) = notification.object().and_then(|object| object.downcast::<NSWindow>().ok()) {
                settings::resigned(&window);
            }
        }
    }
);

thread_local! {
    static DELEGATE: OnceCell<Retained<WindowDelegate>> = const { OnceCell::new() };
}

pub fn delegate(mtm: MainThreadMarker) -> Retained<WindowDelegate> {
    return DELEGATE.with(|cell| cell.get_or_init(|| unsafe { msg_send![WindowDelegate::alloc(mtm), init] }).clone());
}

pub fn end_sheets(window: &NSWindow) {
    while let Some(sheet) = window.attachedSheet() {
        window.endSheet(&sheet);
        if window.attachedSheet().is_some_and(|other| other == sheet) {
            sheet.orderOut(None);
            return;
        }
    }
}

pub fn application_icon(mtm: MainThreadMarker) {
    let bundle = NSBundle::mainBundle().bundlePath().to_string();
    if Path::new(&bundle).extension().is_some_and(|extension| extension.eq_ignore_ascii_case("app")) {
        return;
    }
    let data = NSData::with_bytes(ICON);
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
    }
}

pub fn set_appearance(mtm: MainThreadMarker, mode: AppearanceMode) {
    let appearance = match mode {
        AppearanceMode::Light => NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }),
        AppearanceMode::Dark => NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }),
        AppearanceMode::System => None,
    };
    NSApplication::sharedApplication(mtm).setAppearance(appearance.as_deref());
}

pub fn panes(
    sidebar: &NSViewController,
    content: &NSViewController,
    inspector: &NSViewController,
) -> Panes {
    let sidebar_item = NSSplitViewItem::sidebarWithViewController(sidebar);
    sidebar_item.setCanCollapse(true);
    sidebar_item.setMinimumThickness(SIDEBAR_MIN);
    sidebar_item.setMaximumThickness(SIDEBAR_MAX);
    let content_item = NSSplitViewItem::splitViewItemWithViewController(content);
    let inspector_item = NSSplitViewItem::inspectorWithViewController(inspector);
    inspector_item.setCanCollapse(true);
    inspector_item.setMinimumThickness(INSPECTOR_MIN);
    inspector_item.setMaximumThickness(INSPECTOR_MAX);
    let split = NSSplitViewController::new(sidebar.mtm());
    split.addSplitViewItem(&sidebar_item);
    split.addSplitViewItem(&content_item);
    split.addSplitViewItem(&inspector_item);
    let name = if crate::dev::isolated() { SPLIT_AUTOSAVE_DEV } else { SPLIT_AUTOSAVE };
    split.splitView().setAutosaveName(Some(&ns(name)));
    return Panes { split, sidebar: sidebar_item, inspector: inspector_item };
}

fn visible_size(mtm: MainThreadMarker) -> Option<CGSize> {
    let screen = NSScreen::mainScreen(mtm).or_else(|| NSScreen::screens(mtm).firstObject())?;
    return Some(screen.visibleFrame().size);
}

fn fitted(size: CGSize, limit: CGSize) -> CGSize {
    return CGSize::new(size.width.min(limit.width), size.height.min(limit.height));
}

pub fn main_window(mtm: MainThreadMarker, split: &NSSplitViewController) -> (Retained<NSWindow>, bool) {
    let forced = env("LW_UI_SIZE").and_then(|value| {
        let (width, height) = value.split_once('x')?;
        return Some(CGSize::new(width.parse().ok()?, height.parse().ok()?));
    });
    let visible = visible_size(mtm);
    let size = forced.unwrap_or_else(|| {
        return visible.map_or(WINDOW_SIZE, |visible| {
            return fitted(WINDOW_SIZE, CGSize::new(visible.width * SCREEN_SHARE, visible.height * SCREEN_SHARE));
        });
    });
    let min_size = visible.map_or(MIN_SIZE, |visible| fitted(MIN_SIZE, visible));
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
    window.setDelegate(Some(ProtocolObject::from_ref(&*delegate(mtm))));
    window.setTitle(&ns("LiveWall"));
    window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
    window.setToolbarStyle(NSWindowToolbarStyle::Unified);
    window.setContentMinSize(min_size);
    window.setContentViewController(Some(split));
    window.setContentSize(size);
    let mut restored = false;
    if forced.is_none() {
        let name = ns(if crate::dev::isolated() { AUTOSAVE_DEV } else { AUTOSAVE });
        restored = window.setFrameUsingName(&name);
        window.setFrameAutosaveName(&name);
    }
    return (window, restored);
}
