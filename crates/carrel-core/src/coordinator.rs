//! Deciding which device or process may change an archive (FORMAT.md §8.4).
//!
//! Only one writer at a time may change an archive's metadata and record history. A
//! [`Coordinator`] hands out [`WriterLease`]s; history commits require one. This module
//! provides [`DiskLock`], which every writer takes:
//!
//! - **On a local or external disk,** it's `_System/writer.lock`.
//! - **In iCloud Drive,** a lock file isn't reliable, since several devices can each create it
//!   before syncing, so the device that writes is chosen through `CloudKit` ([`cloud`]), and
//!   the lock file, moved to `_System/cache.nosync/`, only picks the process on this device.
//!   A device that can't reach `CloudKit` (no agent on a Mac, or Linux and Windows) doesn't
//!   write an archive that has used it, and acts through its inbox instead.
//!
//! Leases expire unless renewed, so a crashed writer can't block an archive forever.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::archive::Archive;
use crate::card::{Extra, Id};
use crate::{Access, CARREL_VERSION};

#[cfg(unix)]
pub mod agent_lock;
pub mod cloud;

pub use cloud::{CloudLock, FakeCloudKit, RemoteLock, is_in_icloud_drive};

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
    /// The archive's writer is chosen through `CloudKit`, which can't be reached from here.
    Unavailable(String),
    /// iCloud Drive hasn't yet delivered the history the last writer recorded, so writing
    /// now would build on history this device hasn't seen.
    HistoryBehind {
        /// The commit the last writer recorded.
        head: String,
    },
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
            Self::Unavailable(why) => write!(
                f,
                "this archive's writer is chosen through iCloud, which can't be reached ({why}); \
                 on a Mac, open Carrel so its agent runs, and elsewhere, changes go through this \
                 device's inbox"
            ),
            Self::HistoryBehind { head } => write!(
                f,
                "iCloud Drive hasn't delivered the archive's latest history yet (commit {}); try \
                 again once it has synced",
                head.get(..12).unwrap_or(head.as_str())
            ),
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

/// The writer lock every writer takes: an atomically created lock file, and, for an archive in
/// iCloud Drive, the `CloudKit` lock record too.
#[derive(Debug, Clone)]
pub struct DiskLock {
    path: PathBuf,
    read_only: bool,
    devices: Devices,
}

/// How devices take turns.
#[derive(Debug, Clone)]
enum Devices {
    /// One device uses the archive: the lock file is enough.
    One,
    /// Through `CloudKit`.
    Cloud(CloudLock),
    /// Through `CloudKit`, which this process can't reach.
    Unreachable,
}

impl DiskLock {
    /// The coordinator for `archive`. For an archive in iCloud Drive it uses `CloudKit` when
    /// this process can reach it ([`cloud::remote_lock`]); one that can't writes only an
    /// archive that has never used `CloudKit`, with the lock file alone, as before v0.8.
    #[must_use]
    pub fn for_archive(archive: &Archive) -> Self {
        let root = archive.root();
        let devices = if is_in_icloud_drive(root) {
            match cloud::remote_lock() {
                Some(remote) => Devices::Cloud(CloudLock::new(archive, remote)),
                None if cloud::uses_cloud_lock(root) => Devices::Unreachable,
                None => Devices::One,
            }
        } else {
            Devices::One
        };
        Self::with_devices(archive, devices)
    }

    /// The coordinator for `archive`, with devices taking turns through `cloud`, wherever the
    /// archive is.
    #[must_use]
    pub fn with_cloud(archive: &Archive, cloud: CloudLock) -> Self {
        Self::with_devices(archive, Devices::Cloud(cloud))
    }

    fn with_devices(archive: &Archive, devices: Devices) -> Self {
        let file = match devices {
            Devices::One => LOCK_FILE,
            Devices::Cloud(_) | Devices::Unreachable => cloud::LOCAL_LOCK_FILE,
        };
        Self {
            path: archive.root().join(file),
            read_only: archive.access() == Access::ReadOnly,
            devices,
        }
    }

    /// Whether devices take turns through `CloudKit` here.
    #[must_use]
    pub fn uses_cloud(&self) -> bool {
        matches!(self.devices, Devices::Cloud(_))
    }

