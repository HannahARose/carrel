//! What `carrel sync status` and Status's Sync section show about an archive shared between
//! devices (`docs/design/sync-and-mobile.md`): who holds the writer lock and until when, each
//! device's inbox backlog, what waits in the holding folder for a snapshot, the re-checks
//! waiting for a stronger device, the files not downloaded here, and conflicted copies. Each
//! interface words it in its own way.

use std::path::Path;

use crate::archive::Archive;
use crate::cloud_files::{ConflictedCopy, NotDownloaded};
use crate::coordinator::{Coordinator as _, DiskLock, LockInfo};

/// The holding folder, where devices without Kopia keep replaced content until a Mac
/// snapshots it (#470).
pub const HOLDING_DIR: &str = "_System/holding";

/// The re-check queue, where work done below the preferred level waits for a stronger device
/// to claim it.
pub const RECHECKS_DIR: &str = "_System/tasks";

/// The holding folder is worth a warning when it holds more than this many bytes…
pub const HOLDING_WARN_BYTES: u64 = 1024 * 1024 * 1024;

/// …or something has waited there longer than this many days.
pub const HOLDING_WARN_DAYS: i64 = 7;

/// One device's inbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxBacklog {
    /// The device's identifier, as its inbox folder is named.
    pub device: String,
    /// How many entries wait to be applied.
    pub waiting: usize,
}

/// What waits in the holding folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Holding {
    /// How many files.
    pub files: usize,
    /// Their size in bytes.
    pub bytes: u64,
    /// When the oldest was put there, as `YYYY-MM-DD`.
    pub oldest: Option<String>,
    /// Whether it's large or old enough to warn about.
    pub warn: bool,
}

/// Everything the sync status shows.
#[derive(Debug, Clone)]
pub struct SyncStatus {
    /// Whether the archive is in a folder a sync service shares between devices.
    pub synced: bool,
    /// Who holds the writer lock, even if it has expired.
    pub writer: Option<LockInfo>,
    /// Each device's inbox with entries waiting, by device.
    pub inboxes: Vec<InboxBacklog>,
    /// The holding folder.
    pub holding: Holding,
    /// How many re-checks wait for a stronger device.
    pub rechecks: usize,
    /// The files not downloaded on this device.
    pub not_downloaded: NotDownloaded,
    /// Conflicted copies waiting for the owner to choose.
    pub conflicts: Vec<ConflictedCopy>,
    /// The collections kept downloaded on every device (`sync.keep_downloaded`).
    pub kept: Vec<String>,
}

/// The sync status of `archive`.
#[must_use]
pub fn sync_status(archive: &Archive) -> SyncStatus {
    let root = archive.root();
    let collections = archive.collections().unwrap_or_default();
    SyncStatus {
        synced: DiskLock::is_synced_folder(root),
        // The disk lock until the CloudKit coordinator arrives (#466).
        writer: DiskLock::for_archive(archive).current().ok().flatten(),
        inboxes: inboxes(archive),
        holding: holding(&root.join(HOLDING_DIR)),
        rechecks: count_files(&root.join(RECHECKS_DIR), Some("json")),
        not_downloaded: crate::cloud_files::not_downloaded(root),
        conflicts: crate::cloud_files::conflicts(root),
        kept: collections
            .iter()
            .filter(|c| crate::cloud_files::keeps_downloaded(&collections, &c.path))
            .map(|c| c.path.clone())
            .collect(),
    }
}

/// Each device's waiting inbox entries, counted by their files in
/// `_System/inbox/<device>/`, including actions this version can't read yet, which still wait
/// for a writer that can.
fn inboxes(archive: &Archive) -> Vec<InboxBacklog> {
    let Ok(devices) = std::fs::read_dir(archive.root().join(crate::inbox::INBOX_DIR)) else {
        return Vec::new();
    };
    let mut found: Vec<InboxBacklog> = devices
        .flatten()
        .filter(|d| d.path().is_dir())
        .map(|d| InboxBacklog {
            device: d.file_name().to_string_lossy().into_owned(),
            waiting: count_files(&d.path(), Some("json")),
        })
        .filter(|i| i.waiting > 0)
        .collect();
    found.sort_by(|a, b| a.device.cmp(&b.device));
    found
}

/// What's in the holding folder at `dir`.
fn holding(dir: &Path) -> Holding {
    let mut held = Holding::default();
    let mut oldest: Option<std::time::SystemTime> = None;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                stack.push(entry.path());
                continue;
            }
            if crate::detect::is_ignored(&entry.file_name().to_string_lossy()) {
                continue;
            }
            held.files += 1;
            held.bytes += meta.len();
            if let Ok(modified) = meta.modified() {
                oldest = Some(oldest.map_or(modified, |o| o.min(modified)));
            }
        }
    }
    let age_days = oldest.and_then(|o| o.elapsed().ok()).map_or(0, |d| {
        i64::try_from(d.as_secs() / 86_400).unwrap_or(i64::MAX)
    });
    held.oldest = oldest.map(|o| crate::manifest::timestamp(o).0[..10].to_owned());
    held.warn = held.bytes > HOLDING_WARN_BYTES || age_days > HOLDING_WARN_DAYS;
    held
}

/// How many files are in `dir` and the folders in it, with extension `ext` if given.
fn count_files(dir: &Path, ext: Option<&str>) -> usize {
    let mut count = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if ext.is_none_or(|ext| path.extension().is_some_and(|e| e == ext))
                && !entry.file_name().to_string_lossy().starts_with('.')
            {
                count += 1;
            }
        }
    }
    count
}
