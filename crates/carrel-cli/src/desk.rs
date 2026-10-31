//! `carrel desk`: files and folders being worked on where they are, tracked without being
//! archived (`docs/design/checkout.md`).

use std::fmt::Write as _;
use std::path::Path;

use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::desk::{Desk, add, remove};
use serde_json::json;

use crate::commands::{holder, locate};
use crate::config::Identity;
use crate::{Failure, Report};

/// `carrel desk`: this device's desk items, and whether each is still there.
///
/// # Errors
///
/// [`Failure`] if the archive or the desk can't be read.
pub fn list(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let identity = Identity::load_or_create()?;
    let desk = Desk::load(&archive, identity.device)?;
    let mut text = String::new();
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
    Ok(Report::new(text, json!({ "items": desk.items })))
}

/// `carrel desk add PATH [--title] [--summary]`.
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
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "desk", DEFAULT_LEASE)?;
    let result = add(&archive, &path, title, summary, &lease);
    lease.release()?;
    let entry = result?;
    Ok(Report::new(
        format!(
            "Put {} on the desk, with its card at {}\n",
            entry.path.display(),
            entry.card.display()
        ),
        json!(entry),
    ))
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
