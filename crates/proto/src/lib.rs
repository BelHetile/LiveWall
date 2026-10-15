use std::path::PathBuf;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_LINE: usize = 64 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("frame exceeds {MAX_LINE} bytes")]
    TooLong,
    #[error("malformed frame: {0}")]
    Malformed(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    NotImplemented,
    Malformed,
    NotFound,
    Invalid,
    Busy,
    Internal,
    ConsentRequired,
    ToolMissing,
    Media,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum FillMode {
    #[default]
    Fill,
    Fit,
    Stretch,
    Center,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl Default for Geometry {
    fn default() -> Self {
        return Self { x: 0.0, y: 0.0, zoom: 1.0 };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Modifier {
    Control,
    Option,
    Command,
    Shift,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyCombo {
    pub key: String,
    pub modifiers: Vec<Modifier>,
}

impl KeyCombo {
    pub fn new(key: impl Into<String>, modifiers: Vec<Modifier>) -> Self {
        return Self { key: key.into(), modifiers };
    }

    fn default_combo(key: impl Into<String>) -> Self {
        return Self::new(key, vec![Modifier::Control, Modifier::Option, Modifier::Command]);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HotkeyAction {
    Next,
    Previous,
    TogglePause,
    ToggleMute,
    Random,
}

impl HotkeyAction {
    pub const ALL: [HotkeyAction; 5] = [
        HotkeyAction::Next,
        HotkeyAction::Previous,
        HotkeyAction::TogglePause,
        HotkeyAction::ToggleMute,
        HotkeyAction::Random,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hotkeys {
    #[serde(default)]
    pub next: Option<KeyCombo>,
    #[serde(default)]
    pub previous: Option<KeyCombo>,
    #[serde(default)]
    pub pause: Option<KeyCombo>,
    #[serde(default)]
    pub mute: Option<KeyCombo>,
    #[serde(default)]
    pub random: Option<KeyCombo>,
}

impl Hotkeys {
    pub fn get(&self, action: HotkeyAction) -> Option<&KeyCombo> {
        return match action {
            HotkeyAction::Next => self.next.as_ref(),
            HotkeyAction::Previous => self.previous.as_ref(),
            HotkeyAction::TogglePause => self.pause.as_ref(),
            HotkeyAction::ToggleMute => self.mute.as_ref(),
            HotkeyAction::Random => self.random.as_ref(),
        };
    }

    pub fn set(&mut self, action: HotkeyAction, combo: Option<KeyCombo>) {
        match action {
            HotkeyAction::Next => self.next = combo,
            HotkeyAction::Previous => self.previous = combo,
            HotkeyAction::TogglePause => self.pause = combo,
            HotkeyAction::ToggleMute => self.mute = combo,
            HotkeyAction::Random => self.random = combo,
        }
    }
}

impl Default for Hotkeys {
    fn default() -> Self {
        let mut hotkeys = Self { next: None, previous: None, pause: None, mute: None, random: None };
        hotkeys.set(HotkeyAction::Next, Some(KeyCombo::default_combo("right")));
        hotkeys.set(HotkeyAction::Previous, Some(KeyCombo::default_combo("left")));
        hotkeys.set(HotkeyAction::TogglePause, Some(KeyCombo::default_combo("p")));
        hotkeys.set(HotkeyAction::ToggleMute, Some(KeyCombo::default_combo("m")));
        hotkeys.set(HotkeyAction::Random, Some(KeyCombo::default_combo("r")));
        return hotkeys;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApplyTarget {
    Default,
    Display { display: String },
    Space { space: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case", rename_all_fields = "kebab-case")]
pub enum Request {
    Pause,
    Resume,
    TogglePause,
    Mute,
    Unmute,
    Next,
    Previous,
    Random,
    Status,
    Current,
    Quit,
    Displays,
    Spaces,
    Windows,
    Desktop,
    Rules,
    LibraryList,
    LibraryAdd { paths: Vec<String> },
    LibraryRemove { ids: Vec<String> },
    LibraryRename { id: String, title: String },
    LibraryMove { ids: Vec<String>, folder: Option<String> },
    FolderCreate { name: String },
    FolderRename { id: String, name: String },
    FolderDelete { id: String },
    Apply { item: Option<String>, target: ApplyTarget },
    SetGeometry { item: String, mode: FillMode, geometry: Geometry },
    SetItem { item: String, volume: Option<f32>, speed: Option<f32>, muted: Option<bool>, still_time: Option<f32> },
    SettingsGet,
    SettingsSet { patch: Value },
    HotkeysGet,
    HotkeysSet { action: HotkeyAction, combo: Option<String> },
    LoginItemGet,
    LoginItemSet { enabled: bool },
    Menu,
    Tools,
    ToolsInstall { tool: String, consent: bool },
    Probe { path: String },
    Download { url: String, consent: bool },
    DownloadStatus { id: String },
    DownloadCancel { id: String },
    Downloads,
    Optimize { id: String },
    OptimizeStatus { id: String },
    OptimizeCancel { id: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub v: u32,
    #[serde(flatten)]
    pub request: Request,
}

impl RequestEnvelope {
    pub fn new(request: Request) -> Self {
        return Self { v: PROTOCOL_VERSION, request };
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub v: u32,
    #[serde(flatten)]
    pub body: ResponseBody,
}

impl Response {
    pub fn ok(data: Value) -> Self {
        return Self { v: PROTOCOL_VERSION, body: ResponseBody::Ok { data } };
    }

    pub fn err(code: ErrorCode, message: impl Into<String>) -> Self {
        return Self { v: PROTOCOL_VERSION, body: ResponseBody::Err(ErrorBody { code, message: message.into() }) };
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ResponseBody {
    Ok { data: Value },
    Err(ErrorBody),
}

pub fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(value).unwrap_or_default();
    bytes.push(b'\n');
    return bytes;
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    let trimmed = trim(bytes);
    if trimmed.len() > MAX_LINE {
        return Err(Error::TooLong);
    }
    return Ok(serde_json::from_slice(trimmed)?);
}

pub fn socket_path() -> PathBuf {
    if let Some(path) = std::env::var_os("LIVEWALL_SOCKET") {
        return PathBuf::from(path);
    }
    let mut path = std::env::home_dir().unwrap_or_default();
    path.push("Library/Application Support/LiveWall");
    path.push("control.sock");
    return path;
}

fn trim(mut bytes: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = bytes {
        if !first.is_ascii_whitespace() {
            break;
        }
        bytes = rest;
    }
    while let [rest @ .., last] = bytes {
        if !last.is_ascii_whitespace() {
            break;
        }
        bytes = rest;
    }
    return bytes;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn every_request() -> Vec<Request> {
        return vec![
            Request::Pause,
            Request::Resume,
            Request::TogglePause,
            Request::Mute,
            Request::Unmute,
            Request::Next,
            Request::Previous,
            Request::Random,
            Request::Status,
            Request::Current,
            Request::Quit,
            Request::Displays,
            Request::Spaces,
            Request::Windows,
            Request::Desktop,
            Request::Rules,
            Request::LibraryList,
            Request::LibraryAdd { paths: vec!["/a/b.mp4".into(), "/c/d.gif".into()] },
            Request::LibraryRemove { ids: vec!["a".into(), "b".into()] },
            Request::LibraryRename { id: "a".into(), title: "Title".into() },
            Request::LibraryMove { ids: vec!["a".into()], folder: Some("f".into()) },
            Request::FolderCreate { name: "Folder".into() },
            Request::FolderRename { id: "f".into(), name: "Renamed".into() },
            Request::FolderDelete { id: "f".into() },
            Request::Apply { item: Some("a".into()), target: ApplyTarget::Default },
            Request::Apply { item: None, target: ApplyTarget::Display { display: "d".into() } },
            Request::Apply { item: Some("a".into()), target: ApplyTarget::Space { space: "s".into() } },
            Request::SetGeometry {
                item: "a".into(),
                mode: FillMode::Custom,
                geometry: Geometry { x: 0.5, y: -0.25, zoom: 2.0 },
            },
            Request::SetItem {
                item: "a".into(),
                volume: Some(0.5),
                speed: Some(1.5),
                muted: Some(false),
                still_time: Some(12.0),
            },
            Request::SettingsGet,
            Request::SettingsSet { patch: json!({ "muted": false }) },
            Request::HotkeysGet,
            Request::HotkeysSet { action: HotkeyAction::TogglePause, combo: Some("ctrl+opt+cmd+p".into()) },
            Request::HotkeysSet { action: HotkeyAction::Random, combo: None },
            Request::LoginItemGet,
            Request::LoginItemSet { enabled: true },
            Request::Menu,
            Request::Tools,
            Request::ToolsInstall { tool: "ffmpeg".into(), consent: true },
            Request::Probe { path: "/a/b.mp4".into() },
            Request::Download { url: "https://example.com/v".into(), consent: true },
            Request::DownloadStatus { id: "job".into() },
            Request::DownloadCancel { id: "job".into() },
            Request::Downloads,
            Request::Optimize { id: "a".into() },
            Request::OptimizeStatus { id: "a".into() },
            Request::OptimizeCancel { id: "a".into() },
        ];
    }

    #[test]
    fn round_trip_every_request() {
        for request in every_request() {
            let envelope = RequestEnvelope::new(request.clone());
            let encoded = encode(&envelope);
            assert_eq!(encoded.last(), Some(&b'\n'));
            let decoded: RequestEnvelope = decode(&encoded).expect("decode");
            assert_eq!(decoded, envelope);
            assert_eq!(decoded.request, request);
        }
    }

    #[test]
    fn request_uses_kebab_verbs() {
        let encoded = encode(&RequestEnvelope::new(Request::LibraryList));
        let text = String::from_utf8(encoded).expect("utf8");
        assert!(text.contains("\"cmd\":\"library-list\""), "{text}");
        assert!(text.contains("\"v\":1"), "{text}");

        let encoded = encode(&RequestEnvelope::new(Request::TogglePause));
        let text = String::from_utf8(encoded).expect("utf8");
        assert!(text.contains("\"cmd\":\"toggle-pause\""), "{text}");
    }

    #[test]
    fn round_trip_ok_response() {
        let response = Response::ok(json!({ "paused": true }));
        let encoded = encode(&response);
        let decoded: Response = decode(&encoded).expect("decode");
        assert_eq!(decoded, response);
        let text = String::from_utf8(encoded).expect("utf8");
        assert!(text.contains("\"status\":\"ok\""), "{text}");
    }

    #[test]
    fn round_trip_error_frame() {
        let response = Response::err(ErrorCode::NotFound, "no such item");
        let encoded = encode(&response);
        let text = String::from_utf8(encoded.clone()).expect("utf8");
        assert!(text.contains("\"status\":\"err\""), "{text}");
        assert!(text.contains("\"code\":\"NOT_FOUND\""), "{text}");
        let decoded: Response = decode(&encoded).expect("decode");
        assert_eq!(decoded, response);
    }

    #[test]
    fn rejects_oversize() {
        let mut bytes = vec![b'"'; MAX_LINE + 16];
        bytes.push(b'\n');
        assert!(matches!(decode::<Request>(&bytes), Err(Error::TooLong)));
    }

    #[test]
    fn rejects_malformed() {
        assert!(matches!(decode::<Request>(b"not json\n"), Err(Error::Malformed(_))));
        assert!(matches!(decode::<Request>(b"\n"), Err(Error::Malformed(_))));
        assert!(matches!(decode::<Request>(b"{\"cmd\":\"bogus\"}\n"), Err(Error::Malformed(_))));
    }

    #[test]
    fn geometry_default_is_identity() {
        let g = Geometry::default();
        assert_eq!((g.x, g.y, g.zoom), (0.0, 0.0, 1.0));
    }

    #[test]
    fn hotkey_defaults_are_control_option_command() {
        let hotkeys = Hotkeys::default();
        assert_eq!(
            hotkeys.pause.as_ref().unwrap().modifiers,
            vec![Modifier::Control, Modifier::Option, Modifier::Command]
        );
        assert_eq!(hotkeys.next.as_ref().unwrap().key, "right");
        assert_eq!(hotkeys.previous.as_ref().unwrap().key, "left");
    }

    #[test]
    fn hotkeys_get_set_round_trip_by_action() {
        let mut hotkeys = Hotkeys::default();
        assert_eq!(hotkeys.get(HotkeyAction::Random).unwrap().key, "r");
        hotkeys.set(HotkeyAction::Random, None);
        assert!(hotkeys.get(HotkeyAction::Random).is_none());
        hotkeys.set(HotkeyAction::ToggleMute, Some(KeyCombo::new("M", vec![Modifier::Control])));
        assert_eq!(hotkeys.mute.as_ref().unwrap().modifiers, vec![Modifier::Control]);
    }

    #[test]
    fn hotkeys_partial_object_leaves_missing_actions_unbound() {
        let hotkeys: Hotkeys =
            serde_json::from_str(r#"{"pause":{"key":"P","modifiers":["control"]}}"#).expect("partial");
        assert!(hotkeys.pause.is_some());
        assert!(hotkeys.next.is_none());
        assert!(hotkeys.random.is_none());
    }
}
