//! The owner's decisions on the review and question queues (`docs/design/interactive.md`):
//! approving, holding, moving and rejecting proposals, and answering or dismissing questions.
//! The commands, the interactive mode and the apps all make them through these functions and
//! only differ in how they show the results.

use std::path::PathBuf;

use crate::archive::{Archive, ArchiveError, Collection};
use crate::backup::Backups;
use crate::card::Sensitivity;
use crate::coordinator::{Coordinator, CoordinatorError, DEFAULT_LEASE, DiskLock, Holder};
use crate::history::{Change, History, HistoryError, Operation};
use crate::ingest::filing::{BatchError, Outcome, Placement, file_batch};
use crate::ingest::run::{Run, run};
use crate::ingest::{IngestError, IntakeOptions};
use crate::providers::Registry;
use crate::questions::{QuestionEntry, QuestionQueue, add_fact, clear_waiting, redirect};
use crate::review::{Confidence, ReviewEntry, ReviewError, ReviewQueue, State, short_id};

/// Why a decision couldn't be carried out.
#[derive(Debug)]
pub enum ActionError {
    /// A queue couldn't be read or written, or an entry wasn't found.
    Queue(ReviewError),
    /// The writer lease couldn't be taken or was lost.
    Lease(CoordinatorError),
    /// Filing failed as a whole.
    Batch(BatchError),
    /// The history couldn't be written.
    History(HistoryError),
    /// The archive couldn't be read.
    Archive(ArchiveError),
    /// Ingest couldn't run.
    Ingest(IngestError),
    /// A file operation failed.
    Io {
        /// The path concerned.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The request doesn't make sense, such as a collection that doesn't exist.
    Invalid(String),
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Queue(e) => write!(f, "{e}"),
            Self::Lease(e) => write!(f, "{e}"),
            Self::Batch(e) => write!(f, "{e}"),
            Self::History(e) => write!(f, "{e}"),
            Self::Archive(e) => write!(f, "{e}"),
            Self::Ingest(e) => write!(f, "{e}"),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Invalid(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for ActionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Queue(e) => Some(e),
            Self::Lease(e) => Some(e),
            Self::Batch(e) => Some(e),
            Self::History(e) => Some(e),
            Self::Archive(e) => Some(e),
            Self::Ingest(e) => Some(e),
            Self::Io { source, .. } => Some(source),
            Self::Invalid(_) => None,
        }
    }
}

macro_rules! from {
    ($($variant:ident($error:ty)),*) => {
        $(impl From<$error> for ActionError {
            fn from(e: $error) -> Self {
                Self::$variant(e)
            }
        })*
    };
}

from!(
    Queue(ReviewError),
    Lease(CoordinatorError),
    Batch(BatchError),
    History(HistoryError),
    Archive(ArchiveError),
    Ingest(IngestError)
);

/// Whether a proposal can still be decided.
fn decidable(entry: &ReviewEntry) -> Result<(), ActionError> {
    if matches!(entry.state, State::Rejected | State::Archived) {
        return Err(ActionError::Invalid(format!(
            "{} is already {}",
            short_id(entry.id),
            entry.state.as_str()
        )));
    }
    Ok(())
}

