use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use proto::FillMode;
use serde::Serialize;
use uuid::Uuid;

use crate::media::{probe, thumb};
use crate::model::{Item, ItemKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    Copy,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PathStatus {
    Imported,
    Skipped,
    Rejected,
}

#[derive(Debug, Clone, Serialize)]
pub struct PathResult {
    pub path: String,
    pub status: PathStatus,
    pub id: Option<String>,
    pub title: Option<String>,
    pub thumbnail: Option<String>,
    pub verdict: Option<probe::Verdict>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ImportedItem {
    pub item: Item,
    pub result: PathResult,
}

#[derive(Debug, Default)]
pub struct Report {
    pub imported: Vec<ImportedItem>,
    pub results: Vec<PathResult>,
    pub skipped: usize,
}

struct Prepared {
    id: Uuid,
    path: PathBuf,
    probe: probe::Probe,
    verdict: probe::Verdict,
    needs_optimize: bool,
}

pub fn kind_for(extension: &str) -> Option<ItemKind> {
    return match extension.to_ascii_lowercase().as_str() {
        "mp4" | "mov" | "m4v" | "webm" | "mkv" => Some(ItemKind::Video),
        "gif" => Some(ItemKind::Gif),
        "png" | "jpg" | "jpeg" | "heic" | "webp" | "tiff" | "bmp" => Some(ItemKind::Image),
        _ => None,
    };
}

pub async fn run(
    inputs: &[String],
    dest_dir: &Path,
    thumbs_dir: &Path,
    policy: probe::Policy,
    transfer: Transfer,
) -> Report {
    let mut set = tokio::task::JoinSet::new();
    for (index, input) in inputs.iter().enumerate() {
        let input = input.clone();
        let dest_dir = dest_dir.to_path_buf();
        let thumbs_dir = thumbs_dir.to_path_buf();
        set.spawn(async move {
            let outcome = import_one(input, dest_dir, thumbs_dir, policy, transfer).await;
            return (index, outcome);
        });
    }
    let mut ordered: Vec<(usize, Result<ImportedItem, Box<PathResult>>)> = Vec::with_capacity(inputs.len());
    while let Some(joined) = set.join_next().await {
        if let Ok(entry) = joined {
            ordered.push(entry);
        }
    }
    ordered.sort_by_key(|(index, _)| *index);

    let mut report = Report::default();
    for (_, outcome) in ordered {
        match outcome {
            Ok(imported) => {
                report.results.push(imported.result.clone());
                report.imported.push(imported);
            }
            Err(failed) => {
                report.skipped += 1;
                report.results.push(*failed);
            }
        }
    }
    return report;
}

async fn import_one(
    input: String,
    dest_dir: PathBuf,
    thumbs_dir: PathBuf,
    policy: probe::Policy,
    transfer: Transfer,
) -> Result<ImportedItem, Box<PathResult>> {
    let Some(extension) = extension_of(Path::new(&input)) else {
        return Err(skipped(&input, "unsupported format"));
    };
    let Some(kind) = kind_for(&extension) else {
        return Err(skipped(&input, "unsupported format"));
    };
    let task_input = PathBuf::from(&input);
    let task = tokio::task::spawn_blocking(move || prepare(&task_input, &extension, kind, &dest_dir, policy, transfer));
    let prepared = match task.await {
        Ok(Ok(prepared)) => prepared,
        Ok(Err(rejected)) => return Err(rejected),
        Err(_) => return Err(skipped(&input, "import task failed")),
    };
    let thumbnail =
        thumb::ensure(prepared.id, prepared.probe.kind, prepared.path.clone(), prepared.probe.duration, thumbs_dir)
            .await
            .map(|path| path.to_string_lossy().into_owned());
    let result = PathResult {
        path: input.clone(),
        status: PathStatus::Imported,
        id: Some(prepared.id.to_string()),
        title: Some(title_of(Path::new(&input))),
        thumbnail: thumbnail.clone(),
        verdict: Some(prepared.verdict.clone()),
        reason: None,
    };
    let item = Item {
        id: prepared.id,
        kind: prepared.probe.kind,
        path: prepared.path.to_string_lossy().into_owned(),
        title: title_of(Path::new(&input)),
        added_at: jiff::Timestamp::now(),
        folder: None,
        mode: FillMode::Fill,
        modes: BTreeMap::new(),
        volume: 1.0,
        speed: 1.0,
        muted: true,
        still_time: 0.0,
        optimized: false,
        hotkey: None,
        width: prepared.probe.width,
        height: prepared.probe.height,
        duration: prepared.probe.duration,
        fps: prepared.probe.fps,
        codec: prepared.probe.codec.clone(),
        audio_codec: prepared.probe.audio_codec.clone(),
        hdr: prepared.probe.hdr,
        playable: prepared.probe.decodable,
        needs_optimize: prepared.needs_optimize,
        optimize_reason: reason_text(&prepared.verdict),
    };
    return Ok(ImportedItem { item, result });
}

fn reason_text(verdict: &probe::Verdict) -> Option<String> {
    let probe::Verdict::NeedsOptimize { reasons } = verdict else {
        return None;
    };
    let labels: Vec<String> = reasons.iter().map(ToString::to_string).collect();
    return Some(labels.join(","));
}

fn prepare(
    input: &Path,
    extension: &str,
    kind: ItemKind,
    dest_dir: &Path,
    policy: probe::Policy,
    transfer: Transfer,
) -> Result<Prepared, Box<PathResult>> {
    let id = Uuid::new_v4();
    let target = dest_dir.join(format!("{id}.{extension}"));
    if let Err(error) = place(input, &target, transfer) {
        return Err(skipped(&input.to_string_lossy(), &format!("cannot import file: {error}")));
    }
    let found = probe::inspect(&target, kind);
    let verdict = probe::verdict(&found, policy);
    if let probe::Verdict::Unsupported { reason } = &verdict {
        let reason = reason.clone();
        let _ = std::fs::remove_file(&target);
        return Err(Box::new(PathResult {
            path: input.to_string_lossy().into_owned(),
            status: PathStatus::Rejected,
            id: None,
            title: None,
            thumbnail: None,
            verdict: Some(verdict),
            reason: Some(reason),
        }));
    }
    let needs_optimize = matches!(verdict, probe::Verdict::NeedsOptimize { .. });
    return Ok(Prepared { id, path: target, probe: found, verdict, needs_optimize });
}

fn place(input: &Path, target: &Path, transfer: Transfer) -> std::io::Result<()> {
    return match transfer {
        Transfer::Copy => {
            std::fs::copy(input, target)?;
            Ok(())
        }
        Transfer::Move => {
            if std::fs::rename(input, target).is_ok() {
                Ok(())
            } else {
                std::fs::copy(input, target)?;
                std::fs::remove_file(input)
            }
        }
    };
}

fn extension_of(input: &Path) -> Option<String> {
    return input.extension().and_then(|value| value.to_str()).map(str::to_ascii_lowercase);
}

fn title_of(input: &Path) -> String {
    return input.file_stem().and_then(|stem| stem.to_str()).unwrap_or("untitled").to_string();
}

fn skipped(path: &str, reason: &str) -> Box<PathResult> {
    return Box::new(PathResult {
        path: path.to_string(),
        status: PathStatus::Skipped,
        id: None,
        title: None,
        thumbnail: None,
        verdict: None,
        reason: Some(reason.to_string()),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> probe::Policy {
        return probe::Policy { fps_limit: 30, max_pixels: 2560 };
    }

    #[test]
    fn place_copies_or_moves_the_source() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("a.bin");
        std::fs::write(&source, b"data").expect("write");
        let copy = dir.path().join("copy.bin");
        place(&source, &copy, Transfer::Copy).expect("copy");
        assert!(source.exists(), "copy must keep the original");
        assert!(copy.exists());
        let moved = dir.path().join("moved.bin");
        place(&copy, &moved, Transfer::Move).expect("move");
        assert!(!copy.exists(), "move must remove the original");
        assert!(moved.exists());
    }

    #[test]
    fn kinds_by_extension() {
        assert_eq!(kind_for("MP4"), Some(ItemKind::Video));
        assert_eq!(kind_for("mov"), Some(ItemKind::Video));
        assert_eq!(kind_for("webm"), Some(ItemKind::Video));
        assert_eq!(kind_for("GIF"), Some(ItemKind::Gif));
        assert_eq!(kind_for("png"), Some(ItemKind::Image));
        assert_eq!(kind_for("jpeg"), Some(ItemKind::Image));
        assert_eq!(kind_for("avi"), None);
        assert_eq!(kind_for("txt"), None);
        assert_eq!(kind_for("mp3"), None);
    }

    #[tokio::test]
    async fn unsupported_extension_is_skipped_without_touching_storage() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("Wallpapers");
        let thumbs = dir.path().join("Thumbs");
        std::fs::create_dir_all(&dest).expect("dest");
        std::fs::create_dir_all(&thumbs).expect("thumbs");
        let txt = dir.path().join("notes.txt");
        std::fs::write(&txt, b"not media").expect("write");

        let report = run(&[txt.to_string_lossy().into_owned()], &dest, &thumbs, policy(), Transfer::Copy).await;
        assert!(report.imported.is_empty());
        assert_eq!(report.skipped, 1);
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].status, PathStatus::Skipped);
        assert!(std::fs::read_dir(&dest).expect("read").next().is_none());
    }

    #[tokio::test]
    async fn missing_file_is_skipped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("Wallpapers");
        let thumbs = dir.path().join("Thumbs");
        std::fs::create_dir_all(&dest).expect("dest");
        std::fs::create_dir_all(&thumbs).expect("thumbs");

        let report = run(
            &[dir.path().join("gone.mp4").to_string_lossy().into_owned()],
            &dest,
            &thumbs,
            policy(),
            Transfer::Copy,
        )
        .await;
        assert!(report.imported.is_empty());
        assert_eq!(report.skipped, 1);
        assert_eq!(report.results[0].status, PathStatus::Skipped);
    }
}
