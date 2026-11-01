//! What `carrel status` and the interactive mode's Status screen show about an archive, as
//! plain data: its counts, cards with problems, its history and who is writing, what was
//! filed, changed or worked on lately (Recent), and the suggestions about names, structure and
//! the desk. Each interface words it in its own way.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::archive::{Archive, ArchiveError, ItemRef};
use crate::card::{Id, Problem};
use crate::card_file::CardFile;
use crate::coordinator::{Coordinator as _, CoordinatorError, DiskLock, LockInfo};
use crate::desk::{Desk, DeskError, Suggestions};
use crate::entries::HistoryEntry;
use crate::history::{Entry, History, HistoryError, Operation};
use crate::names::Finding;
use crate::signals::Signal;

/// How many things Recent shows.
pub const RECENT: usize = 10;

/// A card whose fields don't conform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardProblems {
    /// The card's path in the archive.
    pub card: String,
    /// What's wrong with it.
    pub problems: Vec<Problem>,
}

/// Something filed, changed or worked on lately, for Recent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recent {
    /// When, as `YYYY-MM-DD HH:MM` in local time for a change and `YYYY-MM-DD` for work, so
    /// they sort together.
    pub when: String,
    /// What happened.
    pub what: Happened,
}

/// What happened, for [`Recent`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Happened {
    /// A change in the history.
    Change(Entry),
    /// A work session on an item, from its card's approved history.
    Work {
        /// The item.
        item: Id,
        /// Its title.
        title: String,
        /// The day, as `YYYY-MM-DD`.
        date: String,
        /// How long, as the history entry says it, such as `45m`.
        duration: String,
        /// The entry's first line, if it has one.
        summary: Option<String>,
    },
}

/// Everything the Status screen shows about an archive.
#[derive(Debug, Clone)]
pub struct Summary {
    /// How many collections it has.
    pub collections: usize,
    /// How many items it has.
    pub items: usize,
    /// How many items of each kind, by the kind's name in cards (`file`, `folder`, …).
    pub by_kind: BTreeMap<String, usize>,
    /// Cards with problems, in the order items are listed.
    pub problems: Vec<CardProblems>,
    /// The history, newest first, or `None` if it hasn't been started.
    pub history: Option<Vec<Entry>>,
    /// Who holds the writer lock, if anyone, even if it has expired.
    pub writer: Option<LockInfo>,
    /// Whether the archive is in a synced folder.
    pub synced: bool,
    /// What was filed, changed or worked on lately, newest first, at most [`RECENT`].
    pub recent: Vec<Recent>,
    /// Names that won't work on every system.
    pub names: Vec<Finding>,
    /// Signs the structure may need a restructure.
    pub signals: Vec<Signal>,
    /// Suggestions about the desk.
    pub desk: Suggestions,
}

/// Why the summary couldn't be made.
#[derive(Debug)]
pub enum StatusError {
    /// The archive couldn't be read.
    Archive(ArchiveError),
    /// A card couldn't be read.
    Io {
        /// The card's file.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
    /// A card couldn't be parsed.
    Card {
        /// The card's path in the archive.
        card: String,
        /// What's wrong with it.
        message: String,
    },
    /// The history couldn't be read.
    History(HistoryError),
    /// The writer lock couldn't be read.
    Lock(CoordinatorError),
    /// The desk couldn't be read.
    Desk(DeskError),
}

impl std::fmt::Display for StatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Archive(e) => write!(f, "{e}"),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Card { card, message } => write!(f, "{card}: {message}"),
            Self::History(e) => write!(f, "{e}"),
            Self::Lock(e) => write!(f, "{e}"),
            Self::Desk(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StatusError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Archive(e) => Some(e),
            Self::Io { source, .. } => Some(source),
            Self::Card { .. } => None,
            Self::History(e) => Some(e),
            Self::Lock(e) => Some(e),
            Self::Desk(e) => Some(e),
        }
    }
}

impl From<ArchiveError> for StatusError {
    fn from(error: ArchiveError) -> Self {
        Self::Archive(error)
    }
}

impl From<HistoryError> for StatusError {
    fn from(error: HistoryError) -> Self {
        Self::History(error)
    }
}

