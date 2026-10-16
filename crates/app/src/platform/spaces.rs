use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::OnceLock;

use objc2_core_foundation::{CFArray, CFDictionary, CFNumber, CFRetained, CFString, CFType};

use crate::platform::cf_ref;

pub const ALL_SPACES: &str = "*";

const CORE_GRAPHICS: &std::ffi::CStr = c"/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpaceKind {
    User,
    System,
    Fullscreen,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Space {
    pub uuid: String,
    pub id: u64,
    pub kind: SpaceKind,
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct Topology {
    pub current: String,
    pub spaces: Vec<Space>,
    pub fallback: bool,
}

impl Topology {
    pub fn is_active(&self, space_uuid: &str) -> bool {
        if space_uuid == ALL_SPACES {
            return true;
        }
        return self.current == space_uuid;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SurfaceKey {
    pub space: String,
}

impl SurfaceKey {
    pub fn new(space: impl Into<String>) -> Self {
        return Self { space: space.into() };
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceSpec {
    pub key: SurfaceKey,
    pub space_id: Option<u64>,
    pub can_join_all: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Existing {
    pub key: SurfaceKey,
    pub space_id: Option<u64>,
    pub can_join_all: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Create(SurfaceSpec),
    Remove(SurfaceKey),
    Repin(SurfaceKey, Option<u64>),
}

fn all_spaces() -> SurfaceSpec {
    return SurfaceSpec { key: SurfaceKey::new(ALL_SPACES), space_id: None, can_join_all: true };
}

pub fn plan(topology: &Topology) -> Vec<SurfaceSpec> {
    if topology.fallback {
        return vec![all_spaces()];
    }
    let specs: Vec<SurfaceSpec> = topology
        .spaces
        .iter()
        .filter(|space| space.kind == SpaceKind::User)
        .map(|space| SurfaceSpec {
            key: SurfaceKey::new(space.uuid.clone()),
            space_id: Some(space.id),
            can_join_all: false,
        })
        .collect();
    if specs.is_empty() {
        return vec![all_spaces()];
    }
    return specs;
}

pub fn diff(existing: &[Existing], plan: &[SurfaceSpec]) -> Vec<Action> {
    let mut actions = Vec::new();
    for spec in plan {
        match existing.iter().find(|entry| entry.key == spec.key) {
            None => actions.push(Action::Create(spec.clone())),
            Some(entry) => {
                if entry.can_join_all != spec.can_join_all {
                    actions.push(Action::Remove(spec.key.clone()));
                    actions.push(Action::Create(spec.clone()));
                } else if entry.space_id != spec.space_id {
                    actions.push(Action::Repin(spec.key.clone(), spec.space_id));
                }
            }
        }
    }
    for entry in existing {
        if !plan.iter().any(|spec| spec.key == entry.key) {
            actions.push(Action::Remove(entry.key.clone()));
        }
    }
    return actions;
}

type CgsMainConnectionId = unsafe extern "C" fn() -> u32;
type CgsCopyManagedDisplaySpaces = unsafe extern "C" fn(u32) -> *mut c_void;
type CgsAddWindowsToSpaces = unsafe extern "C" fn(u32, *const c_void, *const c_void);
type CgsRemoveWindowsFromSpaces = unsafe extern "C" fn(u32, *const c_void, *const c_void);

#[derive(Clone, Copy)]
struct FnPtrs {
    main_connection: CgsMainConnectionId,
    copy_display_spaces: CgsCopyManagedDisplaySpaces,
    add_windows: CgsAddWindowsToSpaces,
    remove_windows: CgsRemoveWindowsFromSpaces,
}

static SYMBOLS: OnceLock<Option<FnPtrs>> = OnceLock::new();

#[derive(Clone, Copy)]
pub struct Cgs {
    ptrs: Option<FnPtrs>,
}

impl Cgs {
    pub fn load() -> Self {
        let ptrs = *SYMBOLS.get_or_init(resolve);
        return Self { ptrs };
    }

    #[cfg(test)]
    pub fn is_available(&self) -> bool {
        return self.ptrs.is_some();
    }

    fn connection(&self) -> u32 {
        let Some(ptrs) = self.ptrs else { return 0 };
        return unsafe { (ptrs.main_connection)() };
    }

    pub fn topology(&self, main_uuid: &str) -> Topology {
        let Some(ptrs) = self.ptrs else {
            return Topology { current: String::new(), spaces: Vec::new(), fallback: true };
        };
        let cid = self.connection();
        let raw = unsafe { (ptrs.copy_display_spaces)(cid) };
        let Some(ptr) = NonNull::new(raw.cast::<CFArray>()) else {
            return Topology { current: String::new(), spaces: Vec::new(), fallback: true };
        };
        let array = unsafe { CFRetained::from_raw(ptr) };
        let entries: &CFArray<CFType> = unsafe { array.cast_unchecked() };
        let Some(display) = parse_displays(entries).into_iter().find(|display| display.uuid == main_uuid) else {
            return Topology { current: String::new(), spaces: Vec::new(), fallback: true };
        };
        return Topology { current: display.current, spaces: display.spaces, fallback: false };
    }

    pub fn pin_window(&self, window: u32, space_id: u64) {
        self.move_windows(&[window], &[space_id], true);
    }

    pub fn unpin_window(&self, window: u32, space_id: u64) {
        self.move_windows(&[window], &[space_id], false);
    }

    fn move_windows(&self, windows: &[u32], spaces: &[u64], add: bool) {
        let Some(ptrs) = self.ptrs else { return };
        let cid = self.connection();
        let window_numbers: Vec<CFRetained<CFNumber>> =
            windows.iter().map(|id| CFNumber::new_i64(i64::from(*id))).collect();
        let space_numbers: Vec<CFRetained<CFNumber>> =
            spaces.iter().map(|id| CFNumber::new_i64(i64::try_from(*id).unwrap_or(i64::MAX))).collect();
        let window_refs: Vec<&CFType> = window_numbers.iter().map(|n| cf_ref(&**n)).collect();
        let space_refs: Vec<&CFType> = space_numbers.iter().map(|n| cf_ref(&**n)).collect();
        let window_array = CFArray::<CFType>::from_objects(&window_refs);
        let space_array = CFArray::<CFType>::from_objects(&space_refs);
        let window_ptr = CFRetained::as_ptr(&window_array).as_ptr().cast::<c_void>();
        let space_ptr = CFRetained::as_ptr(&space_array).as_ptr().cast::<c_void>();
        unsafe {
            if add {
                (ptrs.add_windows)(cid, window_ptr, space_ptr);
            } else {
                (ptrs.remove_windows)(cid, window_ptr, space_ptr);
            }
        }
    }
}

fn resolve() -> Option<FnPtrs> {
    unsafe {
        let handle = libc::dlopen(CORE_GRAPHICS.as_ptr(), libc::RTLD_LAZY);
        if handle.is_null() {
            return None;
        }
        let main_connection = libc::dlsym(handle, c"CGSMainConnectionID".as_ptr());
        let copy_display_spaces = libc::dlsym(handle, c"CGSCopyManagedDisplaySpaces".as_ptr());
        let add_windows = libc::dlsym(handle, c"CGSAddWindowsToSpaces".as_ptr());
        let remove_windows = libc::dlsym(handle, c"CGSRemoveWindowsFromSpaces".as_ptr());
        if main_connection.is_null()
            || copy_display_spaces.is_null()
            || add_windows.is_null()
            || remove_windows.is_null()
        {
            return None;
        }
        return Some(FnPtrs {
            main_connection: std::mem::transmute::<*mut c_void, CgsMainConnectionId>(main_connection),
            copy_display_spaces: std::mem::transmute::<*mut c_void, CgsCopyManagedDisplaySpaces>(copy_display_spaces),
            add_windows: std::mem::transmute::<*mut c_void, CgsAddWindowsToSpaces>(add_windows),
            remove_windows: std::mem::transmute::<*mut c_void, CgsRemoveWindowsFromSpaces>(remove_windows),
        });
    }
}

struct DisplayTopology {
    uuid: String,
    current: String,
    spaces: Vec<Space>,
}

fn parse_displays(entries: &CFArray<CFType>) -> Vec<DisplayTopology> {
    let mut displays = Vec::new();
    for entry in entries.iter() {
        let Some(dict) = downcast_dict(&entry) else { continue };
        if let Some(display) = parse_display(dict) {
            displays.push(display);
        }
    }
    return displays;
}

fn parse_display(dict: &CFDictionary<CFString, CFType>) -> Option<DisplayTopology> {
    let identifier_key = CFString::from_str("Display Identifier");
    let spaces_key = CFString::from_str("Spaces");
    let current_key = CFString::from_str("Current Space");

    let uuid = dict.get(&identifier_key).and_then(|value| cf_string(&value))?;

    let current_entry = dict.get(&current_key);
    let current_dict = current_entry.as_ref().and_then(|value| downcast_dict(value));
    let current = current_dict
        .and_then(|current| {
            let key = CFString::from_str("uuid");
            current.get(&key)
        })
        .and_then(|value| cf_string(&value))
        .unwrap_or_default();

    let mut spaces = Vec::new();
    let spaces_entry = dict.get(&spaces_key);
    if let Some(list) = spaces_entry.as_ref().and_then(|value| downcast_array(value)) {
        for entry in list.iter() {
            if let Some(space) = downcast_dict(&entry).and_then(parse_space) {
                spaces.push(space);
            }
        }
    }
    return Some(DisplayTopology { uuid, current, spaces });
}

fn parse_space(dict: &CFDictionary<CFString, CFType>) -> Option<Space> {
    let uuid = dict.get(&CFString::from_str("uuid")).and_then(|value| cf_string(&value))?;
    let id = dict.get(&CFString::from_str("id64")).and_then(|value| cf_u64(&value));
    let kind =
        dict.get(&CFString::from_str("type")).and_then(|value| cf_i64(&value)).map_or(SpaceKind::User, |value| {
            match value {
                2 => SpaceKind::System,
                4 => SpaceKind::Fullscreen,
                _ => SpaceKind::User,
            }
        });
    return Some(Space { uuid, id: id.unwrap_or(0), kind });
}

fn downcast_dict(value: &CFType) -> Option<&CFDictionary<CFString, CFType>> {
    let dict = value.downcast_ref::<CFDictionary>()?;
    return Some(unsafe { dict.cast_unchecked::<CFString, CFType>() });
}

fn downcast_array(value: &CFType) -> Option<&CFArray<CFType>> {
    let array = value.downcast_ref::<CFArray>()?;
    return Some(unsafe { array.cast_unchecked() });
}

fn cf_string(value: &CFType) -> Option<String> {
    return value.downcast_ref::<CFString>().map(ToString::to_string);
}

fn cf_i64(value: &CFType) -> Option<i64> {
    return value.downcast_ref::<CFNumber>().and_then(CFNumber::as_i64);
}

fn cf_u64(value: &CFType) -> Option<u64> {
    return cf_i64(value).and_then(|value| u64::try_from(value).ok());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space(uuid: &str, id: u64, kind: i64) -> Space {
        return Space {
            uuid: uuid.into(),
            id,
            kind: match kind {
                2 => SpaceKind::System,
                4 => SpaceKind::Fullscreen,
                _ => SpaceKind::User,
            },
        };
    }

    fn topology(current: &str, spaces: &[(&str, u64, i64)]) -> Topology {
        return Topology {
            current: current.into(),
            spaces: spaces.iter().map(|(uuid, id, kind)| space(uuid, *id, *kind)).collect(),
            fallback: false,
        };
    }

    #[test]
    fn plan_creates_one_surface_per_user_space() {
        let topology = topology("s1", &[("s1", 10, 0), ("s2", 11, 0), ("sys", 12, 2)]);
        let specs = plan(&topology);
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].key, SurfaceKey::new("s1"));
        assert_eq!(specs[0].space_id, Some(10));
        assert!(!specs[0].can_join_all);
        assert_eq!(specs[1].space_id, Some(11));
    }

    #[test]
    fn plan_falls_back_when_symbols_are_missing() {
        let specs = plan(&Topology { current: String::new(), spaces: Vec::new(), fallback: true });
        assert_eq!(specs.len(), 1);
        assert!(specs[0].can_join_all);
        assert_eq!(specs[0].key, SurfaceKey::new(ALL_SPACES));
    }

    #[test]
    fn plan_falls_back_when_no_user_spaces_exist() {
        let specs = plan(&topology("sys", &[("sys", 12, 2)]));
        assert_eq!(specs.len(), 1);
        assert!(specs[0].can_join_all);
    }

    #[test]
    fn diff_creates_new_and_removes_stale() {
        let specs = plan(&topology("s1", &[("s1", 10, 0)]));
        let existing = vec![
            Existing { key: SurfaceKey::new("s9"), space_id: Some(99), can_join_all: false },
            Existing { key: SurfaceKey::new("s0"), space_id: Some(1), can_join_all: false },
        ];
        let actions = diff(&existing, &specs);
        assert!(actions.contains(&Action::Create(specs[0].clone())));
        assert!(actions.contains(&Action::Remove(SurfaceKey::new("s9"))));
        assert!(actions.contains(&Action::Remove(SurfaceKey::new("s0"))));
    }

    #[test]
    fn diff_repins_when_runtime_id_changes() {
        let specs = plan(&topology("s1", &[("s1", 42, 0)]));
        let existing = vec![Existing { key: SurfaceKey::new("s1"), space_id: Some(7), can_join_all: false }];
        let actions = diff(&existing, &specs);
        assert_eq!(actions, vec![Action::Repin(SurfaceKey::new("s1"), Some(42))]);
    }

    #[test]
    fn diff_recreates_when_collection_behavior_changes() {
        let specs = plan(&Topology { current: String::new(), spaces: Vec::new(), fallback: true });
        let existing = vec![Existing { key: SurfaceKey::new(ALL_SPACES), space_id: None, can_join_all: false }];
        let actions = diff(&existing, &specs);
        assert_eq!(actions, vec![Action::Remove(SurfaceKey::new(ALL_SPACES)), Action::Create(specs[0].clone())]);
    }

    #[test]
    fn active_space_resolution() {
        let topology = topology("s1", &[("s1", 10, 0), ("s2", 11, 0)]);
        assert!(topology.is_active("s1"));
        assert!(!topology.is_active("s2"));
        assert!(topology.is_active(ALL_SPACES));
    }

    #[test]
    fn parse_from_core_foundation_structures() {
        let space = |uuid: &str, id: i64, kind: i64| {
            let keys = [CFString::from_str("uuid"), CFString::from_str("id64"), CFString::from_str("type")];
            let values: [CFRetained<CFType>; 3] = [
                unsafe { CFRetained::cast_unchecked(CFString::from_str(uuid)) },
                unsafe { CFRetained::cast_unchecked(CFNumber::new_i64(id)) },
                unsafe { CFRetained::cast_unchecked(CFNumber::new_i64(kind)) },
            ];
            let refs: Vec<&CFType> = values.iter().map(|v| cf_ref(&**v)).collect();
            let key_refs: Vec<&CFString> = keys.iter().map(|k| &**k).collect();
            return CFDictionary::<CFString, CFType>::from_slices(&key_refs, &refs);
        };

        let spaces = CFArray::<CFType>::from_objects(&[cf_ref(&space("s1", 10, 0)), cf_ref(&space("s2", 11, 2))]);

        let display_keys = [
            CFString::from_str("Display Identifier"),
            CFString::from_str("Spaces"),
            CFString::from_str("Current Space"),
        ];
        let current = {
            let keys = [CFString::from_str("uuid")];
            let uuid: CFRetained<CFType> = unsafe { CFRetained::cast_unchecked(CFString::from_str("s2")) };
            let values: [CFRetained<CFType>; 1] = [uuid];
            let refs: Vec<&CFType> = values.iter().map(|v| cf_ref(&**v)).collect();
            let key_refs: Vec<&CFString> = keys.iter().map(|k| &**k).collect();
            CFDictionary::<CFString, CFType>::from_slices(&key_refs, &refs)
        };
        let display_values: [CFRetained<CFType>; 3] = [
            unsafe { CFRetained::cast_unchecked(CFString::from_str("DISPLAY-UUID")) },
            unsafe { CFRetained::cast_unchecked(spaces) },
            unsafe { CFRetained::cast_unchecked(current) },
        ];
        let display_refs: Vec<&CFType> = display_values.iter().map(|v| cf_ref(&**v)).collect();
        let display_key_refs: Vec<&CFString> = display_keys.iter().map(|k| &**k).collect();
        let display = CFDictionary::<CFString, CFType>::from_slices(&display_key_refs, &display_refs);

        let display_retained: CFRetained<CFType> = unsafe { CFRetained::cast_unchecked(display) };
        let display_arr = CFArray::<CFType>::from_objects(&[cf_ref(&display_retained)]);
        let entries: &CFArray<CFType> = &display_arr;

        let parsed = parse_displays(entries);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].uuid, "DISPLAY-UUID");
        assert_eq!(parsed[0].current, "s2");
        assert_eq!(parsed[0].spaces.len(), 2);
        assert_eq!(parsed[0].spaces[0].uuid, "s1");
        assert_eq!(parsed[0].spaces[0].id, 10);
        assert_eq!(parsed[0].spaces[0].kind, SpaceKind::User);
        assert_eq!(parsed[0].spaces[1].kind, SpaceKind::System);
    }

    #[test]
    fn symbols_resolve_on_this_machine() {
        assert!(Cgs::load().is_available(), "CGS symbols did not resolve");
    }
}
