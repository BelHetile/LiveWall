use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use proto::{
    ApplyTarget, FillMode, Geometry, HotkeyAction, MAX_LINE, Request, RequestEnvelope, Response, ResponseBody,
};
use serde_json::Value;

const EXIT_OK: u8 = 0;
const EXIT_ERROR: u8 = 1;
const EXIT_NOT_RUNNING: u8 = 2;
const EXIT_USAGE: u8 = 64;
const POLL_INTERVAL: Duration = Duration::from_millis(500);
const TIMEOUT: Duration = Duration::from_mins(1);

#[derive(Parser)]
#[command(name = "lwctl", version, about = "Control the running LiveWall app")]
struct Cli {
    #[arg(long, global = true, help = "Print the raw JSON response line")]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Pause,
    Resume,
    TogglePause,
    Mute,
    Unmute,
    Next {
        #[command(flatten)]
        transition: TransitionArgs,
    },
    Previous {
        #[command(flatten)]
        transition: TransitionArgs,
    },
    Random {
        #[command(flatten)]
        transition: TransitionArgs,
    },
    Status,
    Current,
    Quit,
    Displays,
    Spaces,
    Windows,
    Desktop,
    Rules,
    #[command(subcommand)]
    Library(LibraryCommand),
    #[command(subcommand)]
    Folder(FolderCommand),
    Apply {
        #[arg(long)]
        item: Option<String>,
        #[arg(long, value_enum, default_value_t = TargetKind::Default)]
        target: TargetKind,
        #[arg(long)]
        target_id: Option<String>,
        #[command(flatten)]
        transition: TransitionArgs,
    },
    SetGeometry {
        item: String,
        #[arg(long, value_enum)]
        mode: FillModeArg,
        #[arg(long, default_value_t = 0.0)]
        x: f64,
        #[arg(long, default_value_t = 0.0)]
        y: f64,
        #[arg(long, default_value_t = 1.0)]
        zoom: f64,
    },
    SetItem {
        item: String,
        #[arg(long)]
        volume: Option<f32>,
        #[arg(long)]
        speed: Option<f32>,
        #[arg(long)]
        muted: Option<bool>,
        #[arg(long)]
        still_time: Option<f32>,
    },
    #[command(subcommand)]
    Settings(SettingsCommand),
    #[command(subcommand)]
    Hotkeys(HotkeysCommand),
    #[command(subcommand)]
    LoginItem(LoginItemCommand),
    Menu,
    Tools,
    ToolsInstall {
        tool: String,
        #[arg(long)]
        consent: bool,
    },
    Probe {
        path: String,
    },
    Download {
        url: String,
        #[arg(long)]
        consent: bool,
        #[arg(long, help = "Submit and exit without streaming progress")]
        no_wait: bool,
    },
    DownloadStatus {
        id: String,
    },
    DownloadCancel {
        id: String,
    },
    Optimize {
        id: String,
    },
    OptimizeStatus {
        id: String,
    },
    OptimizeCancel {
        id: String,
    },
    Downloads,
}

#[derive(Subcommand)]
enum LibraryCommand {
    List,
    Add {
        paths: Vec<String>,
    },
    Remove {
        ids: Vec<String>,
    },
    Rename {
        id: String,
        #[arg(long)]
        title: String,
    },
    Move {
        ids: Vec<String>,
        #[arg(long)]
        folder: Option<String>,
    },
}

#[derive(Subcommand)]
enum FolderCommand {
    Create { name: String },
    Rename { id: String, name: String },
    Delete { id: String },
}

#[derive(Subcommand)]
enum SettingsCommand {
    Get,
    Set {
        #[arg(help = "JSON object of settings fields to patch")]
        patch: String,
    },
}

#[derive(Subcommand)]
enum HotkeysCommand {
    Get,
    Set {
        #[arg(value_enum)]
        action: HotkeyActionArg,
        #[arg(help = "Combo such as ctrl+opt+cmd+right; omit to clear the binding")]
        combo: Option<String>,
    },
}

