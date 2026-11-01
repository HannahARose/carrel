//! The holding folder (FORMAT.md §7.13, `docs/design/sync-and-mobile.md`): on a device without
//! Kopia, such as an iPad or iPhone, a change that replaces or removes an item's contents (a
//! return, an expanded compressed file) keeps the old version in `_System/holding/`, one
//! folder each, instead of dropping it. A Mac with Kopia snapshots the archive, holding folder
//! and all, and then clears it, so nothing is lost before a backup has it. Until then, Status
//! says how much is waiting, and restore can take a version back out.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::archive::Archive;
use crate::backup::{BackupError, Backups, Snapshot};
use crate::card::{Extra, Id};
use crate::coordinator::Holder;

/// The holding folder, relative to the archive root.
pub const HOLDING_DIR: &str = "_System/holding";

/// The file in each held version's folder that says what it is.
const HELD_FILE: &str = "held.json";

/// How much may wait before Status warns: 2 GB.
pub const LARGE: u64 = 2_000_000_000;

/// How many days a version may wait before Status warns.
pub const OLD_DAYS: i64 = 7;

/// Why a version was kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Why {
    /// A return replaced it with the working copy.
    Return,
    /// It was a compressed file, expanded into what it held.
    Expand,
}

impl Why {
    /// The reason in words, for people.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Return => "replaced by a return",
            Self::Expand => "expanded",
        }
    }
}

/// One kept version: `_System/holding/<id>/held.json`, with the version beside it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Held {
    /// Its identifier, which names its folder.
    pub id: Id,
    /// The item it was the contents of, when there's one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<Id>,
    /// The item's title, for people.
    pub title: String,
    /// Where it was, relative to the archive root.
    pub path: String,
    /// The version's name in its folder.
    pub name: String,
    /// Why it was kept.
    pub why: Why,
    /// Its size in bytes.
    pub size: u64,
    /// The person whose change kept it.
    pub person: Id,
    /// The device that kept it.
    pub device: Id,
    /// When (RFC 3339).
    pub at: String,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Held {
    /// Where the kept version is.
    #[must_use]
    pub fn content(&self, archive: &Archive) -> PathBuf {
        folder(archive, self.id).join(&self.name)
    }

    /// How long it has waited, in days.
    #[must_use]
    pub fn age_days(&self) -> i64 {
        self.at
            .parse::<jiff::Timestamp>()
            .ok()
            .and_then(|at| jiff::Timestamp::now().since(at).ok())
            .map_or(0, |span| span.get_seconds() / 86_400)
    }
}

/// What to say about a version being kept: what it belongs to and why.
#[derive(Debug, Clone)]
pub struct Keeping<'a> {
    /// The item, when there's one.
    pub item: Option<Id>,
    /// Its title.
    pub title: &'a str,
    /// Why it's kept.
    pub why: Why,
    /// The name to keep it under, when not its own, such as a temporary name's real one.
    pub name: Option<&'a str>,
}

fn folder(archive: &Archive, id: Id) -> PathBuf {
    archive.root().join(HOLDING_DIR).join(id.to_string())
}

/// Whether content a change replaces or removes is kept in the holding folder here: when this
/// device has no Kopia set up for `archive`, as on an iPad or iPhone, which can't run it.
#[must_use]
pub fn needed(archive: &Archive) -> bool {
    !kopia_here(archive)
}

#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
fn kopia_here(archive: &Archive) -> bool {
    crate::backup::Kopia::for_archive(archive).is_ok()
}

#[cfg(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
))]
fn kopia_here(_archive: &Archive) -> bool {
    false
}

/// Moves `path`, a file or folder inside `archive`, into the holding folder as `holder`'s,
/// with what `keeping` says about it. Returns what was recorded.
///
/// # Errors
///
/// An I/O error moving it or writing its record; then `path` is where it was.
pub fn keep(
    archive: &Archive,
    path: &Path,
    keeping: &Keeping<'_>,
    holder: Holder,
) -> std::io::Result<Held> {
    let id = Id::new_v7();
    let into = folder(archive, id);
    std::fs::create_dir_all(&into)?;
    let name = keeping.name.map_or_else(
        || {
            path.file_name().map_or_else(
                || "contents".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            )
        },
        str::to_owned,
    );
    let relative = path
        .with_file_name(&name)
        .strip_prefix(archive.root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let held = Held {
        id,
        item: keeping.item,
        title: keeping.title.to_owned(),
        path: relative,
        name: name.clone(),
        why: keeping.why,
        size: size_of(path),
        person: holder.person,
        device: holder.device,
        at: jiff::Timestamp::now().to_string(),
        extra: Extra::new(),
    };
    // A read-only archive copy (a checkout's lock) keeps its lock in the holding folder.
    let _ = crate::checkout::unlock(path);
    if let Err(e) = std::fs::rename(path, into.join(&name)) {
        let _ = std::fs::remove_dir_all(&into);
        return Err(e);
    }
    let text = serde_json::to_string_pretty(&held).map_err(std::io::Error::other)? + "\n";
    std::fs::write(into.join(HELD_FILE), text)?;
    Ok(held)
}

/// Moves `path` into the holding folder when this device needs it ([`needed`]), otherwise
/// leaves it to `otherwise`. Returns what was kept, if it was.
///
/// # Errors
///
/// What [`keep`] or `otherwise` returns.
pub fn keep_or<E: From<std::io::Error>>(
    archive: &Archive,
    path: &Path,
    keeping: &Keeping<'_>,
    holder: Holder,
    otherwise: impl FnOnce(&Path) -> Result<(), E>,
) -> Result<Option<Held>, E> {
    if needed(archive) {
        Ok(Some(keep(archive, path, keeping, holder)?))
    } else {
        otherwise(path).map(|()| None)
    }
}

fn size_of(path: &Path) -> u64 {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::read_dir(path).map_or(0, |entries| {
            entries.flatten().map(|e| size_of(&e.path())).sum()
        }),
        Ok(meta) => meta.len(),
        Err(_) => 0,
    }
}

