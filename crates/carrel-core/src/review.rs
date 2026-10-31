//! The review queue: proposals to file items, waiting for the owner (FORMAT.md §7.9, and
//! `docs/design/ingest.md`).
//!
//! Each proposal is one JSON file in `_System/queue/review/`, so proposals made on different
//! devices never conflict. Nothing is moved into a collection until the owner approves a
//! proposal; the proposal only describes what would happen.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::FormatVersion;
use crate::archive::Archive;
use crate::card::{
    Checksum, DateOrTime, Dates, Extra, Id, Kind, Provenance, Sealing, Sensitivity, Tag,
};

/// Where review entries live, relative to the archive root.
pub const REVIEW_DIR: &str = "_System/queue/review";

/// The document type of a review entry; always `review`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewMarker {
    /// A review entry.
    Review,
}

/// Where a proposal stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    /// Waiting for the owner.
    Proposed,
    /// Something couldn't be settled; see the entry's problems.
    NeedsOwner,
    /// Approved, to be filed where proposed.
    Approved,
    /// To go to Holding.
    Held,
    /// To stay where it is.
    Rejected,
    /// Filed.
    Archived,
}

impl State {
    /// The name used in entries, such as `needs-owner`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::NeedsOwner => "needs-owner",
            Self::Approved => "approved",
            Self::Held => "held",
            Self::Rejected => "rejected",
            Self::Archived => "archived",
        }
    }

    /// Whether the owner still has to decide.
    #[must_use]
    pub fn is_open(self) -> bool {
        matches!(self, Self::Proposed | Self::NeedsOwner)
    }
}

/// Where the item is now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// An absolute path on `device`.
    pub path: PathBuf,
    /// The device the path is on.
    pub device: Id,
    /// The file's checksum, or a folder's manifest content checksum.
    pub checksum: Checksum,
    /// The size in bytes, or a folder's total.
    pub size: u64,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// The card fields a proposal would write. Fields programs fill in when filing (`id`, `item`,
/// `checksum`, `size`, `manifest`) aren't here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardDraft {
    /// The title.
    pub title: String,
    /// A short summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Tags, from the destination's vocabulary or `proposed_tags`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
    /// Dates associated with the item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dates: Option<Dates>,
    /// How sensitive the item is.
    pub sensitivity: Sensitivity,
    /// How program-written fields were produced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated: Option<Provenance>,
    /// Links to other items, such as `version-of` for a likely duplicate the owner files as a
    /// new version. Until the item is filed, a link's `path` is relative to the archive root,
    /// since where the card will be isn't settled.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<crate::card::Link>,
    /// Other card fields.
    #[serde(flatten)]
    pub extra: Extra,
}

/// How sure the proposal is about its destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// The passes disagreed, the model chose Holding, or validation needed a retry.
    Low,
    /// A model chose it and a second pass agreed.
    Medium,
    /// A routing rule decided.
    High,
}

/// Where the item would go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destination {
    /// The collection.
    pub collection: Id,
    /// Its path when proposed, for people; the identifier is authoritative.
    pub path: String,
    /// How sure the proposal is.
    pub confidence: Confidence,
    /// Why it was chosen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    /// The routing rule that decided, if one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// Something validation or the owner needs to settle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    /// The pipeline stage, such as `route`.
    pub stage: String,
    /// What's wrong, in plain words.
    pub message: String,
}

