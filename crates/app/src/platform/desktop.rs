use std::ffi::c_void;
use std::ptr::{self, NonNull};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSScreen, NSWorkspace, NSWorkspaceDesktopImageOptionKey};
use objc2_core_foundation::{
    CFArray, CFData, CFDictionary, CFNumber, CFPropertyListFormat, CFRetained, CFString, CFType,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString};

use crate::cf::{cf_ref, cf_string, file_url};

const HI_SERVICES: &std::ffi::CStr =
    c"/System/Library/Frameworks/ApplicationServices.framework/Frameworks/HIServices.framework/HIServices";

type CopyPicture = unsafe extern "C" fn(u32, i32, *const c_void) -> *mut c_void;
type SetPicture = unsafe extern "C" fn(u32, *const c_void, i32, i32, *const c_void);

#[derive(Clone, Copy)]
struct SpaceFns {
    copy: CopyPicture,
    set: SetPicture,
}

static SYMBOLS: std::sync::OnceLock<Option<SpaceFns>> = std::sync::OnceLock::new();

#[derive(Clone, Copy)]
pub struct DesktopApi {
    space: Option<SpaceFns>,
}

impl DesktopApi {
    pub fn load() -> Self {
        return Self { space: *SYMBOLS.get_or_init(resolve) };
    }

    pub fn per_space(&self) -> bool {
        return self.space.is_some();
    }

    pub fn copy(&self, display: u32, space: &str) -> Option<CFRetained<CFDictionary<CFString, CFType>>> {
        let fns = self.space?;
        let space = CFString::from_str(space);
        let raw = unsafe { (fns.copy)(display, 0, CFRetained::as_ptr(&space).as_ptr().cast()) };
        let pointer = NonNull::new(raw.cast::<CFDictionary>())?;
        let config = unsafe { CFRetained::from_raw(pointer) };
        return Some(unsafe { CFRetained::cast_unchecked(config) });
    }

    pub fn set(&self, display: u32, config: &CFDictionary<CFString, CFType>, space: &str) {
        let Some(fns) = self.space else { return };
        let space = CFString::from_str(space);
        unsafe {
            (fns.set)(display, std::ptr::from_ref(config).cast(), 0, 0, CFRetained::as_ptr(&space).as_ptr().cast());
        }
    }
}

pub fn screen_url(mtm: MainThreadMarker, display: u32) -> Option<String> {
    let screen = screen_for(mtm, display)?;
    let url = NSWorkspace::sharedWorkspace().desktopImageURLForScreen(&screen)?;
    return url.absoluteString().map(|value| value.to_string());
}

pub fn screen_set(mtm: MainThreadMarker, display: u32, path: &str) -> Result<(), String> {
    let screen = screen_for(mtm, display).ok_or_else(|| format!("no screen for display {display}"))?;
    let url = file_url(path);
    let options = NSDictionary::<NSWorkspaceDesktopImageOptionKey, AnyObject>::new();
    return unsafe {
        NSWorkspace::sharedWorkspace().setDesktopImageURL_forScreen_options_error(&url, &screen, &options)
    }
    .map_err(|error| error.localizedDescription().to_string());
}

fn resolve() -> Option<SpaceFns> {
    unsafe {
        let handle = libc::dlopen(HI_SERVICES.as_ptr(), libc::RTLD_LAZY);
        if handle.is_null() {
            return None;
        }
        let copy = libc::dlsym(handle, c"DesktopPictureCopyDisplayForSpace".as_ptr());
        let set = libc::dlsym(handle, c"DesktopPictureSetDisplayForSpace".as_ptr());
        if copy.is_null() || set.is_null() {
            return None;
        }
        return Some(SpaceFns {
            copy: std::mem::transmute::<*mut c_void, CopyPicture>(copy),
            set: std::mem::transmute::<*mut c_void, SetPicture>(set),
        });
    }
}

fn screen_for(mtm: MainThreadMarker, display: u32) -> Option<Retained<NSScreen>> {
    let screens = NSScreen::screens(mtm);
    for index in 0..screens.count() {
        let screen = screens.objectAtIndex(index);
        let description = screen.deviceDescription();
        if let Some(value) = description.objectForKey(&NSString::from_str("NSScreenNumber"))
            && let Some(number) = value.downcast_ref::<NSNumber>()
            && number.unsignedIntValue() == display
        {
            return Some(screen);
        }
    }
    return None;
}

pub fn encode(config: &CFDictionary<CFString, CFType>) -> Option<String> {
    let plist: &CFType = cf_ref(config);
    let data = unsafe {
        objc2_core_foundation::CFPropertyListCreateData(
            None,
            Some(plist),
            CFPropertyListFormat::BinaryFormat_v1_0,
            0,
            ptr::null_mut(),
        )
    }?;
    let bytes = unsafe { std::slice::from_raw_parts(data.byte_ptr(), usize::try_from(data.length()).ok()?) };
    return Some(STANDARD.encode(bytes));
}

pub fn decode(encoded: &str) -> Option<CFRetained<CFDictionary<CFString, CFType>>> {
    let bytes = STANDARD.decode(encoded).ok()?;
    let data = unsafe { CFData::new(None, bytes.as_ptr(), isize::try_from(bytes.len()).ok()?) }?;
    let plist = unsafe {
        objc2_core_foundation::CFPropertyListCreateWithData(None, Some(&data), 0, ptr::null_mut(), ptr::null_mut())
    }?;
    let dict = plist.downcast::<CFDictionary>().ok()?;
    return Some(unsafe { CFRetained::cast_unchecked(dict) });
}

pub fn image_path(config: &CFDictionary<CFString, CFType>) -> Option<String> {
    let key = CFString::from_str("ImageFilePath");
    return cf_string(config.get(&key).as_deref());
}

pub fn is_empty(config: &CFDictionary<CFString, CFType>) -> bool {
    return config.len() == 0;
}

pub fn still_config(path: &str) -> CFRetained<CFDictionary<CFString, CFType>> {
    let key_path = CFString::from_str("ImageFilePath");
    let key_placement = CFString::from_str("Placement");
    let key_color = CFString::from_str("BackgroundColor");
    let path = CFString::from_str(path);
    let placement = CFString::from_str("FillScreen");
    let black = [CFNumber::new_i64(0), CFNumber::new_i64(0), CFNumber::new_i64(0)];
    let color_refs: Vec<&CFType> = black.iter().map(|value| cf_ref(&**value)).collect();
    let color: CFRetained<CFType> = unsafe { CFRetained::cast_unchecked(CFArray::<CFType>::from_objects(&color_refs)) };
    let keys: [&CFString; 3] = [&key_path, &key_placement, &key_color];
    let values: [&CFType; 3] = [cf_ref(&*path), cf_ref(&*placement), cf_ref(&*color)];
    return CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_resolve_on_this_machine() {
        assert!(DesktopApi::load().per_space(), "HIServices desktop-picture symbols did not resolve");
    }

    #[test]
    fn config_round_trips_through_base64() {
        let config = still_config("/tmp/a.png");
        let encoded = encode(&config).expect("encode");
        let decoded = decode(&encoded).expect("decode");
        assert_eq!(image_path(&decoded).as_deref(), Some("/tmp/a.png"));
        assert!(!is_empty(&decoded));
    }

    #[test]
    fn empty_config_is_recognized() {
        let empty = CFDictionary::<CFString, CFType>::from_slices(&[], &[]);
        assert!(is_empty(&empty));
    }
}
