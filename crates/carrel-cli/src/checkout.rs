//! `carrel checkout` and `carrel checkouts`: taking items out of the archive to work on, and
//! what's out (`docs/design/checkout.md`).

use std::fmt::Write as _;
use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};

use carrel_core::archive::{Archive, ItemRef};
use carrel_core::backup::{Kopia, Snapshot};
use carrel_core::card::DateOrTime;
use carrel_core::card_file::CardFile;
use carrel_core::checkout::{ReturnPlan, cancel_checkout, check_out, finish_return, plan_return};
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use serde_json::{Value, json};

use crate::commands::{find_item, holder, locate};
use crate::config::Identity;
use crate::{Failure, Report};

/// Where checkouts go when no folder is given: `checkout.destination`, the Desktop unless
/// it's changed.
fn default_destination(archive: &carrel_core::archive::Archive) -> Result<PathBuf, Failure> {
    let home = carrel_core::local::home_dir();
    carrel_core::settings::checkout_folder(archive.settings(), home.as_deref())
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
        None => default_destination(&archive)?,
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
        let (state, _) = compared(&archive, &rules, item.id, Path::new(&checked.to));
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

pub use carrel_core::checkout::compared;

/// `carrel return ITEM [--from PATH] [--yes] [--trim-snapshots]`, or `--discard` to cancel:
/// shows what changed and the history entry it adds, asks, then brings the working copy back.
/// With backups, it then offers to forget the checkout's session snapshots.
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
    trim: bool,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let items = archive.items()?;
    let item = find_item(&archive, &items, query)?;
    if discard {
        if !crate::models::confirm(
            "Cancel the checkout? The archive copy is unlocked as it was, and the working copy's changes aren't brought back.",
            yes,
        )? {
            return Err(Failure::new("nothing changed"));
        }
        return cancel(&archive, item);
    }
    let from = from.map(|p| crate::paths::expand(&p.to_string_lossy()));
    let plan = plan_return(&archive, item, from.as_deref())?;
    let (backups, _) = crate::backup::for_batch(&archive);
    let sessions = backups
        .as_ref()
        .map(|kopia| session_snapshots(&archive, item, kopia))
        .unwrap_or_default();
    let mut text = format!(
        "Returning “{}” from {}\n{}",
        plan.title,
        plan.working.display(),
        describe_return(&plan)
    );
    if !sessions.is_empty() {
        writeln!(
            text,
            "\n{} session snapshot(s) of the working copy, which can be forgotten once it's returned",
            sessions.len()
        )
        .unwrap();
    }
    eprint!("{text}");
    if !crate::models::confirm("\nReturn it, replacing the archive copy?", yes)? {
        return Err(Failure::new("nothing returned"));
    }
    let mut report = finish(&archive, &plan)?;
    if let Some(kopia) = backups.as_ref().filter(|_| !sessions.is_empty()) {
        let (trimmed, trimming) = trim_sessions(kopia, &plan, &sessions, trim, yes)?;
        report.text.push_str(&trimmed);
        report.json["session_snapshots"] = trimming;
    }
    Ok(report)
}

/// What returning by `plan` changes, and the history entry it adds, as `carrel return` shows
/// them before asking.
pub fn describe_return(plan: &ReturnPlan) -> String {
    let mut text = String::new();
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
    text
}

/// Returns the item `query` names, from where it was checked out to or from `from` if it
/// moved, without asking: what `carrel return --yes [--from PATH] [--trim-snapshots]` does,
/// and `r` on the Active screen once the owner has said yes. With `trim` and backups, the
/// checkout's session snapshots are forgotten once the returned version is snapshotted.
///
/// # Errors
///
/// [`Failure`] if the item isn't checked out or can't be returned.
pub fn return_now(
    archive: Option<&Path>,
    query: &str,
    from: Option<&Path>,
    trim: bool,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let items = archive.items()?;
    let item = find_item(&archive, &items, query)?;
    let from = from.map(|p| crate::paths::expand(&p.to_string_lossy()));
    let plan = plan_return(&archive, item, from.as_deref())?;
    let (backups, _) = crate::backup::for_batch(&archive);
    let sessions = backups
        .as_ref()
        .filter(|_| trim)
        .map(|kopia| session_snapshots(&archive, item, kopia))
        .unwrap_or_default();
    let mut report = finish(&archive, &plan)?;
    if let Some(kopia) = backups.as_ref().filter(|_| !sessions.is_empty()) {
        let (trimmed, trimming) = trim_sessions(kopia, &plan, &sessions, true, true)?;
        report.text.push_str(&trimmed);
        report.json["session_snapshots"] = trimming;
    }
    Ok(report)
}