impl From<CoordinatorError> for StatusError {
    fn from(error: CoordinatorError) -> Self {
        Self::Lock(error)
    }
}

impl From<DeskError> for StatusError {
    fn from(error: DeskError) -> Self {
        Self::Desk(error)
    }
}

/// Where to look for active work, and as of when, for the desk's suggestions.
#[derive(Debug, Clone)]
pub struct Looking {
    /// This device, whose desk it is; `None` if it has no identity yet, so no desk.
    pub device: Option<Id>,
    /// The folders to look in for active work, such as the Desktop and `~/Code`.
    pub places: Vec<PathBuf>,
    /// The person's home, which a checked-out folder written with `~` is in.
    pub home: Option<PathBuf>,
    /// Now.
    pub now: std::time::SystemTime,
}

impl Looking {
    /// Looking on `device` in the Desktop and `~/Code`, as of now.
    #[must_use]
    pub fn here(device: Option<Id>) -> Self {
        Self::in_home(device, crate::local::home_dir())
    }

    /// Looking on `device` in `home`'s Desktop and Code folders, as of now. Tests give a
    /// temporary folder, so they never look in the person's own.
    #[must_use]
    pub fn in_home(device: Option<Id>, home: Option<PathBuf>) -> Self {
        Self {
            device,
            places: home
                .as_ref()
                .map(|home| vec![home.join("Desktop"), home.join("Code")])
                .unwrap_or_default(),
            home,
            now: std::time::SystemTime::now(),
        }
    }
}

/// The summary of `archive` for its Status screen, with the desk's suggestions as `looking`
/// says.
///
/// # Errors
///
/// [`StatusError`] if the archive, a card, the history, the writer lock or the desk can't be
/// read.
pub fn summary(archive: &Archive, looking: &Looking) -> Result<Summary, StatusError> {
    let collections = archive.collections()?.len();
    let items = archive.items()?;
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut problems = Vec::new();
    let mut work = Vec::new();
    for item in &items {
        *by_kind.entry(kind_name(item)).or_default() += 1;
        let file = load_file(archive, item)?;
        let found = file.card().problems();
        if !found.is_empty() {
            problems.push(CardProblems {
                card: item.card.clone(),
                problems: found,
            });
        }
        // A card whose history section can't be read shows its problem in `carrel show`.
        if let Ok(log) = file.history() {
            work.extend(
                log.entries
                    .iter()
                    .filter(|e| !e.pending)
                    .filter_map(|e| worked_on(item.id, &file.card().title, e)),
            );
        }
    }
    let history = match History::open(archive) {
        Ok(history) => Some(history.log()?),
        Err(HistoryError::NotInitialized) => None,
        Err(e) => return Err(e.into()),
    };
    let writer = DiskLock::for_archive(archive).current()?;
    let synced = DiskLock::is_synced_folder(archive.root());
    let recent = recent(history.as_deref(), work);
    let names = crate::names::check(archive)?;
    let signals = crate::signals::signals(archive)?;
    let desk = match looking.device {
        Some(device) => Desk::load(archive, device)?,
        None => Desk::default(),
    };
    let desk = crate::desk::suggest(
        archive,
        &desk,
        &looking.places,
        looking.home.as_deref(),
        looking.now,
    )?;
    Ok(Summary {
        collections,
        items: items.len(),
        by_kind,
        problems,
        history,
        writer,
        synced,
        recent,
        names,
        signals,
        desk,
    })
}

/// The time `seconds` after the Unix epoch, as `YYYY-MM-DD HH:MM` in this device's time zone;
/// the number itself if it's out of range.
#[must_use]
pub fn local_time(seconds: i64) -> String {
    jiff::Timestamp::from_second(seconds).map_or_else(
        |_| seconds.to_string(),
        |t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%Y-%m-%d %H:%M")
                .to_string()
        },
    )
}

