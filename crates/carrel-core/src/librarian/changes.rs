//! The librarian's suggestions, and the changes it may make on its own
//! (`docs/design/librarian.md`, decision 1 as amended).
//!
//! The librarian may suggest four changes: refiling an item, correcting a card's title,
//! summary, created date or tags, renaming an item, and creating a collection. By default each
//! is a proposal the owner confirms. With `librarian.changes` on, it makes those it's sure of
//! itself, above `librarian.confidence`, through the same core functions the commands use, as
//! ordinary history changes marked as the librarian's (`Carrel-Agent: librarian`), which undo
//! reverses one at a time or all together ([`undo_since`]).
//!
//! It never acts on restricted items, never changes a field the owner wrote, and has no way to
//! delete anything, edit a file's contents, change settings or rules, or send anything.

use serde::Serialize;

use crate::archive::{Archive, ItemRef};
use crate::card::Sensitivity;
use crate::collections::NewCollection;
use crate::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder, WriterLease};
use crate::history::{Entry, History, HistoryError};
use crate::providers::Registry;

use super::ItemLink;
use super::tools::Shelf;

/// The name the librarian's changes carry in history (`Carrel-Agent`, FORMAT.md §8.2).
pub const AGENT: &str = "librarian";

/// The card fields the librarian may correct, with the name each has in a card's
/// `generated.fields` when a model wrote it (FORMAT.md §5.6).
const FIELDS: [(&str, &str); 4] = [
    ("title", "title"),
    ("summary", "summary"),
    ("created", "dates"),
    ("tags", "tags"),
];

/// A change the librarian suggests.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "change", rename_all = "kebab-case")]
pub enum Change {
    /// Move an item to another collection.
    Refile {
        /// The item.
        item: ItemLink,
        /// The collection, by path.
        to: String,
    },
    /// Correct one of a card's fields.
    Correct {
        /// The item.
        item: ItemLink,
        /// `title`, `summary`, `created` or `tags`.
        field: String,
        /// The new value; for tags, `+add -remove`.
        value: String,
    },
    /// Rename an item and its card.
    Rename {
        /// The item.
        item: ItemLink,
        /// The new name.
        name: String,
    },
    /// Create a collection.
    Collection {
        /// Its path.
        path: String,
        /// What belongs in it.
        description: String,
    },
}

impl Change {
    /// The change, in words.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Refile { item, to } => format!("refile “{}” to {to}", item.title),
            Self::Correct { item, field, value } => {
                format!("set the {field} of “{}” to “{value}”", item.title)
            }
            Self::Rename { item, name } => format!("rename “{}” to {name}", item.title),
            Self::Collection { path, .. } => format!("create the collection {path}"),
        }
    }

    /// The command that makes the change, for the owner to run.
    #[must_use]
    pub fn command(&self) -> String {
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\\\""));
        match self {
            Self::Refile { item, to } => format!(
                "carrel correct {} {} --yes",
                quote(&item.card),
                quote(&format!("collection: {to}"))
            ),
            Self::Correct { item, field, value } => format!(
                "carrel correct {} {} --yes",
                quote(&item.card),
                quote(&format!("{field}: {value}"))
            ),
            Self::Rename { item, name } => {
                format!("carrel rename {} {}", quote(&item.card), quote(name))
            }
            Self::Collection { path, description } => format!(
                "carrel collection new {} --description {}",
                quote(path),
                quote(description)
            ),
        }
    }

    /// The item it's about, if it's about one.
    #[must_use]
    pub fn item(&self) -> Option<&ItemLink> {
        match self {
            Self::Refile { item, .. } | Self::Correct { item, .. } | Self::Rename { item, .. } => {
                Some(item)
            }
            Self::Collection { .. } => None,
        }
    }
}

/// What became of a suggestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum Outcome {
    /// The librarian made it, as the history change `commit`.
    Made {
        /// The history change.
        commit: Option<String>,
    },
    /// It's for the owner to confirm, and why the librarian didn't make it itself.
    Proposed {
        /// Why it's left to the owner.
        reason: String,
    },
    /// It can't be made: it's against the librarian's rules or doesn't make sense.
    Refused {
        /// Why.
        reason: String,
    },
    /// Making it failed; nothing changed.
    Failed {
        /// Why.
        reason: String,
    },
}

