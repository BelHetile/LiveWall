use std::collections::BTreeMap;

use proto::{FillMode, Geometry, Hotkeys};
use serde::{Deserialize, Serialize};
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
    pub hdr: bool,
    #[serde(default)]
    pub needs_optimize: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    pub id: FolderId,
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Placement {
    #[serde(default)]
    pub default: Assignment,
    #[serde(default)]
    pub displays: BTreeMap<String, Assignment>,
    #[serde(default)]
    pub spaces: BTreeMap<String, Assignment>,
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
    pub app_rules: Vec<AppRule>,
    pub optimize_on_import: bool,
    pub optimize_fps_limit: u32,
    pub stop_after_seconds: u32,
    pub launch_at_login: bool,
    pub open_library_at_launch: bool,
    pub language: Option<Language>,
    pub hotkeys: Hotkeys,
    pub tools: Tools,
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
            app_rules: Vec::new(),
            optimize_on_import: true,
            optimize_fps_limit: 30,
            stop_after_seconds: 30,
            launch_at_login: false,
            open_library_at_launch: false,
            language: None,
            hotkeys: Hotkeys::default(),
            tools: Tools::default(),
        };
    }
}

impl Settings {
    pub(crate) fn normalize(&mut self) {
        self.optimize_fps_limit = self.optimize_fps_limit.clamp(15, 120);
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
            hdr: true,
            needs_optimize: false,
        };
        let text = serde_json::to_string(&item).expect("serialize");
        let back: Item = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(item, back);
    }
}
