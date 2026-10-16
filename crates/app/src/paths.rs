use std::ffi::CString;
use std::fs;
use std::io;
use std::os::macos::fs::MetadataExt;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const DIR_MODE: u32 = 0o700;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Private,
}

impl Visibility {
    pub fn opposite(self) -> Self {
        return match self {
            Self::Public => Self::Private,
            Self::Private => Self::Public,
        };
    }
}

impl From<bool> for Visibility {
    fn from(private: bool) -> Self {
        return if private { Self::Private } else { Self::Public };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirKind {
    Wallpapers,
    Thumbs,
    Stills,
}

#[derive(Debug, Clone)]
pub struct Paths {
    base: PathBuf,
}

impl Paths {
    pub fn detect() -> Self {
        return Self { base: data_dir() };
    }

    #[cfg(test)]
    pub fn at(base: PathBuf) -> Self {
        return Self { base };
    }

    fn join(&self, name: &str) -> PathBuf {
        return self.base.join(name);
    }

    pub fn library_file(&self) -> PathBuf {
        return self.join("library.json");
    }

    pub fn settings_file(&self) -> PathBuf {
        return self.join("settings.json");
    }

    pub fn swap_state_file(&self) -> PathBuf {
        return self.join("swap-state.json");
    }

    pub fn wallpapers_dir(&self) -> PathBuf {
        return self.dir_for(DirKind::Wallpapers, Visibility::Public);
    }

    pub fn thumbs_dir(&self) -> PathBuf {
        return self.dir_for(DirKind::Thumbs, Visibility::Public);
    }

    pub fn stills_dir(&self) -> PathBuf {
        return self.dir_for(DirKind::Stills, Visibility::Public);
    }

    fn private_dir(&self) -> PathBuf {
        return self.join("private");
    }

    pub fn dir_for(&self, kind: DirKind, visibility: Visibility) -> PathBuf {
        let name = match kind {
            DirKind::Wallpapers => "Wallpapers",
            DirKind::Thumbs => "Thumbs",
            DirKind::Stills => "Stills",
        };
        return match visibility {
            Visibility::Public => self.join(name),
            Visibility::Private => self.private_dir().join(name),
        };
    }

    pub fn incoming_dir(&self) -> PathBuf {
        return self.join("Incoming");
    }

    pub fn tools_dir(&self) -> PathBuf {
        return self.join("Tools");
    }

    pub fn ensure(&self) -> io::Result<()> {
        for dir in [
            self.base.clone(),
            self.wallpapers_dir(),
            self.thumbs_dir(),
            self.stills_dir(),
            self.private_dir(),
            self.dir_for(DirKind::Wallpapers, Visibility::Private),
            self.dir_for(DirKind::Thumbs, Visibility::Private),
            self.dir_for(DirKind::Stills, Visibility::Private),
            self.incoming_dir(),
            self.tools_dir(),
        ] {
            create_private_dir(&dir)?;
        }
        hide(&self.private_dir())?;
        let marker = self.private_dir().join(".metadata_never_index");
        if !marker.exists() {
            fs::write(marker, b"")?;
        }
        return Ok(());
    }

    pub fn reset_incoming(&self) -> io::Result<()> {
        let dir = self.incoming_dir();
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        return create_private_dir(&dir);
    }
}

fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("LIVEWALL_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let mut base = std::env::home_dir().unwrap_or_default();
    base.push("Library/Application Support/LiveWall");
    return base;
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(DIR_MODE))?;
    return Ok(());
}

fn hide(dir: &Path) -> io::Result<()> {
    let flags = fs::metadata(dir)?.st_flags();
    if flags & libc::UF_HIDDEN != 0 {
        return Ok(());
    }
    let path = CString::new(dir.as_os_str().as_bytes()).map_err(io::Error::other)?;
    if unsafe { libc::chflags(path.as_ptr(), flags | libc::UF_HIDDEN) } != 0 {
        return Err(io::Error::last_os_error());
    }
    return Ok(());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = Paths { base: dir.path().join("data") };
        return (dir, paths);
    }

    #[test]
    fn ensure_builds_the_private_tree_hidden_and_unindexed() {
        let (_guard, paths) = paths();
        paths.ensure().expect("ensure");
        for visibility in [Visibility::Public, Visibility::Private] {
            for kind in [DirKind::Wallpapers, DirKind::Thumbs, DirKind::Stills] {
                assert!(paths.dir_for(kind, visibility).is_dir());
            }
        }
        assert!(paths.private_dir().join(".metadata_never_index").is_file());
        let flags = fs::metadata(paths.private_dir()).expect("metadata").st_flags();
        assert_ne!(flags & libc::UF_HIDDEN, 0);
        let mode = fs::metadata(paths.private_dir()).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, DIR_MODE);
    }

    #[test]
    fn ensure_is_idempotent_and_keeps_existing_files() {
        let (_guard, paths) = paths();
        paths.ensure().expect("first");
        let kept = paths.dir_for(DirKind::Wallpapers, Visibility::Private).join("a.bin");
        fs::write(&kept, b"x").expect("write");
        paths.ensure().expect("second");
        assert!(kept.exists());
        assert_ne!(fs::metadata(paths.private_dir()).expect("metadata").st_flags() & libc::UF_HIDDEN, 0);
    }

    #[test]
    fn dir_for_picks_by_visibility() {
        let (_guard, paths) = paths();
        assert_eq!(paths.dir_for(DirKind::Wallpapers, Visibility::Public), paths.wallpapers_dir());
        assert_eq!(paths.dir_for(DirKind::Thumbs, Visibility::Public), paths.thumbs_dir());
        assert_eq!(paths.dir_for(DirKind::Stills, Visibility::Public), paths.stills_dir());
        for kind in [DirKind::Wallpapers, DirKind::Thumbs, DirKind::Stills] {
            assert!(paths.dir_for(kind, Visibility::Private).starts_with(paths.private_dir()));
        }
    }

    #[test]
    fn visibility_converts_from_the_private_flag_and_flips() {
        assert_eq!(Visibility::from(true), Visibility::Private);
        assert_eq!(Visibility::from(false), Visibility::Public);
        assert_eq!(Visibility::Public.opposite(), Visibility::Private);
        assert_eq!(Visibility::Private.opposite(), Visibility::Public);
    }
}
