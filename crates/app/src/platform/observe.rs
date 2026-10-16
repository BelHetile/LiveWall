use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplicationDidChangeScreenParametersNotification, NSWorkspace, NSWorkspaceActiveSpaceDidChangeNotification,
    NSWorkspaceDidActivateApplicationNotification, NSWorkspaceDidLaunchApplicationNotification,
    NSWorkspaceDidTerminateApplicationNotification, NSWorkspaceDidWakeNotification,
    NSWorkspaceScreensDidSleepNotification, NSWorkspaceScreensDidWakeNotification,
};
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSNotificationCenter, NSNotificationSuspensionBehavior, NSObject,
    NSObjectProtocol, NSOperationQueue, NSProcessInfoPowerStateDidChangeNotification,
    NSProcessInfoThermalStateDidChangeNotification, NSString,
};

use crate::app::{Event, post};
use crate::platform::detect::AudioMonitor;

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

        #[unsafe(method(screensaverStarted:))]
        fn screensaver_started(&self, _notification: &NSNotification) {
            post(Event::ScreensaverChanged(true));
        }

        #[unsafe(method(screensaverStopped:))]
        fn screensaver_stopped(&self, _notification: &NSNotification) {
            post(Event::ScreensaverChanged(false));
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
    audio: AudioMonitor,
}

impl Observers {
    #[allow(clippy::too_many_lines)]
    pub fn install(mtm: MainThreadMarker) -> Self {
        let mut subscriptions = Vec::new();
        let queue = NSOperationQueue::mainQueue();

        let workspace_center = NSWorkspace::sharedWorkspace().notificationCenter();
        let spaces_block = RcBlock::new(|_notification: NonNull<NSNotification>| {
            post(Event::SpacesChanged);
        });
        let spaces = unsafe {
            workspace_center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceActiveSpaceDidChangeNotification),
                None,
                Some(&queue),
                &spaces_block,
            )
        };
        subscriptions.push((workspace_center.clone(), spaces));

        let frontmost_block = RcBlock::new(|_notification: NonNull<NSNotification>| {
            post(Event::FrontmostChanged);
        });
        for name in [
            unsafe { NSWorkspaceDidActivateApplicationNotification },
            unsafe { NSWorkspaceDidLaunchApplicationNotification },
            unsafe { NSWorkspaceDidTerminateApplicationNotification },
        ] {
            let token = unsafe {
                workspace_center.addObserverForName_object_queue_usingBlock(
                    Some(name),
                    None,
                    Some(&queue),
                    &frontmost_block,
                )
            };
            subscriptions.push((workspace_center.clone(), token));
        }

        let sleep_block = RcBlock::new(|_notification: NonNull<NSNotification>| {
            post(Event::DisplayAsleep(true));
        });
        let sleep = unsafe {
            workspace_center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceScreensDidSleepNotification),
                None,
                Some(&queue),
                &sleep_block,
            )
        };
        subscriptions.push((workspace_center.clone(), sleep));

        let wake_block = RcBlock::new(|_notification: NonNull<NSNotification>| {
            post(Event::DisplayAsleep(false));
        });
        let wake = unsafe {
            workspace_center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceScreensDidWakeNotification),
                None,
                Some(&queue),
                &wake_block,
            )
        };
        subscriptions.push((workspace_center.clone(), wake));

        let app_center = NSNotificationCenter::defaultCenter();
        let screens_block = RcBlock::new(|_notification: NonNull<NSNotification>| {
            post(Event::DisplayChanged);
        });
        let screens = unsafe {
            app_center.addObserverForName_object_queue_usingBlock(
                Some(NSApplicationDidChangeScreenParametersNotification),
                None,
                Some(&queue),
                &screens_block,
            )
        };
        subscriptions.push((app_center, screens));

        let power_block = RcBlock::new(|_notification: NonNull<NSNotification>| {
            post(Event::LowPowerChanged);
        });
        let center = NSNotificationCenter::defaultCenter();
        let power = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSProcessInfoPowerStateDidChangeNotification),
                None,
                Some(&queue),
                &power_block,
            )
        };
        subscriptions.push((center, power));

        let thermal_block = RcBlock::new(|_notification: NonNull<NSNotification>| {
            post(Event::ThermalChanged);
        });
        let center = NSNotificationCenter::defaultCenter();
        let thermal = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(NSProcessInfoThermalStateDidChangeNotification),
                None,
                Some(&queue),
                &thermal_block,
            )
        };
        subscriptions.push((center, thermal));

        let wake_system_block = RcBlock::new(|_notification: NonNull<NSNotification>| {
            post(Event::SystemWake);
        });
        let wake_system = unsafe {
            workspace_center.addObserverForName_object_queue_usingBlock(
                Some(NSWorkspaceDidWakeNotification),
                None,
                Some(&queue),
                &wake_system_block,
            )
        };
        subscriptions.push((workspace_center, wake_system));

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
            lock_center.addObserver_selector_name_object_suspensionBehavior(
                AsRef::<AnyObject>::as_ref(&*lock_observer),
                sel!(screensaverStarted:),
                Some(&NSString::from_str("com.apple.screensaver.didstart")),
                None,
                NSNotificationSuspensionBehavior::DeliverImmediately,
            );
            lock_center.addObserver_selector_name_object_suspensionBehavior(
                AsRef::<AnyObject>::as_ref(&*lock_observer),
                sel!(screensaverStopped:),
                Some(&NSString::from_str("com.apple.screensaver.didstop")),
                None,
                NSNotificationSuspensionBehavior::DeliverImmediately,
            );
        }

        return Self {
            subscriptions,
            lock_center: Some(lock_center),
            lock_observer: Some(lock_observer),
            audio: AudioMonitor::install(),
        };
    }

    pub fn rebind_audio(&mut self) {
        self.audio.rebind();
    }

    pub fn refresh_audio(&mut self) {
        self.audio.refresh();
    }

    pub fn settle_audio(&mut self) {
        self.audio.settle();
    }

    pub fn audio_active(&self) -> bool {
        return self.audio.active();
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
