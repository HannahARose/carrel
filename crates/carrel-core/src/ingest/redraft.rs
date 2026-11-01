//! Drafting a waiting proposal again (#445): its card, collection and sensitivity, with the
//! models this device has now, as when a proposal was drafted while no text model could be
//! reached. The proposal keeps its identifier and batch, and whatever the owner already set:
//! the card fields they edited or corrected, and a collection they chose. The commands, the
//! interactive mode and the apps all draft again through [`redraft`].

use std::path::Path;

use crate::archive::Archive;
use crate::card::{Id, Sensitivity};
use crate::ingest::IntakeOptions;
use crate::ingest::run::{Progress, run_with_progress};
use crate::librarian::model::{NoModel, choose_or_explain};
use crate::providers::{Capability, Registry};
use crate::questions::QUESTION_STAGE;
use crate::review::{Confidence, ReviewEntry, ReviewQueue, State, short_id};

use crate::actions::ActionError;

/// The card fields an owner can set on a proposal, as [`ReviewEntry::owner_fields`] names
/// them.
const CARD_FIELDS: [&str; 5] = ["title", "summary", "tags", "dates", "sensitivity"];

/// Why a proposal couldn't be drafted again.
#[derive(Debug)]
pub enum RedraftError {
    /// This device has no text model to draft with; the proposal is unchanged.
    NoTextModel,
    /// Anything else: the proposal isn't waiting, its file is gone or on another device, or
    /// a queue couldn't be read or written.
    Action(ActionError),
}

impl std::fmt::Display for RedraftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTextModel => {
                f.write_str("no text model on this device; write the card by hand")
            }
            Self::Action(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for RedraftError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoTextModel => None,
            Self::Action(e) => Some(e),
        }
    }
}

impl<E: Into<ActionError>> From<E> for RedraftError {
    fn from(e: E) -> Self {
        Self::Action(e.into())
    }
}

/// What's missing for drafting, and what would let it, for [`RedraftError::NoTextModel`]:
/// the model to download for a device with `memory_gb` of memory, or the provider to put
/// beside Carrel when one's downloaded already. Each interface adds its own way to act on it.
#[must_use]
pub fn no_drafting_model(
    registry: &Registry,
    memory_gb: Option<u64>,
    models_dir: Option<&Path>,
) -> Option<Box<NoModel>> {
    choose_or_explain(registry, None, memory_gb, models_dir).err()
}

/// A proposal drafted again, as saved.
#[derive(Debug, Clone)]
pub struct Redrafted {
    /// The proposal.
    pub entry: ReviewEntry,
    /// The fields the owner had set, which were kept, such as `title` or `collection`.
    pub kept: Vec<&'static str>,
}

/// Whether drafting `entry` again might help: it's waiting, and its card or its collection
/// couldn't be settled, as when no model could be reached or every model failed. Interfaces
/// offer Draft Again prominently for these.
#[must_use]
pub fn wants_redraft(entry: &ReviewEntry) -> bool {
    let card = entry.problems.iter().any(|p| p.stage == "card");
    let route_attempts: Vec<_> = entry
        .attempts
        .iter()
        .filter(|a| a.stage == "route")
        .collect();
    let routing = !owner_chose_collection(entry)
        && (entry
            .destination
            .reasons
            .iter()
            .any(|r| r.contains("no text model"))
            || (!route_attempts.is_empty() && route_attempts.iter().all(|a| a.result != "ok")));
    entry.state.is_open() && (card || routing)
}

/// The waiting proposals drafted on `device` that [`wants_redraft`], oldest first, for
/// `carrel review redraft --all-failed`.
///
/// # Errors
///
/// [`ActionError::Queue`] if the review queue can't be read.
pub fn failed_drafts(archive: &Archive, device: Id) -> Result<Vec<ReviewEntry>, ActionError> {
    let (entries, _) = ReviewQueue::of(archive).list()?;
    Ok(entries
        .into_iter()
        .filter(|e| e.source.device == device && wants_redraft(e))
        .collect())
}