/// A change the librarian suggested, why, how sure it is, and what became of it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Suggestion {
    /// The change.
    #[serde(flatten)]
    pub change: Change,
    /// Why, in the librarian's words.
    pub why: String,
    /// How sure it is, from 0 to 1.
    pub confidence: f64,
    /// What became of it.
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// A suggestion as the model gave it, before it's checked.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct Suggested {
    /// `refile`, `correct`, `rename` or `collection`.
    pub change: String,
    /// The item, by id or card path.
    pub item: String,
    /// For a correction, the field.
    pub field: String,
    /// The collection, new value, name or path.
    pub value: String,
    /// Why.
    pub why: String,
    /// How sure, from 0 to 1.
    pub confidence: f64,
}

/// Whether the librarian may make changes on its own, and as whom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rights {
    /// Who the changes are made for: the owner on this device.
    pub holder: Holder,
    /// How sure it must be ([`crate::documents::LibrarianSettings::confidence`]).
    pub confidence: f64,
}

/// `suggested` checked against the archive, as a change, or why it can't be one.
fn change_of(shelf: &Shelf<'_>, suggested: &Suggested) -> Result<Change, String> {
    let value = suggested.value.trim();
    if value.is_empty() {
        return Err(format!(
            "the {} suggestion had nothing to change to",
            suggested.change
        ));
    }
    let item = || -> Result<ItemLink, String> {
        shelf
            .resolve(&suggested.item)
            .map(|i| shelf.link(i))
            .map_err(|e| e.trim_end_matches('.').to_owned())
    };
    match suggested.change.as_str() {
        "refile" => {
            let item = item()?;
            let to = value.trim_matches('/').to_owned();
            if to == item.collection {
                return Err(format!("“{}” is already in {to}", item.title));
            }
            let collections = shelf.archive.collections().unwrap_or_default();
            if !collections.iter().any(|c| c.path == to) {
                return Err(format!(
                    "there's no collection {to} to refile “{}” to",
                    item.title
                ));
            }
            Ok(Change::Refile { item, to })
        }
        "correct" => {
            let field = suggested.field.trim().to_lowercase();
            if !FIELDS.iter().any(|(name, _)| *name == field) {
                return Err(format!(
                    "the librarian can correct a card's title, summary, created date or tags, not its {field}"
                ));
            }
            Ok(Change::Correct {
                item: item()?,
                field,
                value: value.to_owned(),
            })
        }
        "rename" => Ok(Change::Rename {
            item: item()?,
            name: value.to_owned(),
        }),
        "collection" => Ok(Change::Collection {
            path: value.trim_matches('/').to_owned(),
            description: suggested.why.trim().to_owned(),
        }),
        other => Err(format!("{other} isn't a change the librarian can suggest")),
    }
}

/// Why the librarian mustn't make `change` itself, when it mustn't, as an outcome: refused
/// outright for a restricted item, left to the owner for a field the owner wrote.
fn not_its_own(shelf: &Shelf<'_>, change: &Change) -> Option<Outcome> {
    let item = change.item()?;
    if item.sensitivity == Sensitivity::Restricted {
        return Some(Outcome::Refused {
            reason: "the librarian never changes restricted items".into(),
        });
    }
    if let Change::Correct { field, .. } = change {
        let written = FIELDS
            .iter()
            .find(|(name, _)| name == field)
            .map(|(_, w)| *w)?;
        let card = shelf
            .items
            .iter()
            .find(|i| i.id == item.id)
            .and_then(|i| shelf.card(i));
        let model_wrote = card
            .and_then(|c| c.generated)
            .is_some_and(|g| g.fields.iter().any(|f| f == written));
        if !model_wrote {
            return Some(Outcome::Proposed {
                reason: format!("you wrote its {field}, so only you change it"),
            });
        }
    }
    None
}

