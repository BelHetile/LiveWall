pub mod desktop;
pub mod detect;
pub mod hotkeys;
pub mod observe;
pub mod spaces;
pub mod system;

use objc2_core_foundation::CFType;

pub(crate) fn cf_ref<T: AsRef<CFType>>(value: &T) -> &CFType {
    return value.as_ref();
}
