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
/// sensitive, not a duplicate or a likely one, and nothing for the owner to settle.
#[must_use]
pub fn is_high(entry: &ReviewEntry) -> bool {
    entry.state == State::Proposed
        && entry.destination.confidence == Confidence::High
        && entry.card.sensitivity < Sensitivity::Sensitive
        && entry.duplicate_of.is_none()
        && entry.likely_duplicates.is_empty()
}

/// How the owner files an item that duplicates, or likely duplicates, an archived one
/// (`docs/design/duplicates.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kept {
    /// As a new version of it: a `version-of` link.
    AsVersion,
    /// Beside it: a `related` link.
    Both,
}

impl Kept {
    /// The link type it adds.
    #[must_use]
    pub fn link_type(self) -> &'static str {
        match self {
            Self::AsVersion => "version-of",
            Self::Both => "related",
        }
    }
}

/// The archived items `entry` duplicates or likely duplicates, with their card paths: the
/// exact duplicate first.
///
/// # Errors
///
/// [`ActionError::Archive`] if the archive can't be listed.
pub fn duplicates_of(
    archive: &Archive,
    entry: &ReviewEntry,
) -> Result<Vec<(crate::card::Id, String)>, ActionError> {
    let mut found: Vec<(crate::card::Id, String)> = Vec::new();
    if let Some(id) = entry.duplicate_of {
        let items = archive.items()?;
        if let Some(item) = items.iter().find(|i| i.id == id) {
            found.push((id, item.card.clone()));
        }
    }
    for likely in &entry.likely_duplicates {
        if !found.iter().any(|(id, _)| *id == likely.id) {
            found.push((likely.id, likely.card.clone()));
        }
    }
    Ok(found)
}

/// Links `entry`'s card to the archived item `target` as `kept` says, before it's filed.
/// `target` is an identifier, or its start or end; when it's `None`, the item must duplicate
/// exactly one archived item, which is used.
///
/// # Errors
///
/// [`ActionError::Invalid`] if `target` matches no archived item or several, or isn't given
/// when there's more than one to choose from.
pub fn keep(
    archive: &Archive,
    entry: &mut ReviewEntry,
    kept: Kept,
    target: Option<&str>,
) -> Result<(), ActionError> {
    let (id, card) = if let Some(target) = target {
        let items = archive.items()?;
        let wanted = target.to_lowercase();
        let matches: Vec<_> = items
            .iter()
            .filter(|i| {
                let id = i.id.0.to_string();
                id.starts_with(&wanted) || id.ends_with(&wanted)
            })
            .collect();
        match matches.as_slice() {
            [one] => (one.id, one.card.clone()),
            [] => return Err(ActionError::Invalid(format!("no archived item {target}"))),
            _ => {
                return Err(ActionError::Invalid(format!(
                    "{target} matches several archived items; give more of the identifier"
                )));
            }
        }
    } else {
        let found = duplicates_of(archive, entry)?;
        match <[_; 1]>::try_from(found) {
            Ok([one]) => one,
            Err(found) if found.is_empty() => {
                return Err(ActionError::Invalid(format!(
                    "{} isn't a duplicate of anything; name the item to link to",
                    short_id(entry.id)
                )));
            }
            Err(_) => {
                return Err(ActionError::Invalid(format!(
                    "{} looks like a duplicate of several items; name the one to link to",
                    short_id(entry.id)
                )));
            }
        }
    };
    let link_type = crate::card::LinkType(kept.link_type().into());
    entry.card.links.retain(|l| l.id != id);
    entry.card.links.push(crate::card::Link {
        link_type,
        id,
        path: Some(card),
        note: None,
        extra: crate::card::Extra::new(),
    });
    Ok(())
}

/// One row of a side-by-side comparison: a field of the proposal and of the archived item.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Row {
    /// The field, such as `title`.
    pub label: &'static str,
    /// The proposal's value.
    pub proposed: String,
    /// The archived item's value.
    pub archived: String,
    /// Whether they differ.
    pub differs: bool,
}

