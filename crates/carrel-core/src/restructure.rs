//! Restructure proposals (`docs/design/restructure.md`): a reviewable list of changes to one
//! section of the archive, approved all, in part or not at all, and applied as one change in
//! history that undo reverses as a unit.
//!
//! This module holds what every kind of proposal shares:
//!
//! - the file format, [`Proposal`] and [`ProposedChange`], kept one file per proposal in
//!   `_System/queue/proposals/` (FORMAT.md §7.9) through the same [`Queue`] the review queue
//!   uses ([`ProposalQueue`]);
//! - how invasive a change is ([`Invasiveness`]);
//! - partial approval, which drops the changes that depend on rejected ones ([`select`]);
//! - applying the approved changes as one `restructure` change in history ([`approve`]), and
//!   rejecting a proposal ([`reject`]).
//!
//! The groundwork whole-section proposals build on is here too: [`grouping::group_section`]
//! clusters a section's items by their card embeddings and the tag and link graph
//! ([`crate::graph`]), and [`findability::compare`] tests whether a proposed tree is easier to
//! navigate than the current one.
//!
//! Each kind of change is an [`Edit`]. Tag restructuring ([`tags`]) is the first, and the
//! cheapest: no files move. Whole-section proposals add their own [`Edit`] variants (moving
//! items, creating, renaming, merging and splitting collections) and an arm in
//! `apply_edit` for each, and reuse the rest.

pub mod findability;
pub mod grouping;
pub mod tags;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::FormatVersion;
use crate::archive::Archive;
use crate::card::{DateOrTime, Extra, Id, Tag};
use crate::coordinator::{Coordinator as _, CoordinatorError, DEFAULT_LEASE, DiskLock, Holder};
use crate::history::{Change, History, HistoryError, Operation};
use crate::review::{Queue, QueueEntry, ReviewError};

/// Where proposals wait, relative to the archive root (FORMAT.md §7.9).
pub const PROPOSALS_DIR: &str = "_System/queue/proposals";

/// The proposal queue.
pub type ProposalQueue = Queue<Proposal>;

/// The `carrel: proposal` marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProposalMarker {
    /// The only value.
    #[default]
    Proposal,
}

/// What a proposal reorganizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProposalKind {
    /// Only tags: merging, splitting and retiring them, with no files moved.
    Tags,
    /// A section of the archive: its items, collections, design documents and tags.
    Section,
}

impl ProposalKind {
    /// The kind's name, as files and commands write it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tags => "tags",
            Self::Section => "section",
        }
    }
}

/// How much a change disturbs what the owner knows of the archive, from least to most
/// (`docs/design/restructure.md`, "How invasive a change is"). The order is the ranking:
/// a later kind is more invasive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Invasiveness {
    /// Rewriting design documents and tags: nothing moves.
    Rewriting,
    /// Grouping a collection's items into new subcollections inside it: every item stays
    /// under the same collection.
    Subdividing,
    /// Renaming a collection: everything stays together, under a new name.
    Renaming,
    /// Moving items between branches, and merging or splitting collections across them.
    BetweenBranches,
    /// Moving or merging top-level collections: the archive's main shape changes.
    TopLevel,
}

impl Invasiveness {
    /// Every kind, least invasive first.
    pub const ALL: [Self; 5] = [
        Self::Rewriting,
        Self::Subdividing,
        Self::Renaming,
        Self::BetweenBranches,
        Self::TopLevel,
    ];

    /// Its rank in the design's list, from 1 (rewriting) to 5 (moving top-level collections).
    #[must_use]
    pub fn rank(self) -> u8 {
        match self {
            Self::Rewriting => 1,
            Self::Subdividing => 2,
            Self::Renaming => 3,
            Self::BetweenBranches => 4,
            Self::TopLevel => 5,
        }
    }

    /// The kind in words, for a change list.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Rewriting => "rewriting design documents and tags",
            Self::Subdividing => "subdividing a collection",
            Self::Renaming => "renaming a collection",
            Self::BetweenBranches => "moving items between branches",
            Self::TopLevel => "moving or merging top-level collections",
        }
    }
}

