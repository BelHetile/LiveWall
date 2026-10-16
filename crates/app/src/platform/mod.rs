pub mod auth;
pub mod desktop;
pub mod detect;
pub mod hotkeys;
pub mod observe;
pub mod spaces;
pub mod system;

use objc2_core_foundation::CFType;
use objc2_foundation::NSTimer;

pub(crate) fn cf_ref<T: AsRef<CFType>>(value: &T) -> &CFType {
    return value.as_ref();
}

pub(crate) fn apply_timer_tolerance(timer: &NSTimer, interval: f64) {
    timer.setTolerance(interval * 0.1);
}
