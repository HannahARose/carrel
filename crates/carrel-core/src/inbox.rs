//! Inboxes (FORMAT.md §7.8): how a device that isn't the writer adds to the archive. When
//! another device holds the writer lease, what's shared or added here, such as from a Mac's
//! share sheet, goes into this device's own inbox, `_System/inbox/<device-id>/`, as one JSON
//! file each with a copy of any files beside it. The writer applies every device's inbox in
//! timestamp order: each entry joins its own intake queue, to be drafted and reviewed like
//! anything else, and the whole is recorded as one `inbox` change
//! (`docs/design/macos-integration.md`, decision 4).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::actions::ActionError;
use crate::archive::Archive;
use crate::card::{Extra, Id};
use crate::coordinator::{Coordinator as _, DiskLock, Holder, LockInfo, WriterLease};
use crate::history::{Change, History, HistoryError, Operation};
use crate::ingest::IntakeOptions;
use crate::intake::{self, IntakeEntry};

/// The inboxes' folder, relative to the archive root.
pub const INBOX_DIR: &str = "_System/inbox";

/// What an inbox entry asks the writer to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InboxAction {
    /// Add the files kept beside the entry to the intake queue.
    Add,
    /// Add a web address to the intake queue, as a `url` item.
    AddUrl,
}

/// One request from a device that isn't the writer: `_System/inbox/<device>/<name>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxEntry {
    /// What it asks for.
    pub action: InboxAction,
    /// For `add`, the files' names in the folder beside the entry, named as it is without
    /// `.json`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// For `add`, whether each folder's entries are added one by one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub each: bool,
    /// For `add-url`, the address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// For `add-url`, the title it came with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The owner's note about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The person who asked.
    pub person: Id,
    /// The device it came from.
    pub device: Id,
    /// When it was asked (RFC 3339).
    pub at: String,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// The writer's lease, when a device other than `device` holds it and it hasn't run out: then
/// what `device` adds goes into its inbox.
#[must_use]
pub fn writer_elsewhere(archive: &Archive, device: Id) -> Option<LockInfo> {
    DiskLock::for_archive(archive)
        .current()
        .ok()
        .flatten()
        .filter(|lock| lock.device != device && !lock.is_expired())
}

/// `holder`'s device's inbox folder in `archive`.
#[must_use]
pub fn folder(archive: &Archive, device: Id) -> PathBuf {
    archive.root().join(INBOX_DIR).join(device.to_string())
}

/// A new entry's file name, without `.json`: `<timestamp>-<uuid>`, so names sort in the
/// order they were made.
fn new_name() -> String {
    format!(
        "{}-{}",
        jiff::Timestamp::now().strftime("%Y%m%dT%H%M%S%.6fZ"),
        Id::new_v7()
    )
}

/// Writes `entry` as `name` in its device's inbox.
fn save(archive: &Archive, name: &str, entry: &InboxEntry) -> Result<PathBuf, ActionError> {
    let folder = folder(archive, entry.device);
    std::fs::create_dir_all(&folder).map_err(|e| ActionError::Io {
        path: folder.clone(),
        source: e,
    })?;
    let path = folder.join(format!("{name}.json"));
    let text = serde_json::to_string_pretty(entry).map_err(std::io::Error::other);
    let temporary = folder.join(format!(".{name}.json.new"));
    text.and_then(|t| std::fs::write(&temporary, t + "\n"))
        .and_then(|()| std::fs::rename(&temporary, &path))
        .map_err(|e| ActionError::Io {
            path: path.clone(),
            source: e,
        })?;
    Ok(path)
}

fn entry(action: InboxAction, holder: Holder, note: Option<&str>) -> InboxEntry {
    InboxEntry {
        action,
        files: Vec::new(),
        each: false,
        url: None,
        title: None,
        note: note
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_owned),
        person: holder.person,
        device: holder.device,
        at: jiff::Zoned::now().to_string(),
        extra: Extra::new(),
    }
}

/// Puts copies of `paths` in `holder`'s inbox, for the writer to add, with the owner's `note`.
/// Folders go in whole unless `each`. Returns the entry's file.
///
/// # Errors
///
/// [`ActionError::Io`] if a path can't be copied or the entry written.
pub fn send_paths(
    archive: &Archive,
    holder: Holder,
    paths: &[PathBuf],
    each: bool,
    note: Option<&str>,
) -> Result<PathBuf, ActionError> {
    let name = new_name();
    let files = folder(archive, holder.device).join(&name);
    std::fs::create_dir_all(&files).map_err(|e| ActionError::Io {
        path: files.clone(),
        source: e,
    })?;
    let mut entry = entry(InboxAction::Add, holder, note);
    entry.each = each;
    for path in paths {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| ActionError::Invalid(format!("{} has no name", path.display())))?;
        let taken =
            crate::ingest::filing::free_name(&files, &file_name).map_err(|e| ActionError::Io {
                path: files.clone(),
                source: e,
            })?;
        copy(path, &files.join(&taken)).map_err(|e| ActionError::Io {
            path: path.clone(),
            source: e,
        })?;
        entry.files.push(taken);
    }
    save(archive, &name, &entry)
}

