//! `carrel sessions`: work sessions found on checked-out and desk items, waiting as pending
//! history entries, and approving, combining or dropping them (`docs/design/checkout.md`).
//! Commands that change the archive look for finished sessions first, every few minutes.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::card_file::CardFile;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, WriterLease};
use carrel_core::desk::Desk;
use carrel_core::history::{Change, History, Operation};
use carrel_core::sessions::Scanned;
use serde_json::{Value, json};

use crate::commands::{find_item, holder, locate};
use crate::config::Identity;
use crate::{Failure, Report};

/// What to do with an item's pending entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Approve them.
    Approve,
    /// Drop them.
    Drop,
    /// Combine them into one, still pending.
    Combine,
}

/// How often running Carrel looks for finished sessions, at most.
pub const EVERY: Duration = Duration::from_mins(5);

/// Records finished sessions, holding a lease of its own, and takes their snapshots once it's
/// released: the lines to show, and the JSON.
fn record_and_snapshot(archive: &Archive, lease: WriterLease) -> Result<(String, Value), Failure> {
    let recorded = carrel_core::sessions::record(archive, &lease);
    lease.release()?;
    let recorded = recorded?;
    let mut text = String::new();
    for item in &recorded.items {
        writeln!(
            text,
            "Found {} new session(s) on “{}”",
            item.sessions, item.title
        )
        .unwrap();
    }
    let mut snapshots = Vec::new();
    if !recorded.items.is_empty()
        && let (Some(kopia), _) = crate::backup::for_batch(archive)
    {
        for (id, result) in carrel_core::sessions::snapshot(&recorded, &kopia) {
            match result {
                Ok(snapshot) => snapshots.push(json!({ "item": id, "snapshot": snapshot.id })),
                Err(e) => {
                    writeln!(text, "No snapshot of the working copy was taken: {e}").unwrap();
                    snapshots.push(json!({ "item": id, "error": e.to_string() }));
                }
            }
        }
    }
    Ok((
        text,
        json!({
            "commit": recorded.commit,
            "sessions": recorded.sessions(),
            "snapshots": snapshots,
        }),
    ))
}

/// Records finished sessions when it's been [`EVERY`] since they were last looked for, unless
/// another writer holds the archive: `None` when it didn't look, else what it found, as lines
/// for the owner (empty when nothing).
pub fn record_if_due(archive: Option<&Path>) -> Option<Result<String, Failure>> {
    let archive = locate(archive).ok()?;
    let due = Scanned::path(&archive.doc().id.to_string())
        .is_none_or(|p| Scanned::load(&p).due(EVERY, SystemTime::now()));
    if !due {
        return None;
    }
    let identity = Identity::load_or_create().ok()?;
    let lease = DiskLock::for_archive(&archive)
        .acquire(holder(&identity), "sessions", DEFAULT_LEASE)
        .ok()?;
    Some(record_and_snapshot(&archive, lease).map(|(text, _)| text))
}

/// Before a command that changes the archive: [`record_if_due`], with what it found on
/// standard error unless `quiet`. Nothing here stops the command.
pub fn record_before_command(archive: Option<&Path>, quiet: bool) {
    match record_if_due(archive) {
        Some(Ok(text)) if !quiet && !text.is_empty() => {
            eprintln!("{text}Approve them with `carrel sessions`.");
        }
        Some(Err(Failure(e))) if !quiet => {
            eprintln!("warning: work sessions weren't recorded: {e}");
        }
        _ => {}
    }
}

/// A card with history entries: an item's in the archive, or a desk item's beside it.
struct Card {
    id: Id,
    path: PathBuf,
    /// Whether it's in the archive, so its changes go in the history.
    in_archive: bool,
}

/// Every card that can have pending entries: the archive's, then this device's desk items'.
fn cards(archive: &Archive) -> Result<Vec<Card>, Failure> {
    let mut cards: Vec<Card> = archive
        .items()?
        .into_iter()
        .map(|item| Card {
            id: item.id,
            path: archive.root().join(&item.card),
            in_archive: true,
        })
        .collect();
    let identity = Identity::load_or_create()?;
    cards.extend(
        Desk::load(archive, identity.device)?
            .items
            .into_iter()
            .map(|entry| Card {
                id: entry.id,
                path: entry.card,
                in_archive: false,
            }),
    );
    Ok(cards)
}