/// A restructure proposal: `_System/queue/proposals/<id>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    /// Document type; always `proposal`.
    pub carrel: ProposalMarker,
    /// The format version it was written with.
    pub format: FormatVersion,
    /// The proposal's identifier.
    pub id: Id,
    /// What it reorganizes.
    pub kind: ProposalKind,
    /// The section it covers: a collection's path, or empty for the whole archive.
    #[serde(default)]
    pub section: String,
    /// When it was proposed.
    pub created: DateOrTime,
    /// The program and version that built it, such as `carrel 0.7.0`.
    pub by: String,
    /// The most invasive of its changes.
    pub invasiveness: Invasiveness,
    /// Why the proposal as a whole is worth making.
    pub rationale: String,
    /// The changes, in the order they'd be applied.
    pub changes: Vec<ProposedChange>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl QueueEntry for Proposal {
    const DIR: &'static str = PROPOSALS_DIR;
    const NOUN: &'static str = "restructure proposal";

    fn id(&self) -> Id {
        self.id
    }

    fn check(&self) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for change in &self.changes {
            if !seen.insert(change.id.as_str()) {
                return Err(format!("two changes are numbered {}", change.id));
            }
        }
        for change in &self.changes {
            if let Some(missing) = change
                .depends_on
                .iter()
                .find(|d| !seen.contains(d.as_str()))
            {
                return Err(format!(
                    "change {} depends on {missing}, which isn't in the proposal",
                    change.id
                ));
            }
        }
        Ok(())
    }
}

impl Proposal {
    /// A new, empty proposal of `kind` for `section`.
    #[must_use]
    pub fn new(kind: ProposalKind, section: impl Into<String>) -> Self {
        Self {
            carrel: ProposalMarker::Proposal,
            format: crate::FORMAT_VERSION,
            id: Id::new_v7(),
            kind,
            section: section.into(),
            created: crate::questions::now(),
            by: format!("carrel {}", crate::CARREL_VERSION),
            invasiveness: Invasiveness::Rewriting,
            rationale: String::new(),
            changes: Vec::new(),
            extra: Extra::new(),
        }
    }

    /// The change numbered `id`, if there is one.
    #[must_use]
    pub fn change(&self, id: &str) -> Option<&ProposedChange> {
        self.changes.iter().find(|c| c.id == id)
    }

    /// Every item any change touches, once each, in the order first touched.
    #[must_use]
    pub fn items(&self) -> Vec<Id> {
        let mut seen = BTreeSet::new();
        self.changes
            .iter()
            .flat_map(|c| c.items.iter().map(|t| t.id))
            .filter(|id| seen.insert(*id))
            .collect()
    }
}

/// One change in a proposal, with why and what it touches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposedChange {
    /// Its number in the proposal, such as `3`: what partial approval names.
    pub id: String,
    /// What it does.
    pub change: Edit,
    /// Why it's worth making.
    pub reason: String,
    /// How invasive it is.
    pub invasiveness: Invasiveness,
    /// The items it changes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Touched>,
    /// The items it would concern but leaves alone, each with why, such as tags the owner
    /// wrote themselves.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub held: Vec<Held>,
    /// The design documents it rewrites, relative to the archive root, such as `ARCHIVE.md`
    /// or `Finances/_collection.md`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documents: Vec<String>,
    /// The changes it needs: rejecting one of them drops this one too.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// What a change does. Each kind of proposal adds its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Edit {
    /// Replace tags meaning the same thing with one tag.
    MergeTags {
        /// The tags replaced.
        from: Vec<Tag>,
        /// The tag that replaces them.
        into: Tag,
    },
    /// Replace one tag with narrower ones, each item getting the one for its group.
    SplitTag {
        /// The tag split.
        tag: Tag,
        /// The tags it becomes, and which items get each.
        into: Vec<SplitPart>,
    },
    /// Remove a tag that's hardly used and isn't in any vocabulary.
    RetireTag {
        /// The tag.
        tag: Tag,
    },
}

impl Edit {
    /// A one-line description, such as `merge “receipts” into “receipt”`.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::MergeTags { from, into } => format!("merge {} into “{}”", quoted(from), into.0),
            Self::SplitTag { tag, into } => {
                let parts: Vec<Tag> = into.iter().map(|p| p.tag.clone()).collect();
                format!("split “{}” into {}", tag.0, quoted(&parts))
            }
            Self::RetireTag { tag } => format!("retire “{}”", tag.0),
        }
    }
}

