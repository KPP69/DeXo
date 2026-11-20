//! Portable data-directory resolution and initialisation.
//!
//! Every runtime file DeXo writes lives under `data/` beside the executable.
//! Nothing is written to AppData, Documents, or any other system directory,
//! and there is no silent fallback: when the directory cannot be created or
//! is not writable, startup fails with an explicit error.

use std::fs;
use std::path::{Path, PathBuf};

/// Root data directory for the running executable: `<exe dir>/data`.
pub fn data_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("data")
}

/// Data directory for an explicit executable path (used by tests).
pub fn data_dir_for_exe(exe: &Path) -> PathBuf {
    exe.parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("data")
}

/// All well-known paths inside the data directory.
#[derive(Debug, Clone)]
pub struct DataPaths {
    pub root: PathBuf,
    pub accounts: PathBuf,
    pub strategies: PathBuf,
    pub database: PathBuf,
    pub logs: PathBuf,
    pub cache: PathBuf,
    pub credentials: PathBuf,
    pub webview: PathBuf,
    pub settings_file: PathBuf,
    pub database_file: PathBuf,
}

impl DataPaths {
    pub fn from_root(root: PathBuf) -> Self {
        let database = root.join("database");
        Self {
            settings_file: root.join("settings.json"),
            database_file: database.join("dexo.db"),
            accounts: root.join("accounts"),
            strategies: root.join("strategies"),
            database,
            logs: root.join("logs"),
            cache: root.join("cache"),
            credentials: root.join("credentials"),
            webview: root.join("webview"),
            root,
        }
    }

    /// Every directory that must exist before the app starts.
    pub fn directories(&self) -> [&Path; 8] {
        [
            self.root.as_path(),
            self.accounts.as_path(),
            self.strategies.as_path(),
            self.database.as_path(),
            self.logs.as_path(),
            self.cache.as_path(),
            self.credentials.as_path(),
            self.webview.as_path(),
        ]
    }
}

#[derive(Debug)]
pub enum PathError {
    CreateFailed {
        path: PathBuf,
        source: std::io::Error,
    },
    NotWritable {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateFailed { path, source } => {
                write!(
                    f,
                    "failed to create data directory {}: {source}",
                    path.display()
                )
            }
            Self::NotWritable { path, source } => {
                write!(
                    f,
                    "data directory {} is not writable: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for PathError {}

/// Creates every data directory and probes that the root is writable.
///
/// Returns an error instead of falling back to any system directory.
pub fn ensure_data_dirs(paths: &DataPaths) -> Result<(), PathError> {
    for dir in paths.directories() {
        fs::create_dir_all(dir).map_err(|source| PathError::CreateFailed {
            path: dir.to_path_buf(),
            source,
        })?;
    }
    let probe = paths.root.join(".dexo_write_probe");
    fs::write(&probe, b"dexo").map_err(|source| PathError::NotWritable {
        path: paths.root.clone(),
        source,
    })?;
    let _ = fs::remove_file(&probe);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_is_beside_the_executable() {
        let exe = Path::new("/opt/dexo/DeXo.exe");
        assert_eq!(data_dir_for_exe(exe), PathBuf::from("/opt/dexo/data"));
    }

    #[test]
    fn paths_follow_the_documented_layout() {
        let paths = DataPaths::from_root(PathBuf::from("/tmp/dexo/data"));
        assert_eq!(
            paths.database_file,
            PathBuf::from("/tmp/dexo/data/database/dexo.db")
        );
        assert_eq!(
            paths.settings_file,
            PathBuf::from("/tmp/dexo/data/settings.json")
        );
        assert_eq!(paths.logs, PathBuf::from("/tmp/dexo/data/logs"));
        assert_eq!(
            paths.credentials,
            PathBuf::from("/tmp/dexo/data/credentials")
        );
    }

    #[test]
    fn ensure_data_dirs_creates_the_full_layout() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let paths = DataPaths::from_root(tmp.path().join("data"));
        ensure_data_dirs(&paths).expect("dirs should be created");
        for dir in paths.directories() {
            assert!(dir.is_dir(), "missing directory: {}", dir.display());
        }
        // The writability probe must not be left behind.
        assert!(!paths.root.join(".dexo_write_probe").exists());
    }

    #[test]
    fn ensure_data_dirs_is_idempotent() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let paths = DataPaths::from_root(tmp.path().join("data"));
        ensure_data_dirs(&paths).expect("first call");
        ensure_data_dirs(&paths).expect("second call");
    }
}
