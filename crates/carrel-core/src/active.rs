//! `Active/` and the Journal (FORMAT.md §9.2, §9.3): generated from the cards, never the
//! owner's, so they're rebuilt whole and anything else in them is removed.
//!
//! - `Active/_index.md`: what's checked out and on a desk.
//! - `Active/Checked out/<title>.md`: one note per checked-out item.
//! - `Active/On desk/<title>.md`: a mirror of each desk card ([`crate::mirrors`]), whose
//!   owner notes are copied back to the card first.
//! - `Journal/<year>/<year>-<month>.md`: each month's approved work sessions, newest first,
//!   then the conversations with the librarian the owner saved there, which are kept as they
//!   are ([`CONVERSATIONS`]).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::archive::{Archive, ArchiveError};
use crate::card::{Card, Checkout, Id};
use crate::card_file::CardFile;
use crate::entries::HistoryEntry;
use crate::listing::{link_destination, relative};

/// The heading in a month's Journal under which saved conversations with the librarian are
/// kept (FORMAT.md §9.3): everything from it to the end of the file is the owner's and is
/// kept when the Journal is regenerated.
pub const CONVERSATIONS: &str = "## Conversations with the librarian";

/// What regenerating changed, as paths relative to the archive root.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Regenerated {
    /// Files written or removed.
    pub changed: Vec<String>,
    /// Desk cards that got notes the owner wrote in their mirrors.
    pub copied: Vec<std::path::PathBuf>,
}

/// One checked-out item, for the notes.
struct Out {
    card_path: String,
    title: String,
    id: String,
    checkout: Checkout,
}

/// One approved session, for the journal.
struct Session {
    card_path: String,
    title: String,
    entry: HistoryEntry,
}

/// Rebuilds `Active/` and `Journal/` from the cards.
///
/// # Errors
///
/// [`ArchiveError`] if the archive can't be listed or a file can't be written.
pub fn regenerate(archive: &Archive) -> Result<Regenerated, ArchiveError> {
    let mut out = Vec::new();
    let mut sessions = Vec::new();
    for item in archive.items()? {
        let path = archive.root().join(&item.card);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(file) = CardFile::parse(&text) else {
            continue;
        };
        let card: &Card = file.card();
        if let Some(checkout) = &card.checked_out {
            out.push(Out {
                card_path: item.card.clone(),
                title: card.title.clone(),
                id: item.id.to_string(),
                checkout: checkout.clone(),
            });
        }
        if let Ok(log) = file.history() {
            for entry in log.entries.into_iter().filter(|e| !e.pending) {
                sessions.push(Session {
                    card_path: item.card.clone(),
                    title: card.title.clone(),
                    entry,
                });
            }
        }
    }
    out.sort_by(|a, b| a.checkout.since.0.cmp(&b.checkout.since.0).reverse());

    let mirrors = crate::mirrors::build(archive)?;
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let names = note_names(&out);
    files.insert(
        "Active/_index.md".into(),
        index(&out, &names, &mirrors.on_desk),
    );
    for (name, text) in &mirrors.files {
        files.insert(format!("{}/{name}", crate::mirrors::FOLDER), text.clone());
    }
    for (o, name) in out.iter().zip(&names) {
        files.insert(format!("Active/Checked out/{name}"), note(o));
    }
    for (month, text) in journal(&sessions) {
        files.insert(month, text);
    }
    keep_conversations(archive.root(), &mut files)?;

    let mut regenerated = Regenerated {
        copied: mirrors.copied,
        ..Regenerated::default()
    };
    let root = archive.root();
    for (path, text) in &files {
        if write_if_changed(root, path, text)? {
            regenerated.changed.push(path.clone());
        }
    }
    regenerated
        .changed
        .extend(remove_others(root, "Active/Checked out", &files)?);
    regenerated
        .changed
        .extend(remove_others(root, crate::mirrors::FOLDER, &files)?);
    if root.join("Journal").is_dir() {
        for year in std::fs::read_dir(root.join("Journal")).map_err(io(&root.join("Journal")))? {
            let year = year.map_err(io(&root.join("Journal")))?;
            if year.path().is_dir() {
                let folder = format!("Journal/{}", year.file_name().to_string_lossy());
                regenerated
                    .changed
                    .extend(remove_others(root, &folder, &files)?);
            }
        }
    }
    regenerated.changed.sort();
    Ok(regenerated)
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ArchiveError + '_ {
    move |e| ArchiveError::Invalid {
        path: path.to_owned(),
        message: e.to_string(),
    }
}