fn quoted(tags: &[Tag]) -> String {
    let names: Vec<String> = tags.iter().map(|t| format!("“{}”", t.0)).collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// One of the tags a split tag becomes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SplitPart {
    /// The new tag.
    pub tag: Tag,
    /// What it means, for the vocabularies.
    pub description: String,
    /// The items that get it.
    pub items: Vec<Id>,
}

/// An item a change touches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Touched {
    /// The item.
    pub id: Id,
    /// Its card, relative to the archive root, when the proposal was made.
    pub card: String,
    /// Its title.
    pub title: String,
}

/// An item a change leaves alone, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    /// The item.
    pub id: Id,
    /// Its card, relative to the archive root, when the proposal was made.
    pub card: String,
    /// Its title.
    pub title: String,
    /// Why it's left alone, such as "you wrote its tags".
    pub why: String,
}

/// Which of a proposal's changes an approval takes.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Selection {
    /// The changes to apply, in the proposal's order.
    pub approved: Vec<String>,
    /// The changes the owner left out.
    pub rejected: Vec<String>,
    /// Changes the owner chose that are dropped because one they depend on was left out,
    /// each with the change it needed.
    pub dropped: Vec<(String, String)>,
}

/// Something that stops a restructure.
#[derive(Debug)]
pub enum RestructureError {
    /// The proposal or the request doesn't make sense, for this reason.
    Invalid(String),
    /// Another writer holds the archive.
    Lease(CoordinatorError),
    /// The history couldn't record the change.
    History(HistoryError),
    /// The queue couldn't be read or written.
    Queue(ReviewError),
    /// A file couldn't be read or written.
    Io {
        /// The file.
        path: std::path::PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
}

impl std::fmt::Display for RestructureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(why) => f.write_str(why),
            Self::Lease(e) => write!(f, "{e}"),
            Self::History(e) => write!(f, "{e}"),
            Self::Queue(e) => write!(f, "{e}"),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for RestructureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Lease(e) => Some(e),
            Self::History(e) => Some(e),
            Self::Queue(e) => Some(e),
            Self::Io { source, .. } => Some(source),
            Self::Invalid(_) => None,
        }
    }
}

impl From<CoordinatorError> for RestructureError {
    fn from(e: CoordinatorError) -> Self {
        Self::Lease(e)
    }
}

impl From<HistoryError> for RestructureError {
    fn from(e: HistoryError) -> Self {
        Self::History(e)
    }
}

impl From<ReviewError> for RestructureError {
    fn from(e: ReviewError) -> Self {
        Self::Queue(e)
    }
}

impl From<crate::archive::ArchiveError> for RestructureError {
    fn from(e: crate::archive::ArchiveError) -> Self {
        Self::History(HistoryError::from(e))
    }
}

pub(crate) fn io(path: &std::path::Path) -> impl FnOnce(std::io::Error) -> RestructureError + '_ {
    move |source| RestructureError::Io {
        path: path.to_owned(),
        source,
    }
}

/// Which of `proposal`'s changes to apply when the owner approves `only` (change numbers), or
/// all of them when `only` is `None`. A chosen change that depends on one left out is dropped,
/// and so on down the chain, so nothing runs without what it needs.
///
/// # Errors
///
/// [`RestructureError::Invalid`] if `only` names a change the proposal doesn't have, or
/// nothing is left to apply.
pub fn select(proposal: &Proposal, only: Option<&[String]>) -> Result<Selection, RestructureError> {
    let chosen: BTreeSet<&str> = match only {
        None => proposal.changes.iter().map(|c| c.id.as_str()).collect(),
        Some(ids) => {
            for id in ids {
                if proposal.change(id).is_none() {
                    return Err(RestructureError::Invalid(format!(
                        "the proposal has no change {id}; its changes are numbered 1 to {}",
                        proposal.changes.len()
                    )));
                }
            }
            ids.iter().map(String::as_str).collect()
        }
    };
    let mut selection = Selection::default();
    let mut candidates: Vec<&ProposedChange> = proposal
        .changes
        .iter()
        .filter(|c| chosen.contains(c.id.as_str()))
        .collect();
    // Go round until nothing more drops, so a chain of dependencies drops whole.
    loop {
        let kept: BTreeSet<String> = candidates.iter().map(|c| c.id.clone()).collect();
        let before = candidates.len();
        candidates.retain(|c| match c.depends_on.iter().find(|d| !kept.contains(*d)) {
            Some(missing) => {
                selection.dropped.push((c.id.clone(), missing.clone()));
                false
            }
            None => true,
        });
        if candidates.len() == before {
            break;
        }
    }
    selection.approved = candidates.iter().map(|c| c.id.clone()).collect();
    selection.rejected = proposal
        .changes
        .iter()
        .filter(|c| !chosen.contains(c.id.as_str()))
        .map(|c| c.id.clone())
        .collect();
    if selection.approved.is_empty() {
        return Err(RestructureError::Invalid(
            "nothing is left to apply; reject the proposal instead".into(),
        ));
    }
    Ok(selection)
}

