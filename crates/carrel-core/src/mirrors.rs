//! `Active/On desk/` (FORMAT.md §9.2, §9.4): a mirror of each desk card, so desk items show
//! in the archive beside what's checked out. Notes the owner writes in a mirror are copied back
//! to the desk card the next time the mirrors are rebuilt.
//!
//! A mirror is the desk card's body after a generated `mirror` section, with frontmatter
//! naming the card and the checksum of its notes when last mirrored. The notes are the body
//! without the `mirror` section and the history section's content: history is written on the
//! desk card, never copied back. If a mirror's notes no longer match that checksum, the owner
//! has edited them, and they replace the card's; if the card's notes changed too, the mirror's
//! are added to the card instead of replacing anything.
//!
//! Desk cards are on the device whose desk they're on. A mirror whose card this device can't
//! read is kept as it is, for that device to refresh.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::archive::{Archive, ArchiveError};
use crate::card::{Checksum, Id};
use crate::card_file::CardFile;
use crate::desk::DeskEntry;
use crate::sections;

/// The folder the mirrors are in, relative to the archive root.
pub const FOLDER: &str = "Active/On desk";

/// The section holding what Carrel writes at the top of a mirror.
const SECTION: &str = "mirror";

/// A mirror's frontmatter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Head {
    /// Always `desk-mirror`.
    carrel: String,
    /// The desk card's identifier.
    id: Id,
    /// Where the desk card is, on its device.
    card: PathBuf,
    /// The checksum of the card's notes when the mirror was written.
    mirrored: String,
}

/// One desk item, for `Active/_index.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnDesk {
    /// Its card's title.
    pub title: String,
    /// Where it is, on its device.
    pub path: PathBuf,
    /// When it was put on the desk.
    pub added: String,
    /// Its mirror's file name in [`FOLDER`].
    pub name: String,
}

/// The mirrors, rebuilt.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Mirrors {
    /// Each mirror's text, by its file name in [`FOLDER`].
    pub files: BTreeMap<String, String>,
    /// The desk items, in the order they were added.
    pub on_desk: Vec<OnDesk>,
    /// Desk cards the owner's notes were copied back to.
    pub copied: Vec<PathBuf>,
}

/// Every device's desk items.
fn all_desks(archive: &Archive) -> Vec<DeskEntry> {
    let Ok(devices) = std::fs::read_dir(archive.root().join("_System/devices")) else {
        return Vec::new();
    };
    let mut entries: Vec<DeskEntry> = devices
        .filter_map(Result::ok)
        .filter_map(|d| std::fs::read_to_string(d.path().join("desk.json")).ok())
        .filter_map(|t| serde_json::from_str::<crate::desk::Desk>(&t).ok())
        .flat_map(|d| d.items)
        .collect();
    entries.sort_by(|a, b| a.added.0.cmp(&b.added.0));
    entries
}

/// The mirrors already in [`FOLDER`], by the desk card's identifier: file name, text and head.
fn existing(archive: &Archive) -> BTreeMap<Id, (String, String, Head)> {
    let mut found = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(archive.root().join(FOLDER)) else {
        return found;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        if let Some(head) = head_of(&text) {
            found.insert(head.id, (name, text, head));
        }
    }
    found
}

fn head_of(text: &str) -> Option<Head> {
    let split = crate::frontmatter::split(text).ok()?;
    serde_saphyr::from_str::<Head>(split.yaml)
        .ok()
        .filter(|h| h.carrel == "desk-mirror")
}

/// `body` without section `name`, markers and all.
fn without(body: &str, name: &str) -> Option<String> {
    let all = sections::find_all(body).ok()?;
    Some(match all.into_iter().find(|s| s.name == name) {
        Some(s) => format!("{}{}", &body[..s.whole.start], &body[s.whole.end..]),
        None => body.to_owned(),
    })
}

/// A body's notes: without the mirror section, and with the history section emptied.
fn notes(body: &str) -> Option<String> {
    let rest = without(body, SECTION)?;
    let rest = match sections::get(&rest, crate::card_file::HISTORY).ok()? {
        Some(_) => sections::replace(&rest, crate::card_file::HISTORY, "").ok()?,
        None => rest,
    };
    Some(sections::normalize_tables(&rest))
}

fn checksum(notes: &str) -> String {
    Checksum::of_bytes(notes.as_bytes()).0
}

/// Rebuilds the mirrors of every device's desk cards, first copying back notes the owner wrote
/// in a mirror to its card on this device.
///
/// # Errors
///
/// [`ArchiveError`] if a desk card can't be written.
pub fn build(archive: &Archive) -> Result<Mirrors, ArchiveError> {
    let mut old = existing(archive);
    let mut mirrors = Mirrors::default();
    let mut made: Vec<(OnDesk, Id, String)> = Vec::new();
    for entry in all_desks(archive) {
        let card = std::fs::read_to_string(&entry.card)
            .ok()
            .filter(|t| CardFile::parse(t).is_ok_and(|f| f.card().id == entry.id));
        let previous = old.remove(&entry.id);
        let Some(mut card_text) = card else {
            // Another device's desk, or a card gone missing: keep the mirror as it is.
            if let Some((name, text, _)) = previous {
                mirrors.on_desk.push(OnDesk {
                    title: title_in(&text).unwrap_or_else(|| name.trim_end_matches(".md").into()),
                    path: entry.path.clone(),
                    added: entry.added.0.clone(),
                    name: name.clone(),
                });
                mirrors.files.insert(name, text);
            }
            continue;
        };
        if let Some((_, text, head)) = &previous
            && let Some(updated) = copy_back(text, head, &card_text)
        {
            write_card(&entry.card, &updated)?;
            mirrors.copied.push(entry.card.clone());
            card_text = updated;
        }
        let Ok(file) = CardFile::parse(&card_text) else {
            continue;
        };
        let text = mirror(&entry, &file);
        made.push((
            OnDesk {
                title: file.card().title.clone(),
                path: entry.path.clone(),
                added: entry.added.0.clone(),
                name: String::new(),
            },
            entry.id,
            text,
        ));
    }
    let names = unique_names(&made, &mirrors.files);
    for ((mut on_desk, _, text), name) in made.into_iter().zip(names) {
        on_desk.name.clone_from(&name);
        mirrors.files.insert(name, text);
        mirrors.on_desk.push(on_desk);
    }
    mirrors.on_desk.sort_by(|a, b| a.added.cmp(&b.added));
    Ok(mirrors)
}

