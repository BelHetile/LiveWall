use std::fs;
use std::io;
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
    pub fn for_test(base: std::path::PathBuf) -> Self {
        return Self { base };
    }

    pub fn base(&self) -> &Path {
        return &self.base;
    }

    pub fn join(&self, name: &str) -> PathBuf {
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
            self.incoming_dir(),
            self.tools_dir(),
        ] {
            create_private_dir(&dir)?;
        }
        return Ok(());
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
