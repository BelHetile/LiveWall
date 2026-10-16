use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::process::Process;

const HOMEBREW_BIN: &str = "/opt/homebrew/bin";
const HOMEBREW_NO_AUTO_UPDATE: &str = "HOMEBREW_NO_AUTO_UPDATE";
const HOMEBREW_NO_INSTALL_CLEANUP: &str = "HOMEBREW_NO_INSTALL_CLEANUP";
const HOMEBREW_NO_INTERACTIVE: &str = "HOMEBREW_NO_INTERACTIVE";
const NONINTERACTIVE: &str = "NONINTERACTIVE";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolKind {
    Ffmpeg,
    YtDlp,
    Deno,
}

impl ToolKind {
    pub const ALL: [ToolKind; 3] = [ToolKind::Ffmpeg, ToolKind::YtDlp, ToolKind::Deno];

    fn version_args(self) -> &'static [&'static str] {
        return match self {
            ToolKind::Ffmpeg => &["-version"],
            ToolKind::YtDlp | ToolKind::Deno => &["--version"],
        };
    }

    pub fn from_id(id: &str) -> Option<Self> {
        return serde_json::from_value(serde_json::Value::from(id)).ok();
    }
}

impl fmt::Display for ToolKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match serde_json::to_value(self) {
            Ok(serde_json::Value::String(id)) => formatter.write_str(&id),
            _ => Err(fmt::Error),
        };
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolStatus {
    pub tool: ToolKind,
    pub path: Option<String>,
    pub version: Option<String>,
    pub missing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolsReport {
    pub tools_dir: String,
    pub brew: Option<String>,
    pub tools: Vec<ToolStatus>,
}

fn brew_path() -> PathBuf {
    return std::env::var_os("LIVEWALL_BREW").map_or_else(|| PathBuf::from(HOMEBREW_BIN).join("brew"), PathBuf::from);
}

fn search_dirs(tools_dir: &Path) -> [PathBuf; 2] {
    return [tools_dir.to_path_buf(), PathBuf::from(HOMEBREW_BIN)];
}

pub fn locate(tool: ToolKind, tools_dir: &Path) -> Option<PathBuf> {
    for dir in search_dirs(tools_dir) {
        let candidate = dir.join(tool.to_string());
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    return None;
}

pub fn parse_version(text: &str) -> Option<String> {
    for token in text.split_whitespace() {
        let cleaned = token.trim_start_matches('v');
        if semver::Version::parse(cleaned).is_ok() {
            return Some(cleaned.to_string());
        }
    }
    return text.lines().map(str::trim).find(|line| !line.is_empty()).map(ToString::to_string);
}

pub async fn version(path: &Path, tool: ToolKind) -> Option<String> {
    let mut process = Process::new(path.as_os_str().to_owned());
    process.args(tool.version_args());
    let output = process.output().await.ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let text = if stdout.trim().is_empty() { String::from_utf8_lossy(&output.stderr).into_owned() } else { stdout };
    return parse_version(&text);
}

pub async fn report(tools_dir: &Path) -> ToolsReport {
    let mut tools = Vec::with_capacity(ToolKind::ALL.len());
    for tool in ToolKind::ALL {
        let path = locate(tool, tools_dir);
        let version = match &path {
            Some(path) => version(path, tool).await,
            None => None,
        };
        tools.push(ToolStatus {
            tool,
            path: path.as_ref().map(|path| path.to_string_lossy().into_owned()),
            version,
            missing: path.is_none(),
        });
    }
    let brew = brew_path();
    return ToolsReport {
        tools_dir: tools_dir.to_string_lossy().into_owned(),
        brew: brew.is_file().then(|| brew.to_string_lossy().into_owned()),
        tools,
    };
}

pub async fn install(tool: ToolKind, tools_dir: &Path) -> Result<PathBuf, String> {
    let brew = brew_path();
    if !brew.is_file() {
        return Err(format!("brew was not found at {}", brew.display()));
    }
    let mut process = Process::new(brew.as_os_str().to_owned());
    process.arg("install").arg(tool.to_string());
    process.env(HOMEBREW_NO_AUTO_UPDATE, "1");
    process.env(HOMEBREW_NO_INSTALL_CLEANUP, "1");
    process.env(HOMEBREW_NO_INTERACTIVE, "1");
    process.env(NONINTERACTIVE, "1");
    let output = process.output().await.map_err(|error| error.to_string())?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(format!("brew install {tool} failed: {}", message.trim()));
    }
    return locate(tool, tools_dir).ok_or_else(|| format!("{tool} was not found after installation"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_stub(path: &Path, body: &str) {
        let mut file = std::fs::File::create(path).expect("stub");
        file.write_all(body.as_bytes()).expect("write stub");
        file.sync_all().expect("sync");
        std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).expect("chmod");
    }

    #[test]
    fn tool_ids_round_trip() {
        for tool in ToolKind::ALL {
            assert_eq!(ToolKind::from_id(&tool.to_string()), Some(tool));
        }
        assert_eq!(ToolKind::YtDlp.to_string(), "yt-dlp");
        assert_eq!(ToolKind::from_id("ffprobe"), None);
    }

    #[test]
    fn parse_version_prefers_semver_tokens() {
        assert_eq!(
            parse_version("ffmpeg version 7.1.1 Copyright (c) the FFmpeg developers"),
            Some("7.1.1".to_string())
        );
        assert_eq!(parse_version("2025.09.26"), Some("2025.09.26".to_string()));
        assert_eq!(parse_version("deno 2.1.4 (stable, release)"), Some("2.1.4".to_string()));
    }

    #[test]
    fn parse_version_falls_back_to_first_line() {
        assert_eq!(parse_version("unknown build\nsecond"), Some("unknown build".to_string()));
    }

    #[test]
    fn locate_uses_the_tools_dir_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_stub(&dir.path().join("ffmpeg"), "#!/bin/sh\n");
        let found = locate(ToolKind::Ffmpeg, dir.path()).expect("found");
        assert_eq!(found, dir.path().join("ffmpeg"));
        assert_eq!(search_dirs(dir.path())[0], dir.path());
        assert_eq!(search_dirs(dir.path())[1], PathBuf::from(HOMEBREW_BIN));
    }

    #[tokio::test]
    async fn install_runs_brew_non_interactively_and_relocates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let record = dir.path().join("argv.txt");
        let brew = dir.path().join("brew");
        write_stub(
            &brew,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$LIVEWALL_BREW_RECORD\"\nprintf 'HOMEBREW_NO_AUTO_UPDATE=%s\\n' \"$HOMEBREW_NO_AUTO_UPDATE\" >> \"$LIVEWALL_BREW_RECORD\"\n",
        );
        let tools_dir = dir.path().join("Tools");
        std::fs::create_dir_all(&tools_dir).expect("tools dir");
        write_stub(&tools_dir.join("ffmpeg"), "#!/bin/sh\n");

        unsafe {
            std::env::set_var("LIVEWALL_BREW", &brew);
            std::env::set_var("LIVEWALL_BREW_RECORD", &record);
        }
        let installed = install(ToolKind::Ffmpeg, &tools_dir).await.expect("install");
        unsafe {
            std::env::remove_var("LIVEWALL_BREW");
            std::env::remove_var("LIVEWALL_BREW_RECORD");
        }
        assert_eq!(installed, tools_dir.join("ffmpeg"));
        let argv = std::fs::read_to_string(&record).expect("argv");
        assert!(argv.contains("install"), "{argv}");
        assert!(argv.contains("ffmpeg"), "{argv}");
        assert!(argv.contains("HOMEBREW_NO_AUTO_UPDATE=1"), "{argv}");
    }
}
