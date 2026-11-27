//! Save format v1: envelope, checksum, atomic writes, loading, import/export.
//!
//! Core never reads the wall clock: every entry point takes `now_unix`.

pub mod config;
pub mod lock;
pub mod migrate;
pub mod paths;

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::state::GameState;

pub use config::{
    ColorDepth, Config, ConfigWarning, ParseColorDepthError, Theme, load_config, save_config,
};
pub use lock::{LockError, SaveLock};
pub use migrate::{MigrationChain, MigrationError, MigrationStep};
pub use paths::{
    MAX_SAVE_BYTES, OsFamily, SavePaths, default_data_dir, resolve_data_dir, resolve_data_dir_with,
};

/// Envelope format marker.
pub const FORMAT_MARKER: &str = "hadal-save";
/// Schema version this build writes and supports.
pub const SUPPORTED_SCHEMA: u32 = 2;
/// Application version written into new envelopes.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The serialized save envelope.
///
/// Field order is the canonical file order:
/// `format, schema_version, app_version, saved_at_unix, save_counter, checksum, state`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    /// Always [`FORMAT_MARKER`].
    pub format: String,
    /// Schema version.
    pub schema_version: u32,
    /// Application version that wrote the file.
    pub app_version: String,
    /// Unix timestamp at write time.
    pub saved_at_unix: u64,
    /// Monotonic write counter.
    pub save_counter: u64,
    /// `sha256:<hex>` over the canonical envelope without this field.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub checksum: String,
    /// Pure game state.
    pub state: GameState,
}

impl Envelope {
    /// Builds a sealed envelope with a freshly computed checksum.
    pub fn sealed(state: &GameState, saved_at_unix: u64, save_counter: u64) -> Self {
        let mut envelope = Self {
            format: FORMAT_MARKER.to_owned(),
            schema_version: SUPPORTED_SCHEMA,
            app_version: APP_VERSION.to_owned(),
            saved_at_unix,
            save_counter,
            checksum: String::new(),
            state: state.clone(),
        };
        envelope.checksum = checksum_of(&envelope);
        envelope
    }
}

/// Result of a successful save or export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveInfo {
    /// File written.
    pub path: PathBuf,
    /// Unix timestamp stored.
    pub saved_at_unix: u64,
    /// Counter stored.
    pub save_counter: u64,
}

/// Result of a successful load.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSave {
    /// Loaded state.
    pub state: GameState,
    /// Timestamp stored in the envelope.
    pub saved_at_unix: u64,
    /// Counter stored in the envelope.
    pub save_counter: u64,
    /// Non-fatal recovery warning, if any.
    pub warning: Option<LoadWarning>,
}

/// Non-fatal load warning for the caller to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadWarning {
    /// The primary save was unusable and the backup was loaded.
    RecoveredFromBackup {
        /// Preserved corrupt primary path, if one was preserved.
        corrupt_path: Option<PathBuf>,
    },
}