/// The proposals named by `ids` (full identifiers, or their starts or ends), all found before
/// anything changes, each once.
///
/// # Errors
///
/// [`ActionError::Queue`] for an identifier that matches nothing or several, and
/// [`ActionError::Invalid`] for a proposal that's already rejected or filed.
pub fn proposals(archive: &Archive, ids: &[String]) -> Result<Vec<ReviewEntry>, ActionError> {
    if ids.is_empty() {
        return Err(ActionError::Invalid("name the proposals to decide".into()));
    }
    let queue = ReviewQueue::of(archive);
    let mut entries: Vec<ReviewEntry> = Vec::new();
    for id in ids {
        let entry = queue.find(id)?;
        decidable(&entry)?;
        if !entries.iter().any(|e| e.id == entry.id) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

/// Whether `entry` is approved by "approve all with high confidence": high confidence, not
/// sensitive, not a duplicate, and nothing for the owner to settle.
#[must_use]
pub fn is_high(entry: &ReviewEntry) -> bool {
    entry.state == State::Proposed
        && entry.destination.confidence == Confidence::High
        && entry.card.sensitivity < Sensitivity::Sensitive
        && entry.duplicate_of.is_none()
}

/// Every proposal [`is_high`] accepts.
///
/// # Errors
///
/// [`ActionError::Queue`] if the queue can't be read.
pub fn all_high(archive: &Archive) -> Result<Vec<ReviewEntry>, ActionError> {
    let (entries, _) = ReviewQueue::of(archive).list()?;
    Ok(entries.into_iter().filter(is_high).collect())
}

/// Files `entries` as one batch where `placement` says, taking the writer lease as `holder`
/// for the time it takes, with a snapshot when `backups` are given.
///
/// # Errors
///
/// [`ActionError::Lease`] if another writer holds the archive, or a [`BatchError`].
pub fn decide(
    archive: &Archive,
    entries: Vec<ReviewEntry>,
    placement: Placement,
    holder: Holder,
    backups: Option<&dyn Backups>,
) -> Result<Outcome, ActionError> {
    if entries.is_empty() {
        return Ok(Outcome::default());
    }
    let mut lease = DiskLock::for_archive(archive).acquire(holder, "ingest", DEFAULT_LEASE)?;
    let decided = entries.into_iter().map(|e| (e, placement)).collect();
    let outcome = file_batch(archive, decided, &mut lease, holder.device, backups);
    lease.release()?;
    Ok(outcome?)
}

/// Rejects `entries`: their items stay where they are.
///
/// # Errors
///
/// [`ActionError::Queue`] if an entry can't be saved.
pub fn reject(
    archive: &Archive,
    entries: Vec<ReviewEntry>,
) -> Result<Vec<ReviewEntry>, ActionError> {
    let queue = ReviewQueue::of(archive);
    let mut rejected = Vec::new();
    for mut entry in entries {
        entry.state = State::Rejected;
        queue.save(&entry)?;
        rejected.push(entry);
    }
    Ok(rejected)
}

/// The collection at `path`, compared without case and ignoring slashes at the ends.
///
/// # Errors
///
/// [`ActionError::Invalid`] if there's none.
pub fn collection<'a>(
    collections: &'a [Collection],
    path: &str,
) -> Result<&'a Collection, ActionError> {
    let wanted = path.trim_matches('/');
    collections
        .iter()
        .find(|c| c.path.eq_ignore_ascii_case(wanted))
        .ok_or_else(|| ActionError::Invalid(format!("no collection {wanted}")))
}

/// A proposal moved to another collection: the entry as saved, where it now goes, and the
/// tags left off because they don't apply there.
#[derive(Debug, Clone)]
pub struct Moved {
    /// The proposal, as saved.
    pub entry: ReviewEntry,
    /// The collection's path.
    pub to: String,
    /// Tags left off.
    pub dropped: Vec<String>,
}

/// Points the proposal `id` at the collection at `path`, chosen by the owner. Nothing is filed
/// until it's approved.
///
/// # Errors
///
/// [`ActionError::Queue`], or [`ActionError::Invalid`] for a decided proposal or an unknown
/// collection.
pub fn move_to(archive: &Archive, id: &str, path: &str) -> Result<Moved, ActionError> {
    let queue = ReviewQueue::of(archive);
    let mut entry = queue.find(id)?;
    decidable(&entry)?;
    let collections = archive.collections()?;
    let target = collection(&collections, path)?;
    let dropped = redirect(
        archive,
        &collections,
        &mut entry,
        target,
        "chosen by the owner",
    );
    queue.save(&entry)?;
    Ok(Moved {
        entry,
        to: target.path.clone(),
        dropped,
    })
}

/// The proposals still waiting on `question`.
///
/// # Errors
///
/// [`ActionError::Queue`] if the queue can't be read.
pub fn waiting(
    archive: &Archive,
    question: &QuestionEntry,
) -> Result<Vec<ReviewEntry>, ActionError> {
    let (entries, _) = ReviewQueue::of(archive).list()?;
    Ok(entries
        .into_iter()
        .filter(|e| question.items.contains(&e.id) && e.state.is_open())
        .collect())
}

/// Answers a term question with `fact`: the glossary learns it as one history commit made as
/// `holder`, and the question leaves the queue. Returns the commit. The waiting proposals
/// still wait; [`redraft`] drafts them again.
///
/// # Errors
///
/// [`ActionError::Lease`], [`ActionError::Io`] for the glossary, or a history or queue error.
pub fn teach(
    archive: &Archive,
    question: &QuestionEntry,
    fact: &str,
    holder: Holder,
) -> Result<Option<String>, ActionError> {
    let lease = DiskLock::for_archive(archive).acquire(holder, "glossary", DEFAULT_LEASE)?;
    let result = (|| -> Result<Option<String>, ActionError> {
        add_fact(archive, &question.subject, fact).map_err(|source| ActionError::Io {
            path: archive.root().join(crate::questions::GLOSSARY),
            source,
        })?;
        let change = Change::new(
            Operation::Edit,
            format!("Add “{}” to the glossary", question.subject),
            holder.person,
            holder.device,
        );
        Ok(History::open(archive)?.commit(archive, &change, &lease)?)
    })();
    lease.release()?;
    let commit = result?;
    QuestionQueue::of(archive).remove(question.id)?;
    Ok(commit)
}

