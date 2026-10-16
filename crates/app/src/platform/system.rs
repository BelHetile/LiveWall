use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_core_graphics::{CGError, CGEventSource, CGEventSourceStateID, CGEventType, CGGetOnlineDisplayList};
use objc2_foundation::{
    NSActivityOptions, NSLocale, NSObjectProtocol, NSProcessInfo, NSProcessInfoThermalState, NSString,
};
use objc2_service_management::{SMAppService, SMAppServiceStatus};

pub fn low_power_mode() -> bool {
    return NSProcessInfo::processInfo().isLowPowerModeEnabled();
}

pub fn thermal_hot() -> bool {
    let state = NSProcessInfo::processInfo().thermalState();
    return state == NSProcessInfoThermalState::Serious || state == NSProcessInfoThermalState::Critical;
}

pub fn online_display_count() -> usize {
    let mut count = 0u32;
    let status = unsafe { CGGetOnlineDisplayList(0, std::ptr::null_mut(), &raw mut count) };
    if status != CGError::Success {
        return 0;
    }
    return usize::try_from(count).unwrap_or(0);
}

pub fn idle_seconds() -> f64 {
    return CGEventSource::seconds_since_last_event_type(CGEventSourceStateID::HIDSystemState, CGEventType(u32::MAX));
}

pub fn preferred_languages() -> Vec<String> {
    return NSLocale::preferredLanguages().to_vec().iter().map(ToString::to_string).collect();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LoginItemStatus {
    NotRegistered,
    Enabled,
    RequiresApproval,
    NotFound,
}

pub fn login_item_status() -> LoginItemStatus {
    let service = unsafe { SMAppService::mainAppService() };
    return map_status(unsafe { service.status() });
}

pub fn set_login_item(enabled: bool) -> Result<LoginItemStatus, String> {
    let service = unsafe { SMAppService::mainAppService() };
    let result = if enabled {
        unsafe { service.registerAndReturnError() }
    } else {
        unsafe { service.unregisterAndReturnError() }
    };
    result.map_err(|error| error.localizedDescription().to_string())?;
    return Ok(map_status(unsafe { service.status() }));
}

pub fn open_login_items_settings() {
    unsafe { SMAppService::openSystemSettingsLoginItems() };
}

fn map_status(status: SMAppServiceStatus) -> LoginItemStatus {
    return match status {
        SMAppServiceStatus::Enabled => LoginItemStatus::Enabled,
        SMAppServiceStatus::RequiresApproval => LoginItemStatus::RequiresApproval,
        SMAppServiceStatus::NotFound => LoginItemStatus::NotFound,
        _ => LoginItemStatus::NotRegistered,
    };
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
