//! Applying every device's inbox, as the writer ([`super::apply`]).

use std::path::{Path, PathBuf};

use super::{
    Answer, FlagTarget, InboxEntry, KeepLink, Request, SKIPPED_DAYS, SKIPPED_DIR, Skipped,
};
use crate::actions::{self, ActionError, Kept};
use crate::archive::{Archive, ItemRef};
use crate::backup::Backups;
use crate::card::Id;
use crate::coordinator::{CoordinatorError, Holder};
use crate::correct::Worked;
use crate::correct::prepare::{self, Corrects};
use crate::history::{Change, History, HistoryError, Operation};
use crate::ingest::IntakeOptions;
use crate::ingest::filing::Placement;
use crate::intake::{self, IntakeEntry};
use crate::providers::Registry;
use crate::questions::QuestionQueue;
use crate::review::ReviewQueue;

/// What the writer applies inboxes with.
#[derive(Clone, Copy)]
pub struct Applying<'a> {
    /// The writer: this device, and its person.
    pub writer: Holder,
    /// A folder of this device's, outside the archive, that files sent to be added are moved
    /// to, to be ingested from there.
    pub received: &'a Path,
    /// The providers, for what corrections and edits check.
    pub registry: &'a Registry,
    /// This device's backups, for snapshots before filing and returning, when it has them.
    pub backups: Option<&'a dyn Backups>,
}

/// What applying the inboxes did.
#[derive(Debug, Default)]
pub struct Applied {
    /// The intake entries added, on the writer's device.
    pub queued: Vec<IntakeEntry>,
    /// How many inbox entries were applied.
    pub entries: usize,
    /// Entries that no longer applied, and why; they're recorded as skipped for their device.
    pub skipped: Vec<(PathBuf, String)>,
    /// Entries that couldn't be applied now, and why; they stay in their inbox, to be tried
    /// again.
    pub problems: Vec<(PathBuf, String)>,
    /// The history commits recording what was applied, in order.
    pub commits: Vec<String>,
}

/// Why an entry wasn't applied.
enum NotApplied {
    /// It no longer applies, and never will: recorded as skipped.
    Skip(String),
    /// It can't be applied now, such as while another writer is busy: tried again later.
    Problem(String),
}

impl From<ActionError> for NotApplied {
    fn from(error: ActionError) -> Self {
        match error {
            ActionError::Invalid(why) => Self::Skip(why),
            ActionError::Queue(e) => Self::Skip(e.to_string()),
            other => Self::Problem(other.to_string()),
        }
    }
}

impl From<CoordinatorError> for NotApplied {
    fn from(error: CoordinatorError) -> Self {
        Self::Problem(error.to_string())
    }
}

impl From<HistoryError> for NotApplied {
    fn from(error: HistoryError) -> Self {
        match error {
            HistoryError::UnknownCommit(_)
            | HistoryError::UndoConflict(_)
            | HistoryError::UndoBlocked(_) => Self::Skip(error.to_string()),
            other => Self::Problem(other.to_string()),
        }
    }
}

impl From<crate::checkout::CheckoutError> for NotApplied {
    fn from(error: crate::checkout::CheckoutError) -> Self {
        use crate::checkout::CheckoutError;
        match error {
            CheckoutError::AlreadyOut(_) => Self::Skip("it was checked out elsewhere".into()),
            CheckoutError::CopyChanged(_) => {
                Self::Skip("the item changed after the copy was taken".into())
            }
            CheckoutError::CardOnly(_) | CheckoutError::Missing(_) | CheckoutError::Card(_) => {
                Self::Skip(error.to_string())
            }
            other => Self::Problem(other.to_string()),
        }
    }
}

impl From<prepare::PrepareError> for NotApplied {
    fn from(error: prepare::PrepareError) -> Self {
        use prepare::PrepareError;
        match error {
            PrepareError::NoProposal(_)
            | PrepareError::Correction(_)
            | PrepareError::Refile(_)
            | PrepareError::Queue(_) => Self::Skip(error.to_string()),
            other => Self::Problem(other.to_string()),
        }
    }
}

/// What applying one entry did.
#[derive(Default)]
struct Done {
    queued: Vec<IntakeEntry>,
    commits: Vec<String>,
    /// Part of it no longer applied, and why: recorded as skipped beside what was applied.
    partly: Option<String>,
}

impl Done {
    fn commit(commit: Option<String>) -> Self {
        Self {
            commits: commit.into_iter().collect(),
            ..Self::default()
        }
    }
}

