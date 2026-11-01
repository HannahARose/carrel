//! Flags: the owner's marks on items and collections that need work, set while browsing
//! (FORMAT.md §5.9). A flag is the `needs_work` field on a card or in a collection's design
//! document, with an optional note, who set it and when. Setting and clearing one is an
//! ordinary change in the history, so undo reverses it.
//!
//! What flags feed: Status lists them ([`crate::status::Summary::flagged`]), the crawler takes
//! flagged items and the items of flagged collections first ([`crate::crawl::Stage::Flagged`]),
//! a flagged collection is a restructure signal ([`crate::signals::Signal::Flagged`]), and the
//! librarian can list them. When a correction or an edit is saved on a flagged card, the
//! interfaces offer to clear its flag ([`of_item`]).

use std::fmt::Write as _;

use serde::Serialize;

use crate::actions::ActionError;
use crate::archive::{Archive, ArchiveError, ItemRef};
use crate::card::{Id, NeedsWork};
use crate::card_file::CardFile;
use crate::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
use crate::documents::CollectionDoc;
use crate::history::{Change, History, Operation};

/// How long a flag's note may be, in characters: a few words, not a description.
pub const NOTE_CHARS: usize = 200;

/// What can be flagged: an item, by its card, or a collection, by its folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Flaggable {
    /// An item.
    Item {
        /// Its identifier.
        id: Id,
        /// Its card, relative to the archive root.
        card: String,
    },
    /// A collection.
    Collection {
        /// Its folder, relative to the archive root.
        path: String,
    },
}

impl Flaggable {
    /// The item `item`.
    #[must_use]
    pub fn item(item: &ItemRef) -> Self {
        Self::Item {
            id: item.id,
            card: item.card.clone(),
        }
    }

    /// The collection at `path`.
    #[must_use]
    pub fn collection(path: &str) -> Self {
        Self::Collection {
            path: path.trim().trim_matches('/').to_owned(),
        }
    }

    /// Its card's path or its folder, relative to the archive root.
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Item { card, .. } => card,
            Self::Collection { path } => path,
        }
    }
}

/// Something flagged, as Status and the Flagged views list it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Flagged {
    /// What's flagged.
    #[serde(flatten)]
    pub what: Flaggable,
    /// Its title.
    pub title: String,
    /// The flag: its note, who set it and when.
    pub flag: NeedsWork,
}

/// What flagging or unflagging did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Flagging {
    /// Whether anything changed: flagging something already flagged with the same note, or
    /// unflagging something that isn't, changes nothing.
    pub changed: bool,
    /// The change's summary, as the history shows it.
    pub summary: String,
    /// The history commit, when one was made.
    pub commit: Option<String>,
}

fn io(path: &std::path::Path) -> impl FnOnce(std::io::Error) -> ActionError + '_ {
    move |source| ActionError::Io {
        path: path.to_owned(),
        source,
    }
}

/// `note` trimmed, `None` when empty.
///
/// # Errors
///
/// [`ActionError::Invalid`] if it's longer than [`NOTE_CHARS`] or has more than one line.
fn checked_note(note: Option<&str>) -> Result<Option<String>, ActionError> {
    let Some(note) = note.map(str::trim).filter(|n| !n.is_empty()) else {
        return Ok(None);
    };
    if note.contains('\n') {
        return Err(ActionError::Invalid(
            "a flag's note is one line, a few words saying what needs doing".into(),
        ));
    }
    if note.chars().count() > NOTE_CHARS {
        return Err(ActionError::Invalid(format!(
            "a flag's note is at most {NOTE_CHARS} characters, a few words saying what needs doing"
        )));
    }
    Ok(Some(note.to_owned()))
}