/// Each note's file name: the title made portable, with the start of the item's identifier
/// added when two titles would collide.
fn note_names(out: &[Out]) -> Vec<String> {
    let base: Vec<String> = out
        .iter()
        .map(|o| crate::names::portable(&o.title))
        .collect();
    out.iter()
        .zip(&base)
        .map(|(o, name)| {
            let clash = base
                .iter()
                .filter(|b| b.to_lowercase() == name.to_lowercase())
                .count()
                > 1;
            if clash {
                format!("{name} ({}).md", &o.id[..8])
            } else {
                format!("{name}.md")
            }
        })
        .collect()
}

fn link(from_folder: &str, card_path: &str) -> String {
    link_destination(&relative(from_folder, card_path))
}

fn escape(text: &str) -> String {
    text.replace('[', "\\[")
        .replace(']', "\\]")
        .replace('|', "\\|")
}

fn index(out: &[Out], names: &[String], on_desk: &[crate::mirrors::OnDesk]) -> String {
    let mut text = String::from(
        "# Active\n\nGenerated by Carrel from the cards: what's out of the archive right now. Don't edit it; it's rewritten.\n\n## Checked out\n\n",
    );
    if out.is_empty() {
        text.push_str("Nothing is checked out.\n");
    } else {
        text.push_str("| Item | Where | Since | Due |\n| --- | --- | --- | --- |\n");
        for (o, name) in out.iter().zip(names) {
            writeln!(
                text,
                "| [{}]({}) ([note]({})) | `{}` | {} | {} |",
                escape(&o.title),
                link("Active", &o.card_path),
                link_destination(&format!("Checked out/{name}")),
                o.checkout.to.replace('`', "'"),
                o.checkout.since.0.chars().take(10).collect::<String>(),
                o.checkout.due.as_ref().map_or("", |d| d.0.as_str()),
            )
            .unwrap();
        }
    }
    text.push_str("\n## On the desk\n\n");
    if on_desk.is_empty() {
        text.push_str("Nothing is on the desk.\n");
    } else {
        text.push_str("| Item | Where | Since |\n| --- | --- | --- |\n");
        for d in on_desk {
            writeln!(
                text,
                "| [{}]({}) | `{}` | {} |",
                escape(&d.title),
                link_destination(&format!("On desk/{}", d.name)),
                d.path.display().to_string().replace('`', "'"),
                d.added.chars().take(10).collect::<String>(),
            )
            .unwrap();
        }
    }
    text
}

fn note(o: &Out) -> String {
    let mut text = format!(
        "# {}\n\nChecked out since {} to `{}`. Its card: [{}]({}).\n",
        o.title,
        o.checkout.since.0,
        o.checkout.to.replace('`', "'"),
        escape(&o.title),
        link("Active/Checked out", &o.card_path),
    );
    if let Some(due) = &o.checkout.due {
        writeln!(text, "\nDue back {}.", due.0).unwrap();
    }
    text.push_str("\nGenerated by Carrel; it's removed when the item is returned.\n");
    text
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Each month's journal, by path.
fn journal(sessions: &[Session]) -> BTreeMap<String, String> {
    let mut by_month: BTreeMap<String, Vec<&Session>> = BTreeMap::new();
    for session in sessions {
        let date = &session.entry.date;
        if date.len() >= 7 && date.as_bytes()[4] == b'-' {
            by_month
                .entry(date[..7].to_owned())
                .or_default()
                .push(session);
        }
    }
    by_month
        .into_iter()
        .map(|(month, mut list)| {
            list.sort_by(|a, b| b.entry.date.cmp(&a.entry.date));
            let year = &month[..4];
            let folder = format!("Journal/{year}");
            let mut text = month_heading(&month).unwrap_or_default();
            for s in list {
                let summary = s
                    .entry
                    .lines
                    .iter()
                    .find(|l| !l.trim().is_empty() && !l.starts_with("My notes:"))
                    .map(|l| format!(": {}", l.trim()))
                    .unwrap_or_default();
                writeln!(
                    text,
                    "- {}{} · [{}]({}){summary}",
                    s.entry.date,
                    s.entry
                        .duration
                        .as_ref()
                        .map(|d| format!(" · {d}"))
                        .unwrap_or_default(),
                    escape(&s.title),
                    link(&folder, &s.card_path),
                )
                .unwrap();
            }
            (format!("{folder}/{month}.md"), text)
        })
        .collect()
}

/// The start of the month `month`'s Journal (`2026-11`), before its work sessions, or `None`
/// when `month` isn't `YYYY-MM`.
#[must_use]
pub fn month_heading(month: &str) -> Option<String> {
    let (year, number) = month.split_once('-')?;
    let name = MONTHS.get(number.parse::<usize>().ok()?.checked_sub(1)?)?;
    Some(format!(
        "# {name} {year}\n\nGenerated by Carrel from the cards' history: the month's work sessions, newest first.\n\n"
    ))
}

/// The conversations saved in a month's Journal, `text`: from [`CONVERSATIONS`] to its end.
#[must_use]
pub fn conversations_in(text: &str) -> Option<&str> {
    let at = if text.starts_with(CONVERSATIONS) {
        0
    } else {
        text.find(&format!("\n{CONVERSATIONS}"))? + 1
    };
    Some(&text[at..])
}

/// Adds the conversations saved in each month's Journal to the month's regenerated `files`,
/// so they're kept, with a month heading for a month with no sessions.
fn keep_conversations(
    root: &Path,
    files: &mut BTreeMap<String, String>,
) -> Result<(), ArchiveError> {
    let journal = root.join("Journal");
    let Ok(years) = std::fs::read_dir(&journal) else {
        return Ok(());
    };
    for year in years {
        let year = year.map_err(io(&journal))?;
        let Ok(months) = std::fs::read_dir(year.path()) else {
            continue;
        };
        for month in months {
            let month = month.map_err(io(&year.path()))?;
            let name = month.file_name().to_string_lossy().into_owned();
            let Some(stem) = name.strip_suffix(".md") else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(month.path()) else {
                continue;
            };
            let Some(kept) = conversations_in(&text) else {
                continue;
            };
            let path = format!("Journal/{}/{name}", year.file_name().to_string_lossy());
            let generated = files
                .entry(path)
                .or_insert_with(|| month_heading(stem).unwrap_or_default());
            if !generated.ends_with("\n\n") {
                generated.push('\n');
            }
            generated.push_str(kept);
        }
    }
    Ok(())
}

fn write_if_changed(root: &Path, path: &str, text: &str) -> Result<bool, ArchiveError> {
    let file = root.join(path);
    if std::fs::read_to_string(&file).is_ok_and(|current| current == text) {
        return Ok(false);
    }
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(io(parent))?;
    }
    std::fs::write(&file, text).map_err(io(&file))?;
    Ok(true)
}