/// Applies every device's inbox, as `applying.writer`, in the order the entries were made.
/// Each is applied as its sender's (their person and device in the history), taking the
/// writer lease in their name for the time it takes, so this device must not hold the lease
/// meanwhile. Applied and skipped entries leave their inbox; skipped ones are recorded for
/// their device ([`super::skipped`]); entries that can't be applied now stay.
///
/// # Errors
///
/// [`HistoryError`] if the history can't be opened.
pub fn apply(archive: &Archive, applying: &Applying<'_>) -> Result<Applied, HistoryError> {
    History::open(archive)?;
    let mut applied = Applied::default();
    for (path, entry) in super::waiting(archive) {
        match apply_one(archive, &path, &entry, applying) {
            Ok(done) => {
                applied.queued.extend(done.queued);
                applied.commits.extend(done.commits);
                applied.entries += 1;
                if let Some(reason) = done.partly {
                    record_skipped(archive, &path, &entry, &reason, applying.writer);
                    applied.skipped.push((path.clone(), reason));
                }
                remove_entry(&path);
            }
            Err(NotApplied::Skip(reason)) => {
                record_skipped(archive, &path, &entry, &reason, applying.writer);
                remove_entry(&path);
                applied.skipped.push((path, reason));
            }
            Err(NotApplied::Problem(problem)) => applied.problems.push((path, problem)),
        }
    }
    prune_skipped(archive);
    Ok(applied)
}

fn remove_entry(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir_all(path.with_extension(""));
}

/// Records `entry` as skipped for its device, with `reason`.
fn record_skipped(archive: &Archive, path: &Path, entry: &InboxEntry, reason: &str, by: Holder) {
    let skipped = Skipped {
        entry: entry.clone(),
        reason: reason.to_owned(),
        by: by.device,
        skipped: jiff::Zoned::now().to_string(),
    };
    let to = archive
        .root()
        .join(SKIPPED_DIR)
        .join(entry.device.to_string())
        .join(path.file_name().unwrap_or_default());
    let _ = super::write_json(&to, &skipped);
}