/// What applying one change did.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Outcome {
    /// The change's number.
    pub change: String,
    /// The items it changed.
    pub items: Vec<Id>,
    /// The items it would have changed but left alone this time, each with why: changed by
    /// the owner since it was proposed, checked out, or gone.
    pub skipped: Vec<(Id, String)>,
    /// The design documents it rewrote.
    pub documents: Vec<String>,
    /// Vocabulary entries it kept because items it left alone still use them.
    pub kept: Vec<String>,
}

/// What approving a proposal did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Approved {
    /// Which changes ran, and which didn't.
    pub selection: Selection,
    /// What each change that ran did.
    pub outcomes: Vec<Outcome>,
    /// The change's summary, as History shows it.
    pub summary: String,
    /// The history commit, or `None` when nothing needed changing after all.
    pub commit: Option<String>,
}

/// Applies the changes of `proposal` that `only` names (all of them when `None`) as one
/// `restructure` change in history, holding the writer lease as `holder`, and removes the
/// proposal from the queue. Undoing that change in History reverses every part of it.
///
/// Each change is applied to the archive as it is now, not as it was proposed: an item whose
/// card the owner has changed since, or that's checked out, is left alone and reported.
///
/// # Errors
///
/// [`RestructureError::Invalid`] for an unknown change, [`RestructureError::Lease`] if
/// another writer holds the archive, or an error writing a file, the history or the queue.
///
/// # Panics
///
/// Never: every selected change is in the proposal.
pub fn approve(
    archive: &Archive,
    proposal: &Proposal,
    only: Option<&[String]>,
    holder: Holder,
) -> Result<Approved, RestructureError> {
    let selection = select(proposal, only)?;
    let lease = DiskLock::for_archive(archive).acquire(holder, "restructure", DEFAULT_LEASE)?;
    let result = (|| -> Result<Approved, RestructureError> {
        let mut outcomes = Vec::new();
        for id in &selection.approved {
            let change = proposal
                .change(id)
                .expect("selected changes are in the proposal");
            outcomes.push(apply_edit(archive, change)?);
        }
        // A proposal applied straight from where it was built was never queued.
        match ProposalQueue::of(archive).remove(proposal.id) {
            Ok(()) | Err(ReviewError::NotFound(..)) => {}
            Err(e) => return Err(e.into()),
        }
        let summary = summary(proposal, &selection);
        let mut change = Change::new(
            Operation::Restructure,
            &summary,
            lease.info().person,
            lease.info().device,
        );
        change.body = Some(body(proposal, &selection, &outcomes));
        let mut seen = BTreeSet::new();
        change.items = outcomes
            .iter()
            .flat_map(|o| o.items.iter().copied())
            .filter(|id| seen.insert(*id))
            .collect();
        let commit = History::open(archive)?.commit(archive, &change, &lease)?;
        Ok(Approved {
            selection: selection.clone(),
            outcomes,
            summary,
            commit,
        })
    })();
    lease.release()?;
    result
}

/// Applies one change, dispatching on its kind.
fn apply_edit(archive: &Archive, change: &ProposedChange) -> Result<Outcome, RestructureError> {
    match &change.change {
        Edit::MergeTags { .. } | Edit::SplitTag { .. } | Edit::RetireTag { .. } => {
            tags::apply(archive, change)
        }
    }
}