/// Records that the owner set `changed` on `entry`'s card, so drafting it again keeps them.
/// Names that aren't card fields, such as `collection`, are left out, and `created` is
/// recorded as `dates`.
pub fn note_owner_fields(entry: &mut ReviewEntry, changed: &[&str]) {
    for field in changed {
        let field = if *field == "created" { "dates" } else { field };
        if CARD_FIELDS.contains(&field) && !entry.owner_fields.iter().any(|f| f == field) {
            entry.owner_fields.push(field.to_owned());
        }
    }
}

/// Whether the owner (or their answer to a question) chose `entry`'s collection: routing
/// rules decide with a rule named, and models never with high confidence.
fn owner_chose_collection(entry: &ReviewEntry) -> bool {
    entry.destination.confidence == Confidence::High && entry.destination.rule.is_none()
}

/// The card fields of `entry` the owner set: those recorded, and for proposals edited before
/// they were recorded, a title, summary or tags no model wrote that differ from what a failed
/// draft gives (the file's name, and nothing).
fn owner_set(entry: &ReviewEntry) -> Vec<&'static str> {
    let generated = |field: &str| {
        entry
            .card
            .generated
            .as_ref()
            .is_some_and(|g| g.fields.iter().any(|f| f == field))
    };
    let stem = entry
        .source
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    CARD_FIELDS
        .into_iter()
        .filter(|field| {
            entry.owner_fields.iter().any(|f| f == field)
                || (!generated(field)
                    && match *field {
                        "title" => entry.card.title != stem,
                        "summary" => entry.card.summary.is_some(),
                        "tags" => !entry.card.tags.is_empty() || !entry.proposed_tags.is_empty(),
                        _ => false,
                    })
        })
        .collect()
}

