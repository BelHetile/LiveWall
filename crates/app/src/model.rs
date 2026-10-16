use std::collections::BTreeMap;

use proto::{FillMode, Geometry, Hotkeys, Transition};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const LIBRARY_VERSION: u32 = 1;

pub type ItemId = Uuid;
pub type FolderId = Uuid;

fn default_true() -> bool {
    return true;
}

fn default_one() -> f32 {
    return 1.0;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ItemKind {
    #[default]
    Video,
    Gif,
    Image,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct Item {
    pub id: ItemId,
    pub kind: ItemKind,
    pub path: String,
    pub title: String,
    pub added_at: jiff::Timestamp,
    #[serde(default)]
    pub folder: Option<FolderId>,
    #[serde(default)]
    pub mode: FillMode,
    #[serde(default)]
    pub modes: BTreeMap<FillMode, Geometry>,
    #[serde(default = "default_one")]
    pub volume: f32,
    #[serde(default = "default_one")]
    pub speed: f32,
    #[serde(default = "default_true")]
    pub muted: bool,
    #[serde(default)]
    pub still_time: f32,
    #[serde(default)]
    pub optimized: bool,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub fps: f64,
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub audio_codec: Option<String>,
    #[serde(default)]
    pub hdr: bool,
    #[serde(default = "default_true")]
    pub playable: bool,
    #[serde(default)]
    pub needs_optimize: bool,
    #[serde(default)]
    pub optimize_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    pub id: FolderId,
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub hidden: bool,
}

impl Item {
    pub fn geometry(&self) -> Geometry {
        return self.modes.get(&self.mode).copied().unwrap_or_default();
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Assignment {
    #[serde(default)]
    pub item: Option<ItemId>,
    #[serde(default)]
    pub geometry_override: Option<Geometry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LockAssignment {
    pub item: ItemId,
    #[serde(default)]
    pub time: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Placement {
    #[serde(default)]
    pub default: Assignment,
    #[serde(default)]
    pub spaces: BTreeMap<String, Assignment>,
    #[serde(default)]
    pub lock: Option<LockAssignment>,
    #[serde(default)]
    pub safe_default: Option<ItemId>,
    #[serde(default)]
    pub safe: BTreeMap<String, ItemId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub v: u32,
    pub items: Vec<Item>,
    pub folders: Vec<Folder>,
    pub placement: Placement,
}

impl Default for Library {
    fn default() -> Self {
        return Self { v: LIBRARY_VERSION, items: Vec::new(), folders: Vec::new(), placement: Placement::default() };
    }
}

impl Library {
    pub fn create_folder(&mut self, name: String) -> FolderId {
        let id = Uuid::new_v4();
        self.folders.push(Folder { id, name, icon: None, locked: false, hidden: false });
        return id;
    }

    pub fn is_locked_folder(&self, folder: Option<FolderId>) -> bool {
        return folder.is_some_and(|id| self.folders.iter().any(|candidate| candidate.id == id && candidate.locked));
    }

    pub fn is_private(&self, item: &Item) -> bool {
        return self.is_locked_folder(item.folder);
    }

    pub fn is_private_id(&self, id: ItemId) -> bool {
        return self.items.iter().find(|item| item.id == id).is_some_and(|item| self.is_private(item));
    }

    pub fn folder_items(&self, folder: FolderId) -> Vec<ItemId> {
        return self.items.iter().filter(|item| item.folder == Some(folder)).map(|item| item.id).collect();
    }

    pub fn set_folder_flags(&mut self, id: FolderId, locked: Option<bool>, hidden: Option<bool>) -> bool {
        let Some(folder) = self.folders.iter_mut().find(|folder| folder.id == id) else {
            return false;
        };
        if let Some(locked) = locked {
            folder.locked = locked;
        }
        if let Some(hidden) = hidden {
            folder.hidden = hidden;
        }
        return true;
    }

    pub fn replacement(&self, space: Option<&str>) -> Option<ItemId> {
        let remembered = match space {
            Some(space) => self.placement.safe.get(space).copied(),
            None => self.placement.safe_default,
        };
        let usable = |id: ItemId| {
            return self
                .items
                .iter()
                .find(|item| item.id == id)
                .is_some_and(|item| item.playable && !self.is_private(item));
        };
        return remembered
            .filter(|id| usable(*id))
            .or_else(|| self.items.iter().find(|item| item.playable && !self.is_private(item)).map(|item| item.id));
    }

    pub fn resolve(&self, space: Option<&str>, reveal: bool) -> Option<ItemId> {
        let scoped = space.and_then(|space| {
            let item = self.placement.spaces.get(space)?.item?;
            return Some((item, Some(space)));
        });
        let (assigned, key) = scoped.or_else(|| self.placement.default.item.map(|item| (item, None)))?;
        if reveal || !self.is_private_id(assigned) {
            return Some(assigned);
        }
        return self.replacement(key);
    }

    pub fn blanked(&self, space: &str) -> bool {
        return self.resolve(Some(space), false).is_none() && self.resolve(Some(space), true).is_some();
    }

    pub fn remember(&mut self, space: Option<&str>, item: Option<ItemId>) {
        let Some(item) = item.filter(|id| !self.is_private_id(*id)) else {
            return;
        };
        match space {
            Some(space) => {
                self.placement.safe.insert(space.to_string(), item);
            }
            None => self.placement.safe_default = Some(item),
        }
    }

    pub fn forget_safe(&mut self, removed: &[ItemId]) {
        self.placement.safe_default = self.placement.safe_default.filter(|id| !removed.contains(id));
        self.placement.safe.retain(|_, id| !removed.contains(id));
    }

    pub fn rename_item(&mut self, id: ItemId, title: String) -> bool {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        item.title = title;
        return true;
    }

    pub fn move_items(&mut self, ids: &[ItemId], folder: Option<FolderId>) -> usize {
        let mut moved = 0usize;
        for item in &mut self.items {
            if ids.contains(&item.id) {
                item.folder = folder;
                moved += 1;
            }
        }
        return moved;
    }

    pub fn has_folder(&self, id: FolderId) -> bool {
        return self.folders.iter().any(|folder| folder.id == id);
    }

    pub fn rename_folder(&mut self, id: FolderId, name: String) -> bool {
        let Some(folder) = self.folders.iter_mut().find(|folder| folder.id == id) else {
            return false;
        };
        folder.name = name;
        return true;
    }

    pub fn delete_folder(&mut self, id: FolderId) -> Option<usize> {
        let before = self.folders.len();
        self.folders.retain(|folder| folder.id != id);
        if self.folders.len() == before {
            return None;
        }
        let mut moved = 0usize;
        for item in &mut self.items {
            if item.folder == Some(id) {
                item.folder = None;
                moved += 1;
            }
        }
        return Some(moved);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    En,
    Ru,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RuleScope {
    #[default]
    Running,
    Frontmost,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppRule {
    pub bundle_id: String,
    #[serde(default)]
    pub scope: RuleScope,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tools {
    pub install_consent: bool,
}

impl Default for Tools {
    fn default() -> Self {
        return Self { install_consent: false };
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Privacy {
    pub lock_on_screen_lock: bool,
    pub lock_on_display: bool,
    pub lock_on_idle: bool,
    pub idle_minutes: u32,
}

impl Default for Privacy {
    fn default() -> Self {
        return Self { lock_on_screen_lock: true, lock_on_display: true, lock_on_idle: false, idle_minutes: 15 };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AppearanceMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub mode: AppearanceMode,
    pub theme: String,
    pub accent: String,
    pub canvas: String,
    pub list: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        return Self {
            mode: AppearanceMode::System,
            theme: "Violet".to_string(),
            accent: "#866A8C".to_string(),
            canvas: "#947C8E".to_string(),
            list: false,
        };
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[allow(clippy::struct_excessive_bools)]
pub struct Settings {
    pub muted: bool,
    pub volume: f32,
    pub speed: f32,
    pub auto_rotate: bool,
    pub rotate_minutes: u32,
    pub shuffle: bool,
    pub pause_when_focused: bool,
    pub pause_when_maximized: bool,
    pub pause_when_fullscreen: bool,
    pub pause_when_hot: bool,
    pub app_rules: Vec<AppRule>,
    pub optimize_on_import: bool,
    pub optimize_fps_limit: u32,
    pub stop_after_seconds: u32,
    pub launch_at_login: bool,
    pub open_library_at_launch: bool,
    pub show_in_dock: bool,
    pub language: Option<Language>,
    pub hotkeys: Hotkeys,
    pub tools: Tools,
    pub transition: Transition,
    pub appearance: Appearance,
    pub privacy: Privacy,
}

impl Default for Settings {
    fn default() -> Self {
        return Self {
            muted: true,
            volume: 1.0,
            speed: 1.0,
            auto_rotate: false,
            rotate_minutes: 30,
            shuffle: false,
            pause_when_focused: false,
            pause_when_maximized: true,
            pause_when_fullscreen: true,
            pause_when_hot: true,
            app_rules: Vec::new(),
            optimize_on_import: true,
            optimize_fps_limit: 30,
            stop_after_seconds: 30,
            launch_at_login: false,
            open_library_at_launch: true,
            show_in_dock: true,
            language: None,
            hotkeys: Hotkeys::default(),
            tools: Tools::default(),
            transition: Transition::default(),
            appearance: Appearance::default(),
            privacy: Privacy::default(),
        };
    }
}

impl Settings {
    pub(crate) fn normalize(&mut self) {
        self.optimize_fps_limit = self.optimize_fps_limit.clamp(15, 120);
        self.privacy.idle_minutes = self.privacy.idle_minutes.clamp(1, 1440);
        self.transition.normalize();
    }
}

pub fn apply_patch(settings: &mut Settings, patch: &Value) -> Result<(), String> {
    let mut base = serde_json::to_value(&*settings).map_err(|error| error.to_string())?;
    merge_patch(&mut base, patch);
    let mut merged: Settings = serde_json::from_value(base).map_err(|error| error.to_string())?;
    merged.normalize();
    *settings = merged;
    return Ok(());
}

fn merge_patch(target: &mut Value, patch: &Value) {
    let Some(patch_map) = patch.as_object() else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = Value::Object(serde_json::Map::new());
    }
    let Some(target_map) = target.as_object_mut() else {
        return;
    };
    for (key, value) in patch_map {
        if value.is_null() {
            target_map.remove(key);
        } else {
            merge_patch(target_map.entry(key.clone()).or_insert(Value::Null), value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_defaults_from_empty_object() {
        let settings: Settings = serde_json::from_str("{}").expect("empty object");
        assert!(settings.muted);
        assert!((settings.volume - 1.0).abs() < f32::EPSILON);
        assert!((settings.speed - 1.0).abs() < f32::EPSILON);
        assert!(!settings.auto_rotate);
        assert_eq!(settings.rotate_minutes, 30);
        assert!(!settings.shuffle);
        assert!(settings.pause_when_maximized);
        assert!(settings.pause_when_fullscreen);
        assert!(settings.pause_when_hot);
        assert!(!settings.pause_when_focused);
        assert!(settings.optimize_on_import);
        assert_eq!(settings.optimize_fps_limit, 30);
        assert_eq!(settings.stop_after_seconds, 30);
        assert!(!settings.launch_at_login);
        assert!(settings.language.is_none());
        assert_eq!(settings.hotkeys, Hotkeys::default());
    }

    #[test]
    fn settings_partial_object_keeps_other_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"volume":0.5,"shuffle":true}"#).expect("partial");
        assert!((settings.volume - 0.5).abs() < f32::EPSILON);
        assert!(settings.shuffle);
        assert!(settings.muted);
        assert_eq!(settings.stop_after_seconds, 30);
    }

    #[test]
    fn normalize_clamps_fps_limit() {
        let mut settings = Settings { optimize_fps_limit: 999, ..Settings::default() };
        settings.normalize();
        assert_eq!(settings.optimize_fps_limit, 120);
        let mut settings = Settings { optimize_fps_limit: 1, ..Settings::default() };
        settings.normalize();
        assert_eq!(settings.optimize_fps_limit, 15);
    }

    #[test]
    fn library_defaults_from_empty_object() {
        let library: Library = serde_json::from_str("{}").expect("empty object");
        assert_eq!(library, Library::default());
        assert_eq!(library.v, LIBRARY_VERSION);
    }

    #[test]
    fn legacy_per_display_placement_is_dropped() {
        let library: Library = serde_json::from_str(
            r#"{"v":1,"placement":{"default":{"item":null},"displays":{"D":{"item":"11111111-1111-1111-1111-111111111111"}},"spaces":{"S":{"item":null}}}}"#,
        )
        .expect("legacy placement");
        assert_eq!(library.placement.spaces.len(), 1);
        assert!(library.placement.spaces.contains_key("S"));
    }

    #[test]
    fn item_round_trips() {
        let item = Item {
            id: Uuid::nil(),
            kind: ItemKind::Video,
            path: "/tmp/a.mp4".into(),
            title: "A".into(),
            added_at: jiff::Timestamp::UNIX_EPOCH,
            folder: None,
            mode: FillMode::Fill,
            modes: BTreeMap::from([(FillMode::Custom, Geometry { x: 0.1, y: 0.2, zoom: 1.5 })]),
            volume: 0.5,
            speed: 2.0,
            muted: false,
            still_time: 4.0,
            optimized: true,
            width: 3840,
            height: 2160,
            duration: 12.5,
            fps: 30.0,
            codec: Some("hvc1".into()),
            audio_codec: Some("aac".into()),
            hdr: true,
            playable: true,
            needs_optimize: false,
            optimize_reason: None,
        };
        let text = serde_json::to_string(&item).expect("serialize");
        let back: Item = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(item, back);
    }

    fn sample_item() -> Item {
        return Item {
            id: Uuid::new_v4(),
            kind: ItemKind::Video,
            path: "/tmp/a.mp4".into(),
            title: "A".into(),
            added_at: jiff::Timestamp::UNIX_EPOCH,
            folder: None,
            mode: FillMode::Fill,
            modes: BTreeMap::new(),
            volume: 1.0,
            speed: 1.0,
            muted: true,
            still_time: 0.0,
            optimized: false,
            width: 0,
            height: 0,
            duration: 0.0,
            fps: 0.0,
            codec: None,
            audio_codec: None,
            hdr: false,
            playable: true,
            needs_optimize: false,
            optimize_reason: None,
        };
    }

    #[test]
    fn folder_delete_moves_items_to_root() {
        let mut library = Library::default();
        let folder = library.create_folder("Trips".into());
        assert!(library.has_folder(folder));
        let mut item = sample_item();
        item.folder = Some(folder);
        let id = item.id;
        library.items.push(item);
        assert_eq!(library.move_items(&[id], Some(folder)), 1);
        assert_eq!(library.items[0].folder, Some(folder));
        assert!(library.rename_folder(folder, "Holidays".into()));
        assert_eq!(library.folders[0].name, "Holidays");
        assert_eq!(library.delete_folder(folder), Some(1));
        assert!(!library.has_folder(folder));
        assert_eq!(library.items.len(), 1);
        assert_eq!(library.items[0].folder, None);
        assert_eq!(library.delete_folder(folder), None);
    }

    fn private_library() -> (Library, ItemId, ItemId, ItemId) {
        let mut library = Library::default();
        let vault = library.create_folder("Vault".into());
        library.set_folder_flags(vault, Some(true), None);
        let (mut open, mut hidden, mut spare) = (sample_item(), sample_item(), sample_item());
        hidden.folder = Some(vault);
        open.title = "open".into();
        spare.title = "spare".into();
        let ids = (open.id, hidden.id, spare.id);
        library.items.extend([open, hidden, spare]);
        return (library, ids.0, ids.1, ids.2);
    }

    #[test]
    fn folder_flags_default_to_false_for_legacy_folders() {
        let folder: Folder =
            serde_json::from_str(r#"{"id":"11111111-1111-1111-1111-111111111111","name":"x"}"#).expect("legacy folder");
        assert!(!folder.locked);
        assert!(!folder.hidden);
    }

    #[test]
    fn placement_history_defaults_from_legacy_placement() {
        let library: Library = serde_json::from_str(r#"{"placement":{"default":{"item":null}}}"#).expect("legacy");
        assert!(library.placement.safe_default.is_none());
        assert!(library.placement.safe.is_empty());
    }

    #[test]
    fn privacy_defaults_and_patch() {
        let settings: Settings = serde_json::from_str("{}").expect("empty object");
        assert_eq!(settings.privacy, Privacy::default());
        assert!(settings.privacy.lock_on_screen_lock);
        assert!(settings.privacy.lock_on_display);
        assert!(!settings.privacy.lock_on_idle);
        assert_eq!(settings.privacy.idle_minutes, 15);
        let mut settings = settings;
        apply_patch(&mut settings, &serde_json::json!({ "privacy": { "lock_on_idle": true, "idle_minutes": 0 } }))
            .expect("patch");
        assert!(settings.privacy.lock_on_idle);
        assert_eq!(settings.privacy.idle_minutes, 1);
        assert!(settings.privacy.lock_on_display);
        apply_patch(&mut settings, &serde_json::json!({ "privacy": { "idle_minutes": null } })).expect("reset");
        assert_eq!(settings.privacy.idle_minutes, 15);
    }

    #[test]
    fn items_are_private_only_in_locked_folders() {
        let (mut library, open, hidden, _) = private_library();
        assert!(library.is_private_id(hidden));
        assert!(!library.is_private_id(open));
        assert!(!library.is_private_id(Uuid::new_v4()));
        let vault = library.folders[0].id;
        library.set_folder_flags(vault, Some(false), None);
        assert!(!library.is_private_id(hidden));
        assert!(!library.set_folder_flags(Uuid::new_v4(), Some(true), None));
    }

    #[test]
    fn remember_records_only_non_private_items() {
        let (mut library, open, hidden, _) = private_library();
        library.remember(None, Some(hidden));
        library.remember(Some("S"), Some(hidden));
        assert!(library.placement.safe_default.is_none());
        assert!(library.placement.safe.is_empty());
        library.remember(None, Some(open));
        library.remember(Some("S"), Some(open));
        library.remember(Some("S"), None);
        assert_eq!(library.placement.safe_default, Some(open));
        assert_eq!(library.placement.safe.get("S"), Some(&open));
    }

    #[test]
    fn resolve_substitutes_private_items_only_when_hidden() {
        let (mut library, open, hidden, spare) = private_library();
        library.placement.default.item = Some(hidden);
        library.placement.safe_default = Some(spare);
        assert_eq!(library.resolve(Some("S"), true), Some(hidden));
        assert_eq!(library.resolve(Some("S"), false), Some(spare));
        assert_eq!(library.resolve(None, false), Some(spare));
        library.placement.spaces.insert("S".into(), Assignment { item: Some(hidden), geometry_override: None });
        library.placement.safe.insert("S".into(), open);
        assert_eq!(library.resolve(Some("S"), false), Some(open));
        assert_eq!(library.resolve(Some("T"), false), Some(spare));
        library.placement.spaces.insert("S".into(), Assignment { item: Some(spare), geometry_override: None });
        assert_eq!(library.resolve(Some("S"), false), Some(spare));
    }

    #[test]
    fn replacement_falls_back_to_first_public_playable_item_then_nothing() {
        let (mut library, open, hidden, _) = private_library();
        library.placement.default.item = Some(hidden);
        assert_eq!(library.resolve(None, false), Some(open));
        library.placement.safe_default = Some(hidden);
        assert_eq!(library.resolve(None, false), Some(open));
        library.placement.safe_default = Some(Uuid::new_v4());
        assert_eq!(library.resolve(None, false), Some(open));
        assert!(!library.blanked("S"));
        library.items.retain(|item| item.id == hidden);
        assert_eq!(library.resolve(None, false), None);
        assert_eq!(library.resolve(None, true), Some(hidden));
        assert!(library.blanked("S"));
        library.placement.default.item = None;
        assert!(!library.blanked("S"));
    }

    #[test]
    fn replacement_skips_unplayable_items_and_forgotten_history() {
        let (mut library, open, hidden, spare) = private_library();
        library.placement.default.item = Some(hidden);
        library.items[0].playable = false;
        assert_eq!(library.resolve(None, false), Some(spare));
        library.placement.safe_default = Some(spare);
        library.placement.safe.insert("S".into(), open);
        library.forget_safe(&[spare]);
        assert!(library.placement.safe_default.is_none());
        assert_eq!(library.placement.safe.get("S"), Some(&open));
        library.forget_safe(&[open]);
        assert!(library.placement.safe.is_empty());
    }

    #[test]
    fn folder_items_lists_members() {
        let (library, _, hidden, _) = private_library();
        assert_eq!(library.folder_items(library.folders[0].id), vec![hidden]);
    }

    #[test]
    fn rename_item_reports_missing() {
        let mut library = Library::default();
        assert!(!library.rename_item(Uuid::new_v4(), "x".into()));
        let item = sample_item();
        let id = item.id;
        library.items.push(item);
        assert!(library.rename_item(id, "New".into()));
        assert_eq!(library.items[0].title, "New");
    }

    #[test]
    fn merge_patch_nests_objects_and_deletes_nulls() {
        let mut settings = Settings::default();
        settings.hotkeys.random = Some(proto::KeyCombo::new("r", vec![proto::Modifier::Control]));
        apply_patch(
            &mut settings,
            &serde_json::json!({
                "volume": 0.25,
                "hotkeys": { "random": null },
                "tools": { "install_consent": true },
            }),
        )
        .expect("patch");
        assert!((settings.volume - 0.25).abs() < f32::EPSILON);
        assert!(settings.hotkeys.random.is_none());
        assert!(settings.hotkeys.next.is_some());
        assert!(settings.tools.install_consent);
        assert!(settings.optimize_on_import);
    }

    #[test]
    fn merge_patch_null_resets_fields_to_default() {
        let mut settings = Settings { volume: 0.25, rotate_minutes: 5, ..Settings::default() };
        apply_patch(&mut settings, &serde_json::json!({ "volume": null, "rotate_minutes": null })).expect("patch");
        assert!((settings.volume - 1.0).abs() < f32::EPSILON);
        assert_eq!(settings.rotate_minutes, 30);
    }

    #[test]
    fn merge_patch_rejects_invalid_and_keeps_settings() {
        let mut settings = Settings::default();
        let before = settings.clone();
        assert!(apply_patch(&mut settings, &serde_json::json!({ "volume": "loud" })).is_err());
        assert_eq!(settings, before);
        assert!(apply_patch(&mut settings, &serde_json::json!("not an object")).is_err());
        assert_eq!(settings, before);
    }

    #[test]
    fn transition_defaults_to_empty() {
        let settings: Settings = serde_json::from_str("{}").expect("empty object");
        assert_eq!(settings.transition, Transition::default());
    }

    #[test]
    fn merge_patch_merges_and_clears_transition_fields() {
        let mut settings = Settings::default();
        apply_patch(&mut settings, &serde_json::json!({ "transition": { "kind": "wipe", "angle": 90.0 } }))
            .expect("patch");
        assert_eq!(settings.transition.kind, Some(proto::TransitionKind::Wipe));
        assert_eq!(settings.transition.angle, Some(90.0));

        apply_patch(&mut settings, &serde_json::json!({ "transition": { "angle": null } })).expect("patch");
        assert_eq!(settings.transition.kind, Some(proto::TransitionKind::Wipe));
        assert!(settings.transition.angle.is_none());
        assert!((settings.volume - 1.0).abs() < f32::EPSILON, "unrelated field clobbered");
    }

    #[test]
    fn transition_patch_with_bad_values_is_rejected() {
        let mut settings = Settings::default();
        let before = settings.clone();
        assert!(apply_patch(&mut settings, &serde_json::json!({ "transition": { "bezier": [1.0, 2.0] } })).is_err());
        assert_eq!(settings, before);
        assert!(apply_patch(&mut settings, &serde_json::json!({ "transition": { "pos": "nowhere" } })).is_err());
        assert_eq!(settings, before);
    }

    #[test]
    fn normalize_clamps_transition_duration_and_wave() {
        let mut settings = Settings::default();
        settings.transition.duration = Some(-3.0);
        settings.transition.wave = Some([-1.0, 5.0]);
        settings.normalize();
        assert_eq!(settings.transition.duration, Some(0.0));
        assert_eq!(settings.transition.wave, Some([0.0, 5.0]));
    }
}
