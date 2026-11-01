//! History and Status: every change with undo, as `carrel log` and `carrel undo` do, and what
//! the Status screen shows ([`carrel_core::status::summary`]): counts, problems, the writer,
//! Recent, the structure's signals and the desk's suggestions.

use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, LockInfo};
use carrel_core::history::{Entry, History, HistoryError};
use carrel_core::status::{Happened, Looking, Recent as CoreRecent, local_time};

use crate::{ArchiveHandle, CarrelError, text};

/// A change in the history.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Change {
    /// The commit's identifier.
    pub id: String,
    /// What it did, in a line.
    pub summary: String,
    /// The operation, such as `approve`, `edit` or `return`, when it's recorded.
    pub operation: Option<String>,
    /// When, as `YYYY-MM-DD HH:MM` in this device's time zone.
    pub when: String,
    /// When, as seconds since the Unix epoch.
    pub time: i64,
    /// The items it concerns.
    pub items: Vec<String>,
    /// The device it was made on.
    pub device: Option<String>,
    /// The backup snapshot taken with it.
    pub snapshot: Option<String>,
    /// The change it undid, if it's an undo.
    pub reverts: Option<String>,
    /// Who made it on the owner's behalf without asking, such as `librarian`, when someone
    /// did (`Carrel-Agent`, FORMAT.md §8.2).
    pub agent: Option<String>,
}

impl From<&Entry> for Change {
    fn from(entry: &Entry) -> Self {
        Self {
            id: entry.id.clone(),
            summary: entry.summary.clone(),
            operation: entry.operation.map(|o| o.as_str().to_owned()),
            when: local_time(entry.time),
            time: entry.time,
            items: entry.items.iter().map(ToString::to_string).collect(),
            device: entry.device.map(|d| d.to_string()),
            snapshot: entry.snapshot.clone(),
            reverts: entry.reverts.clone(),
            agent: entry.agent.clone(),
        }
    }
}

/// Something filed, changed or worked on lately.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RecentEntry {
    /// When, as `YYYY-MM-DD HH:MM` for a change and `YYYY-MM-DD` for work.
    pub when: String,
    /// A change, when it's one.
    pub change: Option<Change>,
    /// For work, the item.
    pub item: Option<String>,
    /// For work, the item's title; for a change, its summary.
    pub title: String,
    /// For work, how long, such as `45m`.
    pub duration: Option<String>,
    /// For work, the entry's first line.
    pub note: Option<String>,
}

impl From<&CoreRecent> for RecentEntry {
    fn from(recent: &CoreRecent) -> Self {
        match &recent.what {
            Happened::Change(entry) => Self {
                when: recent.when.clone(),
                change: Some(entry.into()),
                item: None,
                title: entry.summary.clone(),
                duration: None,
                note: None,
            },
            Happened::Work {
                item,
                title,
                duration,
                summary,
                ..
            } => Self {
                when: recent.when.clone(),
                change: None,
                item: Some(item.to_string()),
                title: title.clone(),
                duration: Some(duration.clone()),
                note: summary.clone(),
            },
        }
    }
}

/// Who holds the writer lease.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Writer {
    /// What it's doing, such as `ingest`.
    pub purpose: String,
    /// The program, such as `carrel 0.4.0`.
    pub program: String,
    /// The device holding it.
    pub device: String,
    /// Whether that's this device.
    pub this_device: bool,
    /// When it was taken.
    pub since: String,
    /// Whether it has run out, so the next writer takes it over.
    pub expired: bool,
}

/// A count of items of one kind.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct KindCount {
    /// The kind's name in cards: `file`, `folder`, `bundle`, and so on.
    pub kind: String,
    /// How many.
    pub count: u64,
}

/// A card whose fields don't conform.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CardProblem {
    /// The card's path.
    pub card: String,
    /// What's wrong, in words.
    pub problems: Vec<String>,
}

/// A path whose name won't work on every system.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NameProblem {
    /// The path.
    pub path: String,
    /// What's wrong.
    pub problems: Vec<String>,
    /// A portable name to rename it to, when renaming would fix it.
    pub suggestion: Option<String>,
}

/// A folder that looks like active work, to put on the desk.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ToTrack {
    /// The folder.
    pub path: String,
    /// Whether it's a Git repository.
    pub git: bool,
    /// When anything in it last changed, as a date.
    pub changed: Option<String>,
}

/// A desk item unchanged for a long time, to archive.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ToArchive {
    /// Where it is.
    pub path: String,
    /// When it last changed, as a date.
    pub changed: String,
}

/// What the Status screen shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StatusSummary {
    /// How many collections.
    pub collections: u64,
    /// How many items.
    pub items: u64,
    /// How many of each kind.
    pub by_kind: Vec<KindCount>,
    /// Cards with problems.
    pub problems: Vec<CardProblem>,
    /// Whether the history has been started.
    pub has_history: bool,
    /// How many changes it has.
    pub changes: u64,
    /// Who holds the writer lease, if anyone.
    pub writer: Option<Writer>,
    /// Whether the archive is in a synced folder.
    pub synced: bool,
    /// What was filed, changed or worked on lately, newest first.
    pub recent: Vec<RecentEntry>,
    /// Names that won't work on every system.
    pub names: Vec<NameProblem>,
    /// Signs the structure may need a restructure, in words.
    pub signals: Vec<String>,
    /// Folders to put on the desk.
    pub to_track: Vec<ToTrack>,
    /// Desk items to archive.
    pub to_archive: Vec<ToArchive>,
    /// How many proposals wait in Review.
    pub review: u64,
    /// How many questions wait.
    pub questions: u64,
    /// What the owner flagged as needing work: collections, then items.
    pub flagged: Vec<crate::flags::FlaggedThing>,
    /// This device's inbox: what waits for the writer, and what it skipped.
    pub inbox: crate::inbox::InboxStatus,
    /// What waits in the holding folder for a snapshot.
    pub holding: crate::inbox::HoldingStatus,
}

