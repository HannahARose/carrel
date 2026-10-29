//! Deciding which device or process may change an archive (FORMAT.md §8.4).
//!
//! Only one writer at a time may change an archive's metadata and record history. A
//! [`Coordinator`] hands out [`WriterLease`]s; history commits require one. This module
//! provides the coordinator for archives on a local or external disk, [`DiskLock`], which uses
//! `_System/writer.lock`. Archives shared between devices through a sync service need a
//! coordinator that service supports (`CloudKit` for iCloud Drive, in a later version): a lock
//! file is not reliable when several devices can create it before syncing.
//!
//! Leases expire unless renewed, so a crashed writer can't block an archive forever.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::archive::Archive;
use crate::card::{Extra, Id};
use crate::{Access, CARREL_VERSION};

/// The lock file, relative to the archive root.
pub const LOCK_FILE: &str = "_System/writer.lock";

/// How long a lease lasts unless renewed.
pub const DEFAULT_LEASE: Duration = Duration::from_mins(5);

/// Who is asking to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Holder {
    /// The person making changes.
    pub person: Id,
    /// The device making them.
    pub device: Id,
}

/// The contents of `_System/writer.lock`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockInfo {
    /// A random value identifying this particular lease.
    pub token: Id,
    /// The person holding the lease.
    pub person: Id,
    /// The device holding it.
    pub device: Id,
    /// What the writer is doing, such as `ingest`.
    pub purpose: String,
    /// The program, such as `carrel 0.1.0`.
    pub program: String,
    /// When the lease was first taken.
    pub acquired: String,
    /// When the lease runs out unless renewed (RFC 3339).
    pub expires: String,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl LockInfo {
    /// Whether the lease has run out.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.expires
            .parse::<jiff::Timestamp>()
            .map_or(true, |expires| expires <= jiff::Timestamp::now())
    }
}

/// Why a lease couldn't be taken or used.
#[derive(Debug)]
pub enum CoordinatorError {
    /// Another writer holds a lease that hasn't expired.
    Held(Box<LockInfo>),
    /// The archive uses a newer format version and must not be changed.
    ReadOnly,
    /// This lease was lost: it expired and was taken over, or was broken.
    LeaseLost,
    /// The lock file exists but can't be read as a lock. Use [`DiskLock::force_break`] once
    /// you are sure no other writer is running.
    Corrupt(PathBuf),
    /// A file operation failed.
    Io {
        /// The path concerned.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
}

impl std::fmt::Display for CoordinatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Held(info) => write!(
                f,
                "the archive is being changed by device {} ({}) until {}",
                info.device, info.purpose, info.expires
            ),
            Self::ReadOnly => f.write_str("the archive uses a newer format and is read-only"),
            Self::LeaseLost => f.write_str("this writer's lease expired and was taken over"),
            Self::Corrupt(p) => write!(f, "{} exists but isn't a valid writer lock", p.display()),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for CoordinatorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> CoordinatorError + '_ {
    move |source| CoordinatorError::Io {
        path: path.to_owned(),
        source,
    }
}

fn timestamp(t: jiff::Timestamp) -> String {
    t.to_zoned(jiff::tz::TimeZone::system())
        .strftime("%Y-%m-%dT%H:%M:%S%.f%:z")
        .to_string()
}

/// Something that decides who may write to an archive.
pub trait Coordinator {
    /// Takes the writer role for `purpose`, for `lease` unless renewed.
    ///
    /// # Errors
    ///
    /// [`CoordinatorError::Held`] if another writer holds a live lease, or another
    /// [`CoordinatorError`].
    fn acquire(
        &self,
        holder: Holder,
        purpose: &str,
        lease: Duration,
    ) -> Result<WriterLease, CoordinatorError>;

    /// The current writer, if any (including one whose lease has expired).
    ///
    /// # Errors
    ///
    /// [`CoordinatorError::Corrupt`] or [`CoordinatorError::Io`].
    fn current(&self) -> Result<Option<LockInfo>, CoordinatorError>;
}

