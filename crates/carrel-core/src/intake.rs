//! The intake queue (`docs/design/tui-ingest.md`, FORMAT.md §7.9): items waiting to be
//! ingested, one entry each, processed one at a time with each proposal saved as soon as it's
//! ready. Adding to the queue is quick; the drafting, routing and rating happen later, in
//! [`process`], usually from a worker in the background.
//!
//! Entries are bound to the device that has the files, as review entries are: only it can
//! read them.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::actions::{ActionError, decide};
use crate::archive::Archive;
use crate::backup::Backups;
use crate::card::{DateOrTime, Extra, Id};
use crate::coordinator::Holder;
use crate::ingest::card::Glossary;
use crate::ingest::questions::ask;
use crate::ingest::run::{Progress, Run, run_with_progress};
use crate::ingest::{Candidate, IngestError, IntakeOptions, Skipped, plan};
use crate::providers::Registry;
use crate::questions::{QuestionEntry, QuestionQueue, mark_waiting};
use crate::review::{Queue, QueueEntry, ReviewEntry, ReviewError, ReviewQueue};

/// The intake queue's folder, relative to the archive root.
pub const INTAKE_DIR: &str = "_System/queue/intake";

/// How many times an item is tried before it's left for the owner to retry.
pub const MAX_ATTEMPTS: u32 = 2;

/// An item waiting to be ingested: `_System/queue/intake/<id>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntakeEntry {
    /// Document type; always `intake`.
    pub carrel: IntakeMarker,
    /// The format version it was written with.
    pub format: crate::FormatVersion,
    /// The entry's identifier, which also orders entries added at different times.
    pub id: Id,
    /// The item's absolute path on its device.
    pub path: PathBuf,
    /// The device that has the item.
    pub device: Id,
    /// When it was added.
    pub added: DateOrTime,
    /// Its place in the queue: lower goes first, then the order added.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub order: i64,
    /// How many times processing it has failed.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub attempts: u32,
    /// Why the last attempt failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip_serializing_if
fn is_zero(n: &i64) -> bool {
    *n == 0
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip_serializing_if
fn is_zero_u32(n: &u32) -> bool {
    *n == 0
}

/// The `carrel: intake` marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntakeMarker {
    /// The only value.
    #[default]
    Intake,
}

impl IntakeEntry {
    /// Whether the entry has failed often enough to wait for the owner to retry it.
    #[must_use]
    pub fn stuck(&self) -> bool {
        self.attempts >= MAX_ATTEMPTS
    }
}

impl QueueEntry for IntakeEntry {
    const DIR: &'static str = INTAKE_DIR;
    const NOUN: &'static str = "queued item";

    fn id(&self) -> Id {
        self.id
    }
}

/// The intake queue.
pub type IntakeQueue = Queue<IntakeEntry>;

/// What adding to the queue did.
#[derive(Debug)]
pub struct Queued {
    /// The entries added.
    pub added: Vec<IntakeEntry>,
    /// The items found, in order, as planning them saw them.
    pub candidates: Vec<Candidate>,
    /// Paths left out, and why: skipped by rules, already queued, already waiting for review,
    /// or the same as another item in this batch.
    pub skipped: Vec<Skipped>,
}

/// This device's waiting entries, in the order they'll be processed. Stuck entries are left
/// out, unless `stuck` is set.
///
/// # Errors
///
/// [`ReviewError`] if the queue can't be read.
pub fn waiting(
    archive: &Archive,
    device: Id,
    stuck: bool,
) -> Result<Vec<IntakeEntry>, ReviewError> {
    let (mut entries, _) = IntakeQueue::of(archive).list()?;
    entries.retain(|e| e.device == device && (stuck || !e.stuck()));
    entries.sort_by(|a, b| a.order.cmp(&b.order).then(a.id.0.cmp(&b.id.0)));
    Ok(entries)
}

