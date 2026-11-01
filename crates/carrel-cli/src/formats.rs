//! `carrel formats`: which file types this device reads, and by which provider, then the log of
//! types it couldn't read (`docs/design/models-3d.md`).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use carrel_core::formats::{Unreadable, hint};
use carrel_core::ingest::run::Run;
use carrel_core::providers::Capability;
use serde_json::json;

use crate::Report;

/// Counts the files in `run` whose text couldn't be read in this device's log. Never in
/// unit tests, which would write the owner's own log; the CLI's tests give Carrel a folder of
/// their own instead.
pub fn record(run: &Run) {
    if cfg!(test) {
        return;
    }
    let Some(path) = Unreadable::path() else {
        return;
    };
    let mut log = Unreadable::load(&path);
    let before = log.clone();
    log.record(run, &jiff::Zoned::now().date().to_string());
    if log != before {
        let _ = log.save(&path);
    }
}

/// `carrel formats`.
#[must_use]
pub fn list() -> Report {
    list_with(&crate::providers::registry().0)
}

/// `carrel formats`, for the providers in `registry`.
#[must_use]
pub fn list_with(registry: &carrel_core::providers::Registry) -> Report {
    let mut readers: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (provider, offer) in registry.ranked(Capability::ExtractText) {
        for media_type in &offer.media_types {
            readers.entry(media_type.clone()).or_default().push(format!(
                "{}/{}",
                provider.name(),
                offer.id
            ));
        }
    }
    let mut text = String::from("This device reads:\n");
    for (media_type, offers) in &readers {
        writeln!(text, "  {media_type:<28} {}", offers.join(", ")).unwrap();
    }
    let log = Unreadable::path()
        .map(|p| Unreadable::load(&p))
        .unwrap_or_default();
    if log.entries.is_empty() {
        text.push_str("\nNo file types it couldn't read so far.\n");
    } else {
        text.push_str("\nCouldn't read, most frequent first:\n");
        for entry in &log.entries {
            let extension = if entry.extension.is_empty() {
                "(no extension)".to_owned()
            } else {
                format!(".{}", entry.extension)
            };
            writeln!(
                text,
                "  {extension:<10} {:<28} {:>4} item{}  {}  (since {})",
                entry.media_type,
                entry.count,
                if entry.count == 1 { " " } else { "s" },
                entry.why.describe(),
                entry.first_seen
            )
            .unwrap();
            if let Some(hint) =
                hint(&entry.media_type).filter(|_| !readers.contains_key(&entry.media_type))
            {
                writeln!(text, "             {hint}").unwrap();
            }
        }
    }
    Report::new(
        text,
        json!({
            "reads": readers,
            "unreadable": log.entries,
        }),
    )
}
