//! The crawler's findings, waiting in Review (FORMAT.md §7.9, `queue/crawler/`): a move of an
//! item to a collection it fits better, or a correction of one of its card's fields. Approving
//! one makes the change as the owner, through the corrections path; rejecting one removes it
//! and remembers it, so the crawler doesn't propose it again.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::verdicts::Verdicts;
use crate::archive::{Archive, ItemRef};
use crate::card::{Extra, Id, Sensitivity};
use crate::coordinator::WriterLease;
use crate::librarian::ItemLink;
use crate::librarian::changes::{Change, apply};
use crate::providers::Registry;
use crate::review::{Queue, QueueEntry, ReviewError};

/// Where findings live, relative to the archive root.
pub const FINDINGS_DIR: &str = "_System/queue/crawler";

/// The document type of a finding; always `crawl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CrawlMarker {
    /// The only value.
    Crawl,
}

/// What the crawler found could be better about an item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "kebab-case")]
pub enum Finding {
    /// It belongs in another collection.
    Move {
        /// The collection it's in, by path.
        from: String,
        /// The one it fits better, by path.
        to: String,
    },
    /// One of its card's fields could be better.
    Correct {
        /// `title`, `summary`, `created` or `tags`.
        field: String,
        /// The new value; for tags, `+add` for each to add.
        value: String,
    },
}

impl Finding {
    /// The finding, in words, about the item titled `title`.
    #[must_use]
    pub fn describe(&self, title: &str) -> String {
        match self {
            Self::Move { to, .. } => format!("move “{title}” to {to}"),
            Self::Correct { field, value } => format!("set the {field} of “{title}” to “{value}”"),
        }
    }
}

/// A finding waiting in Review (`queue/crawler/<id>.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrawlEntry {
    /// Always `crawl`.
    pub carrel: CrawlMarker,
    /// The format version.
    pub format: crate::FormatVersion,
    /// The finding's identifier.
    pub id: Id,
    /// When it was found, as RFC 3339.
    pub created: String,
    /// The item.
    pub item: Id,
    /// Its card, relative to the archive root, when it was found.
    pub card: String,
    /// Its card's title when it was found.
    pub title: String,
    /// What could be better.
    #[serde(flatten)]
    pub finding: Finding,
    /// Why, in the model's words.
    pub reason: String,
    /// How sure the model was, from 0 to 1.
    pub confidence: f64,
    /// The model, as `provider/offer`.
    pub model: String,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl QueueEntry for CrawlEntry {
    const DIR: &'static str = FINDINGS_DIR;
    const NOUN: &'static str = "crawler finding";

    fn id(&self) -> Id {
        self.id
    }

    fn check(&self) -> Result<(), String> {
        match &self.finding {
            Finding::Move { to, .. } if to.trim().is_empty() => Err("it moves nowhere".into()),
            Finding::Correct { value, .. } if value.trim().is_empty() => {
                Err("it corrects to nothing".into())
            }
            _ => Ok(()),
        }
    }
}

impl CrawlEntry {
    /// A finding about `item`, found now by `model`.
    #[must_use]
    pub fn new(
        item: &ItemRef,
        title: &str,
        finding: Finding,
        reason: &str,
        confidence: f64,
        model: &str,
    ) -> Self {
        Self {
            carrel: CrawlMarker::Crawl,
            format: crate::FORMAT_VERSION,
            id: Id::new_v7(),
            created: jiff::Timestamp::now().to_string(),
            item: item.id,
            card: item.card.clone(),
            title: title.to_owned(),
            finding,
            reason: reason.to_owned(),
            confidence,
            model: model.to_owned(),
            extra: Extra::new(),
        }
    }

    /// The change approving it makes, on the item as `items` has it now.
    fn change(&self, items: &[ItemRef]) -> Result<Change, String> {
        let item = items
            .iter()
            .find(|i| i.id == self.item)
            .ok_or_else(|| format!("“{}” is no longer in the archive", self.title))?;
        let link = ItemLink {
            id: item.id,
            title: self.title.clone(),
            card: item.card.clone(),
            collection: item
                .card
                .rsplit_once('/')
                .map_or_else(String::new, |(folder, _)| folder.to_owned()),
            sensitivity: Sensitivity::Personal,
        };
        Ok(match &self.finding {
            Finding::Move { to, .. } => Change::Refile {
                item: link,
                to: to.clone(),
            },
            Finding::Correct { field, value } => Change::Correct {
                item: link,
                field: field.clone(),
                value: value.clone(),
            },
        })
    }
}