/// Removes the Markdown files in `folder` that weren't just generated.
fn remove_others(
    root: &Path,
    folder: &str,
    keep: &BTreeMap<String, String>,
) -> Result<Vec<String>, ArchiveError> {
    let dir = root.join(folder);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut removed = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io(&dir))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{folder}/{name}");
        if Path::new(&name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
            && !keep.contains_key(&path)
        {
            std::fs::remove_file(entry.path()).map_err(io(&entry.path()))?;
            removed.push(path);
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{DateOrTime, Extra, Id};

    fn sample() -> (tempfile::TempDir, Archive) {
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for e in std::fs::read_dir(from).unwrap() {
                let e = e.unwrap();
                if e.file_type().unwrap().is_dir() {
                    copy(&e.path(), &to.join(e.file_name()));
                } else {
                    std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
                }
            }
        }
        let dir = tempfile::tempdir().unwrap();
        copy(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
            &dir.path().join("a"),
        );
        let archive = Archive::open(&dir.path().join("a")).unwrap();
        (dir, archive)
    }

    fn edit(archive: &Archive, card: &str, change: impl FnOnce(&mut CardFile)) {
        let path = archive.root().join(card);
        let mut file = CardFile::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        change(&mut file);
        std::fs::write(&path, file.to_text().unwrap()).unwrap();
    }

    #[test]
    fn checked_out_items_and_approved_sessions_are_listed() {
        let (_dir, archive) = sample();
        let card = "Projects/Hardware/parts-list.csv.md";
        edit(&archive, card, |file| {
            file.card_mut().checked_out = Some(Checkout {
                to: "/Users/me/Desktop/parts-list.csv".into(),
                device: Id::new_v7(),
                person: Id::new_v7(),
                since: DateOrTime("2026-10-30T09:00:00-04:00".into()),
                due: Some(DateOrTime("2026-11-15".into())),
                manifest: None,
                extra: Extra::new(),
            });
            let mut log = file.history().unwrap();
            let mut done = crate::entries::HistoryEntry::pending(
                "2026-10-29",
                Some(70),
                vec!["Swapped the regulator.".into()],
            );
            done.pending = false;
            log.add(done);
            log.add(crate::entries::HistoryEntry::pending(
                "2026-10-30",
                None,
                vec!["Pending.".into()],
            ));
            file.set_history(&log).unwrap();
        });
        std::fs::create_dir_all(archive.root().join("Active/Checked out")).unwrap();
        std::fs::write(archive.root().join("Active/Checked out/Old.md"), "stale").unwrap();

        let done = regenerate(&archive).unwrap();
        assert!(done.changed.contains(&"Active/_index.md".to_owned()));
        assert!(
            done.changed
                .contains(&"Active/Checked out/Old.md".to_owned()),
            "stale notes go"
        );
        let index = std::fs::read_to_string(archive.root().join("Active/_index.md")).unwrap();
        assert!(
            index.contains("../Projects/Hardware/parts-list.csv.md"),
            "{index}"
        );
        assert!(index.contains("2026-11-15"));
        let title = CardFile::parse(&std::fs::read_to_string(archive.root().join(card)).unwrap())
            .unwrap()
            .card()
            .title
            .clone();
        let note = archive.root().join(format!(
            "Active/Checked out/{}.md",
            crate::names::portable(&title)
        ));
        assert!(
            std::fs::read_to_string(note)
                .unwrap()
                .contains("../../Projects/Hardware/parts-list.csv.md")
        );

        let month =
            std::fs::read_to_string(archive.root().join("Journal/2026/2026-10.md")).unwrap();
        assert!(month.starts_with("# October 2026"), "{month}");
        assert!(month.contains("2026-10-29 · 1h 10m"), "{month}");
        assert!(month.contains("Swapped the regulator."));
        assert!(
            !month.contains("Pending."),
            "pending entries aren't in the journal"
        );

        assert_eq!(
            regenerate(&archive).unwrap().changed,
            Vec::<String>::new(),
            "nothing changes twice"
        );
    }
}

