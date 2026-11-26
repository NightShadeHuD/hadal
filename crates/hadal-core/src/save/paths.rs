//! Save file paths and platform data-directory resolution.
//!
//! Environment and home lookup is injectable so tests never mutate the real
//! process environment.

use std::path::{Path, PathBuf};

/// Maximum accepted save/import file size (4 MiB).
pub const MAX_SAVE_BYTES: u64 = 4 * 1024 * 1024;

/// Primary save file name.
pub const SAVE_FILE: &str = "save.json";
/// Backup save file name.
pub const BACKUP_FILE: &str = "save.bak.json";
/// Advisory lock file name.
pub const LOCK_FILE: &str = "save.lock";
/// Config file name.
pub const CONFIG_FILE: &str = "config.json";
/// Exports directory name.
pub const EXPORTS_DIR: &str = "exports";

/// Platform family for default-path resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OsFamily {
    /// Windows.
    Windows,
    /// macOS.
    MacOs,
    /// Linux and other Unix-like systems.
    Linux,
}

/// Returns the current platform family.
pub fn os_family() -> OsFamily {
    if cfg!(target_os = "windows") {
        OsFamily::Windows
    } else if cfg!(target_os = "macos") {
        OsFamily::MacOs
    } else {
        OsFamily::Linux
    }
}

/// The home directory from the real environment, if known.
fn real_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

/// Resolves the data directory using the real environment.
///
/// Order: explicit override, then `HADAL_DATA_DIR`, then the platform default.
pub fn resolve_data_dir(explicit: Option<&Path>) -> PathBuf {
    resolve_data_dir_with(
        explicit,
        &|key| std::env::var(key).ok(),
        real_home().as_deref(),
        os_family(),
    )
}

/// The platform default data directory (honoring `HADAL_DATA_DIR`).
pub fn default_data_dir() -> PathBuf {
    resolve_data_dir(None)
}

/// Resolves the data directory with injectable environment and home lookup.
///
/// Order: explicit override, then `HADAL_DATA_DIR`, then the platform default.
/// An empty environment value is treated as unset.
pub fn resolve_data_dir_with(
    explicit: Option<&Path>,
    env: &dyn Fn(&str) -> Option<String>,
    home: Option<&Path>,
    os: OsFamily,
) -> PathBuf {
    if let Some(path) = explicit {
        return path.to_path_buf();
    }
    if let Some(value) = env("HADAL_DATA_DIR")
        && !value.is_empty()
    {
        return PathBuf::from(value);
    }
    match os {
        OsFamily::Windows => {
            let base = env("APPDATA")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .or_else(|| home.map(|h| h.join("AppData").join("Roaming")));
            base.unwrap_or_else(|| PathBuf::from(".")).join("hadal")
        }
        OsFamily::MacOs => home
            .map(|h| h.join("Library").join("Application Support").join("hadal"))
            .unwrap_or_else(|| PathBuf::from(".").join("hadal")),
        OsFamily::Linux => {
            if let Some(xdg) = env("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
                PathBuf::from(xdg).join("hadal")
            } else {
                home.map(|h| h.join(".local").join("share").join("hadal"))
                    .unwrap_or_else(|| PathBuf::from(".").join("hadal"))
            }
        }
    }
}

/// File layout inside a data directory.
#[derive(Clone, Debug)]
pub struct SavePaths {
    dir: PathBuf,
}

impl SavePaths {
    /// Creates a path set for `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The data directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Primary save path.
    pub fn save(&self) -> PathBuf {
        self.dir.join(SAVE_FILE)
    }

    /// Backup save path.
    pub fn backup(&self) -> PathBuf {
        self.dir.join(BACKUP_FILE)
    }

    /// Lock file path.
    pub fn lock(&self) -> PathBuf {
        self.dir.join(LOCK_FILE)
    }

    /// Config file path.
    pub fn config(&self) -> PathBuf {
        self.dir.join(CONFIG_FILE)
    }

    /// Exports directory path.
    pub fn exports(&self) -> PathBuf {
        self.dir.join(EXPORTS_DIR)
    }

    /// Crash log path for a given Unix timestamp.
    pub fn crash_log(&self, unix: u64) -> PathBuf {
        self.dir.join(format!("crash-{unix}.log"))
    }

    /// Preserved corrupt-save path for a given Unix timestamp.
    pub fn corrupt_save(&self, unix: u64) -> PathBuf {
        self.dir.join(format!("save.corrupt-{unix}.json"))
    }

    /// Temporary file used during atomic writes.
    pub fn temp(&self) -> PathBuf {
        self.dir.join("save.json.tmp")
    }
}
