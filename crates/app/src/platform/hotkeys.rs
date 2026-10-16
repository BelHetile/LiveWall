use std::collections::BTreeMap;
use std::ffi::c_void;
use std::ptr;

use proto::{HotkeyAction, KeyCombo};

use crate::app::{Event, post};
use crate::hotkey;

#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

type EventHandlerUpp = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;

const KEYBOARD_CLASS: u32 = u32::from_be_bytes(*b"keyb");
const HOTKEY_PRESSED: u32 = 5;
const PARAM_DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");
const TYPE_HOTKEY_ID: u32 = u32::from_be_bytes(*b"hkid");
const HOTKEY_EXCLUSIVE: u32 = 1;
const EVENT_HOT_KEY_EXISTS_ERR: i32 = -9878;
const EVENT_NOT_HANDLED_ERR: i32 = -9874;
const PARAM_ERR: i32 = -50;
const SIGNATURE: u32 = u32::from_be_bytes(*b"LWr1");
#[cfg(feature = "test-support")]
const ATTRIBUTE_NONE: u32 = 0;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: EventHandlerUpp,
        num_types: usize,
        list: *const EventTypeSpec,
        user_data: *mut c_void,
        out_ref: *mut *mut c_void,
    ) -> i32;
    fn RemoveEventHandler(handler: *mut c_void) -> i32;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        id: EventHotKeyId,
        target: *mut c_void,
        options: u32,
        out_ref: *mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(reference: *mut c_void) -> i32;
    fn GetEventParameter(
        event: *mut c_void,
        name: u32,
        kind: u32,
        actual_type: *mut u32,
        buffer_size: usize,
        actual_size: *mut usize,
        data: *mut c_void,
    ) -> i32;
}

#[cfg(feature = "test-support")]
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn CreateEvent(
        allocator: *mut c_void,
        class_id: u32,
        kind: u32,
        when: f64,
        attributes: u32,
        out_event: *mut *mut c_void,
    ) -> i32;
    fn SetEventParameter(event: *mut c_void, name: u32, kind: u32, size: usize, data: *const c_void) -> i32;
    fn SendEventToEventTarget(event: *mut c_void, target: *mut c_void) -> i32;
    fn ReleaseEvent(event: *mut c_void) -> i32;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterError {
    TakenByOtherApp,
    Failed(i32),
}

impl RegisterError {
    fn from_status(status: i32) -> Self {
        if status == EVENT_HOT_KEY_EXISTS_ERR {
            return Self::TakenByOtherApp;
        }
        return Self::Failed(status);
    }
}

unsafe extern "C" fn event_handler(_call: *mut c_void, event: *mut c_void, _user: *mut c_void) -> i32 {
    return unsafe {
        let mut id = EventHotKeyId { signature: 0, id: 0 };
        let status = GetEventParameter(
            event,
            PARAM_DIRECT_OBJECT,
            TYPE_HOTKEY_ID,
            ptr::null_mut(),
            size_of::<EventHotKeyId>(),
            ptr::null_mut(),
            ptr::from_mut(&mut id).cast(),
        );
        if status != 0 {
            return EVENT_NOT_HANDLED_ERR;
        }
        if let Some(action) = hotkey::action_from_id(id.id) {
            post(Event::Hotkey(action));
        }
        0
    };
}

pub struct Hotkeys {
    target: *mut c_void,
    handler: *mut c_void,
    refs: BTreeMap<HotkeyAction, *mut c_void>,
    combos: BTreeMap<HotkeyAction, KeyCombo>,
}

impl Hotkeys {
    pub fn install() -> Self {
        let target = unsafe { GetApplicationEventTarget() };
        let mut handler: *mut c_void = ptr::null_mut();
        if !target.is_null() {
            let spec = EventTypeSpec { event_class: KEYBOARD_CLASS, event_kind: HOTKEY_PRESSED };
            let status = unsafe {
                InstallEventHandler(
                    target,
                    event_handler,
                    1,
                    &raw const spec,
                    ptr::null_mut(),
                    ptr::from_mut(&mut handler),
                )
            };
            if status != 0 {
                handler = ptr::null_mut();
            }
        }
        return Self { target, handler, refs: BTreeMap::new(), combos: BTreeMap::new() };
    }

    pub fn register(&mut self, action: HotkeyAction, combo: &KeyCombo) -> Result<(), RegisterError> {
        if let Some(current) = self.combos.get(&action)
            && hotkey::same(current, combo)
        {
            return Ok(());
        }
        let Some(code) = hotkey::keycode(&combo.key) else {
            return Err(RegisterError::Failed(PARAM_ERR));
        };
        let mask = hotkey::modifier_mask(&combo.modifiers);
        let id = EventHotKeyId { signature: SIGNATURE, id: hotkey::registration_id(action) };
        let mut reference: *mut c_void = ptr::null_mut();
        let status = unsafe {
            RegisterEventHotKey(code, mask, id, self.target, HOTKEY_EXCLUSIVE, ptr::from_mut(&mut reference))
        };
        if status != 0 {
            return Err(RegisterError::from_status(status));
        }
        if let Some(old) = self.refs.insert(action, reference) {
            unsafe { UnregisterEventHotKey(old) };
        }
        self.combos.insert(action, combo.clone());
        return Ok(());
    }

    pub fn unregister(&mut self, action: HotkeyAction) {
        self.combos.remove(&action);
        if let Some(reference) = self.refs.remove(&action) {
            unsafe { UnregisterEventHotKey(reference) };
        }
    }

    pub fn is_registered(&self, action: HotkeyAction) -> bool {
        return self.refs.contains_key(&action);
    }
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        for reference in std::mem::take(&mut self.refs).into_values() {
            unsafe { UnregisterEventHotKey(reference) };
        }
        if !self.handler.is_null() {
            unsafe { RemoveEventHandler(self.handler) };
        }
    }
}

#[cfg(feature = "test-support")]
pub fn synthesize_press(action: HotkeyAction) -> bool {
    return unsafe {
        let target = GetApplicationEventTarget();
        if target.is_null() {
            return false;
        }
        let mut event: *mut c_void = ptr::null_mut();
        if CreateEvent(ptr::null_mut(), KEYBOARD_CLASS, HOTKEY_PRESSED, 0.0, ATTRIBUTE_NONE, ptr::from_mut(&mut event))
            != 0
            || event.is_null()
        {
            return false;
        }
        let id = EventHotKeyId { signature: SIGNATURE, id: hotkey::registration_id(action) };
        let parameter = SetEventParameter(
            event,
            PARAM_DIRECT_OBJECT,
            TYPE_HOTKEY_ID,
            size_of::<EventHotKeyId>(),
            ptr::from_ref(&id).cast(),
        );
        let status = if parameter == 0 { SendEventToEventTarget(event, target) } else { parameter };
        ReleaseEvent(event);
        status == 0
    };
}