/// One provider call made for a proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    /// The pipeline stage.
    pub stage: String,
    /// The provider's name.
    pub provider: String,
    /// The offer used.
    pub offer: String,
    /// `ok`, `unsupported`, `unavailable` or `failed`.
    pub result: String,
    /// What went wrong, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// A proposal to file one item: `_System/queue/review/<id>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewEntry {
    /// Document type; always `review`.
    pub carrel: ReviewMarker,
    /// The format version it was written with.
    pub format: FormatVersion,
    /// The entry's identifier.
    pub id: Id,
    /// The ingest batch it came from.
    pub batch: Id,
    /// When it was proposed.
    pub created: DateOrTime,
    /// Where it stands.
    pub state: State,
    /// Where the item is now.
    pub source: Source,
    /// The item's kind: `file`, `folder` or `bundle`.
    pub kind: Kind,
    /// For bundles, how they're sealed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed: Option<Sealing>,
    /// An archived item with the same checksum.
    #[serde(default)]
    pub duplicate_of: Option<Id>,
    /// Archived items it's likely a duplicate of, and why (`docs/design/duplicates.md`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub likely_duplicates: Vec<crate::ingest::likely::Likely>,
    /// The card it would get.
    pub card: CardDraft,
    /// At most one new tag for the destination's vocabulary.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proposed_tags: Vec<Tag>,
    /// Where it would go.
    pub destination: Destination,
    /// What needs settling.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<Problem>,
    /// Every provider call made for it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<Attempt>,
    /// Why the card's sensitivity is what it is, when it's sensitive or restricted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sensitivity_reasons: Vec<String>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// Why a queue couldn't be read or written.
#[derive(Debug)]
pub enum ReviewError {
    /// A file operation failed.
    Io {
        /// The path concerned.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// An entry isn't valid.
    Invalid {
        /// The entry's path.
        path: PathBuf,
        /// What's wrong.
        message: String,
    },
    /// No entry matches: what kind of entry, and the identifier given.
    NotFound(&'static str, String),
    /// Several entries match the identifier given: what kind, and the identifier.
    Ambiguous(&'static str, String),
    /// The archive uses a newer format and must not be changed.
    ReadOnly,
}

impl std::fmt::Display for ReviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Invalid { path, message } => write!(f, "{}: {message}", path.display()),
            Self::NotFound(what, id) => write!(f, "no {what} {id}"),
            Self::Ambiguous(what, id) => {
                write!(f, "several {what}s match {id}; give more of it")
            }
            Self::ReadOnly => f.write_str("the archive uses a newer format and is read-only"),
        }
    }
}

impl std::error::Error for ReviewError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ReviewError + '_ {
    move |source| ReviewError::Io {
        path: path.to_owned(),
        source,
    }
}

/// An entry kept in one of the queues in `_System/queue/` (FORMAT.md §7.9), one JSON file
/// per entry named for its identifier.
pub trait QueueEntry: Serialize + serde::de::DeserializeOwned {
    /// The queue's folder, relative to the archive root.
    const DIR: &'static str;
    /// What an entry is called in messages, such as `proposal`.
    const NOUN: &'static str;
    /// The entry's identifier.
    fn id(&self) -> Id;
    /// What's wrong with the entry beyond its shape, if anything.
    ///
    /// # Errors
    ///
    /// A message saying what's wrong.
    fn check(&self) -> Result<(), String> {
        Ok(())
    }
}

impl QueueEntry for ReviewEntry {
    const DIR: &'static str = REVIEW_DIR;
    const NOUN: &'static str = "proposal";

    fn id(&self) -> Id {
        self.id
    }

    fn check(&self) -> Result<(), String> {
        if self.proposed_tags.len() > 1 {
            return Err("at most one proposed tag is allowed".into());
        }
        Ok(())
    }
}

/// One of an archive's queues.
#[derive(Debug, Clone)]
pub struct Queue<T> {
    dir: PathBuf,
    read_only: bool,
    entries: std::marker::PhantomData<T>,
}

/// The review queue: proposals to file items.
pub type ReviewQueue = Queue<ReviewEntry>;

impl<T: QueueEntry> Queue<T> {
    /// The queue of `archive`.
    #[must_use]
    pub fn of(archive: &Archive) -> Self {
        Self {
            dir: archive.root().join(T::DIR),
            read_only: archive.access() == crate::Access::ReadOnly,
            entries: std::marker::PhantomData,
        }
    }

