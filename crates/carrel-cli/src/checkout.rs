//! `carrel checkout` and `carrel checkouts`: taking items out of the archive to work on, and
//! what's out (`docs/design/checkout.md`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use carrel_core::card::DateOrTime;
use carrel_core::card_file::CardFile;
use carrel_core::checkout::{
    CheckoutError, cancel_checkout, check_out, compare, finish_return, manifest_path, plan_return,
};
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::manifest::Manifest;
use serde_json::json;

use crate::commands::{find_item, holder, locate};
use crate::config::Identity;
use crate::{Failure, Report};

/// Where checkouts go when no folder is given: the Desktop.
fn default_destination() -> Result<PathBuf, Failure> {
    carrel_core::local::home_dir()
        .map(|home| home.join("Desktop"))
        .ok_or_else(|| Failure::new("no home folder to check out to; give one with --to"))
}

/// `carrel checkout ITEM [--to FOLDER] [--due DATE]`.
///
/// # Errors
///
/// [`Failure`] if the item isn't found or can't come out.
pub fn checkout(
    archive: Option<&Path>,
    query: &str,
    to: Option<&Path>,
    due: Option<&str>,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let items = archive.items()?;
    let item = find_item(&archive, &items, query)?;
    let destination = match to {
        Some(to) => crate::paths::expand(&to.to_string_lossy()),
        None => default_destination()?,
    };
    let due = due
        .map(|d| {
            d.parse::<jiff::civil::Date>()
                .map(|date| DateOrTime(date.to_string()))
                .map_err(|_| Failure::new(format!("{d} isn't a date; give it as YYYY-MM-DD")))
        })
        .transpose()?;
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "checkout", DEFAULT_LEASE)?;
    let result = check_out(&archive, item, &destination, due, &lease);
    lease.release()?;
    let out = result?;
    Ok(Report::new(
        format!(
            "Checked out “{}” to {}\nThe archive copy is read-only until it's returned.\n",
            out.title,
            out.working.display(),
        ),
        json!({
            "item": out.item,
            "title": out.title,
            "working": out.working,
            "commit": out.commit,
        }),
    ))
}

/// `carrel checkouts`: what's out, where, since when, and whether it has changed.
///
/// # Errors
///
/// [`Failure`] if the archive can't be read.
pub fn list(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let rules = archive.rules().map_err(|e| Failure::new(e.to_string()))?;
    let mut text = String::new();
    let mut out = Vec::new();
    for item in archive.items()? {
        let path = archive.root().join(&item.card);
        let Ok(card_text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(file) = CardFile::parse(&card_text) else {
            continue;
        };
        let card = file.card();
        let Some(checked) = &card.checked_out else {
            continue;
        };
        let working = PathBuf::from(&checked.to);
        let state = std::fs::read_to_string(manifest_path(&archive, item.id))
            .ok()
            .and_then(|t| Manifest::parse(&t).ok())
            .map_or("not compared (no saved manifest)", |then| {
                match compare(&then, &working, &rules) {
                    Ok(diff) if diff.is_empty() => "unchanged",
                    Ok(_) => "changed",
                    Err(CheckoutError::Missing(_)) => "missing",
                    Err(_) => "not compared",
                }
            });
        writeln!(
            text,
            "{}  {}\n  at {} since {}{}  ({state})",
            &item.id.to_string()[24..],
            card.title,
            checked.to,
            checked.since.0,
            checked
                .due
                .as_ref()
                .map(|d| format!(", due {}", d.0))
                .unwrap_or_default()
        )
        .unwrap();
        out.push(json!({
            "item": item.id,
            "title": card.title,
            "card": item.card,
            "to": checked.to,
            "since": checked.since.0,
            "due": checked.due.as_ref().map(|d| d.0.clone()),
            "state": state,
        }));
    }
    if out.is_empty() {
        text.push_str("Nothing is checked out\n");
    }
    Ok(Report::new(text, json!({ "checked_out": out })))
}

/// `carrel return ITEM [--from PATH] [--yes]`, or `--discard` to cancel: shows what changed
/// and the history entry it adds, asks, then brings the working copy back.
///
/// # Errors
///
/// [`Failure`] if the item isn't checked out or can't be returned.
pub fn return_item(
    archive: Option<&Path>,
    query: &str,
    from: Option<&Path>,
    yes: bool,
    discard: bool,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let items = archive.items()?;
    let item = find_item(&archive, &items, query)?;
    let identity = Identity::load_or_create()?;
    if discard {
        if !crate::models::confirm(
            "Cancel the checkout? The archive copy is unlocked as it was, and the working copy's changes aren't brought back.",
            yes,
        )? {
            return Err(Failure::new("nothing changed"));
        }
        let lease =
            DiskLock::for_archive(&archive).acquire(holder(&identity), "return", DEFAULT_LEASE)?;
        let result = cancel_checkout(&archive, item, &lease);
        lease.release()?;
        let commit = result?;
        return Ok(Report::new(
            "Cancelled the checkout; the archive copy is as it was, and the working copy is still where it was.\n",
            json!({ "item": item.id, "cancelled": true, "commit": commit }),
        ));
    }
    let from = from.map(|p| crate::paths::expand(&p.to_string_lossy()));
    let plan = plan_return(&archive, item, from.as_deref())?;
    let mut text = format!(
        "Returning “{}” from {}\n",
        plan.title,
        plan.working.display()
    );
    let d = &plan.diff;
    if d.is_empty() {
        text.push_str("  nothing changed\n");
    }
    for (label, list) in [
        ("changed", &d.changed),
        ("added", &d.added),
        ("removed", &d.removed),
    ] {
        for path in list.iter().take(20) {
            writeln!(text, "  {label:8} {path}").unwrap();
        }
    }
    for (from, to) in d.renamed.iter().take(20) {
        writeln!(text, "  renamed  {from} → {to}").unwrap();
    }
    if !plan.commits.is_empty() {
        writeln!(text, "  {} commit(s) since checkout", plan.commits.len()).unwrap();
    }
    if plan.uncommitted {
        text.push_str("  warning: the repository has changes that aren't committed\n");
    }
    text.push_str("\nHistory entry:\n");
    for line in &plan.entry.lines {
        writeln!(text, "  {line}").unwrap();
    }
    eprint!("{text}");
    if !crate::models::confirm("\nReturn it, replacing the archive copy?", yes)? {
        return Err(Failure::new("nothing returned"));
    }
    let (backups, _) = crate::backup::for_batch(&archive);
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "return", DEFAULT_LEASE)?;
    let result = finish_return(
        &archive,
        &plan,
        &lease,
        backups
            .as_ref()
            .map(|b| b as &dyn carrel_core::backup::Backups),
    );
    lease.release()?;
    let returned = result?;
    Ok(Report::new(
        format!(
            "Returned “{}”. The working copy is still at {}; delete it when you're ready.\n",
            plan.title,
            plan.working.display()
        ),
        json!({
            "item": plan.item,
            "commit": returned.commit,
            "snapshot": returned.snapshot,
            "working": plan.working,
            "changed": d.changed, "added": d.added, "removed": d.removed,
            "renamed": d.renamed,
            "commits": plan.commits,
        }),
    ))
}
