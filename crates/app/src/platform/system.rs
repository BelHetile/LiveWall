use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSActivityOptions, NSObjectProtocol, NSProcessInfo, NSString};

pub fn low_power_mode() -> bool {
    return NSProcessInfo::processInfo().isLowPowerModeEnabled();
}

pub struct AppNap {
    token: Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

impl AppNap {
    pub fn new() -> Self {
        return Self { token: None };
    }

    pub fn begin(&mut self) {
        if self.token.is_some() {
            return;
        }
        let reason = NSString::from_str("LiveWall wallpaper playback");
        self.token = Some(
            NSProcessInfo::processInfo()
                .beginActivityWithOptions_reason(NSActivityOptions::UserInitiatedAllowingIdleSystemSleep, &reason),
        );
    }

    pub fn end(&mut self) {
        if let Some(token) = self.token.take() {
            unsafe { NSProcessInfo::processInfo().endActivity(&token) };
        }
    }

    pub fn active(&self) -> bool {
        return self.token.is_some();
    }
}

impl Default for AppNap {
    fn default() -> Self {
        return Self::new();
    }
}

impl Drop for AppNap {
    fn drop(&mut self) {
        self.end();
    }
}