/// Every kept version, oldest first. Folders without a readable record are left out.
#[must_use]
pub fn list(archive: &Archive) -> Vec<Held> {
    let Ok(folders) = std::fs::read_dir(archive.root().join(HOLDING_DIR)) else {
        return Vec::new();
    };
    let mut found: Vec<Held> = folders
        .flatten()
        .filter_map(|f| std::fs::read_to_string(f.path().join(HELD_FILE)).ok())
        .filter_map(|t| serde_json::from_str(&t).ok())
        .collect();
    found.sort_by_key(|h| h.id);
    found
}

/// The kept version whose identifier starts or ends with `id`, as a short identifier does.
///
/// # Errors
///
/// Why there's none, or more than one.
pub fn find(archive: &Archive, id: &str) -> Result<Held, String> {
    let all = list(archive);
    let matching: Vec<&Held> = all
        .iter()
        .filter(|h| {
            let full = h.id.to_string();
            full.starts_with(id.trim()) || full.ends_with(id.trim())
        })
        .collect();
    match matching.as_slice() {
        [held] => Ok((*held).clone()),
        [] => Err(format!("nothing in the holding folder is {id}")),
        _ => Err(format!(
            "several kept versions start with {id}; give more of it"
        )),
    }
}

/// What's waiting in the holding folder for a snapshot, for Status.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Waiting {
    /// How many versions.
    pub count: usize,
    /// Their size in bytes.
    pub size: u64,
    /// How many days the oldest has waited.
    pub oldest_days: Option<i64>,
    /// What's worth warning about: too much, or too old.
    pub warnings: Vec<String>,
}

/// What's waiting in `archive`'s holding folder.
#[must_use]
pub fn waiting(archive: &Archive) -> Waiting {
    let held = list(archive);
    let size = held.iter().map(|h| h.size).sum();
    let oldest_days = held.iter().map(Held::age_days).max();
    let mut warnings = Vec::new();
    if size >= LARGE {
        warnings.push(format!(
            "The holding folder has {} waiting for a snapshot; open Carrel on a Mac with backups to clear it.",
            crate::human_size(size)
        ));
    }
    if let Some(days) = oldest_days.filter(|d| *d >= OLD_DAYS) {
        warnings.push(format!(
            "A version has waited {days} days in the holding folder for a snapshot; open Carrel on a Mac with backups to clear it."
        ));
    }
    Waiting {
        count: held.len(),
        size,
        oldest_days,
        warnings,
    }
}

/// A free path beside the archive for `held`, restored: `NAME (held 2026-10-29)`, then
/// `NAME (held 2026-10-29, 2)` and so on.
#[must_use]
pub fn restore_beside(archive: &Archive, held: &Held) -> PathBuf {
    let parent = archive
        .root()
        .parent()
        .map_or_else(|| archive.root().to_owned(), Path::to_owned);
    let date = held.at.get(..10).unwrap_or_default();
    let mut target = parent.join(format!("{} (held {date})", held.name));
    let mut n = 2;
    while target.exists() {
        target = parent.join(format!("{} (held {date}, {n})", held.name));
        n += 1;
    }
    target
}

/// Copies the kept version `held` to `to`, which must not exist yet. Nothing in the archive
/// changes; the version stays held until a snapshot clears it.
///
/// # Errors
///
/// An I/O error, or [`std::io::ErrorKind::AlreadyExists`] if `to` exists.
pub fn restore(archive: &Archive, held: &Held, to: &Path) -> std::io::Result<()> {
    if to.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("{} already exists", to.display()),
        ));
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::ingest::filing::copy(&held.content(archive), to)
        .map_err(|e| std::io::Error::other(e.to_string()))
}

/// What clearing the holding folder did.
#[derive(Debug, Clone, Default)]
pub struct Cleared {
    /// The snapshot that took them, when there was anything to take.
    pub snapshot: Option<Snapshot>,
    /// The versions cleared.
    pub cleared: Vec<Held>,
}

/// Snapshots `archive` with `backups`, holding folder and all, then clears the versions that
/// were held before the snapshot started. Nothing happens when nothing is held.
///
/// # Errors
///
/// [`BackupError`] if the snapshot can't be taken; then nothing is cleared.
pub fn snapshot_and_clear(
    archive: &Archive,
    backups: &dyn Backups,
) -> Result<Cleared, BackupError> {
    let held = list(archive);
    if held.is_empty() {
        return Ok(Cleared::default());
    }
    let snapshot = backups.snapshot(
        archive.root(),
        &format!(
            "The holding folder: {} kept version{}",
            held.len(),
            if held.len() == 1 { "" } else { "s" }
        ),
    )?;
    Ok(Cleared {
        snapshot: Some(snapshot),
        cleared: clear(archive, held),
    })
}

/// Removes `held` from the holding folder, once a snapshot has them. Returns those removed.
#[must_use]
pub fn clear(archive: &Archive, held: Vec<Held>) -> Vec<Held> {
    held.into_iter()
        .filter(|h| {
            let path = folder(archive, h.id);
            let _ = crate::checkout::unlock(&h.content(archive));
            std::fs::remove_dir_all(&path).is_ok() || !path.exists()
        })
        .collect()
}