/// Drafts the waiting proposal `id` (its identifier, or the start or end of it) again with
/// `registry`'s models on `device`, reporting each stage to `progress`, and saves it: a new
/// card, collection and sensitivity, keeping its identifier, batch, and what the owner set.
/// Questions the new draft raises are asked, as at ingest.
///
/// # Errors
///
/// [`RedraftError::NoTextModel`] when this device has no text model, leaving the proposal as
/// it was; [`RedraftError::Action`] when the proposal isn't waiting, its item was added on
/// another device or is gone, or a queue can't be read or written.
pub fn redraft(
    archive: &Archive,
    registry: &Registry,
    id: &str,
    device: Id,
    progress: &mut dyn FnMut(Progress<'_>),
) -> Result<Redrafted, RedraftError> {
    let queue = ReviewQueue::of(archive);
    let old = queue.find(id)?;
    let short = short_id(old.id);
    if !old.state.is_open() {
        return Err(ActionError::Invalid(format!(
            "{short} is already {}; only a waiting proposal can be drafted again",
            old.state.as_str()
        ))
        .into());
    }
    if old.source.device != device {
        return Err(ActionError::Invalid(format!(
            "{short} was added on another device; draft it again there"
        ))
        .into());
    }
    if registry.ranked(Capability::TextModel).is_empty() {
        return Err(RedraftError::NoTextModel);
    }
    let _ = crate::cloud_files::ensure_downloaded(&old.source.path);
    if !old.source.path.exists() {
        return Err(ActionError::Invalid(format!(
            "{short}'s item, {}, is no longer there",
            old.source.path.display()
        ))
        .into());
    }
    let one = IntakeOptions {
        each: false,
        whole: true,
    };
    let mut run = run_with_progress(
        archive,
        registry,
        std::slice::from_ref(&old.source.path),
        one,
        device,
        progress,
    )?;
    let Some(Some(drafted)) = run.entries.first().cloned() else {
        let why = match run.candidates.first().and_then(|c| c.duplicate_of.as_ref()) {
            Some(duplicate) => format!("it's now the same as {}, already filed", duplicate.card),
            None => "nothing could be proposed for it".to_owned(),
        };
        return Err(ActionError::Invalid(format!("{short} wasn't drafted again: {why}")).into());
    };
    let created = run
        .texts
        .first()
        .and_then(Option::as_ref)
        .and_then(|t| t.created.clone());
    let (entry, kept) = merge(archive, &old, drafted, created.as_ref())?;
    run.entries = vec![Some(entry.clone())];
    // Questions the old draft raised that name it stay in their queue, so it still waits on them.
    run.save(archive)?;
    let questions = crate::intake::ask_with_waiting(archive, &[entry.id])?;
    let mut entry = queue.find(&entry.id.to_string())?;
    crate::questions::mark_waiting(&mut entry, &questions);
    Ok(Redrafted { entry, kept })
}

/// `drafted`, the new proposal for `old`'s item, with `old`'s identifier, batch and history,
/// and what the owner set on it. `created` is the date the item records, if any. Returns the
/// proposal and the fields kept.
fn merge(
    archive: &Archive,
    old: &ReviewEntry,
    mut new: ReviewEntry,
    created: Option<&crate::card::DateOrTime>,
) -> Result<(ReviewEntry, Vec<&'static str>), ActionError> {
    new.id = old.id;
    new.batch = old.batch;
    new.created = old.created.clone();
    new.owner_fields.clone_from(&old.owner_fields);
    let mut attempts = old.attempts.clone();
    attempts.append(&mut new.attempts);
    new.attempts = attempts;
    for (key, value) in &old.extra {
        new.extra
            .entry(key.clone())
            .or_insert_with(|| value.clone());
    }
    if let Some(card) = crate::desk::carried_card(&old.extra) {
        crate::desk::carry(&mut new, &card);
    }
    if old.compressed.as_ref().is_some_and(|p| p.by_owner) {
        new.compressed.clone_from(&old.compressed);
        new.problems.retain(|p| p.stage != "compressed");
    }

    let mut kept = owner_set(old);
    // A date the owner left as the file recorded it is the file's, not theirs.
    if kept.contains(&"dates")
        && !old.owner_fields.iter().any(|f| f == "dates")
        && old.card.dates.as_ref().and_then(|d| d.created.as_ref()) == created
    {
        kept.retain(|f| *f != "dates");
    }
    keep_card_fields(old, &mut new, &kept);
    if !kept.is_empty() {
        new.problems
            .retain(|p| !matches!(p.stage.as_str(), "card" | "sensitivity"));
    }
    if owner_chose_collection(old) {
        let collections = archive.collections()?;
        if let Some(target) = collections
            .iter()
            .find(|c| c.doc.id == old.destination.collection)
        {
            let reason = old
                .destination
                .reasons
                .first()
                .cloned()
                .unwrap_or_else(|| "chosen by the owner".to_owned());
            crate::questions::redirect(archive, &collections, &mut new, target, reason);
            new.proposed_tags.clone_from(&old.proposed_tags);
            kept.push("collection");
        }
    }
    for waiting in old.problems.iter().filter(|p| p.stage == QUESTION_STAGE) {
        if !new.problems.contains(waiting) {
            new.problems.push(waiting.clone());
        }
    }
    new.state = if new.problems.iter().any(|p| p.stage != "route") {
        State::NeedsOwner
    } else {
        State::Proposed
    };
    Ok((new, kept))
}

/// Puts `old`'s values of the card fields `kept` on `new`, which are then no longer a model's.
fn keep_card_fields(old: &ReviewEntry, new: &mut ReviewEntry, kept: &[&str]) {
    for field in kept {
        match *field {
            "title" => new.card.title.clone_from(&old.card.title),
            "summary" => new.card.summary.clone_from(&old.card.summary),
            "tags" => {
                new.card.tags.clone_from(&old.card.tags);
                new.proposed_tags.clone_from(&old.proposed_tags);
            }
            "dates" => new.card.dates.clone_from(&old.card.dates),
            "sensitivity" => {
                // As when correcting: a restricted value a rule found keeps it sensitive.
                let floor = if new
                    .sensitivity_reasons
                    .iter()
                    .any(|r| crate::ingest::sensitivity::restricted_by_rule(r))
                {
                    Sensitivity::Sensitive
                } else {
                    Sensitivity::Public
                };
                new.card.sensitivity = old.card.sensitivity.max(floor);
            }
            _ => {}
        }
    }
    if let Some(generated) = &mut new.card.generated {
        generated.fields.retain(|f| !kept.contains(&f.as_str()));
        if let Some(old) = &old.card.generated {
            for (key, value) in &old.extra {
                generated
                    .extra
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            }
        }
        if generated.fields.is_empty() && generated.extra.is_empty() {
            new.card.generated = None;
        }
    }
}
