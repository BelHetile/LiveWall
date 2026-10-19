use std::fs;
use std::io;
use std::path::PathBuf;

use proto::{ErrorCode, Request, Response};
use serde::Serialize;

use super::{App, LOCKED, NO_SUCH_FOLDER, ok};
use crate::engine::still;
use crate::i18n;
use crate::media::{optimize, thumb};
use crate::model::{Folder, FolderId, Item, ItemId, Library, LockAssignment, Privacy};
use crate::paths::{DirKind, Paths, Visibility};

#[derive(Serialize)]
pub struct LibraryView<'a> {
    items: Vec<&'a Item>,
    folders: Vec<&'a Folder>,
}

impl Library {
    pub fn is_locked_folder(&self, folder: Option<FolderId>) -> bool {
        return folder.is_some_and(|id| self.folders.iter().any(|candidate| candidate.id == id && candidate.locked));
    }

    pub fn is_private(&self, item: &Item) -> bool {
        return self.is_locked_folder(item.folder);
    }

    pub fn is_private_id(&self, id: ItemId) -> bool {
        return self.items.iter().find(|item| item.id == id).is_some_and(|item| self.is_private(item));
    }

    pub fn visibility(&self, item: &Item) -> Visibility {
        return Visibility::from(self.is_private(item));
    }

    pub fn folder_items(&self, folder: FolderId) -> Vec<ItemId> {
        return self.items.iter().filter(|item| item.folder == Some(folder)).map(|item| item.id).collect();
    }

    pub fn set_folder_flags(&mut self, id: FolderId, locked: Option<bool>, hidden: Option<bool>) -> bool {
        let Some(folder) = self.folders.iter_mut().find(|folder| folder.id == id) else {
            return false;
        };
        if let Some(locked) = locked {
            folder.locked = locked;
        }
        if let Some(hidden) = hidden {
            folder.hidden = hidden;
        }
        return true;
    }

    pub fn replacement(&self, space: Option<&str>) -> Option<ItemId> {
        let remembered = match space {
            Some(space) => self.placement.safe.get(space).copied(),
            None => self.placement.safe_default,
        };
        let usable = |id: ItemId| {
            return self
                .items
                .iter()
                .find(|item| item.id == id)
                .is_some_and(|item| item.playable && !self.is_private(item));
        };
        return remembered
            .filter(|id| usable(*id))
            .or_else(|| self.items.iter().find(|item| item.playable && !self.is_private(item)).map(|item| item.id));
    }

    pub fn resolve(&self, space: Option<&str>, reveal: bool) -> Option<ItemId> {
        let scoped = space.and_then(|space| {
            let item = self.placement.spaces.get(space)?.item?;
            return Some((item, Some(space)));
        });
        let (assigned, key) = scoped.or_else(|| self.placement.default.item.map(|item| (item, None)))?;
        if reveal || !self.is_private_id(assigned) {
            return Some(assigned);
        }
        return self.replacement(key);
    }

    pub fn playable_item(&self, item: Option<ItemId>) -> Option<ItemId> {
        return item.filter(|id| self.items.iter().find(|entry| entry.id == *id).is_some_and(|entry| entry.playable));
    }

    pub fn blanked(&self, space: &str) -> bool {
        return self.resolve(Some(space), false).is_none() && self.resolve(Some(space), true).is_some();
    }

    pub fn remember(&mut self, space: Option<&str>, item: Option<ItemId>) {
        let Some(item) = item.filter(|id| !self.is_private_id(*id)) else {
            return;
        };
        match space {
            Some(space) => {
                self.placement.safe.insert(space.to_string(), item);
            }
            None => self.placement.safe_default = Some(item),
        }
    }

    pub fn forget_safe(&mut self, removed: &[ItemId]) {
        self.placement.safe_default = self.placement.safe_default.filter(|id| !removed.contains(id));
        self.placement.safe.retain(|_, id| !removed.contains(id));
    }
}

pub fn reveals_desktop(unlocked: bool, privacy: &Privacy) -> bool {
    return unlocked || !privacy.hide_desktop;
}

