use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::ExitStatus;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::ChildStdout;
use tokio::runtime::Handle;
use tokio::sync::Notify;

use crate::jobs::{self, ESCALATION};
use crate::media::TranscodeError;
use crate::process::Process;

pub type Report = Arc<dyn Fn(f64, Option<f64>) + Send + Sync>;

#[derive(Debug, Default, Clone, PartialEq)]
struct ProgressState {
    out_time_us: i64,
    speed: Option<f64>,
    ended: bool,
}

fn apply_line(state: &mut ProgressState, line: &str) {
    let Some((key, value)) = line.split_once('=') else {
        return;
    };
    let key = key.trim();
    let value = value.trim();
    match key {
        "out_time_us" | "out_time_ms" => {
            if let Ok(parsed) = value.parse::<i64>() {
                state.out_time_us = parsed;
            }
        }
        "speed" => state.speed = value.trim_end_matches('x').parse::<f64>().ok(),
        "progress" => state.ended = value == "end",
        _ => {}
    }
}

#[allow(clippy::cast_precision_loss)]
fn percent(state: &ProgressState, total_seconds: f64) -> f64 {
    if total_seconds <= 0.0 {
        return if state.ended { 1.0 } else { 0.0 };
    }
    let done = state.out_time_us.max(0) as f64 / 1_000_000.0;
    return (done / total_seconds).clamp(0.0, 1.0);
}

fn has_hevc_videotoolbox(encoders: &str) -> bool {
    return encoders.lines().any(|line| line.split_whitespace().any(|token| token == "hevc_videotoolbox"));
}

pub async fn supports_hevc(ffmpeg: &Path) -> bool {
    let mut process = Process::new(ffmpeg.as_os_str().to_owned());
    process.args(["-hide_banner", "-encoders"]);
    let Ok(output) = process.output().await else {
        return false;
    };
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    return has_hevc_videotoolbox(&text);
}

fn spawn_reader(handle: &Handle, stdout: ChildStdout, total: f64, report: Report) {
    handle.spawn(async move {
        let mut state = ProgressState::default();
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            apply_line(&mut state, &line);
            report(percent(&state, total), state.speed);
        }
    });
}

pub async fn run(
    ffmpeg: &Path,
    args: Vec<OsString>,
    duration: f64,
    report: Report,
    notify: &Notify,
) -> Result<(), TranscodeError> {
    let handle = Handle::current();
    let mut process = Process::new(ffmpeg.as_os_str().to_owned());
    process.args(args);
    let mut child =
        process.spawn_piped().map_err(|error| TranscodeError::Failed(format!("cannot start ffmpeg: {error}")))?;
    let stdout = child.take_stdout();
    let stderr = child.take_stderr();
    if let Some(stdout) = stdout {
        spawn_reader(&handle, stdout, duration, report);
    }
    let stderr_task = jobs::drain_stderr(&handle, stderr);

    let mut status: Option<io::Result<ExitStatus>> = None;
    let cancelled = tokio::select! {
        result = child.wait() => {
            status = Some(result);
            false
        }
        () = notify.notified() => true,
    };

    if cancelled {
        let _ = child.cancel(ESCALATION).await;
        let _ = jobs::join_stderr(stderr_task).await;
        return Err(TranscodeError::Cancelled);
    }

    let stderr_text = jobs::join_stderr(stderr_task).await;

    let success = matches!(status, Some(Ok(exit)) if exit.success());
    if !success {
        return Err(TranscodeError::Failed(jobs::failure_reason(&stderr_text, "ffmpeg")));
    }
    return Ok(());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hevc_encoder_detection() {
        let text = " V....D h264_videotoolbox    VideoToolbox H.264 Encoder\n V....D hevc_videotoolbox    VideoToolbox H.265 Encoder";
        assert!(has_hevc_videotoolbox(text));
        assert!(!has_hevc_videotoolbox(" V....D h264_videotoolbox  VideoToolbox H.264 Encoder"));
    }

    #[test]
    fn progress_lines_drive_percent_and_speed() {
        let mut state = ProgressState::default();
        apply_line(&mut state, "out_time_us=1500000");
        apply_line(&mut state, "speed=1.5x");
        assert!((percent(&state, 3.0) - 0.5).abs() < 1e-9);
        assert_eq!(state.speed, Some(1.5));
        apply_line(&mut state, "progress=end");
        assert!(state.ended);
        assert!((percent(&state, 0.0) - 1.0).abs() < 1e-9);
    }
}
