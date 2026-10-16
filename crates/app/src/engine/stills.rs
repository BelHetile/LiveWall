use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::engine::desktop::{self, Target};
use crate::engine::{Engine, still};
use crate::model::{Item, ItemId, ItemKind, Library};
use crate::paths::{DirKind, Paths, Visibility};

const STILL_EXTENSION: &str = "png";

pub enum Still {
    Ready(String),
    Pending(String),
}

pub struct Stills {
    public_dir: PathBuf,
    private_dir: PathBuf,
    pending: HashSet<String>,
}

impl Stills {
    pub fn new(paths: &Paths) -> Self {
        return Self {
            public_dir: paths.dir_for(DirKind::Stills, Visibility::Public),
            private_dir: paths.dir_for(DirKind::Stills, Visibility::Private),
            pending: HashSet::new(),
        };
    }

    pub fn ensure(&mut self, item: &Item, time: f64, visibility: Visibility) -> Still {
        if item.kind == ItemKind::Image && still::accepts(&item.path) {
            return Still::Ready(item.path.clone());
        }
        let key = still::cache_key(&item.path, still::mtime_secs(&item.path), time);
        let dir = match visibility {
            Visibility::Public => &self.public_dir,
            Visibility::Private => &self.private_dir,
        };
        let dest = still::cache_file(dir, &key);
        let path = dest.to_string_lossy().into_owned();
        if dest.exists() {
            return Still::Ready(path);
        }
        if self.pending.insert(path.clone()) {
            still::generate(item.id, item.kind, &item.path, time, dest);
        }
        return Still::Pending(path);
    }

    pub fn settle(&mut self, path: &str) {
        self.pending.remove(path);
    }

    pub fn sweep<'a>(&'a self, in_use: impl Iterator<Item = &'a str>) {
        let keep: HashSet<OsString> = in_use
            .chain(self.pending.iter().map(String::as_str))
            .filter_map(|path| Path::new(path).file_name().map(OsString::from))
            .collect();
        for dir in [&self.public_dir, &self.private_dir] {
            sweep_dir(dir, &keep);
        }
    }
}

fn sweep_dir(dir: &Path, keep: &HashSet<OsString>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if keep.contains(&entry.file_name()) || path.extension().is_none_or(|extension| extension != STILL_EXTENSION) {
            continue;
        }
        if let Err(error) = fs::remove_file(&path) {
            tracing::warn!(%error, path = %path.display(), "cannot remove unused still");
        }
    }
}

impl Engine {
    pub fn refresh_desktop(&mut self, library: &Library, force: bool) {
        let (targets, stills) = self.desktop_stills(library);
        self.desktop.update(self.mtm, &targets, &stills, force);
        self.sweep_stills();
    }

    pub fn on_unlock(&mut self, library: &Library) {
        let (targets, stills) = self.desktop_stills(library);
        self.desktop.on_unlock(self.mtm, &targets, &stills);
        self.sweep_stills();
    }

    pub fn on_lock(&mut self) {
        self.desktop.on_lock();
    }

    pub fn on_desktop_verify(&mut self) {
        self.desktop.verify_tick(self.mtm);
        if self.desktop.settled() {
            self.sweep_stills();
        }
    }

    pub fn desktop_report(&self) -> desktop::DesktopReport {
        return self.desktop.report(self.mtm, &self.desktop_targets());
    }

    pub fn restore_desktop(&mut self) {
        self.desktop.restore(self.mtm);
    }

    pub fn desktop_targets(&self) -> Vec<Target> {
        let mut targets: Vec<Target> = Vec::new();
        for surface in self.surfaces.values() {
            if targets.iter().any(|target| target.space == surface.spec.key.space) {
                continue;
            }
            targets.push(Target { space: surface.spec.key.space.clone(), display_id: self.display.id });
        }
        return targets;
    }

    pub fn on_still_ready(&mut self, item: ItemId, path: &str, library: &Library) {
        self.stills.settle(path);
        self.finish_unload_wait(item, path, library);
        self.refresh_desktop(library, false);
    }

    pub fn on_still_failed(&mut self, path: &str) {
        self.stills.settle(path);
    }

    fn sweep_stills(&self) {
        self.stills.sweep(self.desktop.stills_in_use().chain(self.unload.stills()));
    }

    fn desktop_stills(&mut self, library: &Library) -> (Vec<Target>, BTreeMap<String, String>) {
        let targets = self.desktop_targets();
        let (stills, blanked) = self.resolve_stills(library, &targets);
        self.desktop.revert(self.mtm, &blanked);
        return (targets, stills);
    }

    fn resolve_stills(&mut self, library: &Library, targets: &[Target]) -> (BTreeMap<String, String>, Vec<String>) {
        let mut stills = BTreeMap::new();
        let mut blanked = Vec::new();
        let lock = library.placement.lock.and_then(|lock| {
            let item = library.playable_item(Some(lock.item).filter(|id| !library.is_private_id(*id)))?;
            let entry = library.items.iter().find(|entry| entry.id == item)?;
            return Some((entry, lock.time));
        });
        for target in targets {
            let (item, time) = if let Some(found) = lock {
                found
            } else {
                let Some(item_id) = library.playable_item(library.resolve(Some(&target.space), false)) else {
                    if library.blanked(&target.space) {
                        blanked.push(target.space.clone());
                    }
                    continue;
                };
                let Some(item) = library.items.iter().find(|entry| entry.id == item_id) else { continue };
                (item, item.still_time)
            };
            if let Still::Ready(path) = self.stills.ensure(item, f64::from(time), Visibility::Public) {
                stills.insert(target.space.clone(), path);
            }
        }
        return (stills, blanked);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn still(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, b"png").expect("write still");
        return path;
    }

    #[test]
    fn sweep_removes_only_unused_stills() {
        let dir = tempfile::tempdir().expect("tempdir");
        let unused = still(dir.path(), "unused.png");
        let shown = still(dir.path(), "shown.png");
        let foreign = still(dir.path(), "notes.txt");
        let keep = HashSet::from([OsString::from("shown.png")]);
        sweep_dir(dir.path(), &keep);
        assert!(!unused.exists());
        assert!(shown.exists());
        assert!(foreign.exists());
    }

    #[test]
    fn sweep_ignores_a_missing_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        sweep_dir(&dir.path().join("absent"), &HashSet::new());
    }
}