pub fn holds_private(library: &Library, unlocked: bool, privacy: &Privacy) -> bool {
    return !unlocked
        && reveals_desktop(unlocked, privacy)
        && library.resolve(None, true).is_some_and(|id| library.is_private_id(id));
}

#[derive(Debug, PartialEq, Eq)]
pub enum Title {
    Name(String),
    Private,
}

pub fn menu_title(library: &Library, unlocked: bool, privacy: &Privacy) -> Option<Title> {
    let reveal = reveals_desktop(unlocked, privacy);
    let id = library.resolve(None, reveal)?;
    if !unlocked && library.is_private_id(id) {
        return Some(Title::Private);
    }
    return library.items.iter().find(|item| item.id == id).map(|item| Title::Name(item.title.clone()));
}

pub fn visible_items(library: &Library, unlocked: bool) -> Vec<&Item> {
    return library.items.iter().filter(|item| unlocked || !library.is_private(item)).collect();
}

pub fn visible_folders(library: &Library, unlocked: bool) -> Vec<&Folder> {
    return library.folders.iter().filter(|folder| unlocked || !(folder.locked && folder.hidden)).collect();
}

pub fn denies(library: &Library, request: &Request) -> bool {
    let item = |id: &String| id.parse::<ItemId>().is_ok_and(|id| library.is_private_id(id));
    let folder = |id: &String| id.parse::<FolderId>().is_ok_and(|id| library.is_locked_folder(Some(id)));
    return match request {
        Request::LibraryRemove { ids } => ids.iter().any(item),
        Request::LibraryMove { ids, folder: target } => ids.iter().any(item) || target.as_ref().is_some_and(folder),
        Request::LibraryAdd { folder: target, .. } => target.as_ref().is_some_and(folder),
        Request::LibraryRename { id, .. }
        | Request::Optimize { id }
        | Request::OptimizeCancel { id }
        | Request::SetGeometry { item: id, .. }
        | Request::SetItem { item: id, .. }
        | Request::ItemHotkey { item: id, .. }
        | Request::LockScreen { item: Some(id), .. }
        | Request::Apply { item: Some(id), .. } => item(id),
        Request::FolderRename { id, .. } | Request::FolderDelete { id } | Request::FolderStyle { id, .. } => folder(id),
        _ => false,
    };
}