/// Cancels the checkout of the item `query` names, without asking: what
/// `carrel return --discard --yes` does.
///
/// # Errors
///
/// [`Failure`] if the item isn't checked out or can't be written.
pub fn cancel_now(archive: Option<&Path>, query: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let items = archive.items()?;
    let item = find_item(&archive, &items, query)?;
    cancel(&archive, item)
}

fn cancel(archive: &Archive, item: &ItemRef) -> Result<Report, Failure> {
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(archive).acquire(holder(&identity), "return", DEFAULT_LEASE)?;
    let result = cancel_checkout(archive, item, &lease);
    lease.release()?;
    let commit = result?;
    Ok(Report::new(
        "Cancelled the checkout; the archive copy is as it was, and the working copy is still where it was.\n",
        json!({ "item": item.id, "cancelled": true, "commit": commit }),
    ))
}

/// Brings the working copy back by `plan`, holding the writer lease.
fn finish(archive: &Archive, plan: &ReturnPlan) -> Result<Report, Failure> {
    let identity = Identity::load_or_create()?;
    let (backups, _) = crate::backup::for_batch(archive);
    let lease =
        DiskLock::for_archive(archive).acquire(holder(&identity), "return", DEFAULT_LEASE)?;
    let result = finish_return(
        archive,
        plan,
        &lease,
        backups
            .as_ref()
            .map(|b| b as &dyn carrel_core::backup::Backups),
    );
    lease.release()?;
    let returned = result?;
    let d = &plan.diff;
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

/// The session snapshots taken of `item`'s working copy, where it was checked out to, since
/// then; none when they can't be listed.
fn session_snapshots(archive: &Archive, item: &ItemRef, kopia: &Kopia) -> Vec<Snapshot> {
    carrel_core::sessions::return_snapshots(archive, item, kopia)
}

/// After a return: snapshots the working copy, which is now the returned version, then
/// forgets the checkout's session `snapshots` if the owner says so: `trim`, or asked on a
/// terminal. Without either, as with only `--yes`, they're kept; deleting backups is the
/// owner's call. What happened, as lines and JSON.
fn trim_sessions(
    kopia: &Kopia,
    plan: &ReturnPlan,
    snapshots: &[Snapshot],
    trim: bool,
    yes: bool,
) -> Result<(String, Value), Failure> {
    let forget = trim
        || (!yes
            && std::io::stdin().is_terminal()
            && crate::models::confirm(
                &format!(
                    "Forget the {} session snapshot(s) of the working copy, keeping the return's?",
                    snapshots.len()
                ),
                false,
            )?);
    let kept = carrel_core::sessions::keep_returned_version(
        kopia,
        &plan.working,
        &plan.title,
        plan.item,
        snapshots,
        forget,
    );
    let returned = match kept.returned {
        Ok(snapshot) => snapshot,
        Err(e) => {
            return Ok((
                format!(
                    "The {} session snapshot(s) are kept: the returned version couldn't be snapshotted ({e}).\n",
                    snapshots.len()
                ),
                json!({ "kept": snapshots.len(), "error": e }),
            ));
        }
    };
    let forgotten = kept.forgotten;
    let mut text = String::new();
    for (id, e) in &kept.failures {
        writeln!(text, "Couldn't forget snapshot {id}: {e}").unwrap();
    }
    writeln!(
        text,
        "Snapshot {} holds the returned version; {}.",
        returned.trailer(),
        if forget {
            format!("{forgotten} session snapshot(s) forgotten")
        } else {
            format!(
                "the {} session snapshot(s) are kept (`--trim-snapshots` forgets them)",
                snapshots.len()
            )
        }
    )
    .unwrap();
    Ok((
        text,
        json!({
            "returned": returned.id,
            "forgotten": forgotten,
            "kept": snapshots.len() - forgotten,
        }),
    ))
}
