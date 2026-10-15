use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Serialize, de::DeserializeOwned};
use tempfile::NamedTempFile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub message: String,
    pub backup: Option<PathBuf>,
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(backup) = &self.backup {
            write!(f, " (backed up to {})", backup.display())?;
        }
        return Ok(());
    }
}

pub fn load<T: Default + DeserializeOwned>(path: &Path) -> (T, Option<Warning>) {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return (T::default(), None),
        Err(error) => {
            return (
                T::default(),
                Some(Warning { message: format!("cannot read {}: {error}", path.display()), backup: None }),
            );
        }
    };
    match serde_json::from_slice::<T>(&bytes) {
        Ok(value) => (value, None),
        Err(error) => {
            let backup = damaged_backup(path).ok();
            let warning = Warning { message: format!("corrupt {}: {error}", path.display()), backup };
            return (T::default(), Some(warning));
        }
    }
}

pub fn save<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut file = NamedTempFile::new_in(dir)?;
    serde_json::to_writer_pretty(&mut file, value).map_err(io::Error::other)?;
    file.as_file_mut().write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    return Ok(());
}

fn damaged_backup(path: &Path) -> io::Result<PathBuf> {
    let stamp = jiff::Timestamp::now().as_second();
    let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("library");
    let backup = path.with_file_name(format!("{stem}-damaged-{stamp}.json"));
    fs::rename(path, &backup)?;
    return Ok(backup);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Library, Settings};

    #[test]
    fn missing_file_yields_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("library.json");
        let (library, warning) = load::<Library>(&path);
        assert_eq!(library, Library::default());
        assert!(warning.is_none());
    }

    #[test]
    fn settings_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        let settings = Settings { volume: 0.25, shuffle: true, ..Settings::default() };
        save(&path, &settings).expect("save");
        let (loaded, warning) = load::<Settings>(&path);
        assert_eq!(loaded, settings);
        assert!(warning.is_none());
    }

    #[test]
    fn empty_object_loads_with_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        fs::write(&path, b"{}").expect("write");
        let (settings, warning) = load::<Settings>(&path);
        assert_eq!(settings, Settings::default());
        assert!(warning.is_none());
    }

    #[test]
    fn corrupt_file_is_backed_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("library.json");
        fs::write(&path, b"{ this is not json").expect("write");
        let (library, warning) = load::<Library>(&path);
        assert_eq!(library, Library::default());
        let warning = warning.expect("warning");
        let backup = warning.backup.expect("backup path");
        assert!(backup.exists());
        assert!(!path.exists());
        let name = backup.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        assert!(name.starts_with("library-damaged-"), "{name}");
        assert!(std::path::Path::new(name).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("json")), "{name}");
    }
}