    /// Whether `root` is inside a folder synced by iCloud Drive or another file provider
    /// (`~/Library/Mobile Documents` or `~/Library/CloudStorage`). A lock file is only safe there
    /// while a single device uses the archive.
    #[must_use]
    pub fn is_synced_folder(root: &Path) -> bool {
        let text = root.to_string_lossy().replace('\\', "/");
        if text.contains("/Library/Mobile Documents/") || text.contains("/Library/CloudStorage/") {
            return true;
        }
        // The folders sync clients make on Windows and Linux: OneDrive (also "OneDrive -
        // Company"), Dropbox, Google Drive and iCloud for Windows.
        root.components().any(|c| {
            let name = c.as_os_str().to_string_lossy();
            name == "OneDrive"
                || name.starts_with("OneDrive - ")
                || name == "Dropbox"
                || name.starts_with("Dropbox (")
                || name == "Google Drive"
                || name == "My Drive"
                || name == "iCloudDrive"
        })
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
        match rename_retrying(&self.path, &aside) {
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
        let mut lease = self.acquire_file(holder, purpose, lease)?;
        match &self.devices {
            Devices::One => {}
            Devices::Cloud(cloud) => {
                // Dropping the lease on failure releases the lock file.
                lease.cloud = Some(cloud.take(&lease.info, lease_length(&lease.info))?);
            }
            Devices::Unreachable => {
                return Err(CoordinatorError::Unavailable(
                    "this process has no connection to CloudKit".into(),
                ));
            }
        }
        Ok(lease)
    }

    fn current(&self) -> Result<Option<LockInfo>, CoordinatorError> {
        if let Devices::Cloud(cloud) = &self.devices
            && let Ok(current) = cloud.current()
        {
            return Ok(current);
        }
        self.read_settled()
    }
}

/// How long `info`'s lease was taken for.
fn lease_length(info: &LockInfo) -> Duration {
    let span = |t: &str| t.parse::<jiff::Timestamp>().ok();
    match (span(&info.acquired), span(&info.expires)) {
        (Some(from), Some(to)) => Duration::try_from(to.duration_since(from)).unwrap_or_default(),
        _ => DEFAULT_LEASE,
    }
}

impl DiskLock {
    fn acquire_file(
        &self,
        holder: Holder,
        purpose: &str,
        lease: Duration,
    ) -> Result<WriterLease, CoordinatorError> {
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
                    agent: None,
                    cloud: None,
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
}

/// The right to change an archive, held until released, dropped or expired.
#[derive(Debug)]
pub struct WriterLease {
    path: PathBuf,
    info: LockInfo,
    released: bool,
    /// The program acting for the holder on its own, such as the librarian, while it is.
    agent: Option<String>,
    /// The lease's hold on the `CloudKit` lock record, for an archive in iCloud Drive.
    cloud: Option<cloud::CloudHold>,
}

impl WriterLease {
    /// What this lease recorded in the lock file.
    #[must_use]
    pub fn info(&self) -> &LockInfo {
        &self.info
    }

    /// Marks the changes made with this lease from now on as made by `agent` acting for the
    /// holder on its own, such as `librarian`, or, with `None`, as the holder's own again.
    /// History records it in each change's `Carrel-Agent` trailer (FORMAT.md §8.2).
    pub fn act_as(&mut self, agent: Option<&str>) {
        self.agent = agent.map(str::to_owned);
    }

    /// The program acting for the holder, when one is ([`WriterLease::act_as`]).
    #[must_use]
    pub fn agent(&self) -> Option<&str> {
        self.agent.as_deref()
    }

    /// Confirms the lease is still this writer's: the lock file holds this lease's token and
    /// hasn't expired, nor has its hold on the `CloudKit` lock record.
    ///
    /// # Errors
    ///
    /// [`CoordinatorError::LeaseLost`] if not.
    pub fn check(&self) -> Result<(), CoordinatorError> {
        self.check_file()?;
        self.cloud.as_ref().map_or(Ok(()), cloud::CloudHold::check)
    }

    fn check_file(&self) -> Result<(), CoordinatorError> {
        let text = std::fs::read_to_string(&self.path).map_err(|_| CoordinatorError::LeaseLost)?;
        let current: LockInfo =
            serde_json::from_str(&text).map_err(|_| CoordinatorError::LeaseLost)?;
        if current.token != self.info.token || current.is_expired() {
            return Err(CoordinatorError::LeaseLost);
        }
        Ok(())
    }

    /// Extends the lease to `lease` from now, for long operations. In iCloud Drive, it also
    /// records the history head, so other devices wait for it.
    ///
    /// # Errors
    ///
    /// [`CoordinatorError::LeaseLost`] if the lease was already lost,
    /// [`CoordinatorError::Unavailable`] if `CloudKit` can't be reached, or an I/O error.
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
        rename_retrying(&temp, &self.path).map_err(io(&self.path))?;
        match &mut self.cloud {
            Some(cloud) => cloud.renew(lease),
            None => Ok(()),
        }
    }

    /// Gives up the lease. A lease that was already lost is left alone. In iCloud Drive, it
    /// records the history head first, so the next writer waits for it.
    ///
    /// # Errors
    ///
    /// [`CoordinatorError::Io`] if the lock can't be removed, or
    /// [`CoordinatorError::Unavailable`] if `CloudKit` can't be reached (the lease then runs
    /// out by itself).
    pub fn release(mut self) -> Result<(), CoordinatorError> {
        self.release_inner()
    }

    fn release_inner(&mut self) -> Result<(), CoordinatorError> {
        if self.released {
            return Ok(());
        }
        self.released = true;
        let cloud = self
            .cloud
            .as_mut()
            .map_or(Ok(()), cloud::CloudHold::release);
        if self.check_file().is_ok() {
            std::fs::remove_file(&self.path).map_err(io(&self.path))?;
        }
        cloud
    }
}

impl Drop for WriterLease {
    fn drop(&mut self) {
        let _ = self.release_inner();
    }
}

/// Renames `from` to `to`. On Windows a rename fails while another program (an antivirus
/// scanner, a search indexer, or a reader of the lock) has either file open, so it's retried
/// for up to a second.
fn rename_retrying(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut attempts = 0;
    loop {
        match std::fs::rename(from, to) {
            Err(e)
                if cfg!(windows)
                    && e.kind() == std::io::ErrorKind::PermissionDenied
                    && attempts < 20 =>
            {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            result => return result,
        }
    }
}