/// A proposal beside an archived item it duplicates or likely duplicates.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Comparison {
    /// The archived item's identifier.
    pub id: crate::card::Id,
    /// Its card's path in the archive.
    pub card: String,
    /// Why it matched: "same checksum" for an exact duplicate, else the likely reasons.
    pub why: Vec<String>,
    /// The fields side by side.
    pub rows: Vec<Row>,
}

/// The summary's first line, cut to about 80 characters.
fn summary_start(summary: Option<&str>) -> String {
    let line = summary.and_then(|s| s.lines().next()).unwrap_or("").trim();
    if line.chars().count() > 80 {
        let cut: String = line.chars().take(79).collect();
        format!("{}…", cut.trim_end())
    } else {
        line.to_owned()
    }
}

/// `entry` beside each archived item it duplicates or likely duplicates, the exact duplicate
/// first. An archived card that can't be read is left out.
///
/// # Errors
///
/// [`ActionError::Archive`] if the archive can't be listed.
pub fn compare(archive: &Archive, entry: &ReviewEntry) -> Result<Vec<Comparison>, ActionError> {
    let mut comparisons = Vec::new();
    for (id, card_path) in duplicates_of(archive, entry)? {
        let Ok(text) = std::fs::read_to_string(archive.root().join(&card_path)) else {
            continue;
        };
        let Ok(file) = crate::card_file::CardFile::parse(&text) else {
            continue;
        };
        let card = file.card();
        let why = if entry.duplicate_of == Some(id) {
            vec!["same checksum".to_owned()]
        } else {
            entry
                .likely_duplicates
                .iter()
                .find(|l| l.id == id)
                .map(|l| l.why.iter().map(ToString::to_string).collect())
                .unwrap_or_default()
        };
        let created = |dates: Option<&crate::card::Dates>| {
            dates
                .and_then(|d| d.created.as_ref())
                .map(|d| d.0.clone())
                .unwrap_or_default()
        };
        let tags = |tags: &[crate::card::Tag]| {
            tags.iter()
                .map(|t| t.0.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let folder = card_path.rsplit_once('/').map_or("", |(f, _)| f).to_owned();
        let pairs = [
            ("title", entry.card.title.clone(), card.title.clone()),
            (
                "created",
                created(entry.card.dates.as_ref()),
                created(card.dates.as_ref()),
            ),
            (
                "file name",
                entry
                    .source
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                card.item.clone().unwrap_or_default(),
            ),
            (
                "size",
                crate::human_size(entry.source.size),
                card.size.map(crate::human_size).unwrap_or_default(),
            ),
            ("collection", entry.destination.path.clone(), folder),
            ("tags", tags(&entry.card.tags), tags(&card.tags)),
            (
                "summary",
                summary_start(entry.card.summary.as_deref()),
                summary_start(card.summary.as_deref()),
            ),
        ];
        let rows = pairs
            .into_iter()
            .map(|(label, proposed, archived)| Row {
                label,
                differs: proposed != archived,
                proposed,
                archived,
            })
            .collect();
        comparisons.push(Comparison {
            id,
            card: card_path,
            why,
            rows,
        });
    }
    Ok(comparisons)
}

/// The link an approval adds when the owner didn't choose: `version-of` when the item only
/// looks like a newer version of one archived item, and nothing otherwise.
fn default_links(entry: &mut ReviewEntry) {
    if !entry.card.links.is_empty() || entry.duplicate_of.is_some() {
        return;
    }
    if let [likely] = entry.likely_duplicates.as_slice()
        && likely.is_newer_version()
    {
        entry.card.links.push(crate::card::Link {
            link_type: crate::card::LinkType(Kept::AsVersion.link_type().into()),
            id: likely.id,
            path: Some(likely.card.clone()),
            note: None,
            extra: crate::card::Extra::new(),
        });
    }
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
    let decided = entries
        .into_iter()
        .map(|mut e| {
            default_links(&mut e);
            (e, placement)
        })
        .collect();
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

/// Says whether the compressed file proposal `id` is expanded on approval or kept compressed,
/// as the owner chose in Review (`docs/design/compressed.md`). The choice settles a proposal
/// that was asking, and lifts the size limits when it's to expand. Nothing changes on disk
/// until it's approved.
///
/// # Errors
///
/// [`ActionError::Queue`], or [`ActionError::Invalid`] for a decided proposal, one that isn't a
/// compressed file, or expanding one that can't be: split, encrypted or unreadable.
pub fn choose_compressed(
    archive: &Archive,
    id: &str,
    choice: crate::compressed::plan::Choice,
) -> Result<ReviewEntry, ActionError> {
    use crate::compressed::plan::Choice;
    if choice == Choice::Ask {
        return Err(ActionError::Invalid(
            "choose whether to expand it or keep it compressed".into(),
        ));
    }
    let queue = ReviewQueue::of(archive);
    let mut entry = queue.find(id)?;
    decidable(&entry)?;
    let short = short_id(entry.id);
    let Some(plan) = entry.compressed.as_mut() else {
        return Err(ActionError::Invalid(format!(
            "{short} isn't a compressed file"
        )));
    };
    // Only what was listed can be expanded: not a split, unreadable or encrypted archive.
    let encrypted = plan.reason.as_deref() == Some("it's encrypted");
    if choice == Choice::Expand && (plan.files.is_none() || encrypted) {
        return Err(ActionError::Invalid(format!(
            "{short} can't be expanded: {}",
            plan.reason.as_deref().unwrap_or("it couldn't be read")
        )));
    }
    plan.action = choice;
    plan.by_owner = true;
    plan.reason = (choice == Choice::Keep).then(|| "the owner chose to keep it".to_owned());
    entry.problems.retain(|p| p.stage != "compressed");
    if entry.state == State::NeedsOwner && entry.problems.is_empty() {
        entry.state = State::Proposed;
    }
    queue.save(&entry)?;
    Ok(entry)
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
    let commit = learn(archive, &question.subject, fact, holder)?;
    QuestionQueue::of(archive).remove(question.id)?;
    Ok(commit)
}

/// Teaches the glossary that `term` means `fact`, as one history commit made as `holder`, as
/// answering a question or agreeing to a fact from a correction does. Returns the commit.
///
/// # Errors
///
/// [`ActionError::Lease`], [`ActionError::Io`] for the glossary, or a history error.
pub fn learn(
    archive: &Archive,
    term: &str,
    fact: &str,
    holder: Holder,
) -> Result<Option<String>, ActionError> {
    let lease = DiskLock::for_archive(archive).acquire(holder, "glossary", DEFAULT_LEASE)?;
    let result = (|| -> Result<Option<String>, ActionError> {
        add_fact(archive, term, fact).map_err(|source| ActionError::Io {
            path: archive.root().join(crate::questions::GLOSSARY),
            source,
        })?;
        let change = Change::new(
            Operation::Edit,
            format!("Add “{term}” to the glossary"),
            holder.person,
            holder.device,
        );
        Ok(History::open(archive)?.commit(archive, &change, &lease)?)
    })();
    lease.release()?;
    result
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
    let new: Vec<crate::card::Id> = run.entries.iter().flatten().map(|e| e.id).collect();
    let (open, _) = ReviewQueue::of(archive).list()?;
    let senders: Vec<QuestionEntry> = crate::questions::about::senders_in(archive, &open)?
        .into_iter()
        .filter(|q| q.items.iter().any(|id| new.contains(id)))
        .collect();
    crate::questions::answer::raise(archive, senders)?;
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

/// The card fields the owner edits, in a proposal or a filed card
/// (`docs/design/interactive.md`): as YAML in an editor, or field by field in the apps.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardFields {
    /// The title.
    pub title: String,
    /// The summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// The tags, by name.
    #[serde(default)]
    pub tags: Vec<String>,
    /// The dates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dates: Option<crate::card::Dates>,
    /// How sensitive it is.
    pub sensitivity: Sensitivity,
}

/// The fields of the proposal `entry` the owner edits, its proposed new tag among its tags.
#[must_use]
pub fn draft_fields(entry: &ReviewEntry) -> CardFields {
    let mut tags: Vec<String> = entry.card.tags.iter().map(|t| t.0.clone()).collect();
    tags.extend(entry.proposed_tags.iter().map(|t| t.0.clone()));
    CardFields {
        title: entry.card.title.clone(),
        summary: entry.card.summary.clone(),
        tags,
        dates: entry.card.dates.clone(),
        sensitivity: entry.card.sensitivity,
    }
}

/// The fields of the filed card `card` the owner edits.
#[must_use]
pub fn card_fields(card: &crate::card::Card) -> CardFields {
    CardFields {
        title: card.title.clone(),
        summary: card.summary.clone(),
        tags: card.tags.iter().map(|t| t.0.clone()).collect(),
        dates: card.dates.clone(),
        sensitivity: card.sensitivity,
    }
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
    let fields = draft_fields(entry);
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

/// The tag names that apply in the collection at `path`, from which an edited card's tags
/// are chosen.
///
/// # Errors
///
/// [`ActionError::Archive`] if the collections can't be read.
pub fn tags_at(archive: &Archive, path: &str) -> Result<Vec<String>, ActionError> {
    let collections = archive.collections()?;
    Ok(applicable_tags(archive, &collections, path))
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
    fields: CardFields,
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
    let fields: CardFields = match serde_saphyr::from_str(text) {
        Ok(fields) => fields,
        Err(e) => return Ok(Edited::Problems(vec![format!("the YAML isn't valid: {e}")])),
    };
    apply_draft_fields(archive, entry, fields)
}

/// Applies the owner's edited `fields` to the proposal `entry`, checking them first, as
/// [`apply_draft`] does with YAML.
///
/// # Errors
///
/// [`ActionError::Archive`] if the collections can't be read.
pub fn apply_draft_fields(
    archive: &Archive,
    entry: &mut ReviewEntry,
    fields: CardFields,
) -> Result<Edited, ActionError> {
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

/// A correction to a proposal, worked out but not saved: the entry as it would be, the
/// fields changed, where it would now go if that changed, tags left off there, and whether
/// a lower sensitivity the owner asked for was kept at sensitive because a rule found a
/// restricted value.
#[derive(Debug, Clone)]
pub struct CorrectedProposal {
    /// The proposal with the correction applied.
    pub entry: ReviewEntry,
    /// The fields changed, by name.
    pub changed: Vec<&'static str>,
    /// The collection it now goes to, when that changed.
    pub to: Option<String>,
    /// Tags left off because they don't apply in the new collection.
    pub dropped: Vec<String>,
    /// Whether the sensitivity was kept at sensitive although the owner asked for lower.
    pub kept_sensitive: bool,
}

/// Applies `worked`, said as `words`, to a copy of `entry`: the card's fields change, a
/// collection the owner named is taken, and when what the item is changed, it's routed again
/// with `registry`. Nothing is saved; save the returned entry with [`ReviewQueue::save`] once
/// the owner agrees.
///
/// # Errors
///
/// [`ActionError::Invalid`] for a collection that doesn't exist, or an error reading the
/// archive's collections.
pub fn correct_proposal(
    archive: &Archive,
    registry: &Registry,
    entry: &ReviewEntry,
    worked: &crate::correct::Worked,
    words: &str,
) -> Result<CorrectedProposal, ActionError> {
    use crate::correct::Correctable as _;

    let mut entry = entry.clone();
    let mut fields = entry.card.fields();
    let at = crate::card::DateOrTime(
        jiff::Zoned::now()
            .strftime("%Y-%m-%dT%H:%M:%S%:z")
            .to_string(),
    );
    let changed = crate::correct::apply(&mut fields, worked, words, &at);
    let kept_sensitive = fields.sensitivity < Sensitivity::Sensitive
        && entry
            .sensitivity_reasons
            .iter()
            .any(|r| crate::ingest::sensitivity::restricted_by_rule(r));
    if kept_sensitive {
        fields.sensitivity = Sensitivity::Sensitive;
    }
    entry.card.set_fields(fields);
    if let Some(new) = &worked
        .changes
        .tags
        .as_ref()
        .and_then(|t| t.value.new.clone())
        && !entry.proposed_tags.contains(new)
    {
        entry.proposed_tags = vec![new.clone()];
    }

    let collections = archive.collections()?;
    let before = entry.destination.path.clone();
    let mut dropped = Vec::new();
    if let Some(named) = &worked.changes.collection {
        let target = collection(&collections, &named.value)?;
        dropped = redirect(
            archive,
            &collections,
            &mut entry,
            target,
            "chosen by the owner",
        );
    } else if worked.changes.reroute {
        let rules = crate::ingest::route::RoutingRules::of(archive, &collections);
        let router = crate::ingest::route::Router::new(archive, &collections, &rules);
        let candidate = crate::ingest::Candidate {
            path: entry.source.path.clone(),
            kind: entry.kind,
            sealed: entry.sealed,
            markers: Vec::new(),
            checksum: entry.source.checksum.clone(),
            size: entry.source.size,
            duplicate_of: None,
            likely: Vec::new(),
            same_as: None,
            inside: None,
        };
        let draft = crate::ingest::card::Draft {
            title: entry.card.title.clone(),
            summary: entry.card.summary.clone(),
            tags: entry.card.tags.clone(),
            new_tag: entry.proposed_tags.first().cloned(),
            created: entry.card.dates.as_ref().and_then(|d| d.created.clone()),
            generated: None,
            attempts: Vec::new(),
            problems: Vec::new(),
        };
        let source = entry
            .source
            .path
            .parent()
            .unwrap_or(&entry.source.path)
            .to_owned();
        let route = router.route(registry, &candidate, &source, &draft);
        entry.attempts.extend(route.attempts.iter().cloned());
        // Routing again only moves it somewhere it fits: an unsure answer, or Holding, leaves
        // it where it was, as the owner can still move it.
        let sure = route.confidence != Confidence::Low;
        if let Some(target) = route
            .collection
            .and_then(|id| collections.iter().find(|c| c.doc.id == id))
            .filter(|c| sure && c.path != before && c.path != "Holding")
        {
            dropped = redirect(
                archive,
                &collections,
                &mut entry,
                target,
                "routed again after the owner's correction",
            );
            entry.destination.confidence = route.confidence;
            entry.destination.reasons.extend(route.reasons);
        }
    }
    let to = (entry.destination.path != before).then(|| entry.destination.path.clone());
    Ok(CorrectedProposal {
        entry,
        changed,
        to,
        dropped,
        kept_sensitive,
    })
}

/// What the owner's edit of a filed card did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditedCard {
    /// Nothing changed.
    Unchanged,
    /// These fields changed; the card needs writing.
    Changed(Vec<&'static str>),
    /// The edit doesn't check out, for these reasons; the card is as it was.
    Problems(Vec<String>),
}

/// The editable fields of the filed card `card`, in the collection at `collection`, as YAML
/// for the owner's editor, with `problems` to fix listed above them. `floor` is the lowest
/// sensitivity it may have: sensitive when a rule found a restricted value, otherwise public.
///
/// # Errors
///
/// [`ActionError::Archive`] if the collections can't be read.
///
/// # Panics
///
/// Never: the fields always serialize.
pub fn card_text(
    archive: &Archive,
    card: &crate::card::Card,
    collection: &str,
    floor: Sensitivity,
    problems: &[String],
) -> Result<String, ActionError> {
    let collections = archive.collections()?;
    let vocabulary = applicable_tags(archive, &collections, collection);
    let fields = card_fields(card);
    let mut text = String::new();
    for problem in problems {
        writeln!(text, "# To fix: {problem}").unwrap();
    }
    if !problems.is_empty() {
        text.push_str("#\n");
    }
    write!(
        text,
        "# The card for “{}”, in {}. The body below the card's fields is kept exactly.\n\
         # Save to keep your changes; save it unchanged to change nothing. Delete a line to\n\
         # clear it, such as a wrong date.\n\
         # Tags that apply here: {}.\n\
         # Sensitivity: {} or higher.\n",
        card.title,
        if collection.is_empty() {
            "the archive root"
        } else {
            collection
        },
        if vocabulary.is_empty() {
            "none".to_owned()
        } else {
            vocabulary.join(", ")
        },
        level_name(floor),
    )
    .unwrap();
    text.push_str(&serde_saphyr::to_string(&fields).expect("card fields always serialize"));
    Ok(text)
}

/// Applies the owner's edited `text` to the filed `card` in `collection`, checking it first:
/// a title, real dates, tags that apply there, and no sensitivity below `floor`. Fields the
/// owner changed are no longer a model's.
///
/// # Errors
///
/// [`ActionError::Archive`] if the collections can't be read.
pub fn apply_card(
    archive: &Archive,
    card: &mut crate::card::Card,
    collection: &str,
    floor: Sensitivity,
    text: &str,
) -> Result<EditedCard, ActionError> {
    let fields: CardFields = match serde_saphyr::from_str(text) {
        Ok(fields) => fields,
        Err(e) => {
            return Ok(EditedCard::Problems(vec![format!(
                "the YAML isn't valid: {e}"
            )]));
        }
    };
    apply_card_fields(archive, card, collection, floor, fields)
}

/// Applies the owner's edited `fields` to the filed card `card`, in the collection at
/// `collection`, checking them first, as [`apply_card`] does with YAML.
///
/// # Errors
///
/// [`ActionError::Archive`] if the collections can't be read.
pub fn apply_card_fields(
    archive: &Archive,
    card: &mut crate::card::Card,
    collection: &str,
    floor: Sensitivity,
    fields: CardFields,
) -> Result<EditedCard, ActionError> {
    let collections = archive.collections()?;
    let vocabulary = applicable_tags(archive, &collections, collection);
    let checked = match check_fields(fields, &vocabulary, floor) {
        Ok(checked) if !checked.new_tags.is_empty() => {
            let names: Vec<&str> = checked.new_tags.iter().map(|t| t.0.as_str()).collect();
            return Ok(EditedCard::Problems(vec![format!(
                "{} {} a tag in {collection}; add it to the collection's tags first",
                names.join(", "),
                if names.len() == 1 { "isn't" } else { "aren't" }
            )]));
        }
        Ok(checked) => checked,
        Err(problems) => return Ok(EditedCard::Problems(problems)),
    };
    let changed: Vec<&'static str> = [
        ("title", checked.title != card.title),
        ("summary", checked.summary != card.summary),
        ("tags", checked.tags != card.tags),
        ("dates", checked.dates != card.dates),
        ("sensitivity", checked.sensitivity != card.sensitivity),
    ]
    .into_iter()
    .filter_map(|(field, differs)| differs.then_some(field))
    .collect();
    if changed.is_empty() {
        return Ok(EditedCard::Unchanged);
    }
    card.title = checked.title;
    card.summary = checked.summary;
    card.tags = checked.tags;
    card.dates = checked.dates;
    card.sensitivity = checked.sensitivity;
    if let Some(generated) = &mut card.generated {
        generated.fields.retain(|f| !changed.contains(&f.as_str()));
        // Kept while it still holds something, such as the owner's corrections.
        if generated.fields.is_empty() && generated.extra.is_empty() {
            card.generated = None;
        }
    }
    Ok(EditedCard::Changed(changed))
}
