use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use proto::{
    ApplyTarget, FillMode, Geometry, Hotkeys, MAX_LINE, Request, RequestEnvelope, Response, ResponseBody,
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
}

#[derive(Subcommand)]
enum LibraryCommand {
    List,
    Add { paths: Vec<String> },
    Remove { ids: Vec<String> },
    Rename { id: String, #[arg(long)] title: String },
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
        #[arg(long, help = "JSON object of settings fields to patch")]
        patch: String,
    },
}

#[derive(Subcommand)]
enum HotkeysCommand {
    Get,
    Set {
        #[arg(long, help = "JSON object matching the hotkeys schema")]
        json: String,
    },
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
        Command::Next => Request::Next,
        Command::Previous => Request::Previous,
        Command::Random => Request::Random,
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
        Command::Apply { item, target, target_id } => {
            let target = match target {
                TargetKind::Default => ApplyTarget::Default,
                TargetKind::Display => ApplyTarget::Display {
                    display: target_id.ok_or("--target-id is required for --target display")?,
                },
                TargetKind::Space => ApplyTarget::Space {
                    space: target_id.ok_or("--target-id is required for --target space")?,
                },
            };
            Request::Apply { item, target }
        }
        Command::SetGeometry { item, mode, x, y, zoom } => Request::SetGeometry {
            item,
            mode: mode.into(),
            geometry: Geometry { x, y, zoom },
        },
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
            HotkeysCommand::Set { json } => {
                let hotkeys: Hotkeys = serde_json::from_str(&json).map_err(|error| error.to_string())?;
                Request::HotkeysSet { hotkeys }
            }
        },
        Command::DownloadStatus { id } => Request::DownloadStatus { id },
        Command::DownloadCancel { id } => Request::DownloadCancel { id },
        Command::Optimize { id } => Request::Optimize { id },
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