/// Flags `what` as needing work, with `note` saying what, as `holder`: one change in the
/// history. Flagging something already flagged replaces its note, and who and when.
///
/// # Errors
///
/// [`ActionError::Invalid`] if the note is too long, the item is checked out (flag it when
/// it's returned, so the return can't overwrite it), or the card or collection can't be
/// read; [`ActionError::Lease`] if another writer holds the archive; or an error writing.
pub fn flag(
    archive: &Archive,
    what: &Flaggable,
    note: Option<&str>,
    holder: Holder,
) -> Result<Flagging, ActionError> {
    let note = checked_note(note)?;
    let flag = NeedsWork {
        id: Some(Id::new_v7()),
        note: note.clone(),
        person: Some(holder.person),
        at: Some(crate::questions::now()),
        extra: crate::card::Extra::new(),
    };
    let set = |current: &mut Option<NeedsWork>| {
        if current.as_ref().is_some_and(|c| c.note == note) {
            return false;
        }
        // The same flag with a new note keeps its identifier.
        let id = current.as_ref().and_then(|c| c.id).or(flag.id);
        *current = Some(NeedsWork { id, ..flag.clone() });
        true
    };
    let summary = |title: &str| match &note {
        Some(note) => format!("Flag “{title}” as needing work: {note}"),
        None => format!("Flag “{title}” as needing work"),
    };
    change(archive, what, holder, set, summary)
}

/// Clears `what`'s flag, as `holder`: one change in the history.
///
/// # Errors
///
/// As [`flag`].
pub fn unflag(
    archive: &Archive,
    what: &Flaggable,
    holder: Holder,
) -> Result<Flagging, ActionError> {
    change(
        archive,
        what,
        holder,
        |current| current.take().is_some(),
        |title| format!("Clear the flag on “{title}”"),
    )
}

/// Reads `what`'s flag, lets `set` change it, and when it did, writes it and records the
/// change, summarized by `summary` from the title.
fn change(
    archive: &Archive,
    what: &Flaggable,
    holder: Holder,
    set: impl FnOnce(&mut Option<NeedsWork>) -> bool,
    summary: impl FnOnce(&str) -> String,
) -> Result<Flagging, ActionError> {
    match what {
        Flaggable::Item { id, card } => {
            let path = archive.root().join(card);
            let text = std::fs::read_to_string(&path).map_err(io(&path))?;
            let mut file =
                CardFile::parse(&text).map_err(|e| ActionError::Invalid(format!("{card}: {e}")))?;
            if file.card().id != *id {
                return Err(ActionError::Invalid(format!(
                    "{card} describes another item now"
                )));
            }
            if file.card().checked_out.is_some() {
                return Err(ActionError::Invalid(
                    "it's checked out; flag it when it's returned, so the return can't overwrite it"
                        .into(),
                ));
            }
            let summary = summary(&file.card().title);
            if !set(&mut file.card_mut().needs_work) {
                return Ok(unchanged(summary));
            }
            let item = ItemRef {
                card: card.clone(),
                item: None,
                id: *id,
                kind: file.card().kind,
            };
            let commit = crate::edit::write(archive, &item, &file, &summary, holder)?;
            Ok(Flagging {
                changed: true,
                summary,
                commit,
            })
        }
        Flaggable::Collection { path } => {
            let document = archive.root().join(path).join("_collection.md");
            let text = std::fs::read_to_string(&document).map_err(io(&document))?;
            let invalid = |e: String| ActionError::Invalid(format!("{path}: {e}"));
            let split = crate::frontmatter::split(&text).map_err(|e| invalid(e.to_string()))?;
            let mut doc: CollectionDoc =
                serde_saphyr::from_str(split.yaml).map_err(|e| invalid(e.to_string()))?;
            let summary = summary(path);
            if !set(&mut doc.needs_work) {
                return Ok(unchanged(summary));
            }
            let yaml = serde_saphyr::to_string(&doc).map_err(|e| invalid(e.to_string()))?;
            let updated = crate::frontmatter::join(&yaml, split.body);
            let lease = DiskLock::for_archive(archive).acquire(holder, "flag", DEFAULT_LEASE)?;
            let result = (|| -> Result<Option<String>, ActionError> {
                let written = document.with_extension("md.tmp-flag");
                std::fs::write(&written, updated).map_err(io(&written))?;
                std::fs::rename(&written, &document).map_err(io(&document))?;
                let change = Change::new(
                    Operation::Edit,
                    &summary,
                    lease.info().person,
                    lease.info().device,
                );
                Ok(History::open(archive)?.commit(archive, &change, &lease)?)
            })();
            lease.release()?;
            Ok(Flagging {
                changed: true,
                summary,
                commit: result?,
            })
        }
    }
}

