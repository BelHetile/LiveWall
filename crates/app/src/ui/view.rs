use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_app_kit::NSImage;
use objc2_foundation::NSString;
use proto::{FillMode, Geometry};
use uuid::Uuid;

use crate::app::App;
use crate::engine::still;
use crate::media::thumb;
use crate::model::{self, ItemId, ItemKind};
use crate::ui::state::Scope;

#[derive(Clone, Copy, PartialEq)]
pub struct Settings {
    pub volume: f64,
    pub muted: bool,
    pub speed: f64,
    pub mode: FillMode,
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

pub struct Item {
    pub id: ItemId,
    pub title: String,
    pub path: PathBuf,
    pub kind: ItemKind,
    pub folder: Option<usize>,
    pub image: Option<Retained<NSImage>>,
    pub added: jiff::Timestamp,
    pub settings: Settings,
    pub modes: BTreeMap<FillMode, Geometry>,
    pub changed: bool,
    pub playable: bool,
    pub optimizable: bool,
    pub progress: Option<f64>,
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

pub struct Lock {
    pub time: f64,
    pub image: Option<Retained<NSImage>>,
}

pub struct Download {
    pub percent: f64,
}

pub struct Library {
    pub items: Vec<Item>,
    pub lock: Option<Lock>,
    pub folders: Vec<String>,
    pub folder_ids: Vec<Uuid>,
    pub spaces: Vec<String>,
    pub desktops: usize,
    pub current: usize,
    pub assignments: Vec<(Target, usize)>,
    pub downloads: Vec<Download>,
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

    pub fn lock(&self) -> Option<usize> {
        return self.find(Target::Lock);
    }

    pub fn position(&self, id: ItemId) -> Option<usize> {
        return self.items.iter().position(|item| item.id == id);
    }
}

#[derive(Default)]
pub struct Thumbs {
    images: HashMap<ItemId, (SystemTime, Retained<NSImage>)>,
    stills: HashMap<PathBuf, Retained<NSImage>>,
    requested: HashSet<ItemId>,
}

impl Thumbs {
    fn image(&mut self, app: &App, item: &model::Item) -> Option<Retained<NSImage>> {
        let file = thumb::file(&app.paths().thumbs_for(app.library().is_private(item)), item.id);
        let Ok(stamp) = fs::metadata(&file).and_then(|meta| meta.modified()) else {
            if self.requested.insert(item.id) {
                app.ensure_thumbnail(item);
            }
            return None;
        };
        if let Some((seen, image)) = self.images.get(&item.id)
            && *seen == stamp
        {
            return Some(image.clone());
        }
        let image = load(&file)?;
        self.images.insert(item.id, (stamp, image.clone()));
        return Some(image);
    }

    fn still(&mut self, app: &App, item: &model::Item, time: f32) -> Option<Retained<NSImage>> {
        let key = still::cache_key(&item.path, still::mtime_secs(&item.path), time);
        let file = still::cache_file(&app.paths().stills_for(app.library().is_private(item)), &key);
        if let Some(image) = self.stills.get(&file) {
            return Some(image.clone());
        }
        let image = load(&file)?;
        self.stills.insert(file, image.clone());
        return Some(image);
    }
}

fn load(file: &std::path::Path) -> Option<Retained<NSImage>> {
    return NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(&file.to_string_lossy()));
}

pub fn build(app: &App, thumbs: &mut Thumbs) -> Library {
    let model = app.library();
    let defaults = app.settings();
    let folder_ids: Vec<Uuid> = model.folders.iter().map(|folder| folder.id).collect();
    let items: Vec<Item> = model
        .items
        .iter()
        .map(|item| {
            let geometry = item.geometry();
            let changed = item.volume.to_bits() != defaults.volume.to_bits()
                || item.speed.to_bits() != defaults.speed.to_bits()
                || !item.muted
                || item.mode != FillMode::Fill
                || item.modes.values().any(|stored| *stored != Geometry::default());
            return Item {
                id: item.id,
                title: item.title.clone(),
                path: PathBuf::from(&item.path),
                kind: item.kind,
                folder: item.folder.and_then(|id| folder_ids.iter().position(|candidate| *candidate == id)),
                image: thumbs.image(app, item),
                added: item.added_at,
                settings: Settings {
                    volume: f64::from(item.volume),
                    muted: item.muted,
                    speed: f64::from(item.speed),
                    mode: item.mode,
                    x: geometry.x,
                    y: geometry.y,
                    zoom: geometry.zoom,
                },
                modes: item.modes.clone(),
                changed,
                playable: item.playable,
                optimizable: item.needs_optimize,
                progress: item.needs_optimize.then(|| app.optimize_progress(item.id)).flatten(),
                width: item.width,
                height: item.height,
                fps: item.fps,
                codec: item.codec.clone(),
                duration: item.duration,
                bytes: fs::metadata(&item.path).map_or(0, |meta| meta.len()),
            };
        })
        .collect();

    let (spaces, current) = app.desktops();
    let placement = &model.placement;
    let position = |id: Option<ItemId>| id.and_then(|id| model.items.iter().position(|item| item.id == id));
    let mut assignments: Vec<(Target, usize)> = Vec::new();
    if let Some(index) = position(placement.default.item) {
        assignments.push((Target::All, index));
    }
    for (space, uuid) in spaces.iter().enumerate() {
        if let Some(index) = position(placement.spaces.get(uuid).and_then(|assignment| assignment.item)) {
            assignments.push((Target::Desktop(space), index));
        }
    }
    let lock = placement.lock.and_then(|lock| {
        let index = position(Some(lock.item))?;
        assignments.push((Target::Lock, index));
        let image = thumbs.still(app, &model.items[index], lock.time);
        return Some(Lock { time: f64::from(lock.time), image });
    });
    let downloads = app.download_progress().into_iter().map(|report| Download { percent: report.progress }).collect();

    return Library {
        items,
        lock,
        folders: model.folders.iter().map(|folder| folder.name.clone()).collect(),
        folder_ids,
        desktops: spaces.len(),
        spaces,
        current,
        assignments,
        downloads,
    };
}
