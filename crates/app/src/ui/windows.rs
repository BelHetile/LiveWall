use std::path::Path;

use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSBackingStoreType, NSImage, NSScreen,
    NSSplitViewController, NSSplitViewItem, NSTitlebarSeparatorStyle, NSView, NSViewController, NSWindow,
    NSWindowStyleMask, NSWindowTitleVisibility, NSWindowToolbarStyle,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_foundation::{NSBundle, NSData};

use crate::ui::inspector::Inspector;
use crate::ui::sidebar::{self, Sidebar};
use crate::ui::wall::WallView;
use crate::ui::widgets::ns;

const ICON: &[u8] = include_bytes!("../../../../resources/AppIcon.iconset/icon_512x512@2x.png");
const WINDOW_SIZE: CGSize = CGSize::new(1500.0, 860.0);
const MIN_SIZE: CGSize = CGSize::new(1180.0, 600.0);
const SCREEN_SHARE: CGFloat = 0.85;
const AUTOSAVE: &str = "LiveWallMain";

pub struct Panes {
    pub split: Retained<NSSplitViewController>,
    pub inspector: Retained<NSSplitViewItem>,
    pub sidebar: Retained<NSSplitViewItem>,
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

pub fn set_appearance(mtm: MainThreadMarker, mode: usize) {
    let appearance = match mode {
        1 => NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }),
        2 => NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }),
        _ => None,
    };
    NSApplication::sharedApplication(mtm).setAppearance(appearance.as_deref());
}

pub fn panes(mtm: MainThreadMarker, sidebar: &NSView, wall: &WallView, inspector: &Inspector) -> Panes {
    let sidebar_controller = NSViewController::new(mtm);
    sidebar_controller.setView(sidebar);
    let sidebar_item = NSSplitViewItem::sidebarWithViewController(&sidebar_controller);
    sidebar_item.setMinimumThickness(Sidebar::min_width());
    sidebar_item.setMaximumThickness(Sidebar::max_width());
    sidebar_item.setCanCollapse(true);
    let wall_controller = NSViewController::new(mtm);
    wall_controller.setView(wall);
    let inspector_controller = NSViewController::new(mtm);
    inspector_controller.setView(&inspector.scroll);
    let split = NSSplitViewController::new(mtm);
    let wall_item = NSSplitViewItem::splitViewItemWithViewController(&wall_controller);
    wall_item.setAutomaticallyAdjustsSafeAreaInsets(false);
    let inspector_item = NSSplitViewItem::inspectorWithViewController(&inspector_controller);
    inspector_item.setMinimumThickness(Inspector::width());
    inspector_item.setMaximumThickness(Inspector::width());
    inspector_item.setCanCollapse(true);
    split.addSplitViewItem(&sidebar_item);
    split.addSplitViewItem(&wall_item);
    split.addSplitViewItem(&inspector_item);
    split.splitView().setPosition_ofDividerAtIndex(sidebar::WIDTH, 0);
    return Panes { split, inspector: inspector_item, sidebar: sidebar_item };
}

fn visible_size(mtm: MainThreadMarker) -> Option<CGSize> {
    let screen = NSScreen::mainScreen(mtm).or_else(|| NSScreen::screens(mtm).firstObject())?;
    return Some(screen.visibleFrame().size);
}

fn fitted(size: CGSize, limit: CGSize) -> CGSize {
    return CGSize::new(size.width.min(limit.width), size.height.min(limit.height));
}

pub fn main_window(mtm: MainThreadMarker, split: &NSSplitViewController) -> (Retained<NSWindow>, bool) {
    let forced = crate::ui::env("LW_UI_SIZE").and_then(|value| {
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
    window.setTitle(&ns("LiveWall"));
    window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
    window.setToolbarStyle(NSWindowToolbarStyle::Unified);
    window.setTitlebarAppearsTransparent(true);
    window.setTitlebarSeparatorStyle(NSTitlebarSeparatorStyle::Shadow);
    if let Some(value) = crate::ui::env("LW_UI_TITLEBAR") {
        window.setTitlebarAppearsTransparent(value == "clear");
    }
    if let Some(value) = crate::ui::env("LW_UI_SEPARATOR") {
        window.setTitlebarSeparatorStyle(match value.as_str() {
            "none" => NSTitlebarSeparatorStyle::None,
            "line" => NSTitlebarSeparatorStyle::Line,
            "shadow" => NSTitlebarSeparatorStyle::Shadow,
            _ => NSTitlebarSeparatorStyle::Automatic,
        });
    }
    window.setContentMinSize(min_size);
    window.setContentViewController(Some(split));
    window.setContentSize(size);
    let mut restored = false;
    if forced.is_none() {
        let name = ns(AUTOSAVE);
        restored = window.setFrameUsingName(&name);
        window.setFrameAutosaveName(&name);
    }
    return (window, restored);
}