/// What becomes of `suggested`: checked, then made when `rights` allow and the librarian is
/// sure enough, else left for the owner to confirm. `model` names the model that suggested it,
/// for a corrected card's provenance.
#[must_use]
pub fn consider(
    shelf: &Shelf<'_>,
    registry: &Registry,
    model: &str,
    suggested: &Suggested,
    rights: Option<&Rights>,
) -> Option<Suggestion> {
    let confidence = if suggested.confidence.is_finite() {
        suggested.confidence.clamp(0.0, 1.0)
    } else {
        0.0
    };
    // Nonsense isn't worth showing the owner.
    let Ok(change) = change_of(shelf, suggested) else {
        return None;
    };
    let outcome = match (not_its_own(shelf, &change), rights) {
        (Some(outcome), _) => outcome,
        (None, None) => Outcome::Proposed {
            reason: "the librarian only suggests changes unless you turn on librarian.changes"
                .into(),
        },
        (None, Some(rights)) if confidence < rights.confidence => Outcome::Proposed {
            reason: format!(
                "it was {:.0}% sure, below the {:.0}% it needs to act on its own",
                confidence * 100.0,
                rights.confidence * 100.0
            ),
        },
        (None, Some(rights)) => make(shelf.archive, registry, model, &change, rights.holder),
    };
    Some(Suggestion {
        change,
        why: suggested.why.trim().to_owned(),
        confidence,
        outcome,
    })
}

/// Makes `change` as the librarian, for `holder`.
fn make(
    archive: &Archive,
    registry: &Registry,
    model: &str,
    change: &Change,
    holder: Holder,
) -> Outcome {
    let mut lease = match DiskLock::for_archive(archive).acquire(holder, AGENT, DEFAULT_LEASE) {
        Ok(lease) => lease,
        Err(e) => {
            return Outcome::Failed {
                reason: e.to_string(),
            };
        }
    };
    lease.act_as(Some(AGENT));
    let result = apply(archive, registry, model, change, &lease);
    let released = lease.release();
    match (result, released) {
        (Ok(commit), Ok(())) => Outcome::Made { commit },
        (Err(reason), _) => Outcome::Failed { reason },
        (Ok(_), Err(e)) => Outcome::Failed {
            reason: e.to_string(),
        },
    }
}

/// Makes `change` with `lease`, through the same core functions as the commands: a refile or
/// correction as a correction (with `model` as the model that wrote the fields), a rename as
/// `carrel rename`, a collection as `carrel collection new`. It's the librarian's change when
/// the lease acts as the librarian, and the owner's when the owner confirmed it. Returns the
/// history change.
///
/// # Errors
///
/// Why it couldn't be made; nothing changed.
pub fn apply(
    archive: &Archive,
    registry: &Registry,
    model: &str,
    change: &Change,
    lease: &WriterLease,
) -> Result<Option<String>, String> {
    use crate::correct::prepare;
    let item_ref = |item: &ItemLink| -> Result<ItemRef, String> {
        archive
            .items()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|i| i.id == item.id)
            .ok_or_else(|| format!("“{}” is no longer in the archive", item.title))
    };
    let by = if lease.agent() == Some(AGENT) {
        "Librarian"
    } else {
        "Owner"
    };
    let correct = |item: &ItemLink, lines: String| -> Result<Option<String>, String> {
        let prepared =
            prepare::prepare_filed_lines(archive, registry, item_ref(item)?, &lines, model)
                .map_err(|e| e.to_string())?
                .said_by(by);
        if prepared.is_empty() {
            return Ok(None);
        }
        let saved = prepare::save_holding(archive, prepared, lease).map_err(|e| e.to_string())?;
        Ok(saved.commit)
    };
    match change {
        Change::Refile { item, to } => correct(item, format!("collection: {to}")),
        Change::Correct { item, field, value } => correct(item, format!("{field}: {value}")),
        Change::Rename { item, name } => {
            crate::rename::rename(archive, &item_ref(item)?, name, lease)
                .map(|r| r.commit)
                .map_err(|e| e.to_string())
        }
        Change::Collection { path, description } => {
            let new = NewCollection {
                description: (!description.is_empty()).then(|| description.clone()),
                tags: Vec::new(),
            };
            crate::collections::create(archive, path, &new, lease)
                .map(|c| c.commit)
                .map_err(|e| e.to_string())
        }
    }
}