    fn path(&self, id: Id) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Every entry, oldest first, with the files that couldn't be read as entries.
    ///
    /// # Errors
    ///
    /// [`ReviewError::Io`] if the queue folder can't be read.
    pub fn list(&self) -> Result<(Vec<T>, Vec<ReviewError>), ReviewError> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Vec::new(), Vec::new()));
            }
            Err(e) => return Err(io(&self.dir)(e)),
        };
        let mut good = Vec::new();
        let mut bad = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path
                .extension()
                .is_none_or(|e| !e.eq_ignore_ascii_case("json"))
            {
                continue;
            }
            match read(&path) {
                Ok(entry) => good.push(entry),
                Err(e) => bad.push(e),
            }
        }
        // Identifiers are `UUIDv7`s, so they sort by creation time.
        good.sort_by_key(|e: &T| e.id().0);
        Ok((good, bad))
    }

    /// The entry whose identifier is `id`, or starts or ends with it. Identifiers are `UUIDv7`s,
    /// whose start is a timestamp shared by a batch's entries, so the end (the random part, as
    /// [`short_id`] shows) is the handier thing to type.
    ///
    /// # Errors
    ///
    /// [`ReviewError::NotFound`], [`ReviewError::Ambiguous`], or an error reading the queue.
    pub fn find(&self, id: &str) -> Result<T, ReviewError> {
        let wanted = id.to_ascii_lowercase();
        let (entries, _) = self.list()?;
        let mut matches = entries.into_iter().filter(|e| {
            let full = e.id().to_string();
            full.starts_with(&wanted) || full.ends_with(&wanted)
        });
        let first = matches
            .next()
            .ok_or_else(|| ReviewError::NotFound(T::NOUN, id.to_owned()))?;
        if matches.next().is_some() {
            return Err(ReviewError::Ambiguous(T::NOUN, id.to_owned()));
        }
        Ok(first)
    }

    /// Writes `entry`, replacing any entry with its identifier. The file is written beside its
    /// place and renamed into it, so readers never see a partial entry.
    ///
    /// # Errors
    ///
    /// [`ReviewError::ReadOnly`] for a newer-format archive, or a file error.
    ///
    /// # Panics
    ///
    /// Never: entries always serialize.
    pub fn save(&self, entry: &T) -> Result<(), ReviewError> {
        if self.read_only {
            return Err(ReviewError::ReadOnly);
        }
        std::fs::create_dir_all(&self.dir).map_err(io(&self.dir))?;
        let path = self.path(entry.id());
        let temp = self.dir.join(format!(".{}.json.tmp", entry.id()));
        let text = serde_json::to_string_pretty(entry).expect("entries always serialize") + "\n";
        std::fs::write(&temp, text).map_err(io(&temp))?;
        std::fs::rename(&temp, &path).map_err(io(&path))
    }

    /// Removes the entry `id`, as when it has been filed.
    ///
    /// # Errors
    ///
    /// [`ReviewError::ReadOnly`], [`ReviewError::NotFound`] or a file error.
    pub fn remove(&self, id: Id) -> Result<(), ReviewError> {
        if self.read_only {
            return Err(ReviewError::ReadOnly);
        }
        let path = self.path(id);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(ReviewError::NotFound(T::NOUN, id.to_string()))
            }
            Err(e) => Err(io(&path)(e)),
        }
    }
}

/// The last eight characters of `id`: its random part, enough to tell a queue's entries apart
/// and to give to [`ReviewQueue::find`].
#[must_use]
pub fn short_id(id: Id) -> String {
    let full = id.to_string();
    full[full.len() - 8..].to_owned()
}

fn read<T: QueueEntry>(path: &Path) -> Result<T, ReviewError> {
    let text = std::fs::read_to_string(path).map_err(io(path))?;
    let entry: T = serde_json::from_str(&text).map_err(|e| ReviewError::Invalid {
        path: path.to_owned(),
        message: e.to_string(),
    })?;
    let expected = format!("{}.json", entry.id());
    if path
        .file_name()
        .is_some_and(|n| n.to_string_lossy() != expected)
    {
        return Err(ReviewError::Invalid {
            path: path.to_owned(),
            message: format!("the file is named for another entry; expected {expected}"),
        });
    }
    entry.check().map_err(|message| ReviewError::Invalid {
        path: path.to_owned(),
        message,
    })?;
    Ok(entry)
}