/// Puts the web address `url` in `holder`'s inbox, for the writer to add as a `url` item.
///
/// # Errors
///
/// [`ActionError::Io`] if the entry can't be written.
pub fn send_url(
    archive: &Archive,
    holder: Holder,
    url: &str,
    title: Option<&str>,
    note: Option<&str>,
) -> Result<PathBuf, ActionError> {
    let mut entry = entry(InboxAction::AddUrl, holder, note);
    entry.url = Some(url.trim().to_owned());
    entry.title = title
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned);
    save(archive, &new_name(), &entry)
}

/// Every device's waiting inbox entries, in the order they were made: each file and what it
/// says. Files that can't be read are left out, for the owner to look at.
#[must_use]
pub fn waiting(archive: &Archive) -> Vec<(PathBuf, InboxEntry)> {
    let mut found = Vec::new();
    let Ok(devices) = std::fs::read_dir(archive.root().join(INBOX_DIR)) else {
        return found;
    };
    for device in devices.flatten().filter(|d| d.path().is_dir()) {
        let Ok(files) = std::fs::read_dir(device.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            let named = path.extension().is_some_and(|e| e == "json")
                && !file.file_name().to_string_lossy().starts_with('.');
            let read = named
                .then(|| std::fs::read_to_string(&path).ok())
                .flatten()
                .and_then(|t| serde_json::from_str::<InboxEntry>(&t).ok());
            if let Some(entry) = read {
                found.push((path, entry));
            }
        }
    }
    found.sort_by(|a, b| a.0.file_name().cmp(&b.0.file_name()));
    found
}

/// What applying the inboxes did.
#[derive(Debug, Default)]
pub struct Applied {
    /// The intake entries added, on the writer's device.
    pub queued: Vec<IntakeEntry>,
    /// How many inbox entries were applied.
    pub entries: usize,
    /// Entries that couldn't be applied, and why; they stay in their inbox.
    pub problems: Vec<(PathBuf, String)>,
    /// The history commit recording it, if anything was applied.
    pub commit: Option<String>,
}

/// Applies every device's inbox, holding `lease`: each entry joins this device's intake
/// queue, its files moved into `received` (a folder of this device's, outside the archive) to
/// be ingested from there, and is then removed from its inbox. The whole is one `inbox`
/// change.
///
/// # Errors
///
/// [`HistoryError`] if the change can't be recorded.
pub fn apply(
    archive: &Archive,
    lease: &WriterLease,
    received: &Path,
) -> Result<Applied, HistoryError> {
    let device = lease.info().device;
    let mut applied = Applied::default();
    for (path, entry) in waiting(archive) {
        match apply_one(archive, &path, &entry, device, received) {
            Ok(queued) => {
                applied.queued.extend(queued);
                applied.entries += 1;
                let _ = std::fs::remove_file(&path);
                let _ = std::fs::remove_dir_all(path.with_extension(""));
            }
            Err(problem) => applied.problems.push((path, problem)),
        }
    }
    if applied.entries > 0 {
        let summary = format!(
            "Apply {} inbox {}",
            applied.entries,
            if applied.entries == 1 {
                "entry"
            } else {
                "entries"
            }
        );
        let change = Change::new(
            Operation::Inbox,
            summary,
            lease.info().person,
            lease.info().device,
        );
        applied.commit = History::open(archive)?.commit(archive, &change, lease)?;
    }
    Ok(applied)
}

/// Adds one inbox entry to `device`'s intake queue.
fn apply_one(
    archive: &Archive,
    path: &Path,
    entry: &InboxEntry,
    device: Id,
    received: &Path,
) -> Result<Vec<IntakeEntry>, String> {
    match entry.action {
        InboxAction::AddUrl => {
            let url = entry.url.as_deref().ok_or("it has no address")?;
            let added = intake::add_url(
                archive,
                url,
                entry.title.as_deref(),
                entry.note.as_deref(),
                device,
            )
            .map_err(|e| e.to_string())?;
            Ok(added.into_iter().collect())
        }
        InboxAction::Add => {
            let files = path.with_extension("");
            let target = received.join(files.file_name().unwrap_or_default());
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            let mut paths = Vec::new();
            for name in &entry.files {
                let to = target.join(name);
                move_or_copy(&files.join(name), &to).map_err(|e| format!("{name}: {e}"))?;
                paths.push(to);
            }
            let options = IntakeOptions {
                each: entry.each,
                whole: !entry.each,
            };
            let queued =
                intake::add_with_note(archive, &paths, options, device, entry.note.as_deref())
                    .map_err(|e| e.to_string())?;
            Ok(queued.added)
        }
    }
}

/// Copies the file or folder at `from` to `to`.
fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(drop)
    }
}

/// Moves `from` to `to`, copying it across volumes.
fn move_or_copy(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy(from, to)
}
