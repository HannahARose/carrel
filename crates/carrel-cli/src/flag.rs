//! `carrel flag`, `carrel unflag` and `carrel flagged`: marking items and collections that
//! need work, with a note (FORMAT.md §5.9), and listing them. Each flag is a change undo can
//! reverse. After a correction or an edit is saved on a flagged card, [`offer_to_clear`] asks
//! whether it settled what the flag was for.

use std::fmt::Write as _;
use std::io::IsTerminal as _;
use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::flags::{self, Flaggable, Flagged};
use serde_json::json;

use crate::commands::{find_item, holder, locate};
use crate::config::Identity;
use crate::{Failure, Report};

/// What `query` names: a collection, by its folder (or its design document), else an item,
/// as other commands find one.
fn flaggable(archive: &Archive, query: &str) -> Result<Flaggable, Failure> {
    let root = std::fs::canonicalize(archive.root()).ok();
    let relative = std::fs::canonicalize(query)
        .ok()
        .zip(root)
        .and_then(|(path, root)| {
            path.strip_prefix(&root)
                .ok()
                .map(|r| r.to_string_lossy().replace('\\', "/"))
        })
        .unwrap_or_else(|| query.replace('\\', "/"));
    let wanted = relative
        .trim_end_matches("/_collection.md")
        .trim_matches('/')
        .to_owned();
    if let Some(collection) = archive
        .collections()?
        .into_iter()
        .find(|c| c.path.eq_ignore_ascii_case(&wanted))
    {
        return Ok(Flaggable::collection(&collection.path));
    }
    let items = archive.items()?;
    Ok(Flaggable::item(find_item(archive, &items, query)?))
}

/// `carrel flag ITEM|COLLECTION [--note TEXT]`.
///
/// # Errors
///
/// [`Failure`] if nothing matches, the note is too long, the item is checked out, or the
/// change can't be written.
pub fn flag(archive: Option<&Path>, query: &str, note: Option<&str>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let what = flaggable(&archive, query)?;
    let identity = Identity::load_or_create()?;
    let done = flags::flag(&archive, &what, note, holder(&identity))?;
    let text = if done.changed {
        format!(
            "{}; undo it from History or with `carrel undo`.\n",
            done.summary
        )
    } else {
        "It's already flagged, with that note.\n".to_owned()
    };
    Ok(Report::new(
        text,
        json!({"flagged": what, "note": note.map(str::trim), "changed": done.changed, "commit": done.commit}),
    ))
}

/// `carrel unflag ITEM|COLLECTION`.
///
/// # Errors
///
/// As [`flag`].
pub fn unflag(archive: Option<&Path>, query: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let what = flaggable(&archive, query)?;
    let identity = Identity::load_or_create()?;
    let done = flags::unflag(&archive, &what, holder(&identity))?;
    let text = if done.changed {
        format!(
            "{}; undo it from History or with `carrel undo`.\n",
            done.summary
        )
    } else {
        "It isn't flagged.\n".to_owned()
    };
    Ok(Report::new(
        text,
        json!({"unflagged": what, "changed": done.changed, "commit": done.commit}),
    ))
}

/// `carrel flagged`: everything flagged, collections first.
///
/// # Errors
///
/// [`Failure`] if the archive can't be read.
pub fn list(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let flagged = flags::all(&archive)?;
    let text = if flagged.is_empty() {
        "Nothing is flagged as needing work. Flag something with `carrel flag ITEM --note \"what\"`.\n"
            .to_owned()
    } else {
        lines(&flagged)
    };
    Ok(Report::new(text, json!({ "flagged": flagged })))
}

/// One line each, under a heading.
fn lines(flagged: &[Flagged]) -> String {
    let mut text = String::from("Flagged as needing work:\n");
    for f in flagged {
        let what = flags::describe(&f.flag);
        match &f.what {
            Flaggable::Collection { path } => writeln!(text, "  ⚑ {path}/ (collection): {what}"),
            Flaggable::Item { card, .. } => writeln!(text, "  ⚑ {card}: {what}"),
        }
        .unwrap();
    }
    text
}

/// The part of `carrel status` about what's flagged.
pub fn status_section(flagged: &[Flagged]) -> String {
    if flagged.is_empty() {
        String::new()
    } else {
        format!("\n{}", lines(flagged))
    }
}

/// After a correction or an edit of `item` was saved, when its card is flagged: asks on the
/// terminal whether to clear the flag, or else says how. Adds what happened to `report`.
///
/// # Errors
///
/// [`Failure`] if the flag can't be cleared.
pub fn offer_to_clear(archive: &Archive, item: Id, report: &mut Report) -> Result<(), Failure> {
    let Some(found) = archive.items()?.into_iter().find(|i| i.id == item) else {
        return Ok(());
    };
    let Some(flag) = flags::of_item(archive, &found) else {
        return Ok(());
    };
    let said = flags::describe(&flag);
    let ask = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    if ask && crate::models::confirm(&format!("It's flagged: {said}. Clear the flag?"), false)? {
        let identity = Identity::load_or_create()?;
        let done = flags::unflag(archive, &Flaggable::item(&found), holder(&identity))?;
        report.text.push_str("Cleared its flag.\n");
        report.json["flag_cleared"] = json!(done.commit);
    } else {
        writeln!(
            report.text,
            "It's still flagged: {said}. Clear it with `carrel unflag {item}`."
        )
        .unwrap();
        report.json["flagged"] = json!(flag);
    }
    Ok(())
}