/// Whether an active item is checked out of the archive or on the desk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Checked out: the working copy is a copy, returned to the archive.
    CheckedOut,
    /// On this device's desk: worked on where it is, not archived.
    Desk,
}

/// An item being worked on, as the Active screens show it.
#[derive(Debug, Clone)]
pub struct ActiveItem {
    /// Checked out or on the desk.
    pub kind: Kind,
    /// Its card's identifier.
    pub id: Id,
    /// Its title.
    pub title: String,
    /// Its card: in the archive for a checkout, beside the item for a desk item.
    pub card: PathBuf,
    /// The working copy, or the desk item itself.
    pub working: PathBuf,
    /// When it was checked out or put on the desk.
    pub since: String,
    /// When a checkout is due back.
    pub due: Option<String>,
    /// How it compares with how it went out: `unchanged`, `changed`, `missing`, and so on.
    pub state: &'static str,
    /// What changed in a checkout's working copy, when it could tell.
    pub diff: Option<crate::manifest::Diff>,
    /// Its pending history entries, in order: `1` is the first.
    pub pending: Vec<HistoryEntry>,
}

impl ActiveItem {
    /// How `carrel sessions` names it: a checkout by its identifier, a desk item by its card.
    #[must_use]
    pub fn query(&self) -> String {
        match self.kind {
            Kind::CheckedOut => self.id.to_string(),
            Kind::Desk => self.card.display().to_string(),
        }
    }
}

/// The pending entries of the card at `path`.
fn pending(path: &Path) -> Vec<HistoryEntry> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(log) = CardFile::parse(&text).and_then(|f| f.history()) else {
        return Vec::new();
    };
    log.pending()
        .into_iter()
        .map(|i| log.entries[i].clone())
        .collect()
}

/// What's checked out of `archive`, then what's on `device`'s desk, each with its pending
/// history entries. Cards that can't be read are left out.
#[must_use]
pub fn items(archive: &Archive, device: Option<Id>) -> Vec<ActiveItem> {
    let mut active = Vec::new();
    let rules = archive.rules().ok();
    for item in archive.items().unwrap_or_default() {
        let card = archive.root().join(&item.card);
        let Ok(text) = std::fs::read_to_string(&card) else {
            continue;
        };
        let Ok(file) = CardFile::parse(&text) else {
            continue;
        };
        let Some(out) = file.card().checked_out.clone() else {
            continue;
        };
        let working = PathBuf::from(&out.to);
        let (state, diff) = rules.as_ref().map_or(("not compared", None), |rules| {
            crate::checkout::compared(archive, rules, item.id, &working)
        });
        active.push(ActiveItem {
            kind: Kind::CheckedOut,
            id: item.id,
            title: file.card().title.clone(),
            pending: pending(&card),
            card,
            working,
            since: out.since.0,
            due: out.due.map(|d| d.0),
            state,
            diff,
        });
    }
    let desk = device
        .and_then(|device| crate::desk::Desk::load(archive, device).ok())
        .unwrap_or_default();
    for entry in desk.items {
        let title = std::fs::read_to_string(&entry.card)
            .ok()
            .and_then(|t| CardFile::parse(&t).ok())
            .map_or_else(
                || entry.path.display().to_string(),
                |f| f.card().title.clone(),
            );
        active.push(ActiveItem {
            kind: Kind::Desk,
            id: entry.id,
            title,
            pending: pending(&entry.card),
            state: if entry.path.exists() {
                "on the desk"
            } else {
                "missing"
            },
            card: entry.card,
            working: entry.path,
            since: entry.added.0,
            due: None,
            diff: None,
        });
    }
    active
}