/// Answers a collection question: every waiting proposal is pointed at the collection at
/// `path`, and the question leaves the queue.
///
/// # Errors
///
/// [`ActionError::Invalid`] for an unknown collection, or a queue error.
pub fn place(
    archive: &Archive,
    question: &QuestionEntry,
    path: &str,
) -> Result<Vec<Moved>, ActionError> {
    let collections = archive.collections()?;
    let target = collection(&collections, path).map_err(|_| {
        ActionError::Invalid(format!(
            "no collection {}; choose one of {}",
            path.trim_matches('/'),
            question.choices.join(", ")
        ))
    })?;
    let queue = ReviewQueue::of(archive);
    let mut moved = Vec::new();
    for mut entry in waiting(archive, question)? {
        clear_waiting(&mut entry, question);
        let dropped = redirect(
            archive,
            &collections,
            &mut entry,
            target,
            format!("you said items about “{}” belong here", question.subject),
        );
        queue.save(&entry)?;
        moved.push(Moved {
            entry,
            to: target.path.clone(),
            dropped,
        });
    }
    QuestionQueue::of(archive).remove(question.id)?;
    Ok(moved)
}

/// Dismisses `question`: its proposals stop waiting on it.
///
/// # Errors
///
/// A queue error.
pub fn dismiss(archive: &Archive, question: &QuestionEntry) -> Result<(), ActionError> {
    let queue = ReviewQueue::of(archive);
    for mut entry in waiting(archive, question)? {
        clear_waiting(&mut entry, question);
        queue.save(&entry)?;
    }
    QuestionQueue::of(archive).remove(question.id)?;
    Ok(())
}

/// An ingest run, saved, with what an owner rule filed straight away.
#[derive(Debug)]
pub struct Ingested {
    /// What ingest found and proposed.
    pub run: Run,
    /// What was filed without review, when a rule said so.
    pub filed: Option<Outcome>,
}

/// Runs ingest over `paths` as `holder` and saves the proposals and questions, then files
/// those an owner rule marks `auto_file`. With `save` false nothing is saved or filed.
///
/// # Errors
///
/// [`ActionError::Ingest`], a queue error, or a filing error.
pub fn ingest(
    archive: &Archive,
    registry: &Registry,
    paths: &[PathBuf],
    options: IntakeOptions,
    holder: Holder,
    backups: Option<&dyn Backups>,
    save: bool,
) -> Result<Ingested, ActionError> {
    let run = run(archive, registry, paths, options, holder.device)?;
    if !save {
        return Ok(Ingested { run, filed: None });
    }
    run.save(archive)?;
    let chosen = run.filed_by_rule();
    let filed = if chosen.is_empty() {
        None
    } else {
        let entries = chosen.into_iter().map(|(e, _)| e).collect();
        Some(decide(
            archive,
            entries,
            Placement::Destination,
            holder,
            backups,
        )?)
    };
    Ok(Ingested { run, filed })
}

/// Drafts the proposals waiting on a term question again, now that the glossary explains the
/// term: those on `holder`'s device whose items are still there are removed and ingested
/// afresh; the rest just stop waiting. Returns the new run, if anything was drafted.
///
/// # Errors
///
/// As [`ingest`].
pub fn redraft(
    archive: &Archive,
    registry: &Registry,
    question: &QuestionEntry,
    holder: Holder,
    backups: Option<&dyn Backups>,
) -> Result<Option<Ingested>, ActionError> {
    let queue = ReviewQueue::of(archive);
    let mut paths = Vec::new();
    for mut entry in waiting(archive, question)? {
        if entry.source.device == holder.device && entry.source.path.exists() {
            queue.remove(entry.id)?;
            paths.push(entry.source.path.clone());
        } else {
            clear_waiting(&mut entry, question);
            queue.save(&entry)?;
        }
    }
    if paths.is_empty() {
        return Ok(None);
    }
    let options = IntakeOptions {
        each: false,
        whole: true,
    };
    ingest(archive, registry, &paths, options, holder, backups, true).map(Some)
}
