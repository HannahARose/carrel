//! `carrel desk`: files and folders being worked on where they are, tracked without being
//! archived (`docs/design/checkout.md`).

use std::fmt::Write as _;
use std::path::Path;

use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::desk::{Desk, add, archive as archive_item, draft, remove};
use serde_json::json;

use crate::commands::{holder, locate};
use crate::config::Identity;
use crate::{Failure, Report};

/// `carrel desk`: this device's desk items, and whether each is still there. It first
/// rebuilds `Active/`, so notes written in a desk card's mirror in `Active/On desk/` are copied
/// back to the card.
///
/// # Errors
///
/// [`Failure`] if the archive or the desk can't be read, or `Active/` written.
pub fn list(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "desk", DEFAULT_LEASE)?;
    let regenerated = carrel_core::active::regenerate(&archive);
    lease.release()?;
    let regenerated = regenerated?;
    let desk = Desk::load(&archive, identity.device)?;
    let mut text = String::new();
    for card in &regenerated.copied {
        writeln!(
            text,
            "Copied the notes you wrote in its mirror to {}",
            card.display()
        )
        .unwrap();
    }
    if desk.items.is_empty() {
        text.push_str("Nothing is on the desk; add something with `carrel desk add PATH`\n");
    }
    for entry in &desk.items {
        writeln!(
            text,
            "{}{}",
            entry.path.display(),
            if entry.path.exists() {
                ""
            } else {
                "  (missing)"
            }
        )
        .unwrap();
    }
    Ok(Report::new(
        text,
        json!({ "items": desk.items, "copied": regenerated.copied }),
    ))
}

/// `carrel desk add PATH [--title] [--summary]`: the card is drafted by this device's text
/// model, as ingest drafts cards, before the writer lock is taken.
///
/// # Errors
///
/// [`Failure`] if it can't be put on the desk.
pub fn add_item(
    archive: Option<&Path>,
    path: &Path,
    title: Option<&str>,
    summary: Option<&str>,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let path = crate::paths::expand(&path.to_string_lossy());
    let identity = Identity::load_or_create()?;
    let drafted = if title.is_some() && summary.is_some() {
        None
    } else {
        let (registry, _) = crate::providers::registry();
        Some(draft(&archive, &registry, &path, identity.device)?)
    };
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "desk", DEFAULT_LEASE)?;
    let result = add(&archive, &path, title, summary, drafted.as_ref(), &lease);
    lease.release()?;
    let entry = result?;
    let mut text = format!(
        "Put {} on the desk, with its card at {}\n",
        entry.path.display(),
        entry.card.display()
    );
    match drafted.as_ref().map(|d| (&d.generated, &d.problems)) {
        Some((Some(generated), _)) => writeln!(
            text,
            "Its card was drafted by {}; edit the card to change it",
            generated.model.as_deref().unwrap_or("a model")
        )
        .unwrap(),
        Some((None, problems)) if title.is_none() && !problems.is_empty() => {
            writeln!(text, "Its title is its name: {}", problems.join("; ")).unwrap();
        }
        _ => {}
    }
    Ok(Report::new(text, json!(entry)))
}

/// `carrel desk remove PATH [--delete-card]`.
///
/// # Errors
///
/// [`Failure`] if it isn't on the desk.
pub fn remove_item(
    archive: Option<&Path>,
    path: &Path,
    delete_card: bool,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let path = crate::paths::expand(&path.to_string_lossy());
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "desk", DEFAULT_LEASE)?;
    let result = remove(&archive, &path, delete_card, &lease);
    lease.release()?;
    let entry = result?;
    Ok(Report::new(
        format!(
            "Took {} off the desk{}\n",
            entry.path.display(),
            if delete_card {
                ", and deleted its card"
            } else {
                "; its card is still beside it"
            }
        ),
        json!(entry),
    ))
}

/// `carrel desk archive PATH`: queues the desk item for ingest, keeping its card and history,
/// and takes it off the desk. A worker drafts its proposal in the background, as for
/// `carrel ingest`.
///
/// # Errors
///
/// [`Failure`] if it isn't on the desk or can't be queued.
pub fn archive_desk_item(archive: Option<&Path>, path: &Path) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let path = crate::paths::expand(&path.to_string_lossy());
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "desk", DEFAULT_LEASE)?;
    let result = archive_item(&archive, &path, &lease);
    lease.release()?;
    let (entry, intake) = result?;
    let started = crate::worker::start(&archive)?;
    let text = format!(
        "Queued {} for ingest, keeping its card and history, and took it off the desk. {}; review its proposal in `carrel review` when it's ready.\n",
        entry.path.display(),
        if started {
            "A worker is drafting it in the background"
        } else if crate::worker::running(&archive)? {
            "The running worker will draft it"
        } else {
            "No worker is started (CARREL_WORKER is off); run `carrel worker` to draft it"
        }
    );
    Ok(Report::new(
        text,
        json!({ "item": entry, "queued": intake, "worker_started": started }),
    ))
}

/// The part of `carrel status` about the desk: folders on the Desktop or in `~/Code` that look
/// like active work, to put on the desk, and desk items unchanged for 60 days, to archive. With
/// it, what `--json` shows as `desk`.
pub fn suggestions_section(found: &carrel_core::desk::Suggestions) -> (String, serde_json::Value) {
    let mut text = String::new();
    if !found.to_track.is_empty() {
        text.push_str(
            "\nLooks like active work (put it on the desk with `carrel desk add PATH`):\n",
        );
        for t in &found.to_track {
            let why = match (&t.changed, t.git) {
                (Some(changed), true) => format!("a Git repository, changed {changed}"),
                (None, true) => "a Git repository".to_owned(),
                (Some(changed), false) => format!("changed {changed}"),
                (None, false) => String::new(),
            };
            writeln!(text, "  {}  ({why})", t.path.display()).unwrap();
        }
    }
    if !found.to_archive.is_empty() {
        writeln!(
            text,
            "\nOn the desk but unchanged for {} days (archive it with `carrel desk archive PATH`):",
            carrel_core::desk::IDLE_DAYS
        )
        .unwrap();
        for a in &found.to_archive {
            writeln!(
                text,
                "  {}  (last changed {})",
                a.entry.path.display(),
                a.changed
            )
            .unwrap();
        }
    }
    (text, json!(found))
}
