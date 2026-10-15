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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hotkeys {
    pub next: KeyCombo,
    pub previous: KeyCombo,
    pub pause: KeyCombo,
    pub mute: KeyCombo,
    pub random: KeyCombo,
}

impl Default for Hotkeys {
    fn default() -> Self {
        return Self {
            next: KeyCombo::default_combo("ArrowRight"),
            previous: KeyCombo::default_combo("ArrowLeft"),
            pause: KeyCombo::default_combo("P"),
            mute: KeyCombo::default_combo("M"),
            random: KeyCombo::default_combo("R"),
        };
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
    SetItem {
        item: String,
        volume: Option<f32>,
        speed: Option<f32>,
        muted: Option<bool>,
        still_time: Option<f32>,
    },
    SettingsGet,
    SettingsSet { patch: Value },
    HotkeysGet,
    HotkeysSet { hotkeys: Hotkeys },
    Download { url: String, consent: bool },
    DownloadStatus { id: String },
    DownloadCancel { id: String },
    Optimize { id: String },
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
        return Self {
            v: PROTOCOL_VERSION,
            body: ResponseBody::Err(ErrorBody { code, message: message.into() }),
        };
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
            Request::HotkeysSet { hotkeys: Hotkeys::default() },
            Request::Download { url: "https://example.com/v".into(), consent: true },
            Request::DownloadStatus { id: "job".into() },
            Request::DownloadCancel { id: "job".into() },
            Request::Optimize { id: "a".into() },
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
            hotkeys.pause.modifiers,
            vec![Modifier::Control, Modifier::Option, Modifier::Command]
        );
        assert_eq!(hotkeys.next.key, "ArrowRight");
        assert_eq!(hotkeys.previous.key, "ArrowLeft");
    }
}
