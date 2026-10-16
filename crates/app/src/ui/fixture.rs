use std::fs;
use std::path::{Path, PathBuf};

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_app_kit::NSImage;
use objc2_foundation::NSString;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::media::{probe, thumb};
use crate::model::ItemKind;
use crate::ui::state::Scope;

#[derive(Clone, Copy, PartialEq)]
pub struct Settings {
    pub volume: f64,
    pub muted: bool,
    pub speed: f64,
    pub fill: isize,
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl Default for Settings {
    fn default() -> Self {
        return Self { volume: 0.6, muted: true, speed: 1.0, fill: 0, x: 0.0, y: 0.0, zoom: 1.0 };
    }
}

pub struct Item {
    pub title: String,
    pub path: PathBuf,
    pub kind: ItemKind,
    pub folder: Option<usize>,
    pub image: Option<Retained<NSImage>>,
    pub added: usize,
    pub settings: Settings,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub codec: Option<String>,
    pub duration: f64,
    pub bytes: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Target {
    All,
    Desktop(usize),
    Lock,
}

pub struct Still {
    pub time: f64,
    pub image: Retained<NSImage>,
}

pub struct Library {
    pub items: Vec<Item>,
    pub still: Option<Still>,
    pub folders: Vec<String>,
    pub desktops: usize,
    pub current: usize,
    pub assignments: Vec<(Target, usize)>,
    thumbs: PathBuf,
}

impl Library {
    pub fn markers(&self, item: usize) -> Vec<Target> {
        return self.assignments.iter().filter(|(_, index)| *index == item).map(|(target, _)| *target).collect();
    }

    fn find(&self, target: Target) -> Option<usize> {
        return self.assignments.iter().find(|(candidate, _)| *candidate == target).map(|(_, index)| *index);
    }

    pub fn showing(&self, scope: Scope) -> Vec<Target> {
        return match scope {
            Scope::All => {
                self.assignments.iter().map(|(target, _)| *target).filter(|target| *target != Target::Lock).collect()
            }
            Scope::Space(space) => [Target::Desktop(space), Target::All]
                .into_iter()
                .find(|target| self.find(*target).is_some())
                .into_iter()
                .collect(),
        };
    }

    pub fn effective(&self, space: usize) -> Option<usize> {
        return self.find(Target::Desktop(space)).or_else(|| self.find(Target::All));
    }

    pub fn desktops(&self, scope: Scope) -> Vec<(usize, Option<usize>)> {
        let spaces: Vec<usize> = match scope {
            Scope::All => (0..self.desktops).collect(),
            Scope::Space(space) => vec![space],
        };
        return spaces.into_iter().map(|space| (space, self.effective(space))).collect();
    }

    pub fn replace(&mut self, space: usize, item: usize) {
        let own = Target::Desktop(space);
        let target = if self.find(own).is_some() || self.find(Target::All).is_none() { own } else { Target::All };
        self.assignments.retain(|(candidate, _)| *candidate != target);
        self.assignments.push((target, item));
    }

    pub fn lock(&self) -> Option<usize> {
        return self.find(Target::Lock);
    }

    pub fn assign(&mut self, scope: Scope, item: usize) {
        let target = match scope {
            Scope::Space(space) => Target::Desktop(space),
            Scope::All => Target::All,
        };
        if target == Target::All {
            self.assignments.retain(|(candidate, _)| !matches!(candidate, Target::All | Target::Desktop(_)));
        } else {
            self.assignments.retain(|(candidate, _)| *candidate != target);
        }
        self.assignments.push((target, item));
    }

    pub fn import(&mut self, path: &Path) -> usize {
        let before = self.items.len();
        let files: Vec<PathBuf> = if path.is_dir() { media_in(path) } else { vec![path.to_path_buf()] };
        for file in files.into_iter().filter(|file| is_media(file)) {
            let added = self.items.iter().map(|item| item.added).max().map_or(0, |last| last + 1);
            self.items.push(item(&file, None, added, &self.thumbs));
        }
        return self.items.len() - before;
    }

    pub fn set_lock(&mut self, item: usize) {
        self.still = None;
        self.assignments.retain(|(target, _)| *target != Target::Lock);
        self.assignments.push((Target::Lock, item));
    }

    pub fn set_lock_frame(&mut self, item: usize, still: Still) {
        self.set_lock(item);
        self.still = Some(still);
    }

    pub fn remove(&mut self, item: usize) {
        if self.lock() == Some(item) {
            self.still = None;
        }
        self.items.remove(item);
        self.assignments.retain(|(_, index)| *index != item);
        for (_, index) in &mut self.assignments {
            if *index > item {
                *index -= 1;
            }
        }
    }
}

const MEDIA: [&str; 7] = ["mp4", "mov", "m4v", "png", "jpg", "jpeg", "gif"];
const DESKTOPS: usize = 3;

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
        for path in media_in(dir) {
            let added = items.len();
            items.push(item(&path, Some(index), added, thumbs));
        }
    }
    items.sort_by_key(|item| item.title.to_lowercase());

    let find = |title: &str| items.iter().position(|item| item.title == title);
    let mut assignments = Vec::new();
    let seeds = [
        (Target::Desktop(0), "Aurora"),
        (Target::Desktop(1), "Aurora"),
        (Target::Desktop(2), "Carpet"),
        (Target::All, "Perlin Fog"),
        (Target::Lock, "Seahorse"),
    ];
    for (target, title) in seeds {
        if let Some(index) = find(title) {
            assignments.push((target, index));
        }
    }

    return Library {
        items,
        still: None,
        folders: folders.into_iter().map(|(name, _)| name).collect(),
        desktops: DESKTOPS,
        current: 0,
        assignments,
        thumbs: thumbs.to_path_buf(),
    };
}

fn media_in(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_media(path))
        .collect();
    files.sort();
    return files;
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

fn item(path: &Path, folder: Option<usize>, added: usize, thumbs: &Path) -> Item {
    let text = path.to_string_lossy().into_owned();
    let kind = kind_of(path);
    let probe = probe::inspect(&text, kind);
    let digest = Sha256::digest(text.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    let image = thumb::generate_sync(Uuid::from_bytes(bytes), kind, &text, probe.duration, thumbs).and_then(|thumb| {
        return NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(&thumb.to_string_lossy()));
    });
    return Item {
        title: path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default(),
        path: path.to_path_buf(),
        kind,
        folder,
        image,
        added,
        settings: Settings::default(),
        width: probe.width,
        height: probe.height,
        fps: probe.fps,
        codec: probe.codec,
        duration: probe.duration,
        bytes: fs::metadata(path).map_or(0, |meta| meta.len()),
    };
}