/// The coordinator for archives on a local or external disk: an atomically created lock file.
#[derive(Debug, Clone)]
pub struct DiskLock {
    path: PathBuf,
    read_only: bool,
}

impl DiskLock {
    /// The coordinator for `archive`.
    #[must_use]
    pub fn for_archive(archive: &Archive) -> Self {
        Self {
            path: archive.root().join(LOCK_FILE),
            read_only: archive.access() == Access::ReadOnly,
        }
    }

    /// Whether `root` is inside a folder synced by iCloud Drive or another file provider
    /// (`~/Library/Mobile Documents` or `~/Library/CloudStorage`). A lock file is only safe there
    /// while a single device uses the archive.
    #[must_use]
    pub fn is_synced_folder(root: &Path) -> bool {
        let text = root.to_string_lossy();
        text.contains("/Library/Mobile Documents/") || text.contains("/Library/CloudStorage/")
    }

    /// Removes the lock whatever its state, returning what it held. Only for when you are sure
    /// no other writer is running, such as after a crash.
    ///
    /// # Errors
    ///
    /// [`CoordinatorError::Io`] if the lock can't be removed.
    pub fn force_break(&self) -> Result<Option<LockInfo>, CoordinatorError> {
        let previous = self.read().ok().flatten();
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(previous),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io(&self.path)(e)),
        }
    }

    fn read(&self) -> Result<Option<LockInfo>, CoordinatorError> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|_| CoordinatorError::Corrupt(self.path.clone())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io(&self.path)(e)),
        }
    }

    /// Like [`DiskLock::read`], but gives a lock that another writer may still be writing a
    /// moment to finish before calling it corrupt (only possible without hard links).
    fn read_settled(&self) -> Result<Option<LockInfo>, CoordinatorError> {
        for _ in 0..20 {
            match self.read() {
                Err(CoordinatorError::Corrupt(_)) => std::thread::sleep(Duration::from_millis(5)),
                other => return other,
            }
        }
        self.read()
    }

    /// Creates the lock file only if it doesn't exist. The complete lock is written beside it
    /// and hard-linked into place, which the operating system does atomically and only if the
    /// lock doesn't exist yet, so of several simultaneous attempts exactly one succeeds and
    /// nobody sees a half-written lock.
    fn create(&self, info: &LockInfo) -> Result<bool, CoordinatorError> {
        use std::io::Write as _;
        let text = serde_json::to_string_pretty(info).expect("locks always serialize") + "\n";
        let temp = self
            .path
            .with_file_name(format!("writer.lock.new-{}", info.token));
        let mut file = std::fs::File::create(&temp).map_err(io(&temp))?;
        file.write_all(text.as_bytes()).map_err(io(&temp))?;
        file.sync_all().map_err(io(&temp))?;
        drop(file);
        let linked = std::fs::hard_link(&temp, &self.path);
        let _ = std::fs::remove_file(&temp);
        match linked {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            // File systems without hard links, such as exFAT: create the lock exclusively.
            Err(_) => self.create_in_place(&text),
        }
    }

    fn create_in_place(&self, text: &str) -> Result<bool, CoordinatorError> {
        use std::io::Write as _;
        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
            Err(e) => return Err(io(&self.path)(e)),
        };
        file.write_all(text.as_bytes()).map_err(io(&self.path))?;
        file.sync_all().map_err(io(&self.path))?;
        Ok(true)
    }

    /// Moves an expired lock aside. Renaming is atomic, so when several writers try at once
    /// only one moves it; the others find it gone and race fairly for [`DiskLock::create`].
    fn break_expired(&self, expired: &LockInfo) -> Result<(), CoordinatorError> {
        let aside = self
            .path
            .with_file_name(format!("writer.lock.expired-{}", expired.token));
        match std::fs::rename(&self.path, &aside) {
            Ok(()) => {
                let _ = std::fs::remove_file(&aside);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io(&self.path)(e)),
        }
    }
}