/// The name of `item`'s kind, as cards write it.
fn kind_name(item: &ItemRef) -> String {
    serde_json::to_value(item.kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn load_file(archive: &Archive, item: &ItemRef) -> Result<CardFile, StatusError> {
    let path = archive.root().join(&item.card);
    let text = std::fs::read_to_string(&path).map_err(|source| StatusError::Io {
        path: path.clone(),
        source,
    })?;
    CardFile::parse(&text).map_err(|e| StatusError::Card {
        card: item.card.clone(),
        message: e.to_string(),
    })
}

/// A work session's approved history entry, as Recent shows it.
fn worked_on(item: Id, title: &str, entry: &HistoryEntry) -> Option<Recent> {
    let duration = entry.duration.as_ref()?;
    let first = entry.lines.iter().map(|l| l.trim()).find(|l| !l.is_empty());
    Some(Recent {
        when: entry.date.clone(),
        what: Happened::Work {
            item,
            title: title.to_owned(),
            date: entry.date.clone(),
            duration: duration.clone(),
            summary: first.map(str::to_owned),
        },
    })
}

/// The latest changes and work sessions, newest first. Recording or approving a session is
/// left out: the session itself is shown, on the day it happened.
fn recent(history: Option<&[Entry]>, mut work: Vec<Recent>) -> Vec<Recent> {
    let changes = history
        .unwrap_or_default()
        .iter()
        .filter(|e| e.operation != Some(Operation::Session))
        .take(RECENT)
        .map(|e| Recent {
            when: local_time(e.time),
            what: Happened::Change(e.clone()),
        });
    work.extend(changes);
    work.sort_by(|a, b| b.when.cmp(&a.when));
    work.truncate(RECENT);
    work
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::InitOptions;

    fn sample() -> (tempfile::TempDir, Archive) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("archive");
        copy_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
            &root,
        );
        let archive = Archive::open(&root).unwrap();
        (dir, archive)
    }

    fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    fn nowhere() -> Looking {
        Looking {
            device: None,
            places: Vec::new(),
            home: None,
            now: std::time::SystemTime::now(),
        }
    }

    #[test]
    fn a_new_archive_has_nothing_to_say() {
        let dir = tempfile::tempdir().unwrap();
        let archive = Archive::init(&dir.path().join("Archive"), &InitOptions::new("New")).unwrap();
        let summary = summary(&archive, &nowhere()).unwrap();
        assert_eq!(summary.items, 0);
        assert_eq!(summary.collections, archive.collections().unwrap().len());
        assert!(summary.by_kind.is_empty());
        assert_eq!(summary.problems, []);
        assert_eq!(summary.recent, []);
        assert!(summary.writer.is_none());
        assert_eq!(summary.desk, Suggestions::default());
    }

    #[test]
    fn items_are_counted_by_kind_and_work_sessions_are_recent() {
        let (_dir, archive) = sample();
        let summary = summary(&archive, &nowhere()).unwrap();
        let items = archive.items().unwrap();
        assert_eq!(summary.items, items.len());
        assert_eq!(summary.by_kind.values().sum::<usize>(), items.len());
        assert!(summary.recent.len() <= RECENT);
        let whens: Vec<&str> = summary.recent.iter().map(|r| r.when.as_str()).collect();
        let mut sorted = whens.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(whens, sorted, "newest first");
    }

    #[test]
    fn recent_puts_work_and_changes_together_newest_first_and_leaves_out_sessions() {
        let entry = |summary: &str, time: i64, operation| Entry {
            id: String::new(),
            summary: summary.into(),
            operation,
            person: None,
            device: None,
            snapshot: None,
            items: Vec::new(),
            moves: Vec::new(),
            reverts: None,
            agent: None,
            time,
        };
        let history = [
            entry(
                "Recorded a session",
                2_000_000_000,
                Some(Operation::Session),
            ),
            entry("Filed a lamp", 1_700_000_000, None),
        ];
        let work = vec![Recent {
            when: "2030-01-01".into(),
            what: Happened::Work {
                item: Id::new_v7(),
                title: "Lamp".into(),
                date: "2030-01-01".into(),
                duration: "45m".into(),
                summary: None,
            },
        }];
        let recent = recent(Some(&history), work);
        assert_eq!(recent.len(), 2);
        assert!(matches!(recent[0].what, Happened::Work { .. }));
        assert!(matches!(&recent[1].what, Happened::Change(e) if e.summary == "Filed a lamp"));
        assert_eq!(recent[1].when, local_time(1_700_000_000));
    }

    #[test]
    fn a_time_out_of_range_is_shown_as_a_number() {
        assert_eq!(local_time(i64::MAX), i64::MAX.to_string());
    }
}