/// Each new mirror's file name: the title made portable, with the start of the identifier
/// added when two titles collide or the name is a kept mirror's.
fn unique_names(made: &[(OnDesk, Id, String)], kept: &BTreeMap<String, String>) -> Vec<String> {
    let base: Vec<String> = made
        .iter()
        .map(|(o, _, _)| crate::names::portable(&o.title))
        .collect();
    made.iter()
        .zip(&base)
        .map(|((_, id, _), name)| {
            let clash = base
                .iter()
                .filter(|b| b.to_lowercase() == name.to_lowercase())
                .count()
                > 1
                || kept
                    .keys()
                    .any(|k| k.to_lowercase() == format!("{name}.md").to_lowercase());
            if clash {
                format!("{name} ({}).md", &id.to_string()[..8])
            } else {
                format!("{name}.md")
            }
        })
        .collect()
}

/// The title in a mirror's heading.
fn title_in(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(str::to_owned)
}

/// The card's text with the owner's notes from `mirror` copied in, if they were edited there.
fn copy_back(mirror: &str, head: &Head, card: &str) -> Option<String> {
    let mirror_body = crate::frontmatter::split(mirror).ok()?.body;
    let mirror_notes = notes(mirror_body)?;
    if checksum(&mirror_notes) == head.mirrored {
        return None;
    }
    let split = crate::frontmatter::split(card).ok()?;
    let card_notes = notes(split.body)?;
    if card_notes == mirror_notes {
        return None;
    }
    let history = sections::get(split.body, crate::card_file::HISTORY)
        .ok()?
        .map(str::to_owned);
    let rest = without(mirror_body, SECTION)?;
    let body = if checksum(&card_notes) == head.mirrored {
        // Only the mirror changed: its notes become the card's, with the card's history.
        match (
            &history,
            sections::get(&rest, crate::card_file::HISTORY).ok()?,
        ) {
            (Some(history), Some(_)) => {
                sections::replace(&rest, crate::card_file::HISTORY, history).ok()?
            }
            (Some(history), None) => {
                sections::append_section(&rest, crate::card_file::HISTORY, history).ok()?
            }
            (None, _) => rest,
        }
    } else {
        // Both changed: keep the card's, and add the mirror's before its history.
        let added = without(&rest, crate::card_file::HISTORY)?;
        let added = format!(
            "\nNotes copied from {FOLDER} on {}:\n\n{}\n",
            jiff::Zoned::now().date(),
            added.trim()
        );
        insert_before_history(split.body, &added)?
    };
    Some(crate::frontmatter::join(split.yaml, &body))
}

/// `body` with `text` inserted just before its history section, or at the end without one.
fn insert_before_history(body: &str, text: &str) -> Option<String> {
    let all = sections::find_all(body).ok()?;
    Some(
        match all
            .into_iter()
            .find(|s| s.name == crate::card_file::HISTORY)
        {
            Some(s) => format!(
                "{}{text}\n{}",
                &body[..s.whole.start],
                &body[s.whole.start..]
            ),
            None => format!("{body}{text}"),
        },
    )
}

fn write_card(path: &Path, text: &str) -> Result<(), ArchiveError> {
    let failed = |e: std::io::Error| ArchiveError::Invalid {
        path: path.to_owned(),
        message: e.to_string(),
    };
    let temp = path.with_extension("md.tmp-mirror");
    std::fs::write(&temp, text).map_err(failed)?;
    std::fs::rename(&temp, path).map_err(failed)
}

/// The mirror of `file`, the desk card of `entry`.
fn mirror(entry: &DeskEntry, file: &CardFile) -> String {
    let card = file.card();
    let mut top = format!(
        "# {}\n\nOn the desk at `{}`. A mirror of its card, generated by Carrel: notes you write here, outside this box and the history, are copied to the card the next time Carrel runs.\n",
        card.title,
        entry.path.display().to_string().replace('`', "'"),
    );
    if let Some(summary) = &card.summary {
        write!(top, "\n{summary}\n").unwrap();
    }
    if !card.tags.is_empty() {
        let tags: Vec<&str> = card.tags.iter().map(|t| t.0.as_str()).collect();
        write!(top, "\nTags: {}\n", tags.join(", ")).unwrap();
    }
    let head = Head {
        carrel: "desk-mirror".into(),
        id: entry.id,
        card: entry.card.clone(),
        mirrored: checksum(&notes(file.body()).unwrap_or_default()),
    };
    let yaml = serde_saphyr::to_string(&head).unwrap_or_default();
    let body = format!(
        "<!-- carrel:{SECTION} -->\n{top}<!-- /carrel:{SECTION} -->\n{}",
        file.body()
    );
    crate::frontmatter::join(&yaml, &body)
}
