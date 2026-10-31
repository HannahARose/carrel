//! `carrel checkout` and `carrel checkouts`: taking items out of the archive to work on, and
//! what's out (`docs/design/checkout.md`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use carrel_core::card::DateOrTime;
use carrel_core::card_file::CardFile;
use carrel_core::checkout::{CheckoutError, check_out, compare, manifest_path};
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