/// Typed save/load errors.
#[derive(Debug, Error)]
pub enum SaveError {
    /// File exceeds the 4 MiB gate.
    #[error("save file too large: {size} bytes (max {max})")]
    TooLarge {
        /// Actual size.
        size: u64,
        /// Maximum allowed.
        max: u64,
    },
    /// Filesystem error.
    #[error("i/o error at {path}: {source}")]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// Malformed JSON.
    #[error("invalid save JSON: {0}")]
    InvalidJson(String),
    /// Wrong format marker.
    #[error("not a HADAL save (format: {found})")]
    BadFormat {
        /// Marker found.
        found: String,
    },
    /// The file is from a newer schema than this build supports.
    #[error(
        "save schema {found} is newer than supported schema {supported}; \
         update HADAL to load it. The file was left unchanged."
    )]
    FutureSchema {
        /// Version found.
        found: u32,
        /// Highest supported.
        supported: u32,
    },
    /// Migration chain failure.
    #[error("migration failed: {0}")]
    Migration(#[from] MigrationError),
    /// Checksum did not match the envelope contents.
    #[error("save checksum mismatch")]
    ChecksumMismatch,
    /// Final deserialization failed (unknown fields, bad shape).
    #[error("save state is invalid: {0}")]
    Deserialize(String),
    /// Envelope could not be encoded.
    #[error("save encoding failed: {0}")]
    Encode(String),
    /// The on-disk save counter is newer than the in-memory counter.
    #[error("disk save is newer (disk counter {disk} > memory {memory})")]
    NewerSaveOnDisk {
        /// Counter on disk.
        disk: u64,
        /// In-memory counter.
        memory: u64,
    },
    /// The current save exists but could not be parsed for its counter.
    #[error("current save is unreadable; refusing to overwrite it")]
    UnreadableCurrent,
    /// No save files exist in the data directory.
    #[error("no save found in {path}")]
    NoSave {
        /// Data directory searched.
        path: PathBuf,
    },
    /// Copying the current save to the backup failed.
    #[error("backup copy failed: {0}")]
    BackupFailed(#[source] std::io::Error),
    /// Neither primary nor backup could be loaded.
    #[error("no valid save: primary failed ({primary}); backup failed ({backup})")]
    NoValidSave {
        /// Primary failure description.
        primary: String,
        /// Backup failure description.
        backup: String,
    },
    /// Lock acquisition failure.
    #[error(transparent)]
    Lock(#[from] LockError),
}

impl SaveError {
    /// Whether this error means the file content was corrupt (and should be
    /// preserved before trying the backup).
    pub fn is_corrupt(&self) -> bool {
        matches!(
            self,
            SaveError::InvalidJson(_)
                | SaveError::BadFormat { .. }
                | SaveError::ChecksumMismatch
                | SaveError::Deserialize(_)
        )
    }
}

fn io_err(path: &Path, source: std::io::Error) -> SaveError {
    SaveError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn is_not_found(err: &SaveError) -> bool {
    matches!(err, SaveError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound)
}

pub fn hex_digest(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = Sha256::digest(bytes);
    let digest_bytes: &[u8] = digest.as_ref();
    let mut out = String::with_capacity(64);
    for byte in digest_bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Canonical checksum of an envelope value, excluding the `checksum` field.
///
/// Canonical form is serde_json's compact serialization of the value minus
/// `checksum` (object keys sorted), which is deterministic across platforms.
fn canonical_checksum(value: &Value) -> String {
    let mut copy = value.clone();
    if let Some(object) = copy.as_object_mut() {
        object.remove("checksum");
    }
    let bytes = serde_json::to_vec(&copy).unwrap_or_default();
    format!("sha256:{}", hex_digest(&bytes))
}

/// Computes the canonical checksum for an envelope.
pub fn checksum_of(envelope: &Envelope) -> String {
    let mut copy = envelope.clone();
    copy.checksum = String::new();
    let value = serde_json::to_value(&copy).unwrap_or(Value::Null);
    canonical_checksum(&value)
}

fn verify_checksum(value: &Value, stored: &str) -> bool {
    !stored.is_empty() && canonical_checksum(value) == stored
}
/// Reads a file, rejecting anything larger than [`MAX_SAVE_BYTES`].
///
/// This is the same size gate used by [`SaveStore::load`], [`import`], and
/// the CLI `verify` subcommand.
pub fn read_capped(path: &Path) -> Result<Vec<u8>, SaveError> {
    let meta = std::fs::metadata(path).map_err(|e| io_err(path, e))?;
    if meta.len() > MAX_SAVE_BYTES {
        return Err(SaveError::TooLarge {
            size: meta.len(),
            max: MAX_SAVE_BYTES,
        });
    }
    std::fs::read(path).map_err(|e| io_err(path, e))
}

/// Verifies a save file by checking the stored bytes against the checksum
/// stored in the envelope.
///
/// Like [`parse_envelope`], this validates the stored bytes exactly: it
/// checks the format marker, gates on schema version, verifies the checksum
/// over the raw JSON value (before any migration), then fully deserializes
/// the envelope (including the migration path for older schemas).
///
/// Returns the **stored** schema version, app version, and save counter from
/// the file header (not the migrated values), so the caller can display the
/// original metadata alongside the checksum result.
///
/// # Errors
///
/// Returns [`SaveError::TooLarge`] if the file exceeds 4 MiB,
/// [`SaveError::FutureSchema`] for unsupported future schemas, and
/// [`SaveError::ChecksumMismatch`] when the checksum does not match.
pub fn verify_save_bytes(path: &Path) -> Result<(u32, String, u64), SaveError> {
    let bytes = read_capped(path)?;
    // Extract the stored header from the raw JSON before any migration.
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|e| SaveError::InvalidJson(e.to_string()))?;
    let stored_schema = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let stored_app = value
        .get("app_version")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let stored_counter = value
        .get("save_counter")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    // Full validation: format gate, checksum, migration, deserialization.
    let chain = MigrationChain::production();
    let _envelope = parse_envelope(&bytes, &chain)?;
    Ok((stored_schema, stored_app, stored_counter))
}

/// Parses and validates envelope bytes: format, schema gate, migration,
/// deserialization and checksum. Checksum covers the stored bytes; for a
/// migrated (older) schema it is verified *before* transformation.
fn parse_envelope(bytes: &[u8], chain: &MigrationChain) -> Result<Envelope, SaveError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|e| SaveError::InvalidJson(e.to_string()))?;

    let found_format = value
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if found_format != FORMAT_MARKER {
        return Err(SaveError::BadFormat {
            found: found_format.to_owned(),
        });
    }

    let found_version = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if found_version > u64::from(SUPPORTED_SCHEMA) {
        let found = u32::try_from(found_version).unwrap_or(u32::MAX);
        return Err(SaveError::FutureSchema {
            found,
            supported: SUPPORTED_SCHEMA,
        });
    }
    let version = found_version as u32;

    let stored_checksum = value
        .get("checksum")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if version == SUPPORTED_SCHEMA {
        // Deserialize first (unknown fields reject), then verify checksum.
        let envelope: Envelope = serde_json::from_value(value.clone())
            .map_err(|e| SaveError::Deserialize(e.to_string()))?;
        if !verify_checksum(&value, stored_checksum) {
            return Err(SaveError::ChecksumMismatch);
        }
        Ok(envelope)
    } else {
        // Authenticate the stored bytes first, then migrate, then deserialize.
        if !verify_checksum(&value, stored_checksum) {
            return Err(SaveError::ChecksumMismatch);
        }
        let mut migrated = chain.migrate(value, version, SUPPORTED_SCHEMA)?;
        if let Some(object) = migrated.as_object_mut() {
            object.insert("schema_version".to_owned(), Value::from(SUPPORTED_SCHEMA));
        }
        serde_json::from_value(migrated).map_err(|e| SaveError::Deserialize(e.to_string()))
    }
}

/// Persistent save store rooted at a data directory.
pub struct SaveStore {
    paths: SavePaths,
    migrations: MigrationChain,
    counter: u64,
}

impl SaveStore {
    /// Creates a store with the production (empty) migration chain.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            paths: SavePaths::new(dir),
            migrations: MigrationChain::production(),
            counter: 0,
        }
    }

    /// Creates a store with an injected migration chain (used by tests).
    pub fn with_migrations(dir: impl Into<PathBuf>, migrations: MigrationChain) -> Self {
        Self {
            paths: SavePaths::new(dir),
            migrations,
            counter: 0,
        }
    }

    /// The path layout.
    pub fn paths(&self) -> &SavePaths {
        &self.paths
    }

    /// The in-memory save counter.
    pub fn counter(&self) -> u64 {
        self.counter
    }

    fn ensure_dir(&self) -> Result<(), SaveError> {
        let dir = self.paths.dir();
        std::fs::create_dir_all(dir).map_err(|e| io_err(dir, e))
    }

    fn load_path(&self, path: &Path) -> Result<Envelope, SaveError> {
        let bytes = read_capped(path)?;
        parse_envelope(&bytes, &self.migrations)
    }

    /// Loads the primary save, falling back to the backup on corruption.
    ///
    /// # Errors
    ///
    /// Returns typed errors for oversize/future-schema files and when neither
    /// primary nor backup is usable.
    pub fn load(&mut self, now_unix: u64) -> Result<LoadedSave, SaveError> {
        let primary_path = self.paths.save();
        match self.load_path(&primary_path) {
            Ok(envelope) => {
                self.counter = self.counter.max(envelope.save_counter);
                Ok(LoadedSave {
                    state: envelope.state,
                    saved_at_unix: envelope.saved_at_unix,
                    save_counter: envelope.save_counter,
                    warning: None,
                })
            }
            Err(primary_err) => {
                let missing = matches!(
                    &primary_err,
                    SaveError::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound
                );
                if primary_err.is_corrupt() {
                    let corrupt_path = self.paths.corrupt_save(now_unix);
                    std::fs::copy(&primary_path, &corrupt_path)
                        .map_err(|e| io_err(&corrupt_path, e))?;
                    match self.load_path(&self.paths.backup()) {
                        Ok(envelope) => {
                            // The corrupt primary is preserved under its new name.
                            let _ = std::fs::remove_file(&primary_path);
                            self.counter = self.counter.max(envelope.save_counter);
                            Ok(LoadedSave {
                                state: envelope.state,
                                saved_at_unix: envelope.saved_at_unix,
                                save_counter: envelope.save_counter,
                                warning: Some(LoadWarning::RecoveredFromBackup {
                                    corrupt_path: Some(corrupt_path),
                                }),
                            })
                        }
                        Err(backup_err) => Err(SaveError::NoValidSave {
                            primary: primary_err.to_string(),
                            backup: backup_err.to_string(),
                        }),
                    }
                } else if missing {
                    match self.load_path(&self.paths.backup()) {
                        Ok(envelope) => {
                            self.counter = self.counter.max(envelope.save_counter);
                            Ok(LoadedSave {
                                state: envelope.state,
                                saved_at_unix: envelope.saved_at_unix,
                                save_counter: envelope.save_counter,
                                warning: Some(LoadWarning::RecoveredFromBackup {
                                    corrupt_path: None,
                                }),
                            })
                        }
                        Err(backup_err) => {
                            // A truly nonexistent data directory gets the typed
                            // NoSave signal. An existing but empty directory keeps
                            // the legacy missing-file signal so the TUI's existing
                            // fresh-install path continues to work (the lock now
                            // creates the directory before load).
                            if is_not_found(&backup_err) && !self.paths.dir().exists() {
                                Err(SaveError::NoSave {
                                    path: self.paths.dir().to_path_buf(),
                                })
                            } else {
                                Err(backup_err)
                            }
                        }
                    }
                } else {
                    Err(primary_err)
                }
            }
        }
    }

    fn read_disk_counter(&self) -> Result<Option<u64>, SaveError> {
        let path = self.paths.save();
        match std::fs::read(&path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(io_err(&path, err)),
            Ok(bytes) => {
                if bytes.len() as u64 > MAX_SAVE_BYTES {
                    return Err(SaveError::UnreadableCurrent);
                }
                let value: Value =
                    serde_json::from_slice(&bytes).map_err(|_| SaveError::UnreadableCurrent)?;
                value
                    .get("save_counter")
                    .and_then(Value::as_u64)
                    .map(Some)
                    .ok_or(SaveError::UnreadableCurrent)
            }
        }
    }

    fn next_counter(&self) -> Result<u64, SaveError> {
        if let Some(disk) = self.read_disk_counter()? {
            if disk > self.counter {
                return Err(SaveError::NewerSaveOnDisk {
                    disk,
                    memory: self.counter,
                });
            }
            return Ok(disk.saturating_add(1));
        }
        Ok(self.counter.saturating_add(1))
    }

    /// Atomically writes an envelope: temp write + fsync, backup current, rename.
    fn write_atomic(&self, envelope: &Envelope) -> Result<(), SaveError> {
        self.ensure_dir()?;
        let dir = self.paths.dir().to_path_buf();
        let bytes = serde_json::to_vec(envelope).map_err(|e| SaveError::Encode(e.to_string()))?;
        let temp = self.paths.temp();
        {
            let mut file = File::create(&temp).map_err(|e| io_err(&temp, e))?;
            file.write_all(&bytes).map_err(|e| io_err(&temp, e))?;
            file.sync_all().map_err(|e| io_err(&temp, e))?;
        }

        let current = self.paths.save();
        let backup = self.paths.backup();
        if current.exists() {
            if let Err(err) = std::fs::copy(&current, &backup) {
                let _ = std::fs::remove_file(&temp);
                return Err(SaveError::BackupFailed(err));
            }
            let sync = OpenOptions::new()
                .write(true)
                .open(&backup)
                .and_then(|file| file.sync_all());
            if let Err(err) = sync {
                let _ = std::fs::remove_file(&temp);
                return Err(SaveError::BackupFailed(err));
            }
        }

        if let Err(err) = std::fs::rename(&temp, &current) {
            let _ = std::fs::remove_file(&temp);
            return Err(io_err(&current, err));
        }

        #[cfg(unix)]
        {
            // Best-effort directory sync.
            if let Ok(dir_file) = File::open(&dir) {
                let _ = dir_file.sync_all();
            }
        }
        let _ = dir;
        Ok(())
    }

    /// Saves `state`, bumping the counter and refusing to clobber newer saves.
    ///
    /// # Errors
    ///
    /// Returns [`SaveError::NewerSaveOnDisk`] if the file on disk has a higher
    /// counter, or an I/O/backup error.
    pub fn save(&mut self, state: &GameState, now_unix: u64) -> Result<SaveInfo, SaveError> {
        let counter = self.next_counter()?;
        let envelope = Envelope::sealed(state, now_unix, counter);
        self.write_atomic(&envelope)?;
        self.counter = counter;
        Ok(SaveInfo {
            path: self.paths.save(),
            saved_at_unix: now_unix,
            save_counter: counter,
        })
    }

    /// Exports an envelope for `state` to an arbitrary path.
    ///
    /// # Errors
    ///
    /// Returns a typed [`SaveError::Io`] naming `path` when the parent
    /// directory is missing or not a directory; export never creates parents.
    /// Encoding failures are also reported.
    pub fn export_to_path(
        &mut self,
        state: &GameState,
        path: &Path,
        now_unix: u64,
    ) -> Result<SaveInfo, SaveError> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            match std::fs::metadata(parent) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => {
                    return Err(io_err(
                        path,
                        std::io::Error::new(
                            std::io::ErrorKind::NotADirectory,
                            format!("export parent is not a directory: {}", parent.display()),
                        ),
                    ));
                }
                Err(err) => {
                    return Err(io_err(
                        path,
                        std::io::Error::new(
                            err.kind(),
                            format!("export parent is unavailable: {}: {err}", parent.display()),
                        ),
                    ));
                }
            }
        }
        let counter = self.counter.saturating_add(1);
        let envelope = Envelope::sealed(state, now_unix, counter);
        let bytes = serde_json::to_vec(&envelope).map_err(|e| SaveError::Encode(e.to_string()))?;
        write_file_atomic(path, &bytes)?;
        self.counter = counter;
        Ok(SaveInfo {
            path: path.to_path_buf(),
            saved_at_unix: now_unix,
            save_counter: counter,
        })
    }

    /// Imports a save from `path`, replacing the current save atomically.
    ///
    /// Validation happens before any filesystem mutation. The imported state
    /// gets `saved_at_unix = now_unix`, granting no offline credit.
    ///
    /// # Errors
    ///
    /// Returns validation errors, [`SaveError::NewerSaveOnDisk`], or I/O errors.
    pub fn import_from_path(&mut self, path: &Path, now_unix: u64) -> Result<GameState, SaveError> {
        let bytes = read_capped(path)?;
        let imported = parse_envelope(&bytes, &self.migrations)?;
        let counter = self.next_counter()?;
        let envelope = Envelope::sealed(&imported.state, now_unix, counter);
        self.write_atomic(&envelope)?;
        self.counter = counter;
        Ok(imported.state)
    }
}

