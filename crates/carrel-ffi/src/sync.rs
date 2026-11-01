//! An archive shared between devices through iCloud Drive (`docs/design/sync-and-mobile.md`),
//! as `carrel sync` offers it: the sync status, downloading evicted contents, a collection's
//! `sync.keep_downloaded`, settling conflicted copies, and moving the archive into iCloud
//! Drive. The apps register their file-coordination provider with [`set_on_demand_files`].

use std::path::Path;
use std::sync::Arc;

use carrel_core::cloud_files::{self, DownloadState, Keep};
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::icloud_move::{self, MovePlan};

use crate::{ArchiveHandle, CarrelError, ProgressListener};

/// Whether a file's contents are on this device, as `NSURLUbiquitousItemDownloadingStatus`
/// and `NSURLUbiquitousItemIsDownloading` say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FileDownloadState {
    /// Its contents are here (`current`, or `downloaded`).
    Downloaded,
    /// Only a placeholder is here (`notDownloaded`).
    NotDownloaded,
    /// It is being downloaded.
    Downloading,
    /// Nothing is there.
    Missing,
}

impl From<FileDownloadState> for DownloadState {
    fn from(state: FileDownloadState) -> Self {
        match state {
            FileDownloadState::Downloaded => Self::Downloaded,
            FileDownloadState::NotDownloaded => Self::NotDownloaded,
            FileDownloadState::Downloading => Self::Downloading,
            FileDownloadState::Missing => Self::Missing,
        }
    }
}

/// iCloud Drive's on-demand files, implemented in Swift with `NSURL` resource values and
/// `NSFileCoordinator`. Paths are the files' own, never their placeholders'.
#[uniffi::export(with_foreign)]
pub trait OnDemandFilesProvider: Send + Sync {
    /// Whether the file at `path` is downloaded.
    fn state(&self, path: String) -> FileDownloadState;

    /// Downloads the file at `path` through file coordination, returning when it's here.
    ///
    /// # Errors
    ///
    /// Why it couldn't be downloaded, as [`CarrelError::Callback`].
    fn download(&self, path: String) -> Result<(), CarrelError>;
}

/// The Swift provider as the core's trait.
struct Foreign(Arc<dyn OnDemandFilesProvider>);

impl cloud_files::OnDemandFiles for Foreign {
    fn state(&self, path: &Path) -> DownloadState {
        self.0.state(path.to_string_lossy().into_owned()).into()
    }

    fn download(&self, path: &Path) -> std::io::Result<()> {
        self.0
            .download(path.to_string_lossy().into_owned())
            .map_err(|e| std::io::Error::other(e.to_string()))
    }
}

/// Uses `provider` for every archive from now on, to tell whether files are downloaded and
/// download them, in place of the core's own placeholder checks.
#[uniffi::export]
pub fn set_on_demand_files(provider: Arc<dyn OnDemandFilesProvider>) {
    cloud_files::set_provider(Arc::new(Foreign(provider)));
}

/// One device's inbox backlog.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct InboxBacklog {
    /// The device's identifier.
    pub device: String,
    /// Whether it's this device.
    pub this_device: bool,
    /// How many entries wait to be applied.
    pub waiting: u64,
}

/// A conflicted copy, for the owner to settle.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ConflictedCopy {
    /// The copy iCloud made, relative to the archive root.
    pub copy: String,
    /// The file it's a version of.
    pub original: String,
}

/// The writer lock, as the sync status shows it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WriterLock {
    /// The device holding it.
    pub device: String,
    /// Whether it's this device.
    pub this_device: bool,
    /// What it's doing.
    pub purpose: String,
    /// Until when, as RFC 3339.
    pub expires: String,
    /// Whether the lease has run out.
    pub expired: bool,
}

