//! The calls the rest of `carrel-core` makes, one line each, where decisions happen: they turn
//! a proposal, a correction or a question into a [`Event`] of identifiers and counts.

use std::time::Instant;

use super::record::{Decision, Event, QuestionOutcome, Routed};
use super::{logging_to, note};
use crate::archive::{Archive, ItemRef};
use crate::card::{DateOrTime, Id};
use crate::ingest::filing::{Outcome, Placement};
use crate::ingest::run::Run;
use crate::questions::QuestionEntry;
use crate::review::ReviewEntry;
use crate::signals::Signal;

/// The model that drafted `entry`'s card, as `provider/offer`.
#[must_use]
pub fn model_of(entry: &ReviewEntry) -> Option<String> {
    entry
        .attempts
        .iter()
        .find(|a| a.stage == "card" && a.result == "ok")
        .map(|a| format!("{}/{}", a.provider, a.offer))
}

/// Where `entry` is going, as its destination says.
fn routed(entry: &ReviewEntry) -> Routed {
    Routed {
        collection: entry.destination.collection,
        confidence: entry.destination.confidence,
        rule: entry.destination.rule.is_some(),
    }
}

/// `entry`'s item's extension.
fn extension(entry: &ReviewEntry) -> String {
    crate::formats::extension(&entry.source.path)
}

/// Seconds from `time` to now, at least 0.
fn since(time: &DateOrTime) -> i64 {
    crate::questions::timestamp_of(time)
        .map_or(0, |t| (jiff::Timestamp::now() - t).get_seconds().max(0))
}

/// Logs the proposals `run` drafted for an intake entry `added` then, drafting from `started`.
pub fn drafted(archive: &Archive, added: Option<&DateOrTime>, run: &Run, started: Instant) {
    let entries: Vec<&ReviewEntry> = run.entries.iter().flatten().collect();
    if entries.is_empty() || logging_to().is_none() {
        return;
    }
    let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let each = elapsed / entries.len() as u64;
    let queued = added.map_or(0, |a| {
        crate::questions::timestamp_of(a).map_or(0, |t| {
            let begun = jiff::Timestamp::now()
                - jiff::SignedDuration::from_millis(i64::try_from(elapsed).unwrap_or(i64::MAX));
            (begun - t).get_seconds().max(0)
        })
    });
    for entry in entries {
        note(
            archive,
            Event::Drafted {
                item: entry.id,
                kind: entry.kind,
                extension: extension(entry),
                model: model_of(entry),
                routed: Some(routed(entry)),
                queued_secs: queued,
                draft_ms: each,
            },
        );
    }
}

/// Proposals being decided, noted before they're filed so that what's logged is how they
/// stood when the owner decided ([`Deciding::filed`]).
#[derive(Debug, Default)]
pub struct Deciding {
    noted: Vec<(Id, Event)>,
}

/// Notes `entries` as they're decided with `placement`, to be logged once they're filed.
#[must_use]
pub fn deciding(entries: &[ReviewEntry], placement: Placement) -> Deciding {
    if logging_to().is_none() {
        return Deciding::default();
    }
    let decision = match placement {
        Placement::Destination => Decision::Approved,
        Placement::Holding => Decision::Held,
    };
    Deciding {
        noted: entries
            .iter()
            .map(|e| (e.id, decided(e, decision)))
            .collect(),
    }
}

fn decided(entry: &ReviewEntry, decision: Decision) -> Event {
    Event::Decided {
        item: entry.id,
        kind: entry.kind,
        extension: extension(entry),
        decision,
        to: routed(entry),
        model: model_of(entry),
        waited_secs: since(&entry.created),
    }
}

impl Deciding {
    /// Logs those `outcome` filed, in `archive`.
    pub fn filed(self, archive: &Archive, outcome: Option<&Outcome>) {
        let Some(outcome) = outcome else { return };
        for (id, event) in self.noted {
            if outcome.filed.iter().any(|f| f.entry == id) {
                note(archive, event);
            }
        }
    }
}

/// Logs that `entries` were rejected.
pub fn rejected(archive: &Archive, entries: &[ReviewEntry]) {
    for entry in entries {
        note(archive, decided(entry, Decision::Rejected));
    }
}

/// Logs that the owner moved `entry` to the collection `to`, before it's changed.
pub fn moved(archive: &Archive, entry: &ReviewEntry, to: Id) {
    if entry.destination.collection != to {
        note(
            archive,
            Event::Moved {
                item: entry.id,
                from: routed(entry),
                to,
            },
        );
    }
}

