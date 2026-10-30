//! The owner's decisions on the review and question queues (`docs/design/interactive.md`):
//! approving, holding, moving and rejecting proposals, and answering or dismissing questions.
//! The commands, the interactive mode and the apps all make them through these functions and
//! only differ in how they show the results.

use std::fmt::Write as _;
use std::path::PathBuf;

use crate::archive::{Archive, ArchiveError, Collection};
use crate::backup::Backups;
use crate::card::Sensitivity;
use crate::coordinator::{Coordinator, CoordinatorError, DEFAULT_LEASE, DiskLock, Holder};
use crate::history::{Change, History, HistoryError, Operation};
use crate::ingest::filing::{BatchError, Outcome, Placement, file_batch};
use crate::ingest::run::{Progress, Run, run_with_progress};
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
/// still wait; [`requeue_waiting`] queues them to be drafted again.
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
/// those an owner rule marks `auto_file`, calling `progress` as it goes. For a dry run that
/// saves nothing, use [`run_with_progress`] alone.
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
    progress: &mut dyn FnMut(Progress<'_>),
) -> Result<Ingested, ActionError> {
    let run = run_with_progress(archive, registry, paths, options, holder.device, progress)?;
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

/// Puts the proposals waiting on a term question back on the intake queue, at the front, now
/// that the glossary explains the term, so the worker drafts them again: those on `device`
/// whose items are still there. The rest just stop waiting. Returns the entries queued.
///
/// The worker does the drafting, so only it loads a model, and the owner isn't kept waiting
/// (`docs/dev/overhead.md`).
///
/// # Errors
///
/// [`ActionError`] if a queue can't be read or written, or the items can't be planned.
pub fn requeue_waiting(
    archive: &Archive,
    question: &QuestionEntry,
    device: crate::card::Id,
) -> Result<Vec<crate::intake::IntakeEntry>, ActionError> {
    let queue = ReviewQueue::of(archive);
    let mut paths = Vec::new();
    for mut entry in waiting(archive, question)? {
        if entry.source.device == device && entry.source.path.exists() {
            queue.remove(entry.id)?;
            paths.push(entry.source.path.clone());
        } else {
            clear_waiting(&mut entry, question);
            queue.save(&entry)?;
        }
    }
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let whole = IntakeOptions {
        each: false,
        whole: true,
    };
    let queued = crate::intake::add(archive, &paths, whole, device)?;
    let mut entries = Vec::new();
    for entry in queued.added {
        entries.push(crate::intake::move_to_front(
            archive,
            &entry.id.to_string(),
            device,
        )?);
    }
    Ok(entries)
}

/// The card fields the owner edits in a proposal (`docs/design/interactive.md`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftFields {
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dates: Option<crate::card::Dates>,
    sensitivity: Sensitivity,
}

/// Problems the owner settles by editing the card themselves.
const SETTLED_BY_EDITING: [&str; 3] = ["card", "sensitivity", "route"];

fn level_name(level: Sensitivity) -> String {
    serde_json::to_value(level)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// A proposal's card as YAML for the owner to edit, with comments saying what applies, and
/// `problems` from an earlier attempt listed first.
///
/// # Errors
///
/// [`ActionError::Archive`] if the collections can't be read.
///
/// # Panics
///
/// Never: the fields always serialize.
pub fn draft_text(
    archive: &Archive,
    entry: &ReviewEntry,
    problems: &[String],
) -> Result<String, ActionError> {
    let collections = archive.collections()?;
    let vocabulary = applicable_tags(archive, &collections, &entry.destination.path);
    let mut tags: Vec<String> = entry.card.tags.iter().map(|t| t.0.clone()).collect();
    tags.extend(entry.proposed_tags.iter().map(|t| t.0.clone()));
    let fields = DraftFields {
        title: entry.card.title.clone(),
        summary: entry.card.summary.clone(),
        tags,
        dates: entry.card.dates.clone(),
        sensitivity: entry.card.sensitivity,
    };
    let mut text = String::new();
    for problem in problems {
        writeln!(text, "# To fix: {problem}").unwrap();
    }
    if !problems.is_empty() {
        text.push_str("#\n");
    }
    write!(
        text,
        "# The card for proposal {}, going to {}.\n\
         # Save to keep your changes; save it unchanged to change nothing.\n\
         # Tags that apply there: {}. One new tag may be added; approving adds it there.\n\
         # Sensitivity: {} or higher.\n",
        short_id(entry.id),
        entry.destination.path,
        if vocabulary.is_empty() {
            "none".to_owned()
        } else {
            vocabulary.join(", ")
        },
        level_name(entry.card.sensitivity),
    )
    .unwrap();
    text.push_str(&serde_saphyr::to_string(&fields).expect("draft fields always serialize"));
    Ok(text)
}

/// The tag names that apply at `path`.
fn applicable_tags(archive: &Archive, collections: &[Collection], path: &str) -> Vec<String> {
    let all: Vec<crate::card::Tag> = crate::ingest::card::Vocabulary::of(archive, collections)
        .tags
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    crate::ingest::route::tags_for(archive, collections, path, &all)
        .into_iter()
        .map(|t| t.0)
        .collect()
}

/// What the owner's edit did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edited {
    /// Nothing changed.
    Unchanged,
    /// The proposal's card changed; the proposal needs saving.
    Changed,
    /// The edit doesn't check out, for these reasons; the proposal is as it was.
    Problems(Vec<String>),
}