/// The librarian's changes in `log` (newest first, as [`History::log`] gives it) made at or
/// after `since` (seconds since the Unix epoch), that haven't been undone, newest first.
#[must_use]
pub fn recent(log: &[Entry], since: i64) -> Vec<Entry> {
    let undone: std::collections::HashSet<&str> =
        log.iter().filter_map(|e| e.reverts.as_deref()).collect();
    log.iter()
        .filter(|e| e.agent.as_deref() == Some(AGENT) && e.time >= since)
        .filter(|e| !undone.contains(e.id.as_str()))
        .cloned()
        .collect()
}

/// What undoing the librarian's changes did.
#[derive(Debug, Default)]
pub struct Undone {
    /// The changes undone, each with the change that undid it.
    pub undone: Vec<(Entry, String)>,
    /// Those that couldn't be undone, and why.
    pub failed: Vec<(Entry, String)>,
}

/// Undoes every change the librarian made at or after `since` that hasn't been undone, newest
/// first, holding `lease`. One that can't be undone, as when the owner changed the same card
/// since, is left and reported; the rest are still undone.
///
/// # Errors
///
/// [`HistoryError`] if the history can't be read.
pub fn undo_since(
    archive: &Archive,
    since: i64,
    lease: &WriterLease,
) -> Result<Undone, HistoryError> {
    let history = History::open(archive)?;
    let mut done = Undone::default();
    for entry in recent(&history.log()?, since) {
        match history.undo(archive, &entry.id, lease) {
            Ok(commit) => done.undone.push((entry, commit)),
            Err(e) => done.failed.push((entry, e.to_string())),
        }
    }
    Ok(done)
}

/// The moment `text` names, as seconds since the Unix epoch: a span back from `now`, such as
/// `30m`, `24h`, `7d` or `2w`, `today`, or a date (`2026-11-01`) or time in RFC 3339, read in
/// `now`'s time zone.
#[must_use]
pub fn parse_since(text: &str, now: &jiff::Zoned) -> Option<i64> {
    let text = text.trim().to_lowercase();
    if text == "today" {
        return now.start_of_day().ok().map(|t| t.timestamp().as_second());
    }
    if let Some(split) = text.len().checked_sub(1) {
        let (number, unit) = text.split_at(split);
        if let Ok(n) = number.parse::<i64>() {
            let seconds = match unit {
                "m" => 60,
                "h" => 3_600,
                "d" => 86_400,
                "w" => 604_800,
                _ => 0,
            };
            if seconds > 0 && n >= 0 {
                return Some(now.timestamp().as_second() - n.saturating_mul(seconds));
            }
        }
    }
    if let Ok(time) = text.parse::<jiff::Timestamp>() {
        return Some(time.as_second());
    }
    let date = text.parse::<jiff::civil::Date>().ok()?;
    date.to_zoned(now.time_zone().clone())
        .ok()
        .map(|t| t.timestamp().as_second())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_reads_spans_dates_and_today() {
        let now: jiff::Zoned = "2026-11-01T12:00:00+00:00[UTC]".parse().unwrap();
        let at = now.timestamp().as_second();
        assert_eq!(parse_since("7d", &now), Some(at - 7 * 86_400));
        assert_eq!(parse_since("24h", &now), Some(at - 86_400));
        assert_eq!(parse_since("today", &now), Some(at - 12 * 3_600));
        assert_eq!(parse_since("2026-10-31", &now), Some(at - 36 * 3_600));
        assert_eq!(parse_since("soon", &now), None);
        assert_eq!(parse_since("7y", &now), None);
    }

    #[test]
    fn recent_leaves_out_the_owners_changes_and_those_undone() {
        let entry = |id: &str, time: i64, agent: Option<&str>, reverts: Option<&str>| Entry {
            id: id.into(),
            summary: String::new(),
            operation: None,
            person: None,
            device: None,
            snapshot: None,
            items: Vec::new(),
            moves: Vec::new(),
            reverts: reverts.map(str::to_owned),
            agent: agent.map(str::to_owned),
            time,
        };
        let log = [
            entry("u", 50, None, Some("b")),
            entry("c", 40, Some(AGENT), None),
            entry("b", 30, Some(AGENT), None),
            entry("o", 20, None, None),
            entry("a", 10, Some(AGENT), None),
        ];
        let ids: Vec<String> = recent(&log, 15).into_iter().map(|e| e.id).collect();
        assert_eq!(ids, ["c"]);
        assert_eq!(recent(&log, 0).len(), 2);
    }
}