/// Removes skipped records older than [`SKIPPED_DAYS`].
fn prune_skipped(archive: &Archive) {
    let Ok(devices) = std::fs::read_dir(archive.root().join(SKIPPED_DIR)) else {
        return;
    };
    let cutoff = jiff::Timestamp::now() - jiff::SignedDuration::from_hours(24 * SKIPPED_DAYS);
    for device in devices.flatten() {
        for (path, skip) in super::entries_in::<Skipped>(&device.path()) {
            let old = skip
                .skipped
                .parse::<jiff::Zoned>()
                .is_ok_and(|at| at.timestamp() < cutoff);
            if old {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn apply_one(
    archive: &Archive,
    path: &Path,
    entry: &InboxEntry,
    applying: &Applying<'_>,
) -> Result<Done, NotApplied> {
    let sender = entry.holder();
    match &entry.request {
        Request::Add { .. } | Request::AddUrl { .. } => add(archive, path, entry, applying),
        Request::Approve {
            proposals,
            holding,
            keep,
        } => approve(
            archive,
            proposals,
            *holding,
            keep.as_ref(),
            sender,
            applying,
        ),
        Request::Reject { proposals } => {
            let (open, partly) = open_proposals(archive, proposals)?;
            actions::reject(archive, open)?;
            Ok(Done {
                partly,
                ..Done::default()
            })
        }
        Request::MoveProposal {
            proposal,
            collection,
        } => {
            actions::move_to(archive, &proposal.to_string(), collection)?;
            Ok(Done::default())
        }
        Request::ChooseCompressed { proposals, choice } => {
            let (open, partly) = open_proposals(archive, proposals)?;
            for proposal in open {
                actions::choose_compressed(archive, &proposal.id.to_string(), *choice)?;
            }
            Ok(Done {
                partly,
                ..Done::default()
            })
        }
        Request::Answer { question, answer } => {
            self::answer(archive, *question, answer, sender, applying)
        }
        Request::AddRule {
            subject,
            collection,
        } => {
            let collections = archive.collections().map_err(ActionError::from)?;
            let target = actions::collection(&collections, collection)?;
            let commit = crate::questions::answer::add_rule(archive, subject, target, sender)?;
            Ok(Done::commit(commit))
        }
        Request::Learn { about, fact } => {
            Ok(Done::commit(actions::learn(archive, about, fact, sender)?))
        }
        Request::Correct { .. } => correct(archive, &entry.request, sender, applying),
        Request::EditFields { .. } | Request::EditBody { .. } => {
            edit(archive, &entry.request, sender, applying)
        }
        Request::CheckOut { .. } | Request::Return { .. } | Request::CancelCheckout { .. } => {
            checkout(archive, path, entry, applying)
        }
        Request::Flag { target, note } => {
            let what = flaggable(archive, target)?;
            let flagged = crate::flags::flag(archive, &what, note.as_deref(), sender)?;
            Ok(Done::commit(flagged.commit))
        }
        Request::Unflag { target } => {
            let what = flaggable(archive, target)?;
            Ok(Done::commit(
                crate::flags::unflag(archive, &what, sender)?.commit,
            ))
        }
        Request::Expand { item, anyway } => {
            let item = find_item(archive, *item)?;
            let lease = super::lease_as(archive, sender, "expand")?;
            let expanded = crate::compressed::item::expand_item(archive, &item, *anyway, &lease)
                .map_err(|e| NotApplied::Skip(e.to_string()))?;
            lease.release()?;
            Ok(Done::commit(expanded.commit))
        }
        Request::Undo { change } => {
            let lease = super::lease_as(archive, sender, "undo")?;
            let commit = History::open(archive)?.undo(archive, change, &lease)?;
            lease.release()?;
            Ok(Done::commit(Some(commit)))
        }
        Request::Record {
            operation,
            summary,
            items,
        } => {
            let lease = super::lease_as(archive, sender, "record")?;
            let operation = Operation::from_name(operation).unwrap_or(Operation::Edit);
            let mut change = Change::new(operation, summary, sender.person, sender.device);
            change.items.clone_from(items);
            let _ = crate::active::regenerate(archive);
            let commit = History::open(archive)?.commit(archive, &change, &lease)?;
            lease.release()?;
            Ok(Done::commit(commit))
        }
    }
}

/// The item `id`, as the archive has it now.
fn find_item(archive: &Archive, id: Id) -> Result<ItemRef, NotApplied> {
    archive
        .items()
        .map_err(|e| NotApplied::Problem(e.to_string()))?
        .into_iter()
        .find(|i| i.id == id)
        .ok_or_else(|| NotApplied::Skip("the item isn't in the archive any more".into()))
}

/// The proposals among `ids` still waiting for a decision, and, when some weren't, what to
/// record as skipped.
fn open_proposals(
    archive: &Archive,
    ids: &[Id],
) -> Result<(Vec<crate::review::ReviewEntry>, Option<String>), NotApplied> {
    let queue = ReviewQueue::of(archive);
    let mut open = Vec::new();
    let mut decided = 0;
    for id in ids {
        match queue.find(&id.to_string()) {
            Ok(entry) if entry.state.is_open() => open.push(entry),
            _ => decided += 1,
        }
    }
    let why = format!(
        "{} already decided elsewhere, or gone",
        if decided == 1 {
            "the proposal was".to_owned()
        } else {
            format!("{decided} of the proposals were")
        }
    );
    if open.is_empty() {
        return Err(NotApplied::Skip(why));
    }
    Ok((open, (decided > 0).then_some(why)))
}

/// Moves an `add` entry's files out of the archive and adds them, or its address, to this
/// device's intake queue, recorded as one `inbox` change as the sender's.
fn add(
    archive: &Archive,
    path: &Path,
    entry: &InboxEntry,
    applying: &Applying<'_>,
) -> Result<Done, NotApplied> {
    let device = applying.writer.device;
    let problem = |e: &dyn std::fmt::Display| NotApplied::Problem(e.to_string());
    let queued = match &entry.request {
        Request::AddUrl { url, title, note } => {
            intake::add_url(archive, url, title.as_deref(), note.as_deref(), device)
                .map_err(|e| problem(&e))?
                .into_iter()
                .collect()
        }
        Request::Add {
            files,
            each,
            note,
            desk_card,
        } => {
            let from = path.with_extension("");
            let target = applying.received.join(from.file_name().unwrap_or_default());
            std::fs::create_dir_all(&target).map_err(|e| problem(&e))?;
            let mut paths = Vec::new();
            for name in files {
                let to = target.join(name);
                move_or_copy(&from.join(name), &to)
                    .map_err(|e| problem(&format!("{name}: {e}")))?;
                paths.push(to);
            }
            let card = desk_card
                .as_ref()
                .map(|name| {
                    let to = target.join(name);
                    move_or_copy(&from.join(name), &to).map(|()| to)
                })
                .transpose()
                .map_err(|e| problem(&e))?;
            let options = IntakeOptions {
                each: *each,
                whole: !*each,
            };
            let mut queued =
                intake::add_with_note(archive, &paths, options, device, note.as_deref())
                    .map_err(|e| problem(&e))?
                    .added;
            if let Some(card) = card {
                keep_desk_card(archive, &mut queued, &card).map_err(|e| problem(&e))?;
            }
            queued
        }
        _ => Vec::new(),
    };
    let lease = super::lease_as(archive, entry.holder(), "inbox")?;
    let change = Change::new(
        Operation::Inbox,
        format!("Apply an inbox entry: {}", entry.request.describe()),
        entry.person,
        entry.device,
    );
    let commit = History::open(archive)?.commit(archive, &change, &lease)?;
    lease.release()?;
    Ok(Done {
        queued,
        commits: commit.into_iter().collect(),
        partly: None,
    })
}

/// Marks a desk item's intake entries to keep its desk card, `card`.
fn keep_desk_card(
    archive: &Archive,
    queued: &mut [IntakeEntry],
    card: &Path,
) -> Result<(), crate::review::ReviewError> {
    let queue = intake::IntakeQueue::of(archive);
    for intake in queued {
        intake.extra.insert(
            crate::desk::CARD_KEY.to_owned(),
            serde_json::Value::String(card.to_string_lossy().into_owned()),
        );
        queue.save(intake)?;
    }
    Ok(())
}

/// Files the open proposals among `ids`, where proposed or in Holding, as the sender's, on
/// this device, where their items are.
fn approve(
    archive: &Archive,
    ids: &[Id],
    holding: bool,
    keep: Option<&super::KeepAs>,
    sender: Holder,
    applying: &Applying<'_>,
) -> Result<Done, NotApplied> {
    let (mut open, partly) = open_proposals(archive, ids)?;
    if let Some(keep) = keep {
        let kept = match keep.link {
            KeepLink::Version => Kept::AsVersion,
            KeepLink::Both => Kept::Both,
        };
        for entry in &mut open {
            actions::keep(archive, entry, kept, keep.target.as_deref())?;
        }
    }
    let placement = if holding {
        Placement::Holding
    } else {
        Placement::Destination
    };
    let outcome = actions::decide_on(
        archive,
        open,
        placement,
        (sender, applying.writer.device),
        applying.backups,
    )?;
    Ok(Done {
        partly,
        ..Done::commit(outcome.commit)
    })
}

fn answer(
    archive: &Archive,
    id: Id,
    answer: &Answer,
    sender: Holder,
    applying: &Applying<'_>,
) -> Result<Done, NotApplied> {
    use crate::questions::answer as core;
    let question = QuestionQueue::of(archive)
        .find(&id.to_string())
        .map_err(|_| NotApplied::Skip("the question was answered elsewhere, or is gone".into()))?;
    let commit = match answer {
        Answer::Decline => {
            core::decline(archive, &question)?;
            None
        }
        Answer::Skip => {
            core::skip(archive, &question)?;
            None
        }
        Answer::Dismiss => {
            actions::dismiss(archive, &question)?;
            None
        }
        Answer::Settle => {
            core::settled(archive, &question)?;
            None
        }
        Answer::Teach { fact } => {
            let commit = actions::teach(archive, &question, fact, sender)?;
            actions::requeue_waiting(archive, &question, applying.writer.device)?;
            commit
        }
        Answer::Place { collection, create } => {
            let placed = core::place(archive, &question, collection, *create, sender)?;
            placed
                .refiled
                .into_iter()
                .filter_map(|(_, commit)| commit)
                .last()
        }
    };
    Ok(Done::commit(commit))
}

fn correct(
    archive: &Archive,
    request: &Request,
    sender: Holder,
    applying: &Applying<'_>,
) -> Result<Done, NotApplied> {
    let Request::Correct {
        corrects,
        words,
        changes,
        model,
        by,
        answering,
    } = request
    else {
        return Ok(Done::default());
    };
    let worked = Worked {
        changes: changes.clone(),
        model: model.clone(),
        attempts: Vec::new(),
    };
    let registry = applying.registry;
    let prepared = match corrects {
        Corrects::Proposal(id) => {
            let (open, _) = open_proposals(archive, &[*id])?;
            prepare::prepare_review_worked(
                archive,
                registry,
                &open[0].id.to_string(),
                words,
                worked,
            )?
        }
        Corrects::Filed(id) => {
            let item = find_item(archive, *id)?;
            prepare::prepare_filed_worked(archive, registry, item, words, worked)?
        }
    };
    let prepared = match by {
        Some(by) => prepared.said_by(by),
        None => prepared,
    };
    let saved = prepare::save(archive, prepared, || Ok::<_, prepare::PrepareError>(sender))?;
    if let Some(question) = answering
        && let Ok(question) = QuestionQueue::of(archive).find(&question.to_string())
    {
        crate::questions::answer::settled(archive, &question)?;
    }
    Ok(Done::commit(saved.commit))
}

fn edit(
    archive: &Archive,
    request: &Request,
    sender: Holder,
    applying: &Applying<'_>,
) -> Result<Done, NotApplied> {
    use crate::edit::Saved;
    let saved = match request {
        Request::EditFields { item, base, fields } => {
            let item = find_item(archive, *item)?;
            let editing = crate::edit::open(archive, applying.registry, &item)?;
            crate::edit::save(archive, editing, base, (**fields).clone(), sender)?
        }
        Request::EditBody { item, base, body } => {
            let item = find_item(archive, *item)?;
            let editing = crate::edit::open(archive, applying.registry, &item)?;
            crate::edit::save_body(archive, editing, base, body, sender)?
        }
        _ => return Ok(Done::default()),
    };
    match saved {
        Saved::Changed { commit, .. } => Ok(Done::commit(commit)),
        Saved::Unchanged => Ok(Done::default()),
        Saved::Problems(problems) => Err(NotApplied::Skip(problems.join("; "))),
        Saved::FieldsChanged { fields, .. } => Err(NotApplied::Skip(format!(
            "{} changed elsewhere meanwhile",
            fields.join(", ")
        ))),
        Saved::BodyChanged { .. } => Err(NotApplied::Skip(
            "the notes changed elsewhere meanwhile, on the same lines".into(),
        )),
    }
}

/// Records a checkout another device made, swaps in a returned working copy, or cancels a
/// checkout, as the sender's. Returns and cancellations only apply to the sender's own
/// checkouts.
fn checkout(
    archive: &Archive,
    path: &Path,
    entry: &InboxEntry,
    applying: &Applying<'_>,
) -> Result<Done, NotApplied> {
    let sender = entry.holder();
    let theirs = |item: &ItemRef| -> Result<(), NotApplied> {
        let text = std::fs::read_to_string(archive.root().join(&item.card))
            .map_err(|e| NotApplied::Problem(e.to_string()))?;
        let file = crate::card_file::CardFile::parse(&text)
            .map_err(|e| NotApplied::Skip(e.to_string()))?;
        match &file.card().checked_out {
            Some(out) if out.device == sender.device => Ok(()),
            Some(_) => Err(NotApplied::Skip(
                "it's checked out to another device".into(),
            )),
            None => Err(NotApplied::Skip("it isn't checked out any more".into())),
        }
    };
    let lease_for = |purpose| super::lease_as(archive, sender, purpose);
    match &entry.request {
        Request::CheckOut {
            item,
            to,
            checksum,
            due,
            git_head,
        } => {
            let item = find_item(archive, *item)?;
            let lease = lease_for("checkout")?;
            let (_, commit) = crate::checkout::record_checkout(
                archive,
                &item,
                to,
                checksum,
                (due.clone(), git_head.clone()),
                &lease,
            )?;
            lease.release()?;
            Ok(Done::commit(commit))
        }
        Request::Return { item, name } => {
            let item = find_item(archive, *item)?;
            theirs(&item)?;
            let copy = path.with_extension("").join(name);
            let plan = crate::checkout::plan_return(archive, &item, Some(&copy))?;
            let lease = lease_for("return")?;
            let returned =
                crate::checkout::finish_return(archive, &plan, &lease, applying.backups)?;
            lease.release()?;
            Ok(Done::commit(returned.commit))
        }
        Request::CancelCheckout { item } => {
            let item = find_item(archive, *item)?;
            theirs(&item)?;
            let lease = lease_for("return")?;
            let commit = crate::checkout::cancel_checkout(archive, &item, &lease)?;
            lease.release()?;
            Ok(Done::commit(commit))
        }
        _ => Ok(Done::default()),
    }
}

fn flaggable(
    archive: &Archive,
    target: &FlagTarget,
) -> Result<crate::flags::Flaggable, NotApplied> {
    match target {
        FlagTarget::Item { id } => {
            let item = find_item(archive, *id)?;
            Ok(crate::flags::Flaggable::item(&item))
        }
        FlagTarget::Collection { path } => Ok(crate::flags::Flaggable::collection(path)),
    }
}

/// Moves `from` to `to`, copying it across volumes.
fn move_or_copy(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    super::copy(from, to)
}