impl Coordinator for DiskLock {
    fn acquire(
        &self,
        holder: Holder,
        purpose: &str,
        lease: Duration,
    ) -> Result<WriterLease, CoordinatorError> {
        if self.read_only {
            return Err(CoordinatorError::ReadOnly);
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(io(parent))?;
        }
        let now = jiff::Timestamp::now();
        let info = LockInfo {
            token: Id::new_v7(),
            person: holder.person,
            device: holder.device,
            purpose: purpose.to_owned(),
            program: format!("carrel {CARREL_VERSION}"),
            acquired: timestamp(now),
            expires: timestamp(now + lease),
            extra: Extra::new(),
        };
        // Two rounds: at most one expired lock needs clearing before the real attempt.
        for _ in 0..2 {
            if self.create(&info)? {
                return Ok(WriterLease {
                    path: self.path.clone(),
                    info,
                    released: false,
                });
            }
            match self.read_settled()? {
                Some(existing) if existing.is_expired() => self.break_expired(&existing)?,
                Some(existing) => return Err(CoordinatorError::Held(Box::new(existing))),
                None => {} // released in between; try again
            }
        }
        match self.read()? {
            Some(existing) => Err(CoordinatorError::Held(Box::new(existing))),
            None => Err(CoordinatorError::LeaseLost),
        }
    }

    fn current(&self) -> Result<Option<LockInfo>, CoordinatorError> {
        self.read_settled()
    }
}

/// The right to change an archive, held until released, dropped or expired.
#[derive(Debug)]
pub struct WriterLease {
    path: PathBuf,
    info: LockInfo,
    released: bool,
}

impl WriterLease {
    /// What this lease recorded in the lock file.
    #[must_use]
    pub fn info(&self) -> &LockInfo {
        &self.info
    }

    /// Confirms the lease is still this writer's: the lock file holds this lease's token and
    /// hasn't expired.
    ///
    /// # Errors
    ///
    /// [`CoordinatorError::LeaseLost`] if not.
    pub fn check(&self) -> Result<(), CoordinatorError> {
        let text = std::fs::read_to_string(&self.path).map_err(|_| CoordinatorError::LeaseLost)?;
        let current: LockInfo =
            serde_json::from_str(&text).map_err(|_| CoordinatorError::LeaseLost)?;
        if current.token != self.info.token || current.is_expired() {
            return Err(CoordinatorError::LeaseLost);
        }
        Ok(())
    }

    /// Extends the lease to `lease` from now, for long operations.
    ///
    /// # Errors
    ///
    /// [`CoordinatorError::LeaseLost`] if the lease was already lost, or an I/O error.
    ///
    /// # Panics
    ///
    /// Never: a lock always serializes.
    pub fn renew(&mut self, lease: Duration) -> Result<(), CoordinatorError> {
        self.check()?;
        self.info.expires = timestamp(jiff::Timestamp::now() + lease);
        let text = serde_json::to_string_pretty(&self.info).expect("locks always serialize") + "\n";
        // Write beside the lock and rename over it, so readers never see a partial file.
        let temp = self
            .path
            .with_file_name(format!("writer.lock.renew-{}", self.info.token));
        std::fs::write(&temp, text).map_err(io(&temp))?;
        std::fs::rename(&temp, &self.path).map_err(io(&self.path))
    }

    /// Gives up the lease. A lease that was already lost is left alone.
    ///
    /// # Errors
    ///
    /// [`CoordinatorError::Io`] if the lock can't be removed.
    pub fn release(mut self) -> Result<(), CoordinatorError> {
        self.release_inner()
    }

    fn release_inner(&mut self) -> Result<(), CoordinatorError> {
        if self.released {
            return Ok(());
        }
        self.released = true;
        if self.check().is_ok() {
            std::fs::remove_file(&self.path).map_err(io(&self.path))?;
        }
        Ok(())
    }
}

impl Drop for WriterLease {
    fn drop(&mut self) {
        let _ = self.release_inner();
    }
}