#[derive(Subcommand)]
enum LoginItemCommand {
    Get,
    Set {
        #[arg(action = clap::ArgAction::Set)]
        enabled: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum HotkeyActionArg {
    Next,
    Previous,
    TogglePause,
    ToggleMute,
    Random,
}

#[derive(clap::Args, Clone)]
struct TransitionArgs {
    #[arg(short = 't', long, value_enum, help = "Wallpaper transition to play")]
    transition: Option<TransitionKindArg>,
    #[arg(long, help = "Transition duration in seconds")]
    duration: Option<f64>,
    #[arg(long, help = "Bezier control points as a,b,c,d")]
    bezier: Option<String>,
    #[arg(long, help = "Wipe and wave angle in degrees")]
    angle: Option<f64>,
    #[arg(long, help = "Circle origin: x,y or an alias such as center")]
    pos: Option<String>,
    #[arg(long, help = "Flip the y axis of --pos")]
    invert_y: bool,
    #[arg(long, help = "Wave size as width,height")]
    wave: Option<String>,
}

impl TransitionArgs {
    fn build(self) -> Result<Option<proto::Transition>, String> {
        let mut out = proto::Transition::default();
        let mut any = false;
        if let Some(kind) = self.transition {
            out.kind = Some(kind.into());
            any = true;
        }
        if let Some(duration) = self.duration {
            out.duration = Some(duration);
            any = true;
        }
        if let Some(bezier) = self.bezier {
            out.bezier = Some(proto::parse_bezier(&bezier)?);
            any = true;
        }
        if let Some(angle) = self.angle {
            out.angle = Some(angle);
            any = true;
        }
        if let Some(pos) = self.pos {
            out.pos = Some(proto::Pos::parse(&pos)?);
            any = true;
        }
        if self.invert_y {
            out.invert_y = Some(true);
            any = true;
        }
        if let Some(wave) = self.wave {
            out.wave = Some(proto::parse_wave(&wave)?);
            any = true;
        }
        return Ok(if any { Some(out) } else { None });
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum TransitionKindArg {
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

#[derive(Clone, Copy, ValueEnum)]
enum TargetKind {
    Default,
    Display,
    Space,
}

#[derive(Clone, Copy, ValueEnum)]
enum FillModeArg {
    Fill,
    Fit,
    Stretch,
    Center,
    Custom,
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            let code = if error.use_stderr() { EXIT_USAGE } else { EXIT_OK };
            return ExitCode::from(code);
        }
    };

    let mut stream = match connect() {
        Ok(stream) => stream,
        Err(code) => return ExitCode::from(code),
    };

    if let Command::Download { url, consent, no_wait } = cli.command {
        let code = run_download(&mut stream, url, consent, no_wait, cli.json);
        return ExitCode::from(code);
    }

    let request = match build_request(cli.command) {
        Ok(request) => request,
        Err(message) => {
            eprintln!("lwctl: {message}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let code = match exchange(&mut stream, request) {
        Ok((response, raw)) => print_response(&response, &raw, cli.json),
        Err(error) => {
            eprintln!("lwctl: {error}");
            EXIT_ERROR
        }
    };
    return ExitCode::from(code);
}

fn connect() -> Result<UnixStream, u8> {
    let path = proto::socket_path();
    let stream = match UnixStream::connect(&path) {
        Ok(stream) => stream,
        Err(error) => {
            match error.kind() {
                ErrorKind::NotFound | ErrorKind::ConnectionRefused => {
                    eprintln!("lwctl: LiveWall is not running");
                }
                _ => {
                    eprintln!("lwctl: cannot connect to {}: {error}", path.display());
                }
            }
            return Err(EXIT_NOT_RUNNING);
        }
    };
    if stream.set_read_timeout(Some(TIMEOUT)).is_err() || stream.set_write_timeout(Some(TIMEOUT)).is_err() {
        eprintln!("lwctl: cannot configure socket timeouts");
        return Err(EXIT_NOT_RUNNING);
    }
    return Ok(stream);
}

fn build_request(command: Command) -> Result<Request, String> {
    let request = match command {
        Command::Pause => Request::Pause,
        Command::Resume => Request::Resume,
        Command::TogglePause => Request::TogglePause,
        Command::Mute => Request::Mute,
        Command::Unmute => Request::Unmute,
        Command::Next { transition } => Request::Next { transition: transition.build()? },
        Command::Previous { transition } => Request::Previous { transition: transition.build()? },
        Command::Random { transition } => Request::Random { transition: transition.build()? },
        Command::Status => Request::Status,
        Command::Current => Request::Current,
        Command::Quit => Request::Quit,
        Command::Displays => Request::Displays,
        Command::Spaces => Request::Spaces,
        Command::Windows => Request::Windows,
        Command::Desktop => Request::Desktop,
        Command::Rules => Request::Rules,
        Command::Library(command) => match command {
            LibraryCommand::List => Request::LibraryList,
            LibraryCommand::Add { paths } => Request::LibraryAdd { paths },
            LibraryCommand::Remove { ids } => Request::LibraryRemove { ids },
            LibraryCommand::Rename { id, title } => Request::LibraryRename { id, title },
            LibraryCommand::Move { ids, folder } => Request::LibraryMove { ids, folder },
        },
        Command::Folder(command) => match command {
            FolderCommand::Create { name } => Request::FolderCreate { name },
            FolderCommand::Rename { id, name } => Request::FolderRename { id, name },
            FolderCommand::Delete { id } => Request::FolderDelete { id },
        },
        Command::Apply { item, target, target_id, transition } => {
            let target = match target {
                TargetKind::Default => ApplyTarget::Default,
                TargetKind::Display => {
                    ApplyTarget::Display { display: target_id.ok_or("--target-id is required for --target display")? }
                }
                TargetKind::Space => {
                    ApplyTarget::Space { space: target_id.ok_or("--target-id is required for --target space")? }
                }
            };
            Request::Apply { item, target, transition: transition.build()? }
        }
        Command::SetGeometry { item, mode, x, y, zoom } => {
            Request::SetGeometry { item, mode: mode.into(), geometry: Geometry { x, y, zoom } }
        }
        Command::SetItem { item, volume, speed, muted, still_time } => {
            Request::SetItem { item, volume, speed, muted, still_time }
        }
        Command::Settings(command) => match command {
            SettingsCommand::Get => Request::SettingsGet,
            SettingsCommand::Set { patch } => {
                let patch: Value = serde_json::from_str(&patch).map_err(|error| error.to_string())?;
                Request::SettingsSet { patch }
            }
        },
        Command::Hotkeys(command) => match command {
            HotkeysCommand::Get => Request::HotkeysGet,
            HotkeysCommand::Set { action, combo } => Request::HotkeysSet { action: action.into(), combo },
        },
        Command::LoginItem(command) => match command {
            LoginItemCommand::Get => Request::LoginItemGet,
            LoginItemCommand::Set { enabled } => Request::LoginItemSet { enabled },
        },
        Command::Menu => Request::Menu,
        Command::Tools => Request::Tools,
        Command::ToolsInstall { tool, consent } => Request::ToolsInstall { tool, consent },
        Command::Probe { path } => Request::Probe { path },
        Command::DownloadStatus { id } => Request::DownloadStatus { id },
        Command::DownloadCancel { id } => Request::DownloadCancel { id },
        Command::Optimize { id } => Request::Optimize { id },
        Command::OptimizeStatus { id } => Request::OptimizeStatus { id },
        Command::OptimizeCancel { id } => Request::OptimizeCancel { id },
        Command::Downloads => Request::Downloads,
        Command::Download { .. } => return Err("download is handled separately".into()),
    };
    return Ok(request);
}

fn exchange(stream: &mut UnixStream, request: Request) -> Result<(Response, String), String> {
    let frame = proto::encode(&RequestEnvelope::new(request));
    stream.write_all(&frame).map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())?;
    let (response, raw) = read_response(stream)?;
    return Ok((response, raw));
}

fn read_response(stream: &mut UnixStream) -> Result<(Response, String), String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|error| error.to_string())?);
    let mut line = Vec::new();
    reader.read_until(b'\n', &mut line).map_err(|error| error.to_string())?;
    if line.is_empty() {
        return Err("app closed the connection".into());
    }
    if line.len() > MAX_LINE {
        return Err("response line too long".into());
    }
    let raw = String::from_utf8_lossy(&line).trim_end().to_string();
    let response = proto::decode::<Response>(&line).map_err(|error| error.to_string())?;
    return Ok((response, raw));
}

fn print_response(response: &Response, raw: &str, json: bool) -> u8 {
    match &response.body {
        ResponseBody::Ok { data } => {
            if json {
                println!("{raw}");
            } else {
                match serde_json::to_string_pretty(data) {
                    Ok(text) => println!("{text}"),
                    Err(_) => println!("{data}"),
                }
            }
            return EXIT_OK;
        }
        ResponseBody::Err(body) => {
            eprintln!("lwctl: {:?}: {}", body.code, body.message);
            return EXIT_ERROR;
        }
    }
}

fn run_download(stream: &mut UnixStream, url: String, consent: bool, no_wait: bool, json: bool) -> u8 {
    let (response, raw) = match exchange(stream, Request::Download { url, consent }) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("lwctl: {error}");
            return EXIT_ERROR;
        }
    };
    let data = match &response.body {
        ResponseBody::Ok { data } => data,
        ResponseBody::Err(body) => {
            eprintln!("lwctl: {:?}: {}", body.code, body.message);
            return EXIT_ERROR;
        }
    };
    if no_wait {
        return print_response(&response, &raw, json);
    }
    let Some(id) = data.get("id").and_then(Value::as_str) else {
        return print_response(&response, &raw, json);
    };
    let id = id.to_string();
    if is_terminal(data) {
        return finish_download(data);
    }
    loop {
        std::thread::sleep(POLL_INTERVAL);
        let (status, _raw) = match exchange(stream, Request::DownloadStatus { id: id.clone() }) {
            Ok(result) => result,
            Err(error) => {
                eprintln!("lwctl: {error}");
                return EXIT_ERROR;
            }
        };
        let data = match &status.body {
            ResponseBody::Ok { data } => data,
            ResponseBody::Err(body) => {
                eprintln!("lwctl: {:?}: {}", body.code, body.message);
                return EXIT_ERROR;
            }
        };
        if let Some(progress) = data.get("progress").and_then(Value::as_f64) {
            eprintln!("{:.1}%", progress * 100.0);
        }
        if is_terminal(data) {
            return finish_download(data);
        }
    }
}