/// Plans `paths` as `carrel ingest` would, without running any model, so the owner sees what
/// would be added.
///
/// # Errors
///
/// [`IngestError`] if the paths can't be planned.
pub fn preview(
    archive: &Archive,
    paths: &[PathBuf],
    options: IntakeOptions,
    device: Id,
) -> Result<Queued, ActionError> {
    let (candidates, mut skipped) = plan(archive, paths, options).map_err(ActionError::Ingest)?;
    let (queued, _) = IntakeQueue::of(archive).list()?;
    let (proposed, _) = ReviewQueue::of(archive).list()?;
    let mut added = Vec::new();
    let now = now();
    for candidate in &candidates {
        let reason = if candidate.same_as.is_some() {
            Some("the same as another item added with it".to_owned())
        } else if queued
            .iter()
            .any(|q| q.device == device && q.path == candidate.path)
        {
            Some("already queued".to_owned())
        } else if proposed.iter().any(|p| {
            p.state.is_open()
                && p.source.device == device
                && p.source.checksum == candidate.checksum
        }) {
            Some("already waiting for review".to_owned())
        } else {
            None
        };
        if let Some(reason) = reason {
            skipped.push(Skipped {
                path: candidate.path.clone(),
                reason,
            });
            continue;
        }
        added.push(IntakeEntry {
            carrel: IntakeMarker::Intake,
            format: crate::FORMAT_VERSION,
            id: Id::new_v7(),
            path: candidate.path.clone(),
            device,
            added: now.clone(),
            order: 0,
            attempts: 0,
            failure: None,
            extra: Extra::new(),
        });
    }
    Ok(Queued {
        added,
        candidates,
        skipped,
    })
}

/// Adds `paths` to the queue for `device`, as [`preview`] plans them. Returns what was added
/// and what was left out.
///
/// # Errors
///
/// [`ActionError`] if the paths can't be planned or the queue can't be written.
pub fn add(
    archive: &Archive,
    paths: &[PathBuf],
    options: IntakeOptions,
    device: Id,
) -> Result<Queued, ActionError> {
    let queued = preview(archive, paths, options, device)?;
    let queue = IntakeQueue::of(archive);
    for entry in &queued.added {
        queue.save(entry)?;
    }
    Ok(queued)
}

/// Moves the waiting entry `id` to the front of this device's queue.
///
/// # Errors
///
/// [`ReviewError`] if it isn't found or the queue can't be written.
pub fn move_to_front(archive: &Archive, id: &str, device: Id) -> Result<IntakeEntry, ReviewError> {
    let queue = IntakeQueue::of(archive);
    let mut entry = queue.find(id)?;
    let first = waiting(archive, device, true)?
        .first()
        .map_or(0, |e| e.order);
    entry.order = first.min(entry.order) - 1;
    queue.save(&entry)?;
    Ok(entry)
}

/// Clears a stuck entry's failures, so it's tried again.
///
/// # Errors
///
/// [`ReviewError`] if it isn't found or the queue can't be written.
pub fn retry(archive: &Archive, id: &str) -> Result<IntakeEntry, ReviewError> {
    let queue = IntakeQueue::of(archive);
    let mut entry = queue.find(id)?;
    entry.attempts = 0;
    entry.failure = None;
    queue.save(&entry)?;
    Ok(entry)
}

/// Takes the entry `id` off the queue, whichever device it's waiting on.
///
/// # Errors
///
/// [`ReviewError`] if it isn't found or the queue can't be written.
pub fn remove(archive: &Archive, id: &str) -> Result<IntakeEntry, ReviewError> {
    let queue = IntakeQueue::of(archive);
    let entry = queue.find(id)?;
    queue.remove(entry.id)?;
    Ok(entry)
}

/// This device's queue as `carrel ingest --status` and the Ingest screen show it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Listing {
    /// The entries waiting, in the order they'll be processed.
    pub waiting: Vec<IntakeEntry>,
    /// The entries that have failed too often to be tried again without the owner.
    pub failed: Vec<IntakeEntry>,
}

/// This device's queue: what's waiting, in order, and what has failed.
///
/// # Errors
///
/// [`ReviewError`] if the queue can't be read.
pub fn listing(archive: &Archive, device: Id) -> Result<Listing, ReviewError> {
    let (failed, waiting) = waiting(archive, device, true)?
        .into_iter()
        .partition(IntakeEntry::stuck);
    Ok(Listing { waiting, failed })
}

/// How many of the entries `ids` are still waiting to be processed, on any device. Those that
/// have failed too often count as done.
///
/// # Errors
///
/// [`ReviewError`] if the queue can't be read.
pub fn remaining(archive: &Archive, ids: &[Id]) -> Result<usize, ReviewError> {
    let (entries, _) = IntakeQueue::of(archive).list()?;
    Ok(entries
        .iter()
        .filter(|e| ids.contains(&e.id) && !e.stuck())
        .count())
}

/// What processing one entry did.
#[derive(Debug)]
pub struct Processed {
    /// The entry.
    pub entry: IntakeEntry,
    /// The run for its one item, or why it failed.
    pub run: Result<Run, String>,
    /// Questions asked or extended because of it.
    pub questions: Vec<QuestionEntry>,
    /// What an owner rule filed straight away.
    pub filed: Option<crate::ingest::filing::Outcome>,
}