/// Everything the sync status shows, as `carrel sync status` does.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SyncStatus {
    /// Whether the archive is in a folder a sync service shares.
    pub synced: bool,
    /// Who holds the writer lock, if anyone.
    pub writer: Option<WriterLock>,
    /// Each device's inbox with entries waiting.
    pub inboxes: Vec<InboxBacklog>,
    /// Files in the holding folder waiting for a snapshot.
    pub holding_files: u64,
    /// Their size in bytes.
    pub holding_bytes: u64,
    /// When the oldest was put there, as `YYYY-MM-DD`.
    pub holding_oldest: Option<String>,
    /// Whether the holding folder is large or old enough to warn about.
    pub holding_warn: bool,
    /// Re-checks waiting for a stronger device.
    pub rechecks: u64,
    /// The files not downloaded here, relative to the archive root.
    pub not_downloaded: Vec<String>,
    /// Their size in bytes, where known.
    pub not_downloaded_bytes: u64,
    /// The collections kept downloaded.
    pub kept_downloaded: Vec<String>,
    /// Conflicted copies waiting for the owner.
    pub conflicts: Vec<ConflictedCopy>,
    /// Where the archive was moved, when this is the old copy left behind.
    pub moved_to: Option<String>,
}

/// A move into iCloud Drive, checked and ready to run.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ArchiveMove {
    /// The archive now.
    pub from: String,
    /// Where it goes.
    pub to: String,
    /// How many files it has.
    pub files: u64,
    /// Their size in bytes.
    pub bytes: u64,
    /// The bytes free at the destination, where known.
    pub free: Option<u64>,
    /// Whether the destination is in iCloud Drive or another synced folder.
    pub synced: bool,
}

impl From<MovePlan> for ArchiveMove {
    fn from(plan: MovePlan) -> Self {
        Self {
            from: plan.from.to_string_lossy().into_owned(),
            to: plan.to.to_string_lossy().into_owned(),
            files: plan.files as u64,
            bytes: plan.bytes,
            free: plan.free,
            synced: plan.synced,
        }
    }
}

/// The folder an archive would move to by default: a folder of its name in iCloud Drive, if
/// iCloud Drive is turned on.
#[uniffi::export]
#[must_use]
pub fn default_icloud_destination(archive_root: String) -> Option<String> {
    let name = Path::new(&archive_root).file_name()?.to_owned();
    Some(
        icloud_move::icloud_drive()?
            .join(name)
            .to_string_lossy()
            .into_owned(),
    )
}

