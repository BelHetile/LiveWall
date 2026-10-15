use std::collections::BTreeMap;
use std::path::Path;

use proto::FillMode;
use uuid::Uuid;

use crate::model::{Item, ItemKind, Library};
use crate::paths::Paths;

pub struct AddReport {
    pub added: Vec<Uuid>,
    pub skipped: usize,
}

pub fn add(paths: &Paths, library: &mut Library, inputs: &[String]) -> AddReport {
    let directory = paths.wallpapers_dir();
    let mut report = AddReport { added: Vec::new(), skipped: 0 };
    for input in inputs {
        match import_one(Path::new(input), &directory) {
            Some(item) => {
                report.added.push(item.id);
                library.items.push(item);
            }
            None => report.skipped += 1,
        }
    }
    return report;
}

pub fn kind_for(extension: &str) -> Option<ItemKind> {
    return match extension.to_ascii_lowercase().as_str() {
        "mp4" | "mov" | "m4v" | "webm" | "mkv" => Some(ItemKind::Video),
        "gif" => Some(ItemKind::Gif),
        "png" | "jpg" | "jpeg" | "heic" | "webp" | "tiff" | "bmp" => Some(ItemKind::Image),
        _ => None,
    };
}

fn import_one(source: &Path, directory: &Path) -> Option<Item> {
    let extension = source.extension()?.to_str()?.to_ascii_lowercase();
    let kind = kind_for(&extension)?;
    let id = Uuid::new_v4();
    let target = directory.join(format!("{id}.{extension}"));
    std::fs::copy(source, &target).ok()?;
    let title = source.file_stem().and_then(|stem| stem.to_str()).unwrap_or("untitled").to_string();
    return Some(Item {
        id,
        kind,
        path: target.to_string_lossy().into_owned(),
        title,
        added_at: jiff::Timestamp::now(),
        folder: None,
        mode: FillMode::Fill,
        modes: BTreeMap::new(),
        volume: 1.0,
        speed: 1.0,
        muted: true,
        still_time: 0.0,
        optimized: false,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_extension() {
        assert_eq!(kind_for("MP4"), Some(ItemKind::Video));
        assert_eq!(kind_for("mov"), Some(ItemKind::Video));
        assert_eq!(kind_for("webm"), Some(ItemKind::Video));
        assert_eq!(kind_for("GIF"), Some(ItemKind::Gif));
        assert_eq!(kind_for("png"), Some(ItemKind::Image));
        assert_eq!(kind_for("jpeg"), Some(ItemKind::Image));
        assert_eq!(kind_for("avi"), None);
        assert_eq!(kind_for("mp3"), None);
    }

    #[test]
    fn copies_into_storage_and_skips_unknown() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = Paths::for_test(dir.path().to_path_buf());
        paths.ensure().expect("dirs");
        let source = dir.path().join("clip.mp4");
        std::fs::write(&source, b"fake").expect("write");

        let mut library = Library::default();
        let report = add(&paths, &mut library, &[source.to_string_lossy().into_owned(), "/does/not/exist.xyz".into()]);
        assert_eq!(report.added.len(), 1);
        assert_eq!(report.skipped, 1);
        assert_eq!(library.items.len(), 1);
        assert_eq!(library.items[0].kind, ItemKind::Video);
        assert_eq!(library.items[0].title, "clip");
        assert!(Path::new(&library.items[0].path).exists());
    }
}
