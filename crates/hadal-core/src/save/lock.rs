//! Single-instance advisory lock on `<data>/save.lock`.
//!
//! Uses `std::fs::File::try_lock(&self) -> Result<(), std::fs::TryLockError>`
//! (stable in Rust 1.89). `TryLockError::WouldBlock` maps to
//! [`LockError::AlreadyRunning`]. The lock is released when the [`SaveLock`]
//! (and thus its file handle) is dropped.
//!
//! `acquire` creates the data directory (including parents) when missing, so a
//! first-ever launch into a fresh `--data-dir` works.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::paths::LOCK_FILE;

/// Error acquiring the save lock.
#[derive(Debug, Error)]
pub enum LockError {
    /// Another process already holds the lock.
    #[error("another HADAL instance is already running")]
    AlreadyRunning,
    /// The data-directory path exists but is not a directory.
    #[error("data directory path is not a directory: {path}")]
    NotADirectory {
        /// Offending path.
        path: PathBuf,
    },
    /// Creating the data directory failed.
    #[error("failed to create data directory {path}: {source}")]
    CreateDir {
        /// Directory path.
        path: PathBuf,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
    /// Underlying I/O failure (message includes the path).
    #[error("lock i/o error: {0}")]
    Io(#[from] io::Error),
}

fn with_path(path: &Path, err: io::Error) -> io::Error {
    io::Error::new(err.kind(), format!("{}: {err}", path.display()))
}

/// A held advisory save lock. Released on drop.
#[derive(Debug)]
pub struct SaveLock {
    _file: File,
    path: PathBuf,
}

impl SaveLock {
    /// Acquires the lock in `dir`, creating `dir` (and parents) if needed.
    ///
    /// # Errors
    ///
    /// - [`LockError::AlreadyRunning`] if the lock is held elsewhere.
    /// - [`LockError::NotADirectory`] if `dir` exists as a regular file.
    /// - [`LockError::CreateDir`] if the directory cannot be created.
    /// - [`LockError::Io`] for other filesystem failures (message includes path).
    pub fn acquire(dir: &Path) -> Result<Self, LockError> {
        if dir.exists() && !dir.is_dir() {
            return Err(LockError::NotADirectory {
                path: dir.to_path_buf(),
            });
        }
        if let Err(source) = std::fs::create_dir_all(dir) {
            // A concurrent file may have appeared at the path.
            if dir.exists() && !dir.is_dir() {
                return Err(LockError::NotADirectory {
                    path: dir.to_path_buf(),
                });
            }
            return Err(LockError::CreateDir {
                path: dir.to_path_buf(),
                source,
            });
        }

        let path = dir.join(LOCK_FILE);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|err| LockError::Io(with_path(&path, err)))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file, path }),
            Err(TryLockError::WouldBlock) => Err(LockError::AlreadyRunning),
            Err(TryLockError::Error(err)) => Err(LockError::Io(with_path(&path, err))),
        }
    }

    /// The lock file path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}