impl ArchiveHandle {
    fn lease(&self, purpose: &str) -> Result<carrel_core::coordinator::WriterLease, CarrelError> {
        let archive = self.fresh()?;
        Ok(DiskLock::for_archive(&archive).acquire(self.holder()?, purpose, DEFAULT_LEASE)?)
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// The sync status, as `carrel sync status` shows it.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive can't be read.
    pub fn sync_status(&self) -> Result<SyncStatus, CarrelError> {
        let archive = self.fresh()?;
        let me = self.identity().ok().map(|i| i.device.to_string());
        let status = carrel_core::sync_status::sync_status(&archive);
        Ok(SyncStatus {
            synced: status.synced,
            writer: status.writer.map(|w| WriterLock {
                this_device: me.as_deref() == Some(w.device.to_string().as_str()),
                device: w.device.to_string(),
                expired: w.is_expired(),
                purpose: w.purpose,
                expires: w.expires,
            }),
            inboxes: status
                .inboxes
                .into_iter()
                .map(|i| InboxBacklog {
                    this_device: me.as_deref() == Some(i.device.as_str()),
                    device: i.device,
                    waiting: i.waiting as u64,
                })
                .collect(),
            holding_files: status.holding.files as u64,
            holding_bytes: status.holding.bytes,
            holding_oldest: status.holding.oldest,
            holding_warn: status.holding.warn,
            rechecks: status.rechecks as u64,
            not_downloaded: status.not_downloaded.files,
            not_downloaded_bytes: status.not_downloaded.bytes,
            kept_downloaded: status.kept,
            conflicts: status
                .conflicts
                .into_iter()
                .map(|c| ConflictedCopy {
                    copy: c.copy,
                    original: c.original,
                })
                .collect(),
            moved_to: icloud_move::moved_to(archive.root())
                .map(|p| p.to_string_lossy().into_owned()),
        })
    }

    /// Downloads the item `item`'s (its identifier) contents if iCloud evicted any, before
    /// opening, previewing or sharing it, returning their path.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for an unknown or card-only item, or
    /// [`CarrelError::Failed`] if it can't be downloaded.
    pub fn download_item(&self, item: String) -> Result<String, CarrelError> {
        let archive = self.fresh()?;
        let found = Self::item(&archive, &item)?;
        let contents = found
            .item
            .ok_or_else(|| CarrelError::invalid("the item is only a card"))?;
        let path = archive.root().join(contents);
        cloud_files::ensure_downloaded(&path)?;
        Ok(path.to_string_lossy().into_owned())
    }

    /// Downloads everything in the collection at `path` that iCloud evicted. Returns how many
    /// files it downloaded.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if a file can't be downloaded.
    pub fn download_collection(&self, path: String) -> Result<u64, CarrelError> {
        let archive = self.fresh()?;
        let folder = archive.root().join(path.trim().trim_matches('/'));
        let waiting = cloud_files::not_downloaded_in(&folder, cloud_files::provider().as_ref());
        cloud_files::ensure_downloaded(&folder)?;
        Ok(waiting.len() as u64)
    }

    /// Whether the collection at `path` keeps its items downloaded on every device: its own
    /// `sync.keep_downloaded`, else the collection above's.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive can't be read.
    pub fn keeps_downloaded(&self, path: String) -> Result<bool, CarrelError> {
        let collections = self.fresh()?.collections()?;
        Ok(cloud_files::keeps_downloaded(&collections, &path))
    }

    /// Sets the collection at `path`'s `sync.keep_downloaded`, or clears it with `None` so the
    /// collection above decides, as one change undo can reverse; then downloads what it now
    /// keeps. Returns the history commit, or `None` when nothing changed.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`] for an unknown collection or a failed write.
    pub fn set_keep_downloaded(
        &self,
        path: String,
        on: Option<bool>,
    ) -> Result<Option<String>, CarrelError> {
        let lease = self.lease("sync.keep_downloaded")?;
        let commit = cloud_files::set_keep_downloaded(&self.fresh()?, &path, on, &lease)?;
        drop(lease);
        if on == Some(true) {
            let _ = cloud_files::keep_downloaded_pass(&self.fresh()?);
        }
        Ok(commit)
    }

    /// Settles the conflicted copy `copy` by keeping the copy (`keep_copy`) or the original;
    /// the other goes to the Trash. Returns the history commit, if any.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`] if `copy` isn't a conflicted copy or can't be moved.
    pub fn resolve_conflict(
        &self,
        copy: String,
        keep_copy: bool,
    ) -> Result<Option<String>, CarrelError> {
        let lease = self.lease("resolve")?;
        let keep = if keep_copy {
            Keep::Copy
        } else {
            Keep::Original
        };
        Ok(cloud_files::resolve(&self.fresh()?, &copy, keep, &lease)?)
    }

    /// Checks that the archive can move to `to`: nothing is there, and there's room.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] saying why it can't.
    pub fn plan_icloud_move(&self, to: String) -> Result<ArchiveMove, CarrelError> {
        Ok(icloud_move::plan_move(&self.fresh()?, Path::new(&to))?.into())
    }

    /// Moves the archive to `to`: copies it, holding the writer lease, reporting bytes to
    /// `progress`, checks the copy against the original, then makes the copy this device's
    /// archive. The old copy stays until the owner removes it. Returns the new root; open it
    /// in place of this handle.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`] if it can't be copied or the copy doesn't match.
    pub fn move_into_icloud(
        &self,
        to: String,
        progress: Arc<dyn ProgressListener>,
    ) -> Result<String, CarrelError> {
        let plan = icloud_move::plan_move(&self.fresh()?, Path::new(&to))?;
        let lease = self.lease("move")?;
        icloud_move::copy_archive(&plan, &lease, &mut |done, total| {
            progress.update(done, total, None);
        })?;
        drop(lease);
        icloud_move::switch_over(&plan, &self.config_dir()?)?;
        Ok(plan.to.to_string_lossy().into_owned())
    }
}
