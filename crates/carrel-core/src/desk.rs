//! Desk items (FORMAT.md §9.4, `docs/design/checkout.md`): files and folders being worked on
//! where they are, tracked without being archived. Each gets a card beside it, and this
//! device's `desk.json` (§7.10) lists them. Nothing is written inside them, and nothing is
//! moved.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::archive::Archive;
use crate::card::{Card, CardMarker, DateOrTime, Dates, Extra, Id, Sensitivity};
use crate::card_file::CardFile;
use crate::coordinator::WriterLease;
use crate::history::{Change, History, HistoryError, Operation};

/// One tracked desk item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeskEntry {
    /// Its card's identifier.
    pub id: Id,
    /// Where it is on this device.
    pub path: PathBuf,
    /// Where its card is: beside it, named after it with `.md` added.
    pub card: PathBuf,
    /// When it was added.
    pub added: DateOrTime,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A device's desk items: `_System/devices/<device-id>/desk.json`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Desk {
    /// The items, in the order they were added.
    #[serde(default)]
    pub items: Vec<DeskEntry>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// Why a desk item couldn't be added or removed.
#[derive(Debug)]
pub enum DeskError {
    /// Nothing is at the path.
    Missing(PathBuf),
    /// The path is inside the archive: it's an item already, or should be ingested.
    InArchive(PathBuf),
    /// It's already on the desk.
    Tracked(PathBuf),
    /// It isn't on the desk.
    NotTracked(PathBuf),
    /// Something already has the card's name.
    CardTaken(PathBuf),
    /// A file couldn't be read or written.
    Io {
        /// The path concerned.
        path: PathBuf,
        /// What went wrong.
        message: String,
    },
    /// The history couldn't be written.
    History(HistoryError),
}

impl std::fmt::Display for DeskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(p) => write!(f, "nothing is at {}", p.display()),
            Self::InArchive(p) => write!(
                f,
                "{} is inside the archive; ingest it instead",
                p.display()
            ),
            Self::Tracked(p) => write!(f, "{} is already on the desk", p.display()),
            Self::NotTracked(p) => write!(f, "{} isn't on the desk", p.display()),
            Self::CardTaken(p) => write!(
                f,
                "{} already exists, so its card can't go there",
                p.display()
            ),
            Self::Io { path, message } => write!(f, "{}: {message}", path.display()),
            Self::History(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DeskError {}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> DeskError + '_ {
    move |e| DeskError::Io {
        path: path.to_owned(),
        message: e.to_string(),
    }
}

impl Desk {
    /// Where `device`'s desk is kept.
    #[must_use]
    pub fn path(archive: &Archive, device: Id) -> PathBuf {
        archive
            .root()
            .join(format!("_System/devices/{device}/desk.json"))
    }

    /// `device`'s desk, or an empty one.
    ///
    /// # Errors
    ///
    /// [`DeskError::Io`] if it's there but can't be read.
    pub fn load(archive: &Archive, device: Id) -> Result<Self, DeskError> {
        let path = Self::path(archive, device);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| DeskError::Io {
                path,
                message: e.to_string(),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(io(&path)(e)),
        }
    }

    fn save(&self, archive: &Archive, device: Id) -> Result<(), DeskError> {
        let path = Self::path(archive, device);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io(parent))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| DeskError::Io {
            path: path.clone(),
            message: e.to_string(),
        })?;
        std::fs::write(&path, text + "\n").map_err(io(&path))
    }

    /// The entry for `path`, if it's on the desk.
    #[must_use]
    pub fn find(&self, path: &Path) -> Option<&DeskEntry> {
        self.items.iter().find(|e| e.path == path)
    }
}

/// The card's path for a desk item at `path`: beside it, with `.md` added.
#[must_use]
pub fn card_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".md");
    path.with_file_name(name)
}

