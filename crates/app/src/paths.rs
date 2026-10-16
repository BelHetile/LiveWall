use std::ffi::CString;
use std::fs;
use std::io;
use std::os::macos::fs::MetadataExt;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

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
        return self.join("Wallpapers");
    }

    pub fn thumbs_dir(&self) -> PathBuf {
        return self.join("Thumbs");
    }

    pub fn stills_dir(&self) -> PathBuf {
        return self.join("Stills");
    }

    pub fn private_dir(&self) -> PathBuf {
        return self.join("private");
    }

    pub fn wallpapers_for(&self, private: bool) -> PathBuf {
        if private {
            return self.private_dir().join("Wallpapers");
        }
        return self.wallpapers_dir();
    }

    pub fn thumbs_for(&self, private: bool) -> PathBuf {
        if private {
            return self.private_dir().join("Thumbs");
        }
        return self.thumbs_dir();
    }

    pub fn stills_for(&self, private: bool) -> PathBuf {
        if private {
            return self.private_dir().join("Stills");
        }
        return self.stills_dir();
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
            self.wallpapers_for(true),
            self.thumbs_for(true),
            self.stills_for(true),
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
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
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
        for private in [false, true] {
            assert!(paths.wallpapers_for(private).is_dir());
            assert!(paths.thumbs_for(private).is_dir());
            assert!(paths.stills_for(private).is_dir());
        }
        assert!(paths.private_dir().join(".metadata_never_index").is_file());
        let flags = fs::metadata(paths.private_dir()).expect("metadata").st_flags();
        assert_ne!(flags & libc::UF_HIDDEN, 0);
        let mode = fs::metadata(paths.private_dir()).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn ensure_is_idempotent_and_keeps_existing_files() {
        let (_guard, paths) = paths();
        paths.ensure().expect("first");
        let kept = paths.wallpapers_for(true).join("a.bin");
        fs::write(&kept, b"x").expect("write");
        paths.ensure().expect("second");
        assert!(kept.exists());
        assert_ne!(fs::metadata(paths.private_dir()).expect("metadata").st_flags() & libc::UF_HIDDEN, 0);
    }

    #[test]
    fn dirs_for_pick_by_privacy() {
        let (_guard, paths) = paths();
        assert_eq!(paths.wallpapers_for(false), paths.wallpapers_dir());
        assert_eq!(paths.thumbs_for(false), paths.thumbs_dir());
        assert_eq!(paths.stills_for(false), paths.stills_dir());
        assert!(paths.wallpapers_for(true).starts_with(paths.private_dir()));
        assert!(paths.thumbs_for(true).starts_with(paths.private_dir()));
        assert!(paths.stills_for(true).starts_with(paths.private_dir()));
    }
}
