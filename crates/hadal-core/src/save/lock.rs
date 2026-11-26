//! Single-instance advisory lock on `<data>/save.lock`.
//!
//! Uses `std::fs::File::try_lock(&self) -> Result<(), std::fs::TryLockError>`
//! (stable in Rust 1.89). `TryLockError::WouldBlock` maps to
//! [`LockError::AlreadyRunning`]. The lock is released when the [`SaveLock`]
//! (and thus its file handle) is dropped.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::paths::LOCK_FILE;

/// Error acquiring the save lock.
#[derive(Debug, Error)]
pub enum LockError {
    /// Another process already holds the lock.
    #[error("another HADAL instance is already running")]
    AlreadyRunning,
    /// Underlying I/O failure.
    #[error("lock i/o error: {0}")]
    Io(#[from] std::io::Error),
}

/// A held advisory save lock. Released on drop.
#[derive(Debug)]
pub struct SaveLock {
    _file: File,
    path: PathBuf,
}

impl SaveLock {
    /// Acquires the lock in `dir`, creating `save.lock` if needed.
    ///
    /// # Errors
    ///
    /// Returns [`LockError::AlreadyRunning`] if the lock is held elsewhere, or
    /// [`LockError::Io`] on filesystem failure.
    pub fn acquire(dir: &Path) -> Result<Self, LockError> {
        let path = dir.join(LOCK_FILE);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file, path }),
            Err(TryLockError::WouldBlock) => Err(LockError::AlreadyRunning),
            Err(TryLockError::Error(err)) => Err(LockError::Io(err)),
        }
    }

    /// The lock file path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}
