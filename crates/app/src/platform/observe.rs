use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplicationDidChangeScreenParametersNotification, NSWorkspace, NSWorkspaceActiveSpaceDidChangeNotification,
};
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSNotificationCenter, NSNotificationSuspensionBehavior, NSObject,
    NSObjectProtocol, NSOperationQueue, NSString,
};

use crate::app::{Event, post};

type Subscription = (Retained<NSNotificationCenter>, Retained<ProtocolObject<dyn NSObjectProtocol>>);

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct LockObserver;

    unsafe impl NSObjectProtocol for LockObserver {}

    impl LockObserver {
        #[unsafe(method(screenLocked:))]
        fn screen_locked(&self, _notification: &NSNotification) {
            post(Event::ScreenLocked);
        }

        #[unsafe(method(screenUnlocked:))]
        fn screen_unlocked(&self, _notification: &NSNotification) {
            post(Event::ScreenUnlocked);
        }
    }
);

impl LockObserver {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        return unsafe { msg_send![super(this), init] };
    }
}

pub struct Observers {
    subscriptions: Vec<Subscription>,
    lock_center: Option<Retained<NSDistributedNotificationCenter>>,
    lock_observer: Option<Retained<LockObserver>>,
}

impl Observers {
    pub fn install(mtm: MainThreadMarker) -> Self {
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

        let lock_center = NSDistributedNotificationCenter::defaultCenter();
        let lock_observer = LockObserver::new(mtm);
        unsafe {
            lock_center.addObserver_selector_name_object_suspensionBehavior(
                AsRef::<AnyObject>::as_ref(&*lock_observer),
                sel!(screenLocked:),
                Some(&NSString::from_str("com.apple.screenIsLocked")),
                None,
                NSNotificationSuspensionBehavior::DeliverImmediately,
            );
            lock_center.addObserver_selector_name_object_suspensionBehavior(
                AsRef::<AnyObject>::as_ref(&*lock_observer),
                sel!(screenUnlocked:),
                Some(&NSString::from_str("com.apple.screenIsUnlocked")),
                None,
                NSNotificationSuspensionBehavior::DeliverImmediately,
            );
        }

        return Self {
            subscriptions,
            lock_center: Some(lock_center),
            lock_observer: Some(lock_observer),
        };
    }
}

impl Drop for Observers {
    fn drop(&mut self) {
        for (center, token) in &self.subscriptions {
            unsafe { center.removeObserver(AsRef::<AnyObject>::as_ref(&**token)) };
        }
        if let (Some(center), Some(observer)) = (&self.lock_center, &self.lock_observer) {
            unsafe { center.removeObserver(AsRef::<AnyObject>::as_ref(&**observer)) };
        }
    }
}
