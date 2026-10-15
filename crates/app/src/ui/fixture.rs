use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::media::{probe, thumb};
use crate::model::ItemKind;

pub struct Item {
    pub title: String,
    pub path: PathBuf,
    pub kind: ItemKind,
    pub folder: Option<usize>,
    pub thumb: Option<PathBuf>,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub codec: Option<String>,
    pub duration: f64,
    pub bytes: u64,
}

pub struct Display {
    pub name: String,
    pub spaces: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Space { display: usize, space: usize },
    Display { display: usize },
    Lock,
}

pub struct Library {
    pub items: Vec<Item>,
    pub folders: Vec<String>,
    pub displays: Vec<Display>,
    pub assignments: Vec<(Target, usize)>,
}

impl Library {
    pub fn markers(&self, item: usize) -> Vec<Target> {
        return self.assignments.iter().filter(|(_, index)| *index == item).map(|(target, _)| *target).collect();
    }

    pub fn desktop(&self) -> Option<usize> {
        return self.assignments.iter().find(|(target, _)| !matches!(target, Target::Lock)).map(|(_, index)| *index);
    }

    pub fn lock(&self) -> Option<usize> {
        return self.assignments.iter().find(|(target, _)| matches!(target, Target::Lock)).map(|(_, index)| *index);
    }
}

const MEDIA: [&str; 7] = ["mp4", "mov", "m4v", "png", "jpg", "jpeg", "gif"];

pub fn default_root() -> PathBuf {
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
    return home.join("Documents/livewall-design/design/fixtures/Library");
}

pub fn load(root: &Path, thumbs: &Path) -> Library {
    let mut folders: Vec<(String, PathBuf)> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| (entry.file_name().to_string_lossy().into_owned(), entry.path()))
        .collect();
    folders.sort();

    let mut items = Vec::new();
    for (index, (_, dir)) in folders.iter().enumerate() {
        let mut files: Vec<PathBuf> = fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| is_media(path))
            .collect();
        files.sort();
        items.extend(files.into_iter().map(|path| item(&path, Some(index), thumbs)));
    }
    items.sort_by_key(|item| item.title.to_lowercase());

    let displays = vec![
        Display { name: "Built-in Retina Display".to_string(), spaces: 3 },
        Display { name: "LG UltraFine".to_string(), spaces: 1 },
    ];
    let find = |title: &str| items.iter().position(|item| item.title == title);
    let mut assignments = Vec::new();
    let seeds = [
        (Target::Space { display: 0, space: 0 }, "Aurora"),
        (Target::Space { display: 0, space: 1 }, "Aurora"),
        (Target::Space { display: 0, space: 2 }, "Carpet"),
        (Target::Display { display: 1 }, "Perlin Fog"),
        (Target::Lock, "Seahorse"),
    ];
    for (target, title) in seeds {
        if let Some(index) = find(title) {
            assignments.push((target, index));
        }
    }

    return Library { items, folders: folders.into_iter().map(|(name, _)| name).collect(), displays, assignments };
}

fn is_media(path: &Path) -> bool {
    return path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| MEDIA.contains(&value.to_ascii_lowercase().as_str()));
}

fn kind_of(path: &Path) -> ItemKind {
    return match path.extension().and_then(|value| value.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("gif") => ItemKind::Gif,
        Some("png" | "jpg" | "jpeg") => ItemKind::Image,
        _ => ItemKind::Video,
    };
}

fn item(path: &Path, folder: Option<usize>, thumbs: &Path) -> Item {
    let text = path.to_string_lossy().into_owned();
    let kind = kind_of(path);
    let probe = probe::inspect(&text, kind);
    let digest = Sha256::digest(text.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    let thumb = thumb::generate_sync(Uuid::from_bytes(bytes), kind, &text, probe.duration, thumbs);
    return Item {
        title: path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default(),
        path: path.to_path_buf(),
        kind,
        folder,
        thumb,
        width: probe.width,
        height: probe.height,
        fps: probe.fps,
        codec: probe.codec,
        duration: probe.duration,
        bytes: fs::metadata(path).map_or(0, |meta| meta.len()),
    };
}