fn unchanged(summary: String) -> Flagging {
    Flagging {
        changed: false,
        summary,
        commit: None,
    }
}

/// The flag on the item whose card is `card`, as listed.
#[must_use]
pub fn flagged_item(item: &ItemRef, card: &crate::card::Card) -> Option<Flagged> {
    card.needs_work.as_ref().map(|flag| Flagged {
        what: Flaggable::item(item),
        title: card.title.clone(),
        flag: flag.clone(),
    })
}

/// The flags on the collections, by path.
///
/// # Errors
///
/// [`ArchiveError`] if the collections can't be read.
pub fn flagged_collections(archive: &Archive) -> Result<Vec<Flagged>, ArchiveError> {
    Ok(archive
        .collections()?
        .into_iter()
        .filter_map(|c| {
            let flag = c.doc.needs_work?;
            Some(Flagged {
                what: Flaggable::Collection { path: c.path },
                title: c.doc.title,
                flag,
            })
        })
        .collect())
}

/// Everything flagged in `archive`: collections by path, then items by card. Cards that
/// can't be read are left out.
///
/// # Errors
///
/// [`ArchiveError`] if the collections or items can't be listed.
pub fn all(archive: &Archive) -> Result<Vec<Flagged>, ArchiveError> {
    let mut found = flagged_collections(archive)?;
    for item in archive.items()? {
        let Ok(text) = std::fs::read_to_string(archive.root().join(&item.card)) else {
            continue;
        };
        if let Ok(file) = CardFile::parse(&text)
            && let Some(flagged) = flagged_item(&item, file.card())
        {
            found.push(flagged);
        }
    }
    Ok(found)
}

/// The flag on `item`, if it has one: for offering to clear it once a correction or an edit
/// is saved.
#[must_use]
pub fn of_item(archive: &Archive, item: &ItemRef) -> Option<NeedsWork> {
    let text = std::fs::read_to_string(archive.root().join(&item.card)).ok()?;
    CardFile::parse(&text).ok()?.card().needs_work.clone()
}

/// A flag in words, for people: its note, or that it has none, and when it was set.
#[must_use]
pub fn describe(flag: &NeedsWork) -> String {
    let mut text = flag.note.clone().unwrap_or_else(|| "needs work".to_owned());
    if let Some(day) = flag.at.as_ref().and_then(|at| at.0.get(..10)) {
        write!(text, " (flagged {day})").unwrap();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::DateOrTime;

    #[test]
    fn a_note_is_one_short_line() {
        assert_eq!(checked_note(None).unwrap(), None);
        assert_eq!(checked_note(Some("  ")).unwrap(), None);
        assert_eq!(
            checked_note(Some(" wrong title ")).unwrap().as_deref(),
            Some("wrong title")
        );
        assert!(checked_note(Some("two\nlines")).is_err());
        assert!(checked_note(Some(&"x".repeat(NOTE_CHARS + 1))).is_err());
    }

    #[test]
    fn a_flag_in_words_has_its_note_and_day() {
        let flag = NeedsWork {
            note: Some("split this folder".into()),
            at: Some(DateOrTime("2026-11-01T09:30:00-04:00".into())),
            ..NeedsWork::default()
        };
        assert_eq!(describe(&flag), "split this folder (flagged 2026-11-01)");
        assert_eq!(describe(&NeedsWork::default()), "needs work");
    }
}