/// Puts `proposal` in the queue in place of any waiting proposal of the same kind for the
/// same section, which it supersedes. Returns how many it replaced.
///
/// # Errors
///
/// An error reading or writing the queue.
pub fn queue(archive: &Archive, proposal: &Proposal) -> Result<usize, RestructureError> {
    let queue = ProposalQueue::of(archive);
    let mut replaced = 0;
    for old in waiting(archive, Some(proposal.kind))? {
        if old.section == proposal.section && old.id != proposal.id {
            queue.remove(old.id)?;
            replaced += 1;
        }
    }
    queue.save(proposal)?;
    Ok(replaced)
}

/// Removes `proposal` from the queue without changing anything else.
///
/// # Errors
///
/// An error writing the queue.
pub fn reject(archive: &Archive, proposal: &Proposal) -> Result<(), RestructureError> {
    Ok(ProposalQueue::of(archive).remove(proposal.id)?)
}

/// The proposals waiting, oldest first, of `kind` if given.
///
/// # Errors
///
/// An error reading the queue.
pub fn waiting(
    archive: &Archive,
    kind: Option<ProposalKind>,
) -> Result<Vec<Proposal>, RestructureError> {
    let (proposals, _) = ProposalQueue::of(archive).list()?;
    Ok(proposals
        .into_iter()
        .filter(|p| kind.is_none_or(|k| p.kind == k))
        .collect())
}

/// The waiting proposal `id` names (its start or end, as [`Queue::find`] takes), or the only
/// one of `kind` when `id` is `None`.
///
/// # Errors
///
/// [`RestructureError::Invalid`] when there's none, or several and no `id`, or an error
/// reading the queue.
pub fn find(
    archive: &Archive,
    kind: ProposalKind,
    id: Option<&str>,
) -> Result<Proposal, RestructureError> {
    if let Some(id) = id {
        return Ok(ProposalQueue::of(archive).find(id)?);
    }
    let mut found = waiting(archive, Some(kind))?;
    match found.len() {
        0 => Err(RestructureError::Invalid(format!(
            "no {} proposal is waiting; make one with `carrel restructure {} propose`",
            kind.as_str(),
            kind.as_str()
        ))),
        1 => Ok(found.remove(0)),
        _ => Err(RestructureError::Invalid(format!(
            "{} {} proposals are waiting; name one",
            found.len(),
            kind.as_str()
        ))),
    }
}

fn summary(proposal: &Proposal, selection: &Selection) -> String {
    let what = match proposal.kind {
        ProposalKind::Tags => "tags".to_owned(),
        ProposalKind::Section if proposal.section.is_empty() => "the archive".to_owned(),
        ProposalKind::Section => proposal.section.clone(),
    };
    let total = proposal.changes.len();
    let plural = if total == 1 { "" } else { "s" };
    let of = if selection.approved.len() == total {
        format!("{total} change{plural}")
    } else {
        format!("{} of {total} change{plural}", selection.approved.len())
    };
    format!("Restructure {what}: {of}")
}

fn body(proposal: &Proposal, selection: &Selection, outcomes: &[Outcome]) -> String {
    use std::fmt::Write as _;
    let mut text = format!("Proposal {}.\n", proposal.id);
    for outcome in outcomes {
        let change = proposal
            .change(&outcome.change)
            .expect("outcomes are of the proposal's changes");
        let n = outcome.items.len();
        let _ = writeln!(
            text,
            "\n{}. {} ({n} item{}): {}",
            change.id,
            change.change.describe(),
            if n == 1 { "" } else { "s" },
            change.reason
        );
    }
    if !selection.rejected.is_empty() {
        let _ = writeln!(text, "\nLeft out: {}.", selection.rejected.join(", "));
    }
    for (id, missing) in &selection.dropped {
        let _ = writeln!(text, "Dropped {id}, since it needs {missing}.");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_ranked_as_the_design_lists_them() {
        let ranks: Vec<u8> = Invasiveness::ALL.iter().map(|k| k.rank()).collect();
        assert_eq!(ranks, [1, 2, 3, 4, 5]);
        assert!(Invasiveness::Subdividing < Invasiveness::BetweenBranches);
        assert_eq!(
            serde_json::to_value(Invasiveness::BetweenBranches).unwrap(),
            "between-branches"
        );
    }
}