/// The crawler's findings in `archive`, oldest first.
///
/// # Errors
///
/// [`ReviewError`] if the queue can't be read.
pub fn list(archive: &Archive) -> Result<Vec<CrawlEntry>, ReviewError> {
    Ok(Queue::<CrawlEntry>::of(archive).list()?.0)
}

/// The items with a finding waiting.
///
/// # Errors
///
/// [`ReviewError`] if the queue can't be read.
pub fn open_items(archive: &Archive) -> Result<HashSet<Id>, ReviewError> {
    Ok(list(archive)?.into_iter().map(|e| e.item).collect())
}

/// The finding whose identifier is `id`, or starts or ends with it.
///
/// # Errors
///
/// [`ReviewError::NotFound`], [`ReviewError::Ambiguous`], or an error reading the queue.
pub fn find(archive: &Archive, id: &str) -> Result<CrawlEntry, ReviewError> {
    Queue::<CrawlEntry>::of(archive).find(id)
}

/// Approves `entry`: makes its change as `lease` makes changes, the owner's unless the lease
/// acts as the crawler, through the corrections path, then removes it. Returns the history
/// change.
///
/// # Errors
///
/// Why the change couldn't be made, as when the item has gone; the finding is kept then.
pub fn approve(
    archive: &Archive,
    registry: &Registry,
    entry: &CrawlEntry,
    lease: &WriterLease,
) -> Result<Option<String>, String> {
    let commit = make(archive, registry, entry, lease)?;
    Queue::<CrawlEntry>::of(archive)
        .remove(entry.id)
        .map_err(|e| e.to_string())?;
    Ok(commit)
}

/// Makes `entry`'s change as `lease` makes changes, on the item where it is now, without
/// touching the queue. Returns the history change.
///
/// # Errors
///
/// Why the change couldn't be made; nothing changed.
pub fn make(
    archive: &Archive,
    registry: &Registry,
    entry: &CrawlEntry,
    lease: &WriterLease,
) -> Result<Option<String>, String> {
    let items = archive.items().map_err(|e| e.to_string())?;
    let change = entry.change(&items)?;
    apply(archive, registry, &entry.model, &change, lease)
}

/// Saves `entry` to wait in Review, unless the same finding about the same item is waiting
/// already.
///
/// # Errors
///
/// [`ReviewError`] if the queue can't be read or written.
pub fn propose(archive: &Archive, entry: &CrawlEntry) -> Result<bool, ReviewError> {
    if list(archive)?
        .iter()
        .any(|e| e.item == entry.item && e.finding == entry.finding)
    {
        return Ok(false);
    }
    Queue::<CrawlEntry>::of(archive).save(entry)?;
    Ok(true)
}

/// Rejects `entry`: removes it, and remembers on this device (`device`) that the owner said no,
/// so the crawler doesn't propose it again.
///
/// # Errors
///
/// [`ReviewError`] if it can't be removed, or the verdicts can't be written.
pub fn reject(archive: &Archive, entry: &CrawlEntry, device: Id) -> Result<(), ReviewError> {
    Queue::<CrawlEntry>::of(archive).remove(entry.id)?;
    let mut verdicts = Verdicts::load(archive, device);
    // The verdict may be another device's; this device keeps the owner's answer either way.
    let Some(mut verdict) = verdicts
        .items
        .get(&entry.item)
        .cloned()
        .or_else(|| super::verdicts::all(archive).remove(&entry.item))
    else {
        return Ok(());
    };
    if !verdict.declined.contains(&entry.finding) {
        verdict.declined.push(entry.finding.clone());
    }
    verdicts.items.insert(entry.item, verdict);
    let path = Verdicts::path(archive, device);
    verdicts
        .save(archive, device)
        .map_err(|source| ReviewError::Io { path, source })
}
