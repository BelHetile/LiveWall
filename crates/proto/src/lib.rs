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
    VolumeUp,
    VolumeDown,
}

impl HotkeyAction {
    pub const ALL: [HotkeyAction; 7] = [
        HotkeyAction::Next,
        HotkeyAction::Previous,
        HotkeyAction::TogglePause,
        HotkeyAction::ToggleMute,
        HotkeyAction::Random,
        HotkeyAction::VolumeUp,
        HotkeyAction::VolumeDown,
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
    #[serde(default)]
    pub volume_up: Option<KeyCombo>,
    #[serde(default)]
    pub volume_down: Option<KeyCombo>,
}

impl Hotkeys {
    pub fn get(&self, action: HotkeyAction) -> Option<&KeyCombo> {
        return match action {
            HotkeyAction::Next => self.next.as_ref(),
            HotkeyAction::Previous => self.previous.as_ref(),
            HotkeyAction::TogglePause => self.pause.as_ref(),
            HotkeyAction::ToggleMute => self.mute.as_ref(),
            HotkeyAction::Random => self.random.as_ref(),
            HotkeyAction::VolumeUp => self.volume_up.as_ref(),
            HotkeyAction::VolumeDown => self.volume_down.as_ref(),
        };
    }

    pub fn set(&mut self, action: HotkeyAction, combo: Option<KeyCombo>) {
        match action {
            HotkeyAction::Next => self.next = combo,
            HotkeyAction::Previous => self.previous = combo,
            HotkeyAction::TogglePause => self.pause = combo,
            HotkeyAction::ToggleMute => self.mute = combo,
            HotkeyAction::Random => self.random = combo,
            HotkeyAction::VolumeUp => self.volume_up = combo,
            HotkeyAction::VolumeDown => self.volume_down = combo,
        }
    }
}

impl Default for Hotkeys {
    fn default() -> Self {
        let mut hotkeys = Self {
            next: None,
            previous: None,
            pause: None,
            mute: None,
            random: None,
            volume_up: None,
            volume_down: None,
        };
        hotkeys.set(HotkeyAction::Next, Some(KeyCombo::default_combo("right")));
        hotkeys.set(HotkeyAction::Previous, Some(KeyCombo::default_combo("left")));
        hotkeys.set(HotkeyAction::TogglePause, Some(KeyCombo::default_combo("p")));
        hotkeys.set(HotkeyAction::ToggleMute, Some(KeyCombo::default_combo("m")));
        hotkeys.set(HotkeyAction::Random, Some(KeyCombo::default_combo("r")));
        hotkeys.set(HotkeyAction::VolumeUp, Some(KeyCombo::default_combo("up")));
        hotkeys.set(HotkeyAction::VolumeDown, Some(KeyCombo::default_combo("down")));
        return hotkeys;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApplyTarget {
    Default,
    Space { space: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransitionKind {
    None,
    Simple,
    Fade,
    Left,
    Right,
    Top,
    Bottom,
    Wipe,
    Wave,
    Grow,
    Center,
    Any,
    Outer,
    Random,
}

impl TransitionKind {
    pub const ALL: [TransitionKind; 14] = [
        TransitionKind::None,
        TransitionKind::Simple,
        TransitionKind::Fade,
        TransitionKind::Left,
        TransitionKind::Right,
        TransitionKind::Top,
        TransitionKind::Bottom,
        TransitionKind::Wipe,
        TransitionKind::Wave,
        TransitionKind::Grow,
        TransitionKind::Center,
        TransitionKind::Any,
        TransitionKind::Outer,
        TransitionKind::Random,
    ];

    pub const RANDOM_POOL: [TransitionKind; 12] = [
        TransitionKind::Simple,
        TransitionKind::Fade,
        TransitionKind::Left,
        TransitionKind::Right,
        TransitionKind::Top,
        TransitionKind::Bottom,
        TransitionKind::Wipe,
        TransitionKind::Wave,
        TransitionKind::Grow,
        TransitionKind::Center,
        TransitionKind::Any,
        TransitionKind::Outer,
    ];

    pub fn as_str(self) -> &'static str {
        return match self {
            TransitionKind::None => "none",
            TransitionKind::Simple => "simple",
            TransitionKind::Fade => "fade",
            TransitionKind::Left => "left",
            TransitionKind::Right => "right",
            TransitionKind::Top => "top",
            TransitionKind::Bottom => "bottom",
            TransitionKind::Wipe => "wipe",
            TransitionKind::Wave => "wave",
            TransitionKind::Grow => "grow",
            TransitionKind::Center => "center",
            TransitionKind::Any => "any",
            TransitionKind::Outer => "outer",
            TransitionKind::Random => "random",
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Axis {
    Fraction(f64),
    Pixels(i64),
}

impl Axis {
    fn parse(text: &str) -> Result<Self, String> {
        if let Ok(value) = text.parse::<i64>() {
            return Ok(Self::Pixels(value));
        }
        return text.parse::<f64>().map(Self::Fraction).map_err(|_| format!("invalid coordinate '{text}'"));
    }
}

impl std::fmt::Display for Axis {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return match self {
            Axis::Pixels(value) => write!(formatter, "{value}"),
            Axis::Fraction(value) if value.fract() == 0.0 => write!(formatter, "{value:.1}"),
            Axis::Fraction(value) => write!(formatter, "{value}"),
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pos {
    pub x: Axis,
    pub y: Axis,
}

impl Pos {
    pub fn center() -> Self {
        return Self { x: Axis::Fraction(0.5), y: Axis::Fraction(0.5) };
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let key = text.trim().to_ascii_lowercase();
        if let Some((x, y)) = alias(&key) {
            return Ok(Self { x: Axis::Fraction(x), y: Axis::Fraction(y) });
        }
        let (x, y) = key.split_once(',').ok_or_else(|| format!("invalid position '{text}'"))?;
        return Ok(Self { x: Axis::parse(x.trim())?, y: Axis::parse(y.trim())? });
    }
}

fn alias(key: &str) -> Option<(f64, f64)> {
    return match key {
        "center" => Some((0.5, 0.5)),
        "top" => Some((0.5, 1.0)),
        "bottom" => Some((0.5, 0.0)),
        "left" => Some((0.0, 0.5)),
        "right" => Some((1.0, 0.5)),
        "top-left" => Some((0.0, 1.0)),
        "top-right" => Some((1.0, 1.0)),
        "bottom-left" => Some((0.0, 0.0)),
        "bottom-right" => Some((1.0, 0.0)),
        _ => None,
    };
}

impl std::fmt::Display for Pos {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return write!(formatter, "{},{}", self.x, self.y);
    }
}

impl Serialize for Pos {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        return serializer.serialize_str(&self.to_string());
    }
}

impl<'de> Deserialize<'de> for Pos {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        return Pos::parse(&text).map_err(serde::de::Error::custom);
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Transition {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<TransitionKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bezier: Option<[f64; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos: Option<Pos>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invert_y: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wave: Option<[f64; 2]>,
}

impl Transition {
    pub const DEFAULT_BEZIER: [f64; 4] = [0.54, 0.0, 0.34, 0.99];
    pub const DEFAULT_DURATION: f64 = 1.0;
    pub const DEFAULT_ANGLE: f64 = 45.0;
    pub const DEFAULT_WAVE: [f64; 2] = [20.0, 20.0];

    pub fn normalize(&mut self) {
        if let Some(duration) = &mut self.duration {
            *duration = duration.clamp(0.0, 3600.0);
        }
        if let Some(wave) = &mut self.wave {
            wave[0] = wave[0].max(0.0);
            wave[1] = wave[1].max(0.0);
        }
    }
}

pub fn parse_bezier(text: &str) -> Result<[f64; 4], String> {
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    if parts.len() != 4 {
        return Err(format!("expected four bezier control points, got {}", parts.len()));
    }
    let mut out = [0.0; 4];
    for (slot, part) in out.iter_mut().zip(parts) {
        *slot = part.parse::<f64>().map_err(|_| format!("invalid bezier control point '{part}'"))?;
    }
    return Ok(out);
}

pub fn parse_wave(text: &str) -> Result<[f64; 2], String> {
    let normalized = text.replace(['x', 'X'], ",");
    let parts: Vec<&str> = normalized.split(',').map(str::trim).filter(|part| !part.is_empty()).collect();
    if parts.len() != 2 {
        return Err(format!("expected a wave width and height, got {}", parts.len()));
    }
    let mut out = [0.0; 2];
    for (slot, part) in out.iter_mut().zip(parts) {
        *slot = part.parse::<f64>().map_err(|_| format!("invalid wave dimension '{part}'"))?;
    }
    return Ok(out);
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case", rename_all_fields = "kebab-case")]
pub enum Request {
    Pause,
    Resume,
    TogglePause,
    Mute,
    Unmute,
    Next {
        #[serde(default)]
        transition: Option<Transition>,
    },
    Previous {
        #[serde(default)]
        transition: Option<Transition>,
    },
    Random {
        #[serde(default)]
        transition: Option<Transition>,
    },
    Status,
    Current,
    Quit,
    Spaces,
    Windows,
    Desktop,
    Rules,
    LibraryList,
    LibraryAdd {
        paths: Vec<String>,
    },
    LibraryRemove {
        ids: Vec<String>,
    },
    LibraryRename {
        id: String,
        title: String,
    },
    LibraryMove {
        ids: Vec<String>,
        folder: Option<String>,
    },
    FolderCreate {
        name: String,
    },
    FolderRename {
        id: String,
        name: String,
    },
    FolderDelete {
        id: String,
    },
    Apply {
        item: Option<String>,
        target: ApplyTarget,
        #[serde(default)]
        transition: Option<Transition>,
    },
    SetGeometry {
        item: String,
        mode: FillMode,
        geometry: Geometry,
    },
    SetItem {
        item: String,
        volume: Option<f32>,
        speed: Option<f32>,
        muted: Option<bool>,
        still_time: Option<f32>,
    },
    SettingsGet,
    SettingsSet {
        patch: Value,
    },
    HotkeysGet,
    HotkeysSet {
        action: HotkeyAction,
        combo: Option<String>,
    },
    LoginItemGet,
    LoginItemSet {
        enabled: bool,
    },
    Menu,
    Tools,
    ToolsInstall {
        tool: String,
        consent: bool,
    },
    Probe {
        path: String,
    },
    Download {
        url: String,
        consent: bool,
    },
    DownloadStatus {
        id: String,
    },
    DownloadCancel {
        id: String,
    },
    Downloads,
    Optimize {
        id: String,
    },
    OptimizeStatus {
        id: String,
    },
    OptimizeCancel {
        id: String,
    },
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
            Request::Next { transition: None },
            Request::Previous { transition: None },
            Request::Random {
                transition: Some(Transition {
                    kind: Some(TransitionKind::Wipe),
                    duration: Some(1.5),
                    angle: Some(90.0),
                    ..Transition::default()
                }),
            },
            Request::Status,
            Request::Current,
            Request::Quit,
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
            Request::Apply { item: Some("a".into()), target: ApplyTarget::Default, transition: None },
            Request::Apply {
                item: Some("a".into()),
                target: ApplyTarget::Space { space: "s".into() },
                transition: Some(Transition {
                    kind: Some(TransitionKind::Grow),
                    pos: Some(Pos::parse("top-right").expect("pos")),
                    ..Transition::default()
                }),
            },
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

    #[test]
    fn transition_kinds_use_kebab_wire_names() {
        for kind in TransitionKind::ALL {
            let text = serde_json::to_string(&kind).expect("serialize");
            assert_eq!(text, format!("\"{}\"", kind.as_str()), "{kind:?}");
            let back: TransitionKind = serde_json::from_str(&text).expect("deserialize");
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn pos_parses_aliases_fractions_and_pixels() {
        assert_eq!(Pos::parse("center").unwrap(), Pos { x: Axis::Fraction(0.5), y: Axis::Fraction(0.5) });
        assert_eq!(Pos::parse("TOP-LEFT").unwrap(), Pos { x: Axis::Fraction(0.0), y: Axis::Fraction(1.0) });
        assert_eq!(Pos::parse("0.25,0.75").unwrap(), Pos { x: Axis::Fraction(0.25), y: Axis::Fraction(0.75) });
        assert_eq!(Pos::parse("100, 200").unwrap(), Pos { x: Axis::Pixels(100), y: Axis::Pixels(200) });
        assert_eq!(Pos::parse("0.5,100").unwrap(), Pos { x: Axis::Fraction(0.5), y: Axis::Pixels(100) });
        assert!(Pos::parse("bogus").is_err());
        assert!(Pos::parse("1,2,3").is_err());
        assert!(Pos::parse("0.5,x").is_err());
    }

    #[test]
    fn pos_round_trips_through_a_string() {
        for text in ["center", "0.25,0.75", "100,200", "0.0,1.0"] {
            let pos = Pos::parse(text).expect("parse");
            let encoded = serde_json::to_string(&pos).expect("serialize");
            let decoded: Pos = serde_json::from_str(&encoded).expect("deserialize");
            assert_eq!(decoded, pos, "{text}");
        }
        assert_eq!(serde_json::to_string(&Pos::center()).unwrap(), "\"0.5,0.5\"");
        assert_eq!(
            serde_json::to_string(&Pos { x: Axis::Fraction(1.0), y: Axis::Fraction(0.0) }).unwrap(),
            "\"1.0,0.0\""
        );
        assert_eq!(serde_json::to_string(&Pos { x: Axis::Pixels(10), y: Axis::Pixels(20) }).unwrap(), "\"10,20\"");
    }

    #[test]
    fn bezier_and_wave_parsing() {
        let bezier = parse_bezier("0.54,0,0.34,0.99").expect("bezier");
        for (got, want) in bezier.into_iter().zip([0.54, 0.0, 0.34, 0.99]) {
            assert!((got - want).abs() < 1e-9, "{got} != {want}");
        }
        assert!(parse_bezier("0.5,0.5,0.5").is_err());
        assert!(parse_bezier("a,b,c,d").is_err());
        let wave = parse_wave("20,20").expect("wave");
        assert!((wave[0] - 20.0).abs() < 1e-9 && (wave[1] - 20.0).abs() < 1e-9);
        let wave = parse_wave("30x40").expect("wave");
        assert!((wave[0] - 30.0).abs() < 1e-9 && (wave[1] - 40.0).abs() < 1e-9);
        assert!(parse_wave("20").is_err());
    }

    #[test]
    fn transition_object_round_trips_and_omits_empty_fields() {
        let empty = Transition::default();
        assert_eq!(serde_json::to_string(&empty).unwrap(), "{}");
        let full = Transition {
            kind: Some(TransitionKind::Wave),
            duration: Some(2.5),
            bezier: Some([0.1, 0.2, 0.3, 0.4]),
            angle: Some(90.0),
            pos: Some(Pos::parse("bottom-right").unwrap()),
            invert_y: Some(true),
            wave: Some([12.0, 34.0]),
        };
        let text = serde_json::to_string(&full).expect("serialize");
        let back: Transition = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, full);
    }

    #[test]
    fn transition_rejects_bad_bezier_length() {
        assert!(serde_json::from_str::<Transition>(r#"{"bezier":[0.1,0.2,0.3]}"#).is_err());
        assert!(serde_json::from_str::<Transition>(r#"{"pos":"nonsense"}"#).is_err());
        assert!(serde_json::from_str::<Transition>(r#"{"kind":"spiral"}"#).is_err());
    }

    #[test]
    fn next_verb_omits_default_transition() {
        let encoded = encode(&RequestEnvelope::new(Request::Next { transition: None }));
        let text = String::from_utf8(encoded).expect("utf8");
        assert!(text.contains("\"cmd\":\"next\""), "{text}");
    }
}
