use std::ffi::c_void;
use std::ptr;

use proto::KeyCombo;

use crate::hotkey;

#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

#[repr(C)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

type EventHandlerUpp = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;

const KEYBOARD_CLASS: u32 = u32::from_be_bytes(*b"keyb");
const HOTKEY_PRESSED: u32 = 5;
const PARAM_DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");
const TYPE_HOTKEY_ID: u32 = u32::from_be_bytes(*b"hkid");
const EVENT_NOT_HANDLED_ERR: i32 = -9874;
const SIGNATURE: u32 = u32::from_be_bytes(*b"LWui");

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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    PlayPause,
    Mute,
    Random,
}

impl Command {
    pub const ALL: [Command; 7] = [
        Command::Next,
        Command::Previous,
        Command::VolumeUp,
        Command::VolumeDown,
        Command::PlayPause,
        Command::Mute,
        Command::Random,
    ];

    pub fn title(self) -> &'static str {
        return match self {
            Command::Next => "Next Wallpaper",
            Command::Previous => "Previous Wallpaper",
            Command::VolumeUp => "Volume Up",
            Command::VolumeDown => "Volume Down",
            Command::PlayPause => "Play / Pause",
            Command::Mute => "Mute",
            Command::Random => "Random Wallpaper",
        };
    }

    fn default_combo(self) -> Option<KeyCombo> {
        let key = match self {
            Command::Next => "right",
            Command::Previous => "left",
            Command::VolumeUp => "up",
            Command::VolumeDown => "down",
            Command::PlayPause => "space",
            Command::Mute => "m",
            Command::Random => "r",
        };
        return hotkey::parse(&format!("ctrl+opt+cmd+{key}")).ok();
    }

    fn index(self) -> usize {
        return Command::ALL.iter().position(|command| *command == self).unwrap_or(0);
    }

    fn id(self) -> u32 {
        return u32::try_from(self.index()).unwrap_or(0) + 1;
    }

    fn from_id(id: u32) -> Option<Command> {
        return Command::ALL.into_iter().find(|command| command.id() == id);
    }
}

unsafe extern "C" fn pressed(_call: *mut c_void, event: *mut c_void, _user: *mut c_void) -> i32 {
    let mut id = EventHotKeyId { signature: 0, id: 0 };
    let status = unsafe {
        GetEventParameter(
            event,
            PARAM_DIRECT_OBJECT,
            TYPE_HOTKEY_ID,
            ptr::null_mut(),
            size_of::<EventHotKeyId>(),
            ptr::null_mut(),
            ptr::from_mut(&mut id).cast(),
        )
    };
    if status != 0 || id.signature != SIGNATURE {
        return EVENT_NOT_HANDLED_ERR;
    }
    if let Some(command) = Command::from_id(id.id) {
        crate::ui::command(command);
    }
    return 0;
}

pub struct Hotkeys {
    combos: Vec<Option<KeyCombo>>,
    refs: Vec<*mut c_void>,
    target: *mut c_void,
}

impl Hotkeys {
    pub fn install() -> Self {
        let target = unsafe { GetApplicationEventTarget() };
        if !target.is_null() {
            let spec = EventTypeSpec { event_class: KEYBOARD_CLASS, event_kind: HOTKEY_PRESSED };
            let mut handler: *mut c_void = ptr::null_mut();
            unsafe { InstallEventHandler(target, pressed, 1, &raw const spec, ptr::null_mut(), &raw mut handler) };
        }
        let mut this = Self { combos: Vec::new(), refs: Vec::new(), target };
        this.restore();
        return this;
    }

    pub fn combo(&self, command: Command) -> Option<&KeyCombo> {
        return self.combos.get(command.index()).and_then(Option::as_ref);
    }

    pub fn restore(&mut self) {
        self.combos = Command::ALL.iter().map(|command| command.default_combo()).collect();
        self.resume();
    }

    pub fn suspend(&mut self) {
        for reference in self.refs.drain(..) {
            unsafe { UnregisterEventHotKey(reference) };
        }
    }

    pub fn resume(&mut self) {
        self.suspend();
        if self.target.is_null() {
            return;
        }
        for (command, combo) in Command::ALL.iter().zip(&self.combos) {
            let Some((code, combo)) = combo.as_ref().and_then(|combo| Some((hotkey::keycode(&combo.key)?, combo)))
            else {
                continue;
            };
            let id = EventHotKeyId { signature: SIGNATURE, id: command.id() };
            let mut reference: *mut c_void = ptr::null_mut();
            let status = unsafe {
                RegisterEventHotKey(
                    code,
                    hotkey::modifier_mask(&combo.modifiers),
                    id,
                    self.target,
                    0,
                    &raw mut reference,
                )
            };
            if status == 0 {
                self.refs.push(reference);
            }
        }
    }

    pub fn bind(&mut self, command: Command, combo: Option<KeyCombo>) -> Result<(), String> {
        if let Some(combo) = &combo {
            let mask = hotkey::modifier_mask(&combo.modifiers);
            let function = combo.key.strip_prefix('f').is_some_and(|digits| {
                return !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit());
            });
            if mask & (hotkey::CMD | hotkey::CONTROL | hotkey::OPTION) == 0 && !function {
                return Err("Add ⌃, ⌥ or ⌘".to_string());
            }
            if let Some(owner) = hotkey::reserved(combo) {
                return Err(format!("Used by {owner}"));
            }
            for other in &mut self.combos {
                if other.as_ref().is_some_and(|other| hotkey::same(other, combo)) {
                    *other = None;
                }
            }
        }
        if let Some(slot) = self.combos.get_mut(command.index()) {
            *slot = combo;
        }
        return Ok(());
    }
}
