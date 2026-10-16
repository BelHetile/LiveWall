use std::fs;
use std::io;
use std::path::PathBuf;

use proto::Request;

use crate::engine::still;
use crate::media::thumb;
use crate::model::{Folder, FolderId, Item, ItemId, Library, LockAssignment};
use crate::paths::Paths;

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
        Request::LibraryRename { id, .. }
        | Request::Optimize { id }
        | Request::OptimizeCancel { id }
        | Request::SetGeometry { item: id, .. }
        | Request::SetItem { item: id, .. }
        | Request::Apply { item: Some(id), .. } => item(id),
        Request::FolderRename { id, .. } | Request::FolderDelete { id } => folder(id),
        _ => false,
    };
}

pub fn relocate(paths: &Paths, item: &mut Item, to_private: bool, lock: Option<LockAssignment>) -> io::Result<()> {
    let from = PathBuf::from(&item.path);
    let Some(name) = from.file_name() else {
        return Err(io::Error::other("item has no file name"));
    };
    let target = paths.wallpapers_for(to_private).join(name);
    let mtime = still::mtime_secs(&item.path);
    if from != target {
        match fs::rename(&from, &target) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    move_thumb(paths, item.id, to_private);
    let times = lock.filter(|lock| lock.item == item.id).map(|lock| lock.time);
    drop_stills(paths, item, mtime, times, !to_private);
    item.path = target.to_string_lossy().into_owned();
    return Ok(());
}

pub fn relocate_many(
    paths: &Paths,
    items: &mut [Item],
    ids: &[ItemId],
    to_private: bool,
    lock: Option<LockAssignment>,
) -> io::Result<()> {
    let mut moved: Vec<ItemId> = Vec::new();
    for id in ids {
        let Some(item) = items.iter_mut().find(|item| item.id == *id) else {
            continue;
        };
        if let Err(error) = relocate(paths, item, to_private, lock) {
            for done in moved {
                if let Some(item) = items.iter_mut().find(|item| item.id == done) {
                    let _ = relocate(paths, item, !to_private, lock);
                }
            }
            return Err(error);
        }
        moved.push(*id);
    }
    return Ok(());
}

fn move_thumb(paths: &Paths, id: ItemId, to_private: bool) {
    let source = thumb::file(&paths.thumbs_for(!to_private), id);
    if !source.exists() {
        return;
    }
    if fs::rename(&source, thumb::file(&paths.thumbs_for(to_private), id)).is_err() {
        let _ = fs::remove_file(&source);
    }
}

fn drop_stills(paths: &Paths, item: &Item, mtime: i64, lock_time: Option<f32>, from_private: bool) {
    let dir = paths.stills_for(from_private);
    for time in std::iter::once(item.still_time).chain(lock_time) {
        let key = still::cache_key(&item.path, mtime, time);
        let _ = fs::remove_file(still::cache_file(&dir, &key));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proto::{ApplyTarget, FillMode};
    use uuid::Uuid;

    use super::*;
    use crate::model::ItemKind;

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
        let thumb_public = thumb::file(&paths.thumbs_for(false), entry.id);
        fs::write(&thumb_public, b"thumb").expect("thumb");

        relocate(&paths, &mut entry, true, None).expect("lock");
        let private = paths.wallpapers_for(true).join("a.png");
        assert_eq!(entry.path, private.to_string_lossy());
        assert_eq!(fs::read(&private).expect("moved"), b"a.png");
        assert!(!paths.wallpapers_dir().join("a.png").exists());
        assert!(!thumb_public.exists());
        assert_eq!(fs::read(thumb::file(&paths.thumbs_for(true), entry.id)).expect("thumb"), b"thumb");

        relocate(&paths, &mut entry, false, None).expect("unlock");
        assert_eq!(entry.path, paths.wallpapers_dir().join("a.png").to_string_lossy());
        assert!(paths.wallpapers_dir().join("a.png").exists());
        assert!(!private.exists());
        assert!(thumb_public.exists());
        assert!(!thumb::file(&paths.thumbs_for(true), entry.id).exists());
    }

    #[test]
    fn relocate_drops_known_stills_from_the_old_cache() {
        let (_guard, paths) = setup();
        let mut entry = stored(&paths, "b.png");
        let mtime = still::mtime_secs(&entry.path);
        let lock = LockAssignment { item: entry.id, time: 7.0 };
        let mut files = Vec::new();
        for time in [entry.still_time, lock.time, 9.0] {
            let key = still::cache_key(&entry.path, mtime, time);
            let file = still::cache_file(&paths.stills_for(false), &key);
            fs::write(&file, b"png").expect("still");
            files.push(file);
        }
        relocate(&paths, &mut entry, true, Some(lock)).expect("lock");
        assert!(!files[0].exists());
        assert!(!files[1].exists());
        assert!(files[2].exists());
    }

    #[test]
    fn relocate_tolerates_a_missing_media_file() {
        let (_guard, paths) = setup();
        let mut entry = item(&paths.wallpapers_dir().join("gone.png").to_string_lossy());
        relocate(&paths, &mut entry, true, None).expect("lock");
        assert_eq!(entry.path, paths.wallpapers_for(true).join("gone.png").to_string_lossy());
    }

    #[test]
    fn relocate_many_rolls_back_when_one_move_fails() {
        let (_guard, paths) = setup();
        let first = stored(&paths, "c.png");
        let second = stored(&paths, "d.png");
        fs::create_dir(paths.wallpapers_for(true).join("d.png")).expect("blocker");
        let (first_path, second_path) = (first.path.clone(), second.path.clone());
        let ids = [first.id, second.id];
        let mut items = vec![first, second];

        assert!(relocate_many(&paths, &mut items, &ids, true, None).is_err());
        assert_eq!(items[0].path, first_path);
        assert_eq!(items[1].path, second_path);
        assert!(PathBuf::from(&first_path).exists());
        assert!(PathBuf::from(&second_path).exists());
        assert!(!paths.wallpapers_for(true).join("c.png").exists());
    }

    #[test]
    fn relocate_many_moves_every_listed_item() {
        let (_guard, paths) = setup();
        let first = stored(&paths, "e.png");
        let second = stored(&paths, "f.png");
        let untouched = stored(&paths, "g.png");
        let ids = [first.id, second.id];
        let mut items = vec![first, second, untouched];
        relocate_many(&paths, &mut items, &ids, true, None).expect("lock");
        assert!(paths.wallpapers_for(true).join("e.png").exists());
        assert!(paths.wallpapers_for(true).join("f.png").exists());
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
            Request::SetGeometry { item: private.clone(), mode: FillMode::Fill, geometry: proto::Geometry::default() },
            Request::Apply { item: Some(private.clone()), target: ApplyTarget::Default, transition: None },
            Request::Optimize { id: private.clone() },
            Request::OptimizeCancel { id: private },
            Request::FolderRename { id: folder.clone(), name: "x".into() },
            Request::FolderDelete { id: folder },
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
            Request::FolderCreate { name: "x".into() },
            Request::Status,
            Request::Lock,
        ];
        for request in allowed {
            assert!(!denies(&library, &request), "{request:?}");
        }
    }
}
