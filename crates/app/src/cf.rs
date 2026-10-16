use std::path::Path;
use std::ptr;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::{CFArray, CFDictionary, CFNumber, CFRetained, CFString, CFType, CFURL};
use objc2_foundation::{NSString, NSURL};

pub fn cf_ref<T: AsRef<CFType>>(value: &T) -> &CFType {
    return value.as_ref();
}

pub fn object<T: AsRef<AnyObject> + ?Sized>(value: &T) -> &AnyObject {
    return value.as_ref();
}

pub fn ns_string(value: &CFString) -> &NSString {
    return unsafe { &*ptr::from_ref(value).cast::<NSString>() };
}

pub fn cf_string(value: Option<&CFType>) -> Option<String> {
    return value.and_then(|value| value.downcast_ref::<CFString>()).map(ToString::to_string);
}

pub fn cf_i64(value: Option<&CFType>) -> Option<i64> {
    return value.and_then(|value| value.downcast_ref::<CFNumber>()).and_then(CFNumber::as_i64);
}

pub fn cf_u64(value: Option<&CFType>) -> Option<u64> {
    return cf_i64(value).and_then(|value| u64::try_from(value).ok());
}

pub fn cf_f64(value: Option<&CFType>) -> Option<f64> {
    return value.and_then(|value| value.downcast_ref::<CFNumber>()).and_then(CFNumber::as_f64);
}

pub fn downcast_dict(value: Option<&CFType>) -> Option<&CFDictionary<CFString, CFType>> {
    let dict = value?.downcast_ref::<CFDictionary>()?;
    return Some(unsafe { dict.cast_unchecked::<CFString, CFType>() });
}

pub fn downcast_array(value: Option<&CFType>) -> Option<&CFArray<CFType>> {
    let array = value?.downcast_ref::<CFArray>()?;
    return Some(unsafe { array.cast_unchecked() });
}

pub fn file_url(path: impl AsRef<Path>) -> Retained<NSURL> {
    return NSURL::fileURLWithPath(&NSString::from_str(&path.as_ref().to_string_lossy()));
}

pub fn cf_file_url(path: impl AsRef<Path>) -> Option<CFRetained<CFURL>> {
    return CFURL::from_file_path(path);
}