/// Puts the file or folder at `path` on this device's desk (the lease's), holding `lease`:
/// writes a card beside it with `title` (its name if `None`) and `summary`, lists it in
/// `desk.json`, and commits that as one change.
///
/// # Errors
///
/// [`DeskError`] if it's missing, inside the archive, already on the desk, or its card's
/// name is taken.
pub fn add(
    archive: &Archive,
    path: &Path,
    title: Option<&str>,
    summary: Option<&str>,
    lease: &WriterLease,
) -> Result<DeskEntry, DeskError> {
    let path = std::fs::canonicalize(path).map_err(|_| DeskError::Missing(path.to_owned()))?;
    let root = std::fs::canonicalize(archive.root()).map_err(io(archive.root()))?;
    if path.starts_with(&root) {
        return Err(DeskError::InArchive(path));
    }
    let device = lease.info().device;
    let mut desk = Desk::load(archive, device)?;
    if desk.find(&path).is_some() {
        return Err(DeskError::Tracked(path));
    }
    let card = card_path(&path);
    if card.exists() {
        return Err(DeskError::CardTaken(card));
    }
    let detected = archive.detect(&path).map_err(|e| DeskError::Io {
        path: path.clone(),
        message: e.to_string(),
    })?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let id = Id::new_v7();
    let today = DateOrTime(jiff::Zoned::now().date().to_string());
    let card_value = Card {
        carrel: CardMarker::Card,
        format: crate::FORMAT_VERSION,
        id,
        kind: detected.kind,
        title: title.map_or_else(|| name.clone(), str::to_owned),
        item: Some(name),
        sealed: detected.sealed,
        allow_readme: None,
        summary: summary.map(str::to_owned),
        tags: Vec::new(),
        links: Vec::new(),
        dates: Some(Dates {
            added: Some(today.clone()),
            ..Dates::default()
        }),
        sensitivity: Sensitivity::Personal,
        checksum: None,
        size: None,
        manifest: None,
        location: None,
        url: None,
        checked_out: None,
        generated: None,
        placement: None,
        removed: None,
        expanded_from: None,
        extra: Extra::new(),
    };
    let file = CardFile::new(card_value, "").map_err(|e| DeskError::Io {
        path: card.clone(),
        message: e.to_string(),
    })?;
    let text = file.to_text().map_err(|e| DeskError::Io {
        path: card.clone(),
        message: e.to_string(),
    })?;
    std::fs::write(&card, text).map_err(io(&card))?;
    let entry = DeskEntry {
        id,
        path: path.clone(),
        card: card.clone(),
        added: DateOrTime(
            jiff::Zoned::now()
                .strftime("%Y-%m-%dT%H:%M:%S%:z")
                .to_string(),
        ),
        extra: Extra::new(),
    };
    desk.items.push(entry.clone());
    let result = desk.save(archive, device).and_then(|()| {
        let mut change = Change::new(
            Operation::Edit,
            format!("Put “{}” on the desk", file.card().title),
            lease.info().person,
            device,
        );
        change.items = vec![id];
        History::open(archive)
            .and_then(|h| h.commit(archive, &change, lease))
            .map_err(DeskError::History)
    });
    if let Err(e) = result {
        let _ = std::fs::remove_file(&card);
        desk.items.pop();
        let _ = desk.save(archive, device);
        return Err(e);
    }
    Ok(entry)
}

/// Takes `path` off this device's desk, holding `lease`. Its card stays beside it unless
/// `delete_card`; the item itself is never touched.
///
/// # Errors
///
/// [`DeskError::NotTracked`] if it isn't on the desk, or an error writing.
pub fn remove(
    archive: &Archive,
    path: &Path,
    delete_card: bool,
    lease: &WriterLease,
) -> Result<DeskEntry, DeskError> {
    let wanted = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let device = lease.info().device;
    let mut desk = Desk::load(archive, device)?;
    let Some(at) = desk.items.iter().position(|e| e.path == wanted) else {
        return Err(DeskError::NotTracked(wanted));
    };
    let entry = desk.items.remove(at);
    desk.save(archive, device)?;
    if delete_card {
        let _ = std::fs::remove_file(&entry.card);
    }
    let mut change = Change::new(
        Operation::Edit,
        format!(
            "Take {} off the desk",
            entry.path.file_name().unwrap_or_default().to_string_lossy()
        ),
        lease.info().person,
        device,
    );
    change.items = vec![entry.id];
    History::open(archive)
        .and_then(|h| h.commit(archive, &change, lease))
        .map_err(DeskError::History)?;
    Ok(entry)
}