/// `carrel sessions`: records finished sessions, then lists every pending entry.
///
/// # Errors
///
/// [`Failure`] if the archive can't be read or written.
pub fn list(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "sessions", DEFAULT_LEASE)?;
    let (mut text, recorded) = record_and_snapshot(&archive, lease)?;
    let mut items = Vec::new();
    for card in cards(&archive)? {
        let Ok(text_of_card) = std::fs::read_to_string(&card.path) else {
            continue;
        };
        let Ok(file) = CardFile::parse(&text_of_card) else {
            continue;
        };
        let Ok(log) = file.history() else {
            continue;
        };
        let pending = log.pending();
        if pending.is_empty() {
            continue;
        }
        writeln!(
            text,
            "{}  {}{}",
            &card.id.to_string()[24..],
            file.card().title,
            if card.in_archive {
                ""
            } else {
                " (on the desk)"
            }
        )
        .unwrap();
        let mut entries = Vec::new();
        for (n, &i) in pending.iter().enumerate() {
            let entry = &log.entries[i];
            writeln!(
                text,
                "  {}. {}{}  {}",
                n + 1,
                entry.date,
                entry
                    .duration
                    .as_ref()
                    .map(|d| format!(" · {d}"))
                    .unwrap_or_default(),
                entry.lines.first().map_or("", String::as_str)
            )
            .unwrap();
            entries.push(json!({
                "number": n + 1,
                "date": entry.date,
                "duration": entry.duration,
                "lines": entry.lines,
            }));
        }
        items.push(json!({
            "item": card.id,
            "title": file.card().title,
            "desk": !card.in_archive,
            "pending": entries,
        }));
    }
    if items.is_empty() {
        text.push_str("No pending history entries\n");
    } else {
        text.push_str(
            "\nApprove with `carrel sessions approve ITEM [N...]`, or combine or drop them.\n",
        );
    }
    Ok(Report::new(
        text,
        json!({
            "recorded": recorded["commit"],
            "sessions": recorded["sessions"],
            "snapshots": recorded["snapshots"],
            "items": items,
        }),
    ))
}

/// The card `query` names: an archive item as everywhere else, or a desk item by its
/// identifier, its path or its card's path.
fn find_card(archive: &Archive, query: &str) -> Result<Card, Failure> {
    let items = archive.items()?;
    let in_archive = find_item(archive, &items, query);
    if let Ok(item) = in_archive {
        return Ok(Card {
            id: item.id,
            path: archive.root().join(&item.card),
            in_archive: true,
        });
    }
    let identity = Identity::load_or_create()?;
    let wanted = std::fs::canonicalize(crate::paths::expand(query)).ok();
    Desk::load(archive, identity.device)?
        .items
        .into_iter()
        .find(|e| {
            e.id.to_string() == query
                || wanted
                    .as_ref()
                    .is_some_and(|w| *w == e.path || *w == e.card)
        })
        .map(|e| Card {
            id: e.id,
            path: e.card,
            in_archive: false,
        })
        .ok_or_else(|| {
            in_archive
                .err()
                .unwrap_or_else(|| Failure::new("not found"))
        })
}

/// `carrel sessions approve|drop|combine ITEM [N...]`: `N` are the numbers `carrel sessions`
/// shows; none means all of the item's pending entries.
///
/// # Errors
///
/// [`Failure`] if the item or an entry isn't found.
pub fn decide(
    archive: Option<&Path>,
    query: &str,
    numbers: &[usize],
    decision: Decision,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let card = find_card(&archive, query)?;
    let path = card.path;
    let mut file = CardFile::parse(&std::fs::read_to_string(&path)?)?;
    let mut log = file.history()?;
    let pending = log.pending();
    let chosen: Vec<usize> = if numbers.is_empty() {
        pending.clone()
    } else {
        numbers
            .iter()
            .map(|&n| {
                pending.get(n.wrapping_sub(1)).copied().ok_or_else(|| {
                    Failure::new(format!(
                        "there's no pending entry {n}; `carrel sessions` lists them"
                    ))
                })
            })
            .collect::<Result<_, _>>()?
    };
    if chosen.is_empty() {
        return Err(Failure::new("no pending entries to decide"));
    }
    let title = file.card().title.clone();
    let summary = match decision {
        Decision::Approve => {
            for &i in &chosen {
                log.approve(i)?;
            }
            format!("Approve {} of “{title}”", entries(chosen.len()))
        }
        Decision::Drop => {
            // From the last, so earlier positions stay right.
            let mut sorted = chosen.clone();
            sorted.sort_unstable();
            for &i in sorted.iter().rev() {
                log.drop_entry(i)?;
            }
            format!("Drop {} of “{title}”", entries(chosen.len()))
        }
        Decision::Combine => {
            log.combine(&chosen)?;
            format!("Combine {} of “{title}”", entries(chosen.len()))
        }
    };
    file.set_history(&log)?;
    let commit = write_and_commit(&archive, &path, &file, card.id, &summary)?;
    Ok(Report::new(
        format!("{summary}\n"),
        json!({ "item": card.id, "entries": chosen.len(), "commit": commit }),
    ))
}

/// `1 history entry`, `2 history entries`.
fn entries(n: usize) -> String {
    if n == 1 {
        "1 history entry".to_owned()
    } else {
        format!("{n} history entries")
    }
}

fn write_and_commit(
    archive: &Archive,
    path: &Path,
    file: &CardFile,
    item: Id,
    summary: &str,
) -> Result<Option<String>, Failure> {
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(archive).acquire(holder(&identity), "sessions", DEFAULT_LEASE)?;
    let result = (|| -> Result<Option<String>, Failure> {
        let temp = path.with_extension("md.tmp-sessions");
        std::fs::write(&temp, file.to_text()?)?;
        std::fs::rename(&temp, path)?;
        let mut change = Change::new(
            Operation::Session,
            summary,
            lease.info().person,
            lease.info().device,
        );
        change.items = vec![item];
        Ok(History::open(archive)?.commit(archive, &change, &lease)?)
    })();
    lease.release()?;
    result
}
