//! `carrel sessions`: work sessions found on checked-out items, waiting as pending history
//! entries, and approving, combining or dropping them (`docs/design/checkout.md`).

use std::fmt::Write as _;
use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::card_file::CardFile;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::history::{Change, History, Operation};
use serde_json::json;

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
    let recorded = carrel_core::sessions::record(&archive, &lease);
    lease.release()?;
    let recorded = recorded?;
    let mut text = String::new();
    for (_, title, n) in &recorded.items {
        writeln!(text, "Found {n} new session(s) on “{title}”").unwrap();
    }
    let mut items = Vec::new();
    for item in archive.items()? {
        let Ok(card) = std::fs::read_to_string(archive.root().join(&item.card)) else {
            continue;
        };
        let Ok(file) = CardFile::parse(&card) else {
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
            "{}  {}",
            &item.id.to_string()[24..],
            file.card().title
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
        items.push(json!({ "item": item.id, "title": file.card().title, "pending": entries }));
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
        json!({ "recorded": recorded.commit, "items": items }),
    ))
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
    let items = archive.items()?;
    let item = find_item(&archive, &items, query)?;
    let path = archive.root().join(&item.card);
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
    let commit = write_and_commit(&archive, &path, &file, item.id, &summary)?;
    Ok(Report::new(
        format!("{summary}\n"),
        json!({ "item": item.id, "entries": chosen.len(), "commit": commit }),
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
    item: carrel_core::card::Id,
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