/// Logs the owner's choice for a compressed file proposal.
pub fn compressed(archive: &Archive, entry: &ReviewEntry, choice: crate::compressed::plan::Choice) {
    note(
        archive,
        Event::Compressed {
            item: entry.id,
            choice: choice.as_str().to_owned(),
        },
    );
}

/// Logs how the owner kept a likely duplicate.
pub fn duplicate(archive: &Archive, entry: &ReviewEntry, kept: crate::actions::Kept) {
    note(
        archive,
        Event::Duplicate {
            item: entry.id,
            kept: kept.link_type().to_owned(),
        },
    );
}

/// What a correction being saved was about, for [`corrected`].
#[derive(Debug, Clone, Copy)]
pub enum Corrected<'a> {
    /// A proposal.
    Proposal(&'a ReviewEntry),
    /// A filed item.
    Filed(&'a ItemRef),
}

/// Logs a correction of `about` that changed `fields`, worked out from the fields `before`,
/// by `by` (`Owner`, or a program such as `Librarian`).
pub fn corrected(
    archive: &Archive,
    about: Corrected<'_>,
    changes: &crate::correct::Changes,
    fields: &[&str],
    before: &crate::correct::Fields,
    moved: bool,
    by: &str,
) {
    if fields.is_empty() && !moved {
        return;
    }
    let (item, kind, extension, filed) = match about {
        Corrected::Proposal(e) => (e.id, e.kind, extension(e), false),
        Corrected::Filed(i) => (
            i.id,
            i.kind,
            i.item
                .as_deref()
                .map(|p| crate::formats::extension(std::path::Path::new(p)))
                .unwrap_or_default(),
            true,
        ),
    };
    let model = before.generated.as_ref().and_then(|g| {
        g.model
            .clone()
            .filter(|_| fields.iter().any(|f| g.fields.iter().any(|g| g == f)))
    });
    note(
        archive,
        Event::Corrected {
            item,
            kind,
            extension,
            fields: fields.iter().map(|f| (*f).to_owned()).collect(),
            broad: fields.len() > 1 || changes.reroute || moved,
            filed,
            moved,
            model,
            by: by.to_lowercase(),
        },
    );
}

/// Logs how `question` ended, and how many proposals or items it moved or settled.
pub fn question(
    archive: &Archive,
    question: &QuestionEntry,
    outcome: QuestionOutcome,
    items: usize,
) {
    note(
        archive,
        Event::Question {
            question: question.id,
            kind: question.kind,
            outcome,
            items,
        },
    );
}

/// Logs the structure's signals for `archive` of `items`, at most once a day.
pub fn snapshot(archive: &Archive, items: &[ItemRef], signals: &[Signal]) {
    let Some(dir) = logging_to() else {
        return;
    };
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let id = archive.info().id;
    let marker = dir.join(format!("snapshot-{id}"));
    if std::fs::read_to_string(&marker).is_ok_and(|d| d.trim() == today) {
        return;
    }
    let mut event = (0, 0, 0, 0, 0, 0, 0);
    for signal in signals {
        match signal {
            Signal::Uncertain { low, medium, .. } => {
                event.0 += low;
                event.1 += medium;
            }
            Signal::Crowded { .. } => event.2 += 1,
            Signal::Single { .. } => event.3 += 1,
            Signal::SpreadTag { .. } => event.4 += 1,
            Signal::Entangled { .. } => event.5 += 1,
            Signal::UnevenDetail { .. } => event.6 += 1,
            Signal::Holding { .. } => {}
        }
    }
    let holding = items
        .iter()
        .filter(|i| i.card == "Holding" || i.card.starts_with("Holding/"))
        .count();
    note(
        archive,
        Event::Snapshot {
            items: items.len(),
            holding,
            low: event.0,
            medium: event.1,
            crowded: event.2,
            single: event.3,
            spread_tags: event.4,
            entangled: event.5,
            uneven: event.6,
        },
    );
    let _ = std::fs::write(marker, today + "\n");
}

/// Logs a search of `archive` that found `searched`, for `query`. Only the kind of search and
/// the results' identifiers are kept, never the words.
pub fn search(
    archive: &Archive,
    query: &crate::search::query::Query,
    results: &crate::search::Results,
) {
    use super::record::SearchMode;
    let mode = if query.text.trim().is_empty() {
        SearchMode::Filters
    } else if results.meaning == crate::search::Meaning::Off {
        SearchMode::Words
    } else {
        SearchMode::Both
    };
    let items = results
        .hits
        .iter()
        .filter_map(|h| match &h.found {
            crate::search::Found::Item(item) => Some(item.id),
            _ => None,
        })
        .collect();
    super::searched(archive, mode, results.total, items);
}
