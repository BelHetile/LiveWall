use std::ptr::NonNull;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::MainThreadOnly;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSView, NSWindow, NSWindowCollectionBehavior,
    NSWindowDidChangeOcclusionStateNotification, NSWindowOcclusionState, NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGRect};
use objc2_core_graphics::{CGWindowLevelForKey, CGWindowLevelKey};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol, NSOperationQueue};
use objc2_quartz_core::CALayer;

pub struct WallpaperWindow {
    window: Retained<NSWindow>,
    root: Retained<CALayer>,
    occlusion: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

impl WallpaperWindow {
    pub fn create(mtm: MainThreadMarker, frame: CGRect, can_join_all: bool) -> Self {
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        let level = CGWindowLevelForKey(CGWindowLevelKey::DesktopIconWindowLevelKey);
        window.setLevel(isize::try_from(i64::from(level)).unwrap_or(0) - 1);
        let mut behavior = NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::FullScreenAuxiliary;
        if can_join_all {
            behavior |= NSWindowCollectionBehavior::CanJoinAllSpaces;
        }
        window.setCollectionBehavior(behavior);
        window.setIgnoresMouseEvents(true);
        window.setHasShadow(false);
        window.setOpaque(true);
        window.setBackgroundColor(Some(&NSColor::blackColor()));
        window.setCanHide(false);
        window.setExcludedFromWindowsMenu(true);
        unsafe { window.setReleasedWhenClosed(false) };

        let view = NSView::initWithFrame(NSView::alloc(mtm), CGRect::new(CGPoint::ZERO, frame.size));
        view.setWantsLayer(true);
        let root = view.layer().unwrap_or_else(CALayer::layer);
        root.setMasksToBounds(true);
        window.setContentView(Some(&view));
        window.orderFrontRegardless();

        let block = RcBlock::new(|_notification: NonNull<NSNotification>| {
            crate::app::post(crate::app::Event::OcclusionChanged);
        });
        let occlusion = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(NSWindowDidChangeOcclusionStateNotification),
                Some(AsRef::<AnyObject>::as_ref(&*window)),
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        };

        return Self { window, root, occlusion };
    }

    pub fn root(&self) -> &CALayer {
        return &self.root;
    }

    pub fn window_number(&self) -> u32 {
        return u32::try_from(self.window.windowNumber()).unwrap_or(0);
    }

    pub fn is_on_active_space(&self) -> bool {
        return self.window.isOnActiveSpace();
    }

    pub fn occluded(&self) -> bool {
        return !self.window.occlusionState().contains(NSWindowOcclusionState::Visible);
    }

    pub fn set_frame(&self, frame: CGRect) {
        self.window.setFrame_display(frame, false);
    }

    pub fn close(&self) {
        self.window.close();
    }
}

impl Drop for WallpaperWindow {
    fn drop(&mut self) {
        unsafe {
            NSNotificationCenter::defaultCenter().removeObserver(AsRef::<AnyObject>::as_ref(&*self.occlusion));
        }
    }
}