fn relocate(paths: &Paths, item: &mut Item, to: Visibility, lock: Option<LockAssignment>) -> io::Result<()> {
    let from = PathBuf::from(&item.path);
    let Some(name) = from.file_name() else {
        return Err(io::Error::other("item has no file name"));
    };
    let target = paths.dir_for(DirKind::Wallpapers, to).join(name);
    let mtime = still::mtime_secs(&item.path);
    if from != target {
        match fs::rename(&from, &target) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    move_thumb(paths, item.id, to);
    let times = lock.filter(|lock| lock.item == item.id).map(|lock| lock.time);
    drop_stills(paths, item, mtime, times, to.opposite());
    item.path = target.to_string_lossy().into_owned();
    return Ok(());
}

fn relocate_many(
    paths: &Paths,
    items: &mut [Item],
    ids: &[ItemId],
    to: Visibility,
    lock: Option<LockAssignment>,
) -> io::Result<()> {
    let mut moved: Vec<ItemId> = Vec::new();
    for id in ids {
        let Some(item) = items.iter_mut().find(|item| item.id == *id) else {
            continue;
        };
        if let Err(error) = relocate(paths, item, to, lock) {
            for done in moved {
                if let Some(item) = items.iter_mut().find(|item| item.id == done) {
                    let _ = relocate(paths, item, to.opposite(), lock);
                }
            }
            return Err(error);
        }
        moved.push(*id);
    }
    return Ok(());
}

fn move_thumb(paths: &Paths, id: ItemId, to: Visibility) {
    let source = thumb::file(&paths.dir_for(DirKind::Thumbs, to.opposite()), id);
    if !source.exists() {
        return;
    }
    if fs::rename(&source, thumb::file(&paths.dir_for(DirKind::Thumbs, to), id)).is_err() {
        let _ = fs::remove_file(&source);
    }
}

fn drop_stills(paths: &Paths, item: &Item, mtime: i64, lock_time: Option<f32>, from: Visibility) {
    let dir = paths.dir_for(DirKind::Stills, from);
    for time in std::iter::once(item.still_time).chain(lock_time) {
        let key = still::cache_key(&item.path, mtime, f64::from(time));
        let _ = fs::remove_file(still::cache_file(&dir, &key));
    }
}

impl App {
    pub fn library_view(&self) -> LibraryView<'_> {
        return LibraryView {
            items: visible_items(&self.library, self.unlocked),
            folders: visible_folders(&self.library, self.unlocked),
        };
    }

    pub fn refresh_visibility(&mut self) {
        if self.unlocked {
            self.reapply();
            return;
        }
        let playback = self.playback();
        self.engine.hide_private(&self.library, &playback);
        self.engine.refresh_desktop(&self.library, false);
        self.refresh_power();
    }

    fn optimizing(&self, item: ItemId) -> bool {
        return matches!(
            self.optimize.status(item),
            Some(optimize::JobState::Queued | optimize::JobState::Running { .. })
        );
    }

    fn relocate(&mut self, items: &[ItemId], to: Visibility) -> Result<(), Response> {
        if items.iter().any(|item| self.optimizing(*item)) {
            return Err(Response::err(ErrorCode::Busy, i18n::key("optimize is in progress for this item")));
        }
        let lock = self.library.placement.lock;
        if let Err(error) = relocate_many(&self.paths, &mut self.library.items, items, to, lock) {
            self.reapply();
            return Err(Response::err(ErrorCode::Internal, format!("cannot move media: {error}")));
        }
        for item in items {
            self.engine.rehome(*item, &self.library);
        }
        return Ok(());
    }

    pub fn move_to_folder(&mut self, ids: &[ItemId], folder: Option<FolderId>) -> Response {
        let to = Visibility::from(self.library.is_locked_folder(folder));
        let changing: Vec<ItemId> = self
            .library
            .items
            .iter()
            .filter(|item| ids.contains(&item.id) && self.library.visibility(item) != to)
            .map(|item| item.id)
            .collect();
        if let Err(response) = self.relocate(&changing, to) {
            return response;
        }
        let moved = self.library.move_items(ids, folder);
        self.persist_library();
        self.refresh_visibility();
        return ok(&serde_json::json!({ "moved": moved }));
    }

    pub fn delete_folder(&mut self, id: FolderId) -> Response {
        if !self.library.has_folder(id) {
            return Response::err(ErrorCode::NotFound, NO_SUCH_FOLDER);
        }
        if self.library.is_locked_folder(Some(id))
            && let Err(response) = self.relocate(&self.library.folder_items(id), Visibility::Public)
        {
            return response;
        }
        let Some(moved) = self.library.delete_folder(id) else {
            return Response::err(ErrorCode::NotFound, NO_SUCH_FOLDER);
        };
        self.persist_library();
        self.refresh_visibility();
        return ok(&serde_json::json!({ "deleted": true, "moved": moved }));
    }

    pub fn set_folder_lock(&mut self, id: FolderId, locked: Option<bool>, hidden: Option<bool>) -> Response {
        let Some(folder) = self.library.folders.iter().find(|folder| folder.id == id) else {
            return Response::err(ErrorCode::NotFound, NO_SUCH_FOLDER);
        };
        let (was_locked, was_hidden) = (folder.locked, folder.hidden);
        let unlocking = locked == Some(false);
        let restyling = hidden.is_some_and(|hidden| hidden != was_hidden);
        if was_locked && !self.unlocked && (unlocking || restyling) {
            return Response::err(ErrorCode::Locked, LOCKED);
        }
        if let Some(to_private) = locked.filter(|locked| *locked != was_locked) {
            let members = self.library.folder_items(id);
            if let Err(response) = self.relocate(&members, Visibility::from(to_private)) {
                return response;
            }
        }
        self.library.set_folder_flags(id, locked, hidden);
        self.persist_library();
        if locked == Some(true) && !was_locked {
            self.lock();
        }
        self.refresh_visibility();
        return ok(&serde_json::json!({ "id": id.to_string(), "locked": locked, "hidden": hidden }));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proto::{ApplyTarget, FillMode};
    use uuid::Uuid;

    use super::*;
    use crate::model::{Assignment, ItemKind};

    fn item(path: &str) -> Item {
        return Item {
            id: Uuid::new_v4(),
            kind: ItemKind::Image,
            path: path.to_string(),
            title: "t".into(),
            added_at: jiff::Timestamp::UNIX_EPOCH,
            folder: None,
            mode: FillMode::Fill,
            modes: BTreeMap::new(),
            volume: 1.0,
            speed: 1.0,
            muted: true,
            still_time: 2.0,
            optimized: false,
            hotkey: None,
            width: 0,
            height: 0,
            duration: 0.0,
            fps: 0.0,
            codec: None,
            audio_codec: None,
            hdr: false,
            playable: true,
            needs_optimize: false,
            optimize_reason: None,
        };
    }

    fn setup() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = Paths::at(dir.path().join("data"));
        paths.ensure().expect("ensure");
        return (dir, paths);
    }

    fn stored(paths: &Paths, name: &str) -> Item {
        let path = paths.wallpapers_dir().join(name);
        fs::write(&path, name.as_bytes()).expect("media");
        return item(&path.to_string_lossy());
    }

    #[test]
    fn relocate_moves_media_and_thumb_there_and_back() {
        let (_guard, paths) = setup();
        let mut entry = stored(&paths, "a.png");
        let thumb_public = thumb::file(&paths.thumbs_dir(), entry.id);
        fs::write(&thumb_public, b"thumb").expect("thumb");

        relocate(&paths, &mut entry, Visibility::Private, None).expect("lock");
        let private = paths.dir_for(DirKind::Wallpapers, Visibility::Private).join("a.png");
        assert_eq!(entry.path, private.to_string_lossy());
        assert_eq!(fs::read(&private).expect("moved"), b"a.png");
        assert!(!paths.wallpapers_dir().join("a.png").exists());
        assert!(!thumb_public.exists());
        assert_eq!(
            fs::read(thumb::file(&paths.dir_for(DirKind::Thumbs, Visibility::Private), entry.id)).expect("thumb"),
            b"thumb"
        );

        relocate(&paths, &mut entry, Visibility::Public, None).expect("unlock");
        assert_eq!(entry.path, paths.wallpapers_dir().join("a.png").to_string_lossy());
        assert!(paths.wallpapers_dir().join("a.png").exists());
        assert!(!private.exists());
        assert!(thumb_public.exists());
        assert!(!thumb::file(&paths.dir_for(DirKind::Thumbs, Visibility::Private), entry.id).exists());
    }

    #[test]
    fn relocate_drops_known_stills_from_the_old_cache() {
        let (_guard, paths) = setup();
        let mut entry = stored(&paths, "b.png");
        let mtime = still::mtime_secs(&entry.path);
        let lock = LockAssignment { item: entry.id, time: 7.0 };
        let mut files = Vec::new();
        for time in [entry.still_time, lock.time, 9.0] {
            let key = still::cache_key(&entry.path, mtime, f64::from(time));
            let file = still::cache_file(&paths.stills_dir(), &key);
            fs::write(&file, b"png").expect("still");
            files.push(file);
        }
        relocate(&paths, &mut entry, Visibility::Private, Some(lock)).expect("lock");
        assert!(!files[0].exists());
        assert!(!files[1].exists());
        assert!(files[2].exists());
    }

    #[test]
    fn relocate_tolerates_a_missing_media_file() {
        let (_guard, paths) = setup();
        let mut entry = item(&paths.wallpapers_dir().join("gone.png").to_string_lossy());
        relocate(&paths, &mut entry, Visibility::Private, None).expect("lock");
        assert_eq!(
            entry.path,
            paths.dir_for(DirKind::Wallpapers, Visibility::Private).join("gone.png").to_string_lossy()
        );
    }

    #[test]
    fn relocate_many_rolls_back_when_one_move_fails() {
        let (_guard, paths) = setup();
        let first = stored(&paths, "c.png");
        let second = stored(&paths, "d.png");
        fs::create_dir(paths.dir_for(DirKind::Wallpapers, Visibility::Private).join("d.png")).expect("blocker");
        let (first_path, second_path) = (first.path.clone(), second.path.clone());
        let ids = [first.id, second.id];
        let mut items = vec![first, second];

        assert!(relocate_many(&paths, &mut items, &ids, Visibility::Private, None).is_err());
        assert_eq!(items[0].path, first_path);
        assert_eq!(items[1].path, second_path);
        assert!(PathBuf::from(&first_path).exists());
        assert!(PathBuf::from(&second_path).exists());
        assert!(!paths.dir_for(DirKind::Wallpapers, Visibility::Private).join("c.png").exists());
    }

    #[test]
    fn relocate_many_moves_every_listed_item() {
        let (_guard, paths) = setup();
        let first = stored(&paths, "e.png");
        let second = stored(&paths, "f.png");
        let untouched = stored(&paths, "g.png");
        let ids = [first.id, second.id];
        let mut items = vec![first, second, untouched];
        relocate_many(&paths, &mut items, &ids, Visibility::Private, None).expect("lock");
        assert!(paths.dir_for(DirKind::Wallpapers, Visibility::Private).join("e.png").exists());
        assert!(paths.dir_for(DirKind::Wallpapers, Visibility::Private).join("f.png").exists());
        assert!(paths.wallpapers_dir().join("g.png").exists());
    }

    fn vault() -> (Library, Item, Item) {
        let mut library = Library::default();
        let id = library.create_folder("Vault".into());
        library.set_folder_flags(id, Some(true), None);
        let open = item("/open.png");
        let mut private = item("/private.png");
        private.folder = Some(id);
        library.items.extend([open.clone(), private.clone()]);
        return (library, open, private);
    }

    #[test]
    fn visibility_hides_private_items_and_hidden_locked_folders_only_while_locked() {
        let (mut library, open, private) = vault();
        let shown = |library: &Library, unlocked| {
            return visible_items(library, unlocked).iter().map(|item| item.id).collect::<Vec<_>>();
        };
        assert_eq!(shown(&library, false), vec![open.id]);
        assert_eq!(shown(&library, true), vec![open.id, private.id]);
        assert_eq!(visible_folders(&library, false).len(), 1);
        let id = library.folders[0].id;
        library.set_folder_flags(id, None, Some(true));
        assert!(visible_folders(&library, false).is_empty());
        assert_eq!(visible_folders(&library, true).len(), 1);
    }

    #[test]
    fn locked_session_denies_mutations_on_private_items_and_locked_folders() {
        let (library, open, private) = vault();
        let folder = library.folders[0].id.to_string();
        let (open, private) = (open.id.to_string(), private.id.to_string());
        let denied = [
            Request::LibraryRemove { ids: vec![open.clone(), private.clone()] },
            Request::LibraryMove { ids: vec![private.clone()], folder: None },
            Request::LibraryMove { ids: vec![open.clone()], folder: Some(folder.clone()) },
            Request::LibraryRename { id: private.clone(), title: "x".into() },
            Request::SetItem { item: private.clone(), volume: None, speed: None, muted: None, still_time: None },
            Request::ItemHotkey { item: private.clone(), combo: None, scope: proto::HotkeyScope::Desktop },
            Request::SetGeometry { item: private.clone(), mode: FillMode::Fill, geometry: proto::Geometry::default() },
            Request::Apply { item: Some(private.clone()), target: ApplyTarget::Default, transition: None },
            Request::LockScreen { item: Some(private.clone()), time: None },
            Request::Optimize { id: private.clone() },
            Request::OptimizeCancel { id: private },
            Request::FolderRename { id: folder.clone(), name: "x".into() },
            Request::FolderDelete { id: folder.clone() },
        ];
        for request in denied {
            assert!(denies(&library, &request), "{request:?}");
        }
        let allowed = [
            Request::LibraryRemove { ids: vec![open.clone()] },
            Request::LibraryMove { ids: vec![open.clone()], folder: None },
            Request::Apply { item: Some(open.clone()), target: ApplyTarget::Default, transition: None },
            Request::Apply { item: None, target: ApplyTarget::Default, transition: None },
            Request::SetItem { item: open, volume: None, speed: None, muted: None, still_time: None },
            Request::LockScreen { item: None, time: None },
            Request::FolderCreate { name: "x".into() },
            Request::Status,
            Request::Lock,
        ];
        for request in allowed {
            assert!(!denies(&library, &request), "{request:?}");
        }
    }

    fn hiding(hide_desktop: bool) -> Privacy {
        return Privacy { hide_desktop, ..Privacy::default() };
    }

    fn private_library() -> (Library, ItemId, ItemId, ItemId) {
        let mut library = Library::default();
        let vault = library.create_folder("Vault".into());
        library.set_folder_flags(vault, Some(true), None);
        let (open, mut hidden, spare) = (item("/open.png"), item("/hidden.png"), item("/spare.png"));
        hidden.folder = Some(vault);
        let ids = (open.id, hidden.id, spare.id);
        library.items.extend([open, hidden, spare]);
        return (library, ids.0, ids.1, ids.2);
    }

    #[test]
    fn items_are_private_only_in_locked_folders() {
        let (mut library, open, hidden, _) = private_library();
        assert!(library.is_private_id(hidden));
        assert!(!library.is_private_id(open));
        assert!(!library.is_private_id(Uuid::new_v4()));
        let vault = library.folders[0].id;
        library.set_folder_flags(vault, Some(false), None);
        assert!(!library.is_private_id(hidden));
        assert!(!library.set_folder_flags(Uuid::new_v4(), Some(true), None));
    }

    #[test]
    fn remember_records_only_non_private_items() {
        let (mut library, open, hidden, _) = private_library();
        library.remember(None, Some(hidden));
        library.remember(Some("S"), Some(hidden));
        assert!(library.placement.safe_default.is_none());
        assert!(library.placement.safe.is_empty());
        library.remember(None, Some(open));
        library.remember(Some("S"), Some(open));
        library.remember(Some("S"), None);
        assert_eq!(library.placement.safe_default, Some(open));
        assert_eq!(library.placement.safe.get("S"), Some(&open));
    }

    #[test]
    fn resolve_substitutes_private_items_only_when_hidden() {
        let (mut library, open, hidden, spare) = private_library();
        library.placement.default.item = Some(hidden);
        library.placement.safe_default = Some(spare);
        assert_eq!(library.resolve(Some("S"), true), Some(hidden));
        assert_eq!(library.resolve(Some("S"), false), Some(spare));
        assert_eq!(library.resolve(None, false), Some(spare));
        library.placement.spaces.insert("S".into(), Assignment { item: Some(hidden), geometry_override: None });
        library.placement.safe.insert("S".into(), open);
        assert_eq!(library.resolve(Some("S"), false), Some(open));
        assert_eq!(library.resolve(Some("T"), false), Some(spare));
        library.placement.spaces.insert("S".into(), Assignment { item: Some(spare), geometry_override: None });
        assert_eq!(library.resolve(Some("S"), false), Some(spare));
    }

    #[test]
    fn replacement_falls_back_to_first_public_playable_item_then_nothing() {
        let (mut library, open, hidden, _) = private_library();
        library.placement.default.item = Some(hidden);
        assert_eq!(library.resolve(None, false), Some(open));
        library.placement.safe_default = Some(hidden);
        assert_eq!(library.resolve(None, false), Some(open));
        library.placement.safe_default = Some(Uuid::new_v4());
        assert_eq!(library.resolve(None, false), Some(open));
        assert!(!library.blanked("S"));
        library.items.retain(|item| item.id == hidden);
        assert_eq!(library.resolve(None, false), None);
        assert_eq!(library.resolve(None, true), Some(hidden));
        assert!(library.blanked("S"));
        library.placement.default.item = None;
        assert!(!library.blanked("S"));
    }

    #[test]
    fn replacement_skips_unplayable_items_and_forgotten_history() {
        let (mut library, open, hidden, spare) = private_library();
        library.placement.default.item = Some(hidden);
        library.items[0].playable = false;
        assert_eq!(library.resolve(None, false), Some(spare));
        library.placement.safe_default = Some(spare);
        library.placement.safe.insert("S".into(), open);
        library.forget_safe(&[spare]);
        assert!(library.placement.safe_default.is_none());
        assert_eq!(library.placement.safe.get("S"), Some(&open));
        library.forget_safe(&[open]);
        assert!(library.placement.safe.is_empty());
    }

    #[test]
    fn locking_replaces_private_wallpapers_on_the_desktop_by_default() {
        assert!(Privacy::default().hide_desktop);
        assert!(reveals_desktop(true, &hiding(true)));
        assert!(!reveals_desktop(false, &hiding(true)));
        assert!(reveals_desktop(false, &hiding(false)));
        let (mut library, open, hidden, _) = private_library();
        library.placement.default.item = Some(hidden);
        library.placement.safe_default = Some(open);
        let shown = |privacy: &Privacy| library.resolve(Some("S"), reveals_desktop(false, privacy));
        assert_eq!(shown(&hiding(true)), Some(open));
        assert_eq!(shown(&hiding(false)), Some(hidden));
    }

    #[test]
    fn menu_title_never_names_a_private_wallpaper_while_locked() {
        let (mut library, open, hidden, _) = private_library();
        library.items.iter_mut().for_each(|item| item.title = item.path.clone());
        library.placement.default.item = Some(hidden);
        library.placement.safe_default = Some(open);
        assert_eq!(menu_title(&library, false, &hiding(false)), Some(Title::Private));
        assert_eq!(menu_title(&library, false, &hiding(true)), Some(Title::Name("/open.png".into())));
        assert_eq!(menu_title(&library, true, &hiding(false)), Some(Title::Name("/hidden.png".into())));
        library.placement.default.item = Some(open);
        assert_eq!(menu_title(&library, false, &hiding(false)), Some(Title::Name("/open.png".into())));
        library.placement.default.item = None;
        assert_eq!(menu_title(&library, false, &hiding(false)), None);
    }

    #[test]
    fn automatic_change_holds_while_a_private_wallpaper_is_shown() {
        let (mut library, _, hidden, _) = private_library();
        library.placement.default.item = Some(hidden);
        assert!(holds_private(&library, false, &hiding(false)));
        assert!(!holds_private(&library, true, &hiding(false)));
        assert!(!holds_private(&library, false, &hiding(true)));
        let (library, open, _, _) = private_library();
        let mut library = library;
        library.placement.default.item = Some(open);
        assert!(!holds_private(&library, false, &hiding(false)));
    }

    #[test]
    fn hide_desktop_defaults_to_true_for_older_settings() {
        let settings: crate::model::Settings =
            serde_json::from_str(r#"{"privacy":{"lock_on_idle":true}}"#).expect("older settings");
        assert!(settings.privacy.hide_desktop);
        let mut settings = settings;
        crate::model::apply_patch(&mut settings, &serde_json::json!({ "privacy": { "hide_desktop": false } }))
            .expect("patch");
        assert!(!settings.privacy.hide_desktop);
        assert!(settings.privacy.lock_on_idle);
    }

    #[test]
    fn folder_items_lists_members() {
        let (library, _, hidden, _) = private_library();
        assert_eq!(library.folder_items(library.folders[0].id), vec![hidden]);
    }

    #[test]
    fn visibility_follows_the_containing_folder() {
        let (library, open, hidden, _) = private_library();
        let find = |id: ItemId| library.items.iter().find(|item| item.id == id).expect("item");
        assert_eq!(library.visibility(find(open)), Visibility::Public);
        assert_eq!(library.visibility(find(hidden)), Visibility::Private);
    }
}