/// Drafts, routes and rates `entry`'s item, saves its proposal, asks any questions it raises
/// together with the proposals already waiting, files it if an owner rule says so, and takes it
/// off the queue. A failure is recorded on the entry, which stays queued until it has failed
/// [`MAX_ATTEMPTS`] times.
///
/// # Errors
///
/// [`ActionError`] if the queues can't be written; a failure of the item itself is in
/// [`Processed::run`].
pub fn process(
    archive: &Archive,
    registry: &Registry,
    mut entry: IntakeEntry,
    holder: Holder,
    backups: Option<&dyn Backups>,
    progress: &mut dyn FnMut(Progress<'_>),
) -> Result<Processed, ActionError> {
    let queue = IntakeQueue::of(archive);
    let one = IntakeOptions {
        each: false,
        whole: true,
    };
    let run = match missing(&entry.path).map_or_else(
        || {
            run_with_progress(
                archive,
                registry,
                std::slice::from_ref(&entry.path),
                one,
                holder.device,
                progress,
            )
            .map_err(|e: IngestError| e.to_string())
        },
        Err,
    ) {
        Ok(run) => run,
        Err(failure) => {
            entry.attempts += 1;
            entry.failure = Some(failure.clone());
            queue.save(&entry)?;
            return Ok(Processed {
                entry,
                run: Err(failure),
                questions: Vec::new(),
                filed: None,
            });
        }
    };
    let mut run = run;
    if let Some(card) = crate::desk::carried_card(&entry.extra) {
        for proposal in run.entries.iter_mut().flatten() {
            crate::desk::carry(proposal, &card);
        }
    }
    run.save(archive)?;
    let new: Vec<Id> = run.entries.iter().flatten().map(|e| e.id).collect();
    let questions = ask_with_waiting(archive, &new)?;
    let chosen = run.filed_by_rule();
    let filed = if chosen.is_empty() {
        None
    } else {
        let entries = chosen.into_iter().map(|(e, _)| e).collect();
        Some(decide(
            archive,
            entries,
            crate::ingest::filing::Placement::Destination,
            holder,
            backups,
        )?)
    };
    queue.remove(entry.id)?;
    Ok(Processed {
        entry,
        run: Ok(run),
        questions,
        filed,
    })
}

/// Why `path` can't be ingested now, if it's gone.
fn missing(path: &Path) -> Option<String> {
    (!path.exists()).then(|| format!("{} is no longer there", path.display()))
}

/// Asks the questions the proposals `new` raise together with every open proposal: a question
/// on a subject already asked gains the new proposals, and a new one is saved. Proposals newly
/// named by a question are marked as waiting on it. Returns the questions saved.
///
/// # Errors
///
/// [`ActionError`] if a queue can't be read or written.
pub fn ask_with_waiting(archive: &Archive, new: &[Id]) -> Result<Vec<QuestionEntry>, ActionError> {
    if new.is_empty() {
        return Ok(Vec::new());
    }
    let reviews = ReviewQueue::of(archive);
    let (open, _) = reviews.list()?;
    let mut open: Vec<ReviewEntry> = open.into_iter().filter(|e| e.state.is_open()).collect();
    let collections = archive.collections()?;
    let asked = ask(archive, &collections, &Glossary::of(archive), &open);
    let questions = QuestionQueue::of(archive);
    let (existing, _) = questions.list()?;
    let mut saved = Vec::new();
    for question in asked
        .into_iter()
        .filter(|q| q.items.iter().any(|id| new.contains(id)))
    {
        let same = existing
            .iter()
            .find(|e| e.kind == question.kind && e.subject.eq_ignore_ascii_case(&question.subject));
        let (question, newly) = if let Some(earlier) = same {
            let mut merged = earlier.clone();
            let newly: Vec<Id> = question
                .items
                .iter()
                .filter(|id| !earlier.items.contains(id))
                .copied()
                .collect();
            merged.items.extend(&newly);
            (merged, newly)
        } else {
            let newly = question.items.clone();
            (question, newly)
        };
        if newly.is_empty() {
            continue;
        }
        questions.save(&question)?;
        // In place, so a proposal named by two questions keeps both marks.
        for entry in open.iter_mut().filter(|e| newly.contains(&e.id)) {
            mark_waiting(entry, std::slice::from_ref(&question));
            reviews.save(entry)?;
        }
        saved.push(question);
    }
    Ok(saved)
}

fn now() -> DateOrTime {
    DateOrTime(
        jiff::Zoned::now()
            .strftime("%Y-%m-%dT%H:%M:%S%:z")
            .to_string(),
    )
}
