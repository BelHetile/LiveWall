use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{
    NSApplicationDidChangeScreenParametersNotification, NSWorkspace, NSWorkspaceActiveSpaceDidChangeNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol, NSOperationQueue};

use crate::app::{Event, post};

type Subscription = (Retained<NSNotificationCenter>, Retained<ProtocolObject<dyn NSObjectProtocol>>);

pub struct Observers {
    subscriptions: Vec<Subscription>,
}

impl Observers {
    pub fn install() -> Self {
        let mut subscriptions = Vec::new();

        let workspace_center = NSWorkspace::sharedWorkspace().notificationCenter();
        let spaces_block = RcBlock::new(|_notification: NonNull<NSNotification>| {
            post(Event::SpacesChanged);
        });
        let spaces = unsafe {
            workspace_center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceActiveSpaceDidChangeNotification),
                None,
                Some(&NSOperationQueue::mainQueue()),
                &spaces_block,
            )
        };
        subscriptions.push((workspace_center, spaces));

        let app_center = NSNotificationCenter::defaultCenter();
        let screens_block = RcBlock::new(|_notification: NonNull<NSNotification>| {
            post(Event::ScreensChanged);
        });
        let screens = unsafe {
            app_center.addObserverForName_object_queue_usingBlock(
                Some(NSApplicationDidChangeScreenParametersNotification),
                None,
                Some(&NSOperationQueue::mainQueue()),
                &screens_block,
            )
        };
        subscriptions.push((app_center, screens));

        return Self { subscriptions };
    }
}

impl Drop for Observers {
    fn drop(&mut self) {
        for (center, token) in &self.subscriptions {
            unsafe { center.removeObserver(AsRef::<AnyObject>::as_ref(&**token)) };
        }
    }
}