fn writer(lock: LockInfo, device: Option<carrel_core::card::Id>) -> Writer {
    Writer {
        expired: lock.is_expired(),
        this_device: device == Some(lock.device),
        purpose: lock.purpose,
        program: lock.program,
        device: lock.device.to_string(),
        since: lock.acquired,
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// What the Status screen shows, with the desk's suggestions from the Desktop and
    /// `~/Code`.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive, a card, the history or the desk can't be read.
    pub fn status(&self) -> Result<StatusSummary, CarrelError> {
        let archive = self.fresh()?;
        let device = carrel_core::local::Identity::load_or_create_in(&self.config_dir()?)
            .ok()
            .map(|i| i.device);
        let summary =
            carrel_core::status::summary(&archive, &Looking::in_home(device, self.home()))?;
        let (review, _) = carrel_core::review::ReviewQueue::of(&archive).list()?;
        let (questions, _) = carrel_core::questions::QuestionQueue::of(&archive).list()?;
        Ok(StatusSummary {
            collections: summary.collections as u64,
            items: summary.items as u64,
            by_kind: summary
                .by_kind
                .into_iter()
                .map(|(kind, count)| KindCount {
                    kind,
                    count: count as u64,
                })
                .collect(),
            problems: summary
                .problems
                .into_iter()
                .map(|p| CardProblem {
                    card: p.card,
                    problems: p.problems.iter().map(ToString::to_string).collect(),
                })
                .collect(),
            has_history: summary.history.is_some(),
            changes: summary.history.as_ref().map_or(0, |h| h.len() as u64),
            writer: summary.writer.map(|lock| writer(lock, device)),
            synced: summary.synced,
            recent: summary.recent.iter().map(Into::into).collect(),
            names: summary
                .names
                .into_iter()
                .map(|n| NameProblem {
                    path: n.path,
                    problems: n.problems,
                    suggestion: n.suggestion,
                })
                .collect(),
            signals: summary
                .signals
                .iter()
                .map(carrel_core::signals::Signal::describe)
                .collect(),
            to_track: summary
                .desk
                .to_track
                .into_iter()
                .map(|t| ToTrack {
                    path: text(&t.path),
                    git: t.git,
                    changed: t.changed,
                })
                .collect(),
            to_archive: summary
                .desk
                .to_archive
                .into_iter()
                .map(|t| ToArchive {
                    path: text(&t.entry.path),
                    changed: t.changed,
                })
                .collect(),
            review: review.iter().filter(|e| e.state.is_open()).count() as u64,
            questions: questions.len() as u64,
            flagged: summary.flagged.into_iter().map(Into::into).collect(),
            inbox: device
                .map(|d| crate::inbox::inbox_status(&archive, d))
                .unwrap_or_default(),
            holding: crate::inbox::holding_status(&archive),
        })
    }

    /// The history, newest first, at most `limit` changes (all of them when it's `None`).
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if the archive has no history yet, or
    /// [`CarrelError::Failed`] if it can't be read.
    pub fn history(&self, limit: Option<u32>) -> Result<Vec<Change>, CarrelError> {
        let archive = self.fresh()?;
        let history = match History::open(&archive) {
            Ok(history) => history,
            Err(HistoryError::NotInitialized) => {
                return Err(CarrelError::invalid("this archive has no history yet"));
            }
            Err(e) => return Err(e.into()),
        };
        let mut entries = history.log()?;
        if let Some(limit) = limit {
            entries.truncate(limit as usize);
        }
        Ok(entries.iter().map(Into::into).collect())
    }

    /// Starts the history of an archive that has none yet, as `carrel init` does in an
    /// existing archive.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`] if it has one already or it can't be written.
    pub fn start_history(&self) -> Result<(), CarrelError> {
        let archive = self.fresh()?;
        let lease =
            DiskLock::for_archive(&archive).acquire(self.holder()?, "create", DEFAULT_LEASE)?;
        let result = History::start(&archive, &lease);
        lease.release()?;
        result?;
        Ok(())
    }

    /// Undoes the change `id` (its identifier, or the start of it) as a new change, and
    /// returns that change's commit. Say first what undoing does and doesn't restore.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if no change or several match, [`CarrelError::SentToInbox`]
    /// while another device is the writer, [`CarrelError::Busy`] if another writer holds the
    /// archive, or [`CarrelError::Failed`] if it can't be undone.
    pub fn undo(&self, id: String) -> Result<String, CarrelError> {
        let archive = self.fresh()?;
        let history = History::open(&archive)?;
        let entries = history.log()?;
        let matching: Vec<&Entry> = entries.iter().filter(|e| e.id.starts_with(&id)).collect();
        let entry = match matching.as_slice() {
            [entry] => *entry,
            [] => {
                return Err(CarrelError::invalid(format!(
                    "no change {id} in the history"
                )));
            }
            _ => {
                return Err(CarrelError::invalid(format!(
                    "several changes start with {id}; give more of it"
                )));
            }
        };
        self.through_inbox(&archive, || {
            Ok(carrel_core::inbox::Request::Undo {
                change: entry.id.clone(),
            })
        })?;
        let lease =
            DiskLock::for_archive(&archive).acquire(self.holder()?, "undo", DEFAULT_LEASE)?;
        let result = history.undo(&archive, &entry.id, &lease);
        lease.release()?;
        Ok(result?)
    }
}