/// The edited fields, checked: title, summary, tags in the vocabulary and the new one.
struct Checked {
    title: String,
    summary: Option<String>,
    tags: Vec<crate::card::Tag>,
    new_tags: Vec<crate::card::Tag>,
    dates: Option<crate::card::Dates>,
    sensitivity: Sensitivity,
}

fn check_fields(
    fields: DraftFields,
    vocabulary: &[String],
    floor: Sensitivity,
) -> Result<Checked, Vec<String>> {
    let mut problems = Vec::new();
    let title = fields.title.trim().to_owned();
    if title.is_empty() {
        problems.push("the title is empty".to_owned());
    }
    let summary = fields
        .summary
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty());
    let mut tags = Vec::new();
    let mut new_tags = Vec::new();
    for name in &fields.tags {
        let tag = crate::card::Tag(name.trim().to_owned());
        if !tag.is_well_formed() {
            problems.push(format!(
                "the tag “{name}” isn't lowercase words joined by - (FORMAT.md §6.5)"
            ));
        } else if vocabulary.contains(&tag.0) {
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        } else if !new_tags.contains(&tag) {
            new_tags.push(tag);
        }
    }
    if new_tags.len() > 1 {
        let names: Vec<&str> = new_tags.iter().map(|t| t.0.as_str()).collect();
        problems.push(format!(
            "only one new tag may be added, not {}",
            names.join(", ")
        ));
    }
    if let Some(dates) = &fields.dates {
        for (field, date) in [
            ("created", &dates.created),
            ("modified", &dates.modified),
            ("added", &dates.added),
            ("worked", &dates.worked),
            ("document", &dates.document),
        ] {
            if let Some(date) = date
                && !date.is_valid()
            {
                problems.push(format!(
                    "dates.{field} “{}” isn't a date (2026-10-29) or a time with an offset",
                    date.0
                ));
            }
        }
    }
    if fields.sensitivity < floor {
        problems.push(format!(
            "sensitivity can't go below {}, what was found",
            level_name(floor)
        ));
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(Checked {
        title,
        summary,
        tags,
        new_tags,
        dates: fields.dates,
        sensitivity: fields.sensitivity,
    })
}

/// Applies the owner's edited `text` to `entry`, checking it first.
///
/// # Errors
///
/// [`ActionError::Archive`] if the collections can't be read.
pub fn apply_draft(
    archive: &Archive,
    entry: &mut ReviewEntry,
    text: &str,
) -> Result<Edited, ActionError> {
    let fields: DraftFields = match serde_saphyr::from_str(text) {
        Ok(fields) => fields,
        Err(e) => return Ok(Edited::Problems(vec![format!("the YAML isn't valid: {e}")])),
    };
    let collections = archive.collections()?;
    let vocabulary = applicable_tags(archive, &collections, &entry.destination.path);
    let checked = match check_fields(fields, &vocabulary, entry.card.sensitivity) {
        Ok(checked) => checked,
        Err(problems) => return Ok(Edited::Problems(problems)),
    };

    let card = &entry.card;
    let changed: Vec<&str> = [
        ("title", checked.title != card.title),
        ("summary", checked.summary != card.summary),
        (
            "tags",
            checked.tags != card.tags || checked.new_tags != entry.proposed_tags,
        ),
        ("dates", checked.dates != card.dates),
        ("sensitivity", checked.sensitivity != card.sensitivity),
    ]
    .into_iter()
    .filter_map(|(field, differs)| differs.then_some(field))
    .collect();
    if changed.is_empty() {
        return Ok(Edited::Unchanged);
    }
    entry.card.title = checked.title;
    entry.card.summary = checked.summary;
    entry.card.tags = checked.tags;
    entry.proposed_tags = checked.new_tags;
    entry.card.dates = checked.dates;
    entry.card.sensitivity = checked.sensitivity;
    // Fields the owner wrote are no longer a model's.
    if let Some(generated) = &mut entry.card.generated {
        generated.fields.retain(|f| !changed.contains(&f.as_str()));
        if generated.fields.is_empty() {
            entry.card.generated = None;
        }
    }
    entry
        .problems
        .retain(|p| !SETTLED_BY_EDITING.contains(&p.stage.as_str()));
    if entry.state == State::NeedsOwner && entry.problems.is_empty() {
        entry.state = State::Proposed;
    }
    Ok(Edited::Changed)
}