/// Atomic write to an arbitrary path (no backup): temp + fsync + rename.
fn write_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), SaveError> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let temp = dir.join(".hadal-export.tmp");
    {
        let mut file = File::create(&temp).map_err(|e| io_err(&temp, e))?;
        file.write_all(bytes).map_err(|e| io_err(&temp, e))?;
        file.sync_all().map_err(|e| io_err(&temp, e))?;
    }
    if let Err(err) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(io_err(path, err));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::engine::{Action, BuyQuantity, Engine};
    use crate::save::config::{self, ColorDepth, Theme};
    use crate::save::lock::SaveLock;
    use crate::save::paths::{OsFamily, resolve_data_dir_with};
    use crate::state::GameState;

    fn tempdir() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    fn state_digest(state: &GameState) -> String {
        let bytes = serde_json::to_vec(state).unwrap();
        hex_digest(&bytes)
    }

    fn dir_digest(dir: &std::path::Path) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                let name = entry.file_name().to_string_lossy().into_owned();
                out.insert(name, hex_digest(&std::fs::read(entry.path()).unwrap()));
            }
        }
        out
    }

    fn write_bytes(path: &std::path::Path, bytes: &[u8]) {
        std::fs::write(path, bytes).unwrap();
    }

    fn sample_state(era: u32) -> GameState {
        let mut state = GameState::new();
        state.meta.era_fathoms = era;
        state.meta.fathoms = era;
        state.meta.depth_m = 7912;
        state
    }

    fn mig_0_1(mut value: Value) -> Result<Value, String> {
        let meta = value
            .get_mut("state")
            .and_then(|s| s.get_mut("meta"))
            .and_then(Value::as_object_mut)
            .ok_or_else(|| "missing state.meta".to_owned())?;
        let legacy = meta
            .remove("legacy_era")
            .ok_or_else(|| "missing legacy_era".to_owned())?;
        meta.insert("era_fathoms".to_owned(), legacy);
        Ok(value)
    }

    #[test]
    fn test_save_load_roundtrip_same_hash() {
        let dir = tempdir();
        let mut store = SaveStore::new(dir.path());
        let state = sample_state(5);
        let before = state_digest(&state);
        let info = store.save(&state, 1000).unwrap();
        assert_eq!(info.save_counter, 1);
        let loaded = store.load(2000).unwrap();
        assert_eq!(state_digest(&loaded.state), before);
        assert_eq!(loaded.saved_at_unix, 1000);
        assert_eq!(loaded.save_counter, 1);
        assert!(loaded.warning.is_none());
    }

    #[test]
    fn test_checksum_tamper_detected_and_preserved() {
        let dir = tempdir();
        let mut store = SaveStore::new(dir.path());
        store.save(&sample_state(1), 10).unwrap();
        let path = store.paths().save();
        let text = std::fs::read_to_string(&path).unwrap();
        let tampered = text.replacen("\"echo\":\"0\"", "\"echo\":\"1\"", 1);
        assert_ne!(text, tampered, "tamper pattern must exist");
        write_bytes(&path, tampered.as_bytes());

        // Direct parse reports a checksum mismatch.
        let err = parse_envelope(tampered.as_bytes(), &MigrationChain::empty()).unwrap_err();
        assert!(matches!(err, SaveError::ChecksumMismatch), "{err:?}");

        // Load preserves the corrupt primary and returns a typed failure.
        let err = store.load(99).unwrap_err();
        assert!(matches!(err, SaveError::NoValidSave { .. }), "{err:?}");
        let corrupt = store.paths().corrupt_save(99);
        assert!(corrupt.exists());
        assert_eq!(std::fs::read(&corrupt).unwrap(), tampered.as_bytes());
        assert_eq!(std::fs::read(&path).unwrap(), tampered.as_bytes());
    }

    #[test]
    fn test_future_schema_refused_and_file_unchanged() {
        let dir = tempdir();
        let mut store = SaveStore::new(dir.path());
        store.save(&sample_state(1), 10).unwrap();
        let path = store.paths().save();
        let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["schema_version"] = json!(3);
        let bytes = serde_json::to_vec(&value).unwrap();
        write_bytes(&path, &bytes);

        let err = store.load(50).unwrap_err();
        assert!(
            matches!(
                err,
                SaveError::FutureSchema {
                    found: 3,
                    supported: 2
                }
            ),
            "{err:?}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn test_unknown_envelope_field_rejected() {
        let dir = tempdir();
        let mut store = SaveStore::new(dir.path());
        store.save(&sample_state(1), 10).unwrap();
        let path = store.paths().save();
        let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["extra"] = json!(true);
        let bytes = serde_json::to_vec(&value).unwrap();
        let err = parse_envelope(&bytes, &MigrationChain::empty()).unwrap_err();
        assert!(matches!(err, SaveError::Deserialize(_)), "{err:?}");
    }

    #[test]
    fn test_oversize_rejected() {
        let dir = tempdir();
        let path = dir.path().join("big.json");
        write_bytes(&path, &vec![b'x'; (MAX_SAVE_BYTES as usize) + 1]);
        let mut store = SaveStore::new(dir.path());
        let err = store.import_from_path(&path, 1).unwrap_err();
        assert!(matches!(err, SaveError::TooLarge { .. }), "{err:?}");
    }

    #[test]
    fn test_corrupt_primary_valid_backup_recovers() {
        let dir = tempdir();
        let mut store = SaveStore::new(dir.path());
        let first = sample_state(3);
        let second = sample_state(9);
        store.save(&first, 100).unwrap(); // no backup yet
        store.save(&second, 200).unwrap(); // backup = first
        let primary = store.paths().save();
        let text = std::fs::read_to_string(&primary).unwrap();
        let tampered = text.replacen("\"echo\":\"0\"", "\"echo\":\"1\"", 1);
        write_bytes(&primary, tampered.as_bytes());

        let loaded = store.load(300).unwrap();
        assert_eq!(state_digest(&loaded.state), state_digest(&first));
        match &loaded.warning {
            Some(LoadWarning::RecoveredFromBackup { corrupt_path }) => {
                let corrupt_path = corrupt_path.as_ref().expect("corrupt path");
                assert!(corrupt_path.exists());
                assert_eq!(std::fs::read(corrupt_path).unwrap(), tampered.as_bytes());
            }
            other => panic!("expected recovery warning, got {other:?}"),
        }
        assert!(!primary.exists(), "corrupt primary is rotated aside");
    }

    #[test]
    fn test_corrupt_primary_and_backup_is_typed_without_deleting() {
        let dir = tempdir();
        let mut store = SaveStore::new(dir.path());
        store.save(&sample_state(1), 100).unwrap();
        store.save(&sample_state(2), 200).unwrap();
        let primary = store.paths().save();
        let backup = store.paths().backup();
        write_bytes(&primary, b"{ not json");
        write_bytes(&backup, b"{ also not json");

        let err = store.load(400).unwrap_err();
        assert!(matches!(err, SaveError::NoValidSave { .. }), "{err:?}");
        assert!(primary.exists());
        assert!(backup.exists());
        assert_eq!(std::fs::read(&primary).unwrap(), b"{ not json");
        assert_eq!(std::fs::read(&backup).unwrap(), b"{ also not json");
    }

    #[test]
    fn test_atomic_write_backup_and_no_temp() {
        let dir = tempdir();
        let mut store = SaveStore::new(dir.path());
        store.save(&sample_state(1), 100).unwrap();
        let first_bytes = std::fs::read(store.paths().save()).unwrap();
        store.save(&sample_state(2), 200).unwrap();
        assert!(!store.paths().temp().exists(), "no temp file remains");
        assert_eq!(std::fs::read(store.paths().backup()).unwrap(), first_bytes);
    }

    #[test]
    fn test_backup_copy_failure_aborts_and_keeps_current() {
        let dir = tempdir();
        let mut store = SaveStore::new(dir.path());
        store.save(&sample_state(1), 100).unwrap();
        let current_before = std::fs::read(store.paths().save()).unwrap();
        // Make the backup path un-copyable by turning it into a directory.
        let backup = store.paths().backup();
        if backup.exists() {
            std::fs::remove_file(&backup).unwrap();
        }
        std::fs::create_dir(&backup).unwrap();

        let err = store.save(&sample_state(2), 200).unwrap_err();
        assert!(matches!(err, SaveError::BackupFailed(_)), "{err:?}");
        assert_eq!(std::fs::read(store.paths().save()).unwrap(), current_before);
        assert!(!store.paths().temp().exists());
    }

    #[test]
    fn test_save_counter_clobber_guard() {
        let dir = tempdir();
        let mut store = SaveStore::new(dir.path());
        store.save(&sample_state(1), 100).unwrap();
        let path = store.paths().save();
        // Simulate another process having written a newer save.
        let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["save_counter"] = json!(99);
        write_bytes(&path, &serde_json::to_vec(&value).unwrap());

        let err = store.save(&sample_state(2), 200).unwrap_err();
        assert!(
            matches!(
                err,
                SaveError::NewerSaveOnDisk {
                    disk: 99,
                    memory: 1
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn test_import_bad_checksum_leaves_directory_identical() {
        let dir = tempdir();
        let mut store = SaveStore::new(dir.path());
        store.save(&sample_state(1), 100).unwrap();
        let before = dir_digest(dir.path());

        // Build a valid-looking envelope then corrupt the state bytes.
        let exported_state = sample_state(7);
        let export_dir = tempdir();
        let export_path = export_dir.path().join("import.json");
        let mut exporter = SaveStore::new(export_dir.path());
        exporter
            .export_to_path(&exported_state, &export_path, 150)
            .unwrap();
        let text = std::fs::read_to_string(&export_path).unwrap();
        let tampered = text.replacen("\"echo\":\"0\"", "\"echo\":\"1\"", 1);
        write_bytes(&export_path, tampered.as_bytes());

        let err = store.import_from_path(&export_path, 300).unwrap_err();
        assert!(
            matches!(
                err,
                SaveError::TooLarge { .. }
                    | SaveError::NoValidSave { .. }
                    | SaveError::ChecksumMismatch
            ),
            "{err:?}"
        );
        assert_eq!(dir_digest(dir.path()), before);
    }

    #[test]
    fn test_import_success_backs_up_and_writes() {
        let dir = tempdir();
        let mut store = SaveStore::new(dir.path());
        let first = sample_state(1);
        store.save(&first, 100).unwrap();
        let first_bytes = std::fs::read(store.paths().save()).unwrap();

        let second = sample_state(8);
        let export_dir = tempdir();
        let export_path = export_dir.path().join("import.json");
        let mut exporter = SaveStore::new(export_dir.path());
        exporter.export_to_path(&second, &export_path, 150).unwrap();

        let imported = store.import_from_path(&export_path, 500).unwrap();
        assert_eq!(state_digest(&imported), state_digest(&second));
        assert_eq!(std::fs::read(store.paths().backup()).unwrap(), first_bytes);

        let loaded = store.load(500).unwrap();
        assert_eq!(state_digest(&loaded.state), state_digest(&second));
        assert_eq!(loaded.saved_at_unix, 500, "no offline credit for imports");
    }

    #[test]
    fn test_export_import_roundtrip() {
        let dir = tempdir();
        let mut store = SaveStore::new(dir.path());
        let state = sample_state(4);
        let export_dir = tempdir();
        let path = export_dir.path().join("export.json");
        store.export_to_path(&state, &path, 777).unwrap();
        let imported = store.import_from_path(&path, 888).unwrap();
        assert_eq!(state_digest(&imported), state_digest(&state));
    }

    fn v0_envelope(era: u32) -> Value {
        let state = sample_state(era);
        let mut state_value = serde_json::to_value(&state).unwrap();
        {
            let meta = state_value
                .get_mut("meta")
                .and_then(Value::as_object_mut)
                .unwrap();
            let value = meta.remove("era_fathoms").unwrap();
            meta.insert("legacy_era".to_owned(), value);
            // Genuine pre-v2 state has no tutorial field.
            meta.remove("tutorial");
        }
        let mut envelope = json!({
            "format": FORMAT_MARKER,
            "schema_version": 0,
            "app_version": "0.0.0",
            "saved_at_unix": 11,
            "save_counter": 3,
            "state": state_value,
        });
        let checksum = canonical_checksum(&envelope);
        envelope["checksum"] = Value::String(checksum);
        envelope
    }

    #[test]
    fn test_migration_chain_0_to_1_fixture() {
        let dir = tempdir();
        write_bytes(
            &dir.path().join("save.json"),
            &serde_json::to_vec(&v0_envelope(7)).unwrap(),
        );
        let mut chain = MigrationChain::production();
        chain.register(0, 1, mig_0_1);
        let mut store = SaveStore::with_migrations(dir.path(), chain);
        let loaded = store.load(50).unwrap();
        assert_eq!(loaded.state.meta.era_fathoms, 7);
        assert!(loaded.state.meta.tutorial.completed);
    }

    fn v1_envelope(era: u32) -> Value {
        let state = sample_state(era);
        let mut state_value = serde_json::to_value(&state).unwrap();
        state_value
            .get_mut("meta")
            .and_then(Value::as_object_mut)
            .unwrap()
            .remove("tutorial");
        let mut envelope = json!({
            "format": FORMAT_MARKER,
            "schema_version": 1,
            "app_version": "0.1.0",
            "saved_at_unix": 11,
            "save_counter": 3,
            "state": state_value,
        });
        let checksum = canonical_checksum(&envelope);
        envelope["checksum"] = Value::String(checksum);
        envelope
    }

    #[test]
    fn test_v1_migration_inserts_completed_tutorial() {
        let dir = tempdir();
        write_bytes(
            &dir.path().join("save.json"),
            &serde_json::to_vec(&v1_envelope(5)).unwrap(),
        );
        let mut store = SaveStore::new(dir.path());
        let loaded = store.load(20).unwrap();
        assert!(loaded.state.meta.tutorial.completed);
        assert!(!loaded.state.meta.tutorial.skipped);
        assert_eq!(loaded.state.meta.era_fathoms, 5);

        // Stable across repeated loads.
        let mut store2 = SaveStore::new(dir.path());
        let loaded2 = store2.load(21).unwrap();
        assert_eq!(state_digest(&loaded.state), state_digest(&loaded2.state));
    }

    #[test]
    fn test_new_save_writes_schema_v2() {
        let dir = tempdir();
        let mut store = SaveStore::new(dir.path());
        store.save(&sample_state(1), 10).unwrap();
        let value: Value =
            serde_json::from_slice(&std::fs::read(store.paths().save()).unwrap()).unwrap();
        assert_eq!(value["schema_version"], json!(SUPPORTED_SCHEMA));
        assert_eq!(SUPPORTED_SCHEMA, 2);
    }

    #[test]
    fn test_tutorial_step_roundtrips() {
        let dir = tempdir();
        let mut engine = Engine::new();
        engine.state.meta.tutorial.step = 5;
        assert!(!engine.state.meta.tutorial.completed);
        let mut store = SaveStore::new(dir.path());
        store.save(&engine.state.clone(), 10).unwrap();
        let loaded = store.load(20).unwrap();
        assert_eq!(loaded.state.meta.tutorial.step, 5);
        assert!(!loaded.state.meta.tutorial.completed);
    }

    #[test]
    fn test_v0_without_migration_is_typed_error() {
        let dir = tempdir();
        let path = dir.path().join("save.json");
        let bytes = serde_json::to_vec(&v0_envelope(7)).unwrap();
        write_bytes(&path, &bytes);
        let mut store = SaveStore::new(dir.path());
        let err = store.load(50).unwrap_err();
        assert!(
            matches!(
                err,
                SaveError::Migration(MigrationError::MissingStep { from: 0, to: 1 })
            ),
            "{err:?}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "file unchanged");
    }

    fn step_noop(value: Value) -> Result<Value, String> {
        Ok(value)
    }

    #[test]
    fn test_migration_missing_step_is_error() {
        let mut chain = MigrationChain::empty();
        chain.register(0, 1, step_noop);
        let value = json!({});
        let err = chain.migrate(value.clone(), 0, 2).unwrap_err();
        assert!(
            matches!(err, MigrationError::MissingStep { from: 1, to: 2 }),
            "{err:?}"
        );

        let err = MigrationChain::empty().migrate(value, 0, 1).unwrap_err();
        assert!(matches!(err, MigrationError::MissingStep { from: 0, .. }));

        // Loading a v0 file with the production (empty) chain fails typed.
        let dir2 = tempdir();
        write_bytes(
            &dir2.path().join("save.json"),
            br#"{"format":"hadal-save","schema_version":0,"app_version":"0","saved_at_unix":0,"save_counter":0,"checksum":"sha256:00","state":{}}"#,
        );
        let mut store = SaveStore::new(dir2.path());
        // The fixture is malformed too, but schema/migration is evaluated after
        // checksum; checksum fails first, which is a typed corruption error.
        assert!(store.load(1).is_err());
    }

    fn step_0_1(mut value: Value) -> Result<Value, String> {
        value["a"] = json!(1);
        Ok(value)
    }
    fn step_1_2(mut value: Value) -> Result<Value, String> {
        value["b"] = json!(2);
        Ok(value)
    }

    #[test]
    fn test_migration_chain_ordering() {
        let mut chain = MigrationChain::empty();
        chain.register(0, 1, step_0_1);
        chain.register(1, 2, step_1_2);
        let out = chain.migrate(json!({}), 0, 2).unwrap();
        assert_eq!(out["a"], json!(1));
        assert_eq!(out["b"], json!(2));
    }

    #[test]
    fn test_config_roundtrip_defaults_and_corrupt() {
        let dir = tempdir();
        let path = dir.path().join("config.json");
        let (defaults, warning) = config::load_config(&path);
        assert!(warning.is_none());
        assert_eq!(defaults, Config::default());
        assert_eq!(defaults.theme, Theme::Abyss);
        assert!(!defaults.reduce_motion);
        assert!(!defaults.ascii_only);
        assert!(defaults.mouse);
        assert_eq!(defaults.color_depth, ColorDepth::Auto);
        assert!(!defaults.bell);

        let custom = Config {
            theme: Theme::Mono,
            reduce_motion: true,
            ascii_only: true,
            mouse: false,
            color_depth: ColorDepth::Ansi256,
            bell: true,
        };
        config::save_config(&path, &custom).unwrap();
        let (loaded, warning) = config::load_config(&path);
        assert!(warning.is_none());
        assert_eq!(loaded, custom);

        // Unknown fields are ignored; missing fields use defaults.
        write_bytes(&path, br#"{"theme":"mono","future_field":123}"#);
        let (loaded, warning) = config::load_config(&path);
        assert!(warning.is_none());
        assert_eq!(loaded.theme, Theme::Mono);
        assert!(loaded.mouse);

        // Corrupt config falls back to defaults and is not deleted.
        write_bytes(&path, b"{ not json");
        let (loaded, warning) = config::load_config(&path);
        assert_eq!(loaded, Config::default());
        assert!(matches!(warning, Some(ConfigWarning::Corrupt { .. })));
        assert!(path.exists());
    }

    #[test]
    fn test_color_depth_variants_roundtrip() {
        let dir = tempdir();
        let path = dir.path().join("config.json");
        for depth in [
            ColorDepth::Auto,
            ColorDepth::Ansi16,
            ColorDepth::Ansi256,
            ColorDepth::TrueColor,
            ColorDepth::None,
        ] {
            assert_eq!(depth.to_string().parse::<ColorDepth>().unwrap(), depth);
            let config = Config {
                color_depth: depth,
                ..Config::default()
            };
            config::save_config(&path, &config).unwrap();
            let (loaded, warning) = config::load_config(&path);
            assert!(warning.is_none());
            assert_eq!(loaded.color_depth, depth);
        }

        // `None` is written as "none".
        let config = Config {
            color_depth: ColorDepth::None,
            ..Config::default()
        };
        config::save_config(&path, &config).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"color_depth\": \"none\""), "{text}");

        // Unknown values still fall back to defaults with a warning.
        write_bytes(&path, br#"{"color_depth":"bogus"}"#);
        let (loaded, warning) = config::load_config(&path);
        assert_eq!(loaded, Config::default());
        assert!(matches!(warning, Some(ConfigWarning::Corrupt { .. })));
        assert!(path.exists());
    }

    #[test]
    fn test_lock_creates_nested_dir_and_rejects_regular_file() {
        let base = tempdir();
        let nested = base.path().join("a").join("b").join("c");
        assert!(!nested.exists());
        let lock = SaveLock::acquire(&nested).unwrap();
        assert!(nested.is_dir(), "acquire must create the data directory");
        assert!(matches!(
            SaveLock::acquire(&nested),
            Err(LockError::AlreadyRunning)
        ));
        drop(lock);
        // Dropping releases the lock.
        let reopened = SaveLock::acquire(&nested).unwrap();
        drop(reopened);

        let file = base.path().join("regular_file");
        write_bytes(&file, b"not a directory");
        let err = SaveLock::acquire(&file).unwrap_err();
        match err {
            LockError::NotADirectory { path } => assert_eq!(path, file),
            other => panic!("expected NotADirectory, got {other:?}"),
        }
    }

    #[test]
    fn test_save_into_fresh_dir_then_load() {
        let base = tempdir();
        let dir = base.path().join("fresh").join("nested");
        assert!(!dir.exists());
        let mut store = SaveStore::new(&dir);
        let state = sample_state(3);
        store.save(&state, 10).unwrap();
        assert!(dir.is_dir(), "save must create the data directory");
        let loaded = store.load(11).unwrap();
        assert_eq!(state_digest(&loaded.state), state_digest(&state));
    }

    #[test]
    fn test_export_missing_parent_is_typed_with_path() {
        let base = tempdir();
        let mut store = SaveStore::new(base.path().join("data"));
        let path = base.path().join("missing").join("deeper").join("out.json");
        let err = store
            .export_to_path(&sample_state(1), &path, 1)
            .unwrap_err();
        match err {
            SaveError::Io { path: named, .. } => assert_eq!(named, path),
            other => panic!("expected Io naming the export path, got {other:?}"),
        }
        assert!(!base.path().join("missing").exists());
    }

    #[test]
    fn test_import_into_fresh_dir() {
        let base = tempdir();
        let source_path = base.path().join("import.json");
        let state = sample_state(9);
        let mut exporter = SaveStore::new(base.path().join("unused"));
        exporter.export_to_path(&state, &source_path, 1).unwrap();

        let fresh = base.path().join("newdata").join("sub");
        assert!(!fresh.exists());
        let mut store = SaveStore::new(&fresh);
        let imported = store.import_from_path(&source_path, 20).unwrap();
        assert_eq!(state_digest(&imported), state_digest(&state));
        assert!(fresh.is_dir(), "import must create the data directory");
        let loaded = store.load(20).unwrap();
        assert_eq!(state_digest(&loaded.state), state_digest(&state));
    }

    #[test]
    fn test_load_missing_dir_is_no_save() {
        let base = tempdir();
        let dir = base.path().join("absent");
        let mut store = SaveStore::new(&dir);
        let err = store.load(1).unwrap_err();
        assert!(matches!(err, SaveError::NoSave { .. }), "{err:?}");
        assert!(!dir.exists());
    }

    #[test]
    fn test_lock_acquire_twice_and_release_on_drop() {
        let dir = tempdir();
        let first = SaveLock::acquire(dir.path()).unwrap();
        assert!(matches!(
            SaveLock::acquire(dir.path()),
            Err(LockError::AlreadyRunning)
        ));
        drop(first);
        let second = SaveLock::acquire(dir.path()).unwrap();
        drop(second);
    }

    #[test]
    fn test_data_dir_resolution_injected() {
        let env_map = |pairs: &'static [(&'static str, &'static str)]| {
            move |key: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| (*v).to_owned())
            }
        };

        let explicit = PathBuf::from("/explicit");
        assert_eq!(
            resolve_data_dir_with(
                Some(&explicit),
                &env_map(&[("HADAL_DATA_DIR", "/env")]),
                Some(std::path::Path::new("/home/u")),
                OsFamily::Linux
            ),
            explicit
        );
        assert_eq!(
            resolve_data_dir_with(
                None,
                &env_map(&[("HADAL_DATA_DIR", "/env")]),
                Some(std::path::Path::new("/home/u")),
                OsFamily::Linux
            ),
            PathBuf::from("/env")
        );
        assert_eq!(
            resolve_data_dir_with(
                None,
                &env_map(&[("APPDATA", "C:/AppData")]),
                Some(std::path::Path::new("C:/Users/u")),
                OsFamily::Windows
            ),
            PathBuf::from("C:/AppData").join("hadal")
        );
        assert_eq!(
            resolve_data_dir_with(
                None,
                &env_map(&[]),
                Some(std::path::Path::new("/home/u")),
                OsFamily::MacOs
            ),
            PathBuf::from("/home/u")
                .join("Library")
                .join("Application Support")
                .join("hadal")
        );
        assert_eq!(
            resolve_data_dir_with(
                None,
                &env_map(&[("XDG_DATA_HOME", "/xdg")]),
                Some(std::path::Path::new("/home/u")),
                OsFamily::Linux
            ),
            PathBuf::from("/xdg").join("hadal")
        );
        assert_eq!(
            resolve_data_dir_with(
                None,
                &env_map(&[]),
                Some(std::path::Path::new("/home/u")),
                OsFamily::Linux
            ),
            PathBuf::from("/home/u")
                .join(".local")
                .join("share")
                .join("hadal")
        );
    }

    #[test]
    fn test_auto_buy_survives_save_load() {
        let dir = tempdir();
        let mut engine = Engine::new();
        engine
            .apply(Action::SetAutoBuy {
                id: GeneratorId::Hydrophone,
                enabled: false,
            })
            .unwrap();
        engine
            .apply(Action::SetAutoBuy {
                id: GeneratorId::TidalCore,
                enabled: false,
            })
            .unwrap();
        let mut store = SaveStore::new(dir.path());
        store.save(&engine.state.clone(), 10).unwrap();

        let loaded = store.load(20).unwrap();
        let mut restored = Engine::new();
        restored.state = loaded.state;
        assert!(!restored.view().auto_buy(GeneratorId::Hydrophone));
        assert!(!restored.view().auto_buy(GeneratorId::TidalCore));
        assert!(restored.view().auto_buy(GeneratorId::Drifter));
        assert!(restored.view().auto_buy(GeneratorId::SonarMast));
    }

    #[test]
    fn test_engine_state_survives_save_load() {
        let dir = tempdir();
        let mut engine = Engine::new();
        engine.state.meta.era_fathoms = 12;
        engine.state.run.generators = [5, 2, 0, 0];
        engine.apply(Action::SetTuning { band: 2 }).unwrap();
        engine
            .apply(Action::BuyGenerator {
                id: GeneratorId::Hydrophone,
                quantity: BuyQuantity::One,
            })
            .ok();
        let state = engine.state.clone();
        let mut store = SaveStore::new(dir.path());
        store.save(&state, 1234).unwrap();
        let loaded = store.load(2000).unwrap();
        assert_eq!(state_digest(&loaded.state), state_digest(&state));
    }

    use crate::content::ids::GeneratorId;
}