fn is_terminal(data: &Value) -> bool {
    return matches!(
        data.get("state").and_then(Value::as_str),
        Some("done" | "error" | "cancelled" | "canceled" | "failed")
    );
}

fn finish_download(data: &Value) -> u8 {
    match data.get("state").and_then(Value::as_str) {
        Some("error" | "failed") => {
            eprintln!("lwctl: download failed");
            return EXIT_ERROR;
        }
        Some("cancelled" | "canceled") => {
            eprintln!("lwctl: download cancelled");
            return EXIT_ERROR;
        }
        _ => {
            match data.get("path").and_then(Value::as_str) {
                Some(path) => println!("{path}"),
                None => println!("{data}"),
            }
            return EXIT_OK;
        }
    }
}

impl From<TransitionKindArg> for proto::TransitionKind {
    fn from(value: TransitionKindArg) -> Self {
        return match value {
            TransitionKindArg::None => Self::None,
            TransitionKindArg::Simple => Self::Simple,
            TransitionKindArg::Fade => Self::Fade,
            TransitionKindArg::Left => Self::Left,
            TransitionKindArg::Right => Self::Right,
            TransitionKindArg::Top => Self::Top,
            TransitionKindArg::Bottom => Self::Bottom,
            TransitionKindArg::Wipe => Self::Wipe,
            TransitionKindArg::Wave => Self::Wave,
            TransitionKindArg::Grow => Self::Grow,
            TransitionKindArg::Center => Self::Center,
            TransitionKindArg::Any => Self::Any,
            TransitionKindArg::Outer => Self::Outer,
            TransitionKindArg::Random => Self::Random,
        };
    }
}

impl From<FillModeArg> for FillMode {
    fn from(value: FillModeArg) -> Self {
        return match value {
            FillModeArg::Fill => Self::Fill,
            FillModeArg::Fit => Self::Fit,
            FillModeArg::Stretch => Self::Stretch,
            FillModeArg::Center => Self::Center,
            FillModeArg::Custom => Self::Custom,
        };
    }
}

impl From<HotkeyActionArg> for HotkeyAction {
    fn from(value: HotkeyActionArg) -> Self {
        return match value {
            HotkeyActionArg::Next => Self::Next,
            HotkeyActionArg::Previous => Self::Previous,
            HotkeyActionArg::TogglePause => Self::TogglePause,
            HotkeyActionArg::ToggleMute => Self::ToggleMute,
            HotkeyActionArg::Random => Self::Random,
        };
    }
}
