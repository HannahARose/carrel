//! Restructure proposals (`docs/design/restructure.md`): those waiting, each change with its
//! reason and how invasive it is, what the chosen changes would leave beside what's there
//! now, and approving all or part of one, or rejecting it, as `carrel restructure` does.

use carrel_core::card::Id;
use carrel_core::discover::Catalog;
use carrel_core::graph::Graph;
use carrel_core::restructure::preview::{self, PreviewCollection, PreviewTag};
use carrel_core::restructure::tags::{TagOptions, propose};
use carrel_core::restructure::{
    self, Invasiveness as CoreInvasiveness, Proposal, ProposalQueue, ProposedChange,
    RestructureError,
};

use crate::{ArchiveHandle, CarrelError};

impl From<RestructureError> for CarrelError {
    fn from(error: RestructureError) -> Self {
        match error {
            RestructureError::Invalid(why) => Self::Invalid { message: why },
            RestructureError::Lease(e) => e.into(),
            error => Self::failed(error),
        }
    }
}

/// How much a change disturbs what the owner knows of the archive, least first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, uniffi::Enum)]
pub enum Invasiveness {
    /// Rewriting design documents and tags: nothing moves.
    Rewriting,
    /// Grouping a collection's items into new subcollections inside it.
    Subdividing,
    /// Renaming a collection.
    Renaming,
    /// Moving items between branches, and merging or splitting collections across them.
    BetweenBranches,
    /// Moving or merging top-level collections.
    TopLevel,
}

impl From<CoreInvasiveness> for Invasiveness {
    fn from(kind: CoreInvasiveness) -> Self {
        match kind {
            CoreInvasiveness::Rewriting => Self::Rewriting,
            CoreInvasiveness::Subdividing => Self::Subdividing,
            CoreInvasiveness::Renaming => Self::Renaming,
            CoreInvasiveness::BetweenBranches => Self::BetweenBranches,
            CoreInvasiveness::TopLevel => Self::TopLevel,
        }
    }
}

/// An item a change touches, or leaves alone.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ProposalItem {
    /// Its identifier.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Its card when the proposal was made.
    pub card: String,
    /// Why it's left alone, for an item the change holds back.
    pub why: Option<String>,
}

/// One change in a proposal.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RestructureChange {
    /// Its number in the proposal, which partial approval names.
    pub id: String,
    /// What it does, in a line.
    pub description: String,
    /// Why it's worth making.
    pub reason: String,
    /// How invasive it is.
    pub invasiveness: Invasiveness,
    /// That, in words.
    pub invasiveness_words: String,
    /// The items it changes.
    pub items: Vec<ProposalItem>,
    /// The items it leaves alone, each with why.
    pub held: Vec<ProposalItem>,
    /// The design documents it rewrites.
    pub documents: Vec<String>,
    /// The changes it needs; leaving one of them out drops this one too.
    pub depends_on: Vec<String>,
}

impl From<&ProposedChange> for RestructureChange {
    fn from(change: &ProposedChange) -> Self {
        Self {
            id: change.id.clone(),
            description: change.change.describe(),
            reason: change.reason.clone(),
            invasiveness: change.invasiveness.into(),
            invasiveness_words: change.invasiveness.describe().to_owned(),
            items: change
                .items
                .iter()
                .map(|t| ProposalItem {
                    id: t.id.to_string(),
                    title: t.title.clone(),
                    card: t.card.clone(),
                    why: None,
                })
                .collect(),
            held: change
                .held
                .iter()
                .map(|h| ProposalItem {
                    id: h.id.to_string(),
                    title: h.title.clone(),
                    card: h.card.clone(),
                    why: Some(h.why.clone()),
                })
                .collect(),
            documents: change.documents.clone(),
            depends_on: change.depends_on.clone(),
        }
    }
}

/// A restructure proposal waiting for the owner.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RestructureProposal {
    /// Its identifier.
    pub id: String,
    /// What it reorganizes: `tags`, or `section`.
    pub kind: String,
    /// The section it covers: a collection's path, or empty for the whole archive.
    pub section: String,
    /// When it was proposed.
    pub created: String,
    /// The most invasive of its changes.
    pub invasiveness: Invasiveness,
    /// Why it's worth making.
    pub rationale: String,
    /// The changes, in the order they'd be applied.
    pub changes: Vec<RestructureChange>,
}

impl From<&Proposal> for RestructureProposal {
    fn from(proposal: &Proposal) -> Self {
        Self {
            id: proposal.id.to_string(),
            kind: proposal.kind.as_str().to_owned(),
            section: proposal.section.clone(),
            created: proposal.created.0.clone(),
            invasiveness: proposal.invasiveness.into(),
            rationale: proposal.rationale.clone(),
            changes: proposal.changes.iter().map(Into::into).collect(),
        }
    }
}

/// A collection on one side of a proposal's preview.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PreviewedCollection {
    /// Its folder, relative to the archive root.
    pub path: String,
    /// How many items are directly in it.
    pub items: u64,
    /// Whether the chosen changes create it (after) or remove it (before).
    pub changed: bool,
}

/// A tag a proposal touches, on one side of its preview.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PreviewedTag {
    /// The tag.
    pub tag: String,
    /// How many items have it.
    pub items: u64,
}

/// The archive as it is and as a proposal's chosen changes would leave it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RestructurePreview {
    /// The collections as they are.
    pub before: Vec<PreviewedCollection>,
    /// The collections after.
    pub after: Vec<PreviewedCollection>,
    /// The tags the proposal touches, as they are.
    pub tags_before: Vec<PreviewedTag>,
    /// Those tags after.
    pub tags_after: Vec<PreviewedTag>,
}

fn collections(side: Vec<PreviewCollection>) -> Vec<PreviewedCollection> {
    side.into_iter()
        .map(|c| PreviewedCollection {
            path: c.path,
            items: c.items as u64,
            changed: c.changed,
        })
        .collect()
}

fn tags(side: Vec<PreviewTag>) -> Vec<PreviewedTag> {
    side.into_iter()
        .map(|t| PreviewedTag {
            tag: t.tag,
            items: t.items as u64,
        })
        .collect()
}

/// What approving a proposal did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RestructureApproved {
    /// The change in history, as History shows it.
    pub summary: String,
    /// The history commit, or `None` when nothing needed changing after all.
    pub commit: Option<String>,
    /// The changes that ran.
    pub approved: Vec<String>,
    /// The changes left out.
    pub rejected: Vec<String>,
    /// Changes chosen but dropped because one they need was left out, each with why.
    pub dropped: Vec<String>,
    /// Items left alone this time, each with why: changed since, checked out, or gone.
    pub skipped: Vec<String>,
}

#[uniffi::export]
impl ArchiveHandle {
    /// The restructure proposals waiting, oldest first.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the queue can't be read.
    pub fn restructure_proposals(&self) -> Result<Vec<RestructureProposal>, CarrelError> {
        let archive = self.fresh()?;
        Ok(restructure::waiting(&archive, None)?
            .iter()
            .map(Into::into)
            .collect())
    }

    /// Proposes tag restructures for the whole archive, as `carrel restructure tags propose`
    /// does, putting the proposal in the queue in place of any tag proposal waiting. `None`
    /// when there's nothing worth proposing.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive can't be read or the queue written.
    pub fn propose_tag_restructure(&self) -> Result<Option<RestructureProposal>, CarrelError> {
        let archive = self.fresh()?;
        let index = carrel_core::index::related::open(&archive);
        let vectors = |id: Id| index.as_ref().ok().and_then(|i| i.card_vector(id));
        let proposal = propose(&archive, &vectors, &TagOptions::default())?;
        if proposal.changes.is_empty() {
            return Ok(None);
        }
        restructure::queue(&archive, &proposal)?;
        Ok(Some((&proposal).into()))
    }

    /// What the proposal `id`'s changes named in `only` would leave, beside the archive as it
    /// is now. Nothing is written.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if there's no such proposal or the archive can't be read.
    pub fn restructure_preview(
        &self,
        id: String,
        only: Vec<String>,
    ) -> Result<RestructurePreview, CarrelError> {
        let archive = self.fresh()?;
        let proposal = ProposalQueue::of(&archive).find(&id)?;
        let graph = Graph::build(&Catalog::read(&archive)?);
        let shown = preview::preview(&graph, &proposal, &only);
        Ok(RestructurePreview {
            before: collections(shown.before),
            after: collections(shown.after),
            tags_before: tags(shown.tags_before),
            tags_after: tags(shown.tags_after),
        })
    }

    /// Approves the proposal `id`: the changes `only` names, or all of them when it's
    /// `None`, as one change in history that undoes as one. A change that needs one left out
    /// is dropped too.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for an unknown change, [`CarrelError::Busy`] if another
    /// writer holds the archive, or [`CarrelError::Failed`].
    pub fn approve_restructure(
        &self,
        id: String,
        only: Option<Vec<String>>,
    ) -> Result<RestructureApproved, CarrelError> {
        let archive = self.fresh()?;
        let proposal = ProposalQueue::of(&archive).find(&id)?;
        let approved = restructure::approve(&archive, &proposal, only.as_deref(), self.holder()?)?;
        Ok(RestructureApproved {
            summary: approved.summary,
            commit: approved.commit,
            approved: approved.selection.approved,
            rejected: approved.selection.rejected,
            dropped: approved
                .selection
                .dropped
                .into_iter()
                .map(|(change, needs)| format!("{change}, which needs {needs}"))
                .collect(),
            skipped: approved
                .outcomes
                .iter()
                .flat_map(|o| o.skipped.iter())
                .map(|(id, why)| {
                    let title = proposal
                        .changes
                        .iter()
                        .flat_map(|c| c.items.iter())
                        .find(|t| t.id == *id)
                        .map_or_else(|| id.to_string(), |t| format!("“{}”", t.title));
                    format!("{title}: {why}")
                })
                .collect(),
        })
    }

    /// Rejects the proposal `id`: it leaves the queue and nothing else changes.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if there's no such proposal or the queue can't be written.
    pub fn reject_restructure(&self, id: String) -> Result<(), CarrelError> {
        let archive = self.fresh()?;
        let proposal = ProposalQueue::of(&archive).find(&id)?;
        restructure::reject(&archive, &proposal)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use carrel_core::card::Tag;
    use carrel_core::restructure::{Edit, ProposalKind, Touched};

    use super::*;
    use crate::tests::Sample;

    /// Queues a tag proposal for the sample: retire `oscillators` on the items that have it,
    /// and merge `notes` into `note` on none, so the second changes nothing.
    fn queued(sample: &Sample) -> String {
        let archive = sample.handle.fresh().unwrap();
        let graph = Graph::build(&Catalog::read(&archive).unwrap());
        let tagged: Vec<Touched> = graph
            .items
            .iter()
            .filter(|i| i.tags.iter().any(|t| t == "oscillators"))
            .map(|i| Touched {
                id: i.id,
                card: i.card.clone(),
                title: i.title.clone(),
            })
            .collect();
        assert!(
            !tagged.is_empty(),
            "the sample has items tagged oscillators"
        );
        let mut proposal = Proposal::new(ProposalKind::Tags, "");
        proposal.rationale = "Tidier tags.".into();
        let change = |id: &str, edit: Edit, items: Vec<Touched>| ProposedChange {
            id: id.into(),
            change: edit,
            reason: "rarely used".into(),
            invasiveness: CoreInvasiveness::Rewriting,
            items,
            held: Vec::new(),
            documents: Vec::new(),
            depends_on: Vec::new(),
            extra: carrel_core::card::Extra::new(),
        };
        proposal.changes = vec![
            change(
                "1",
                Edit::RetireTag {
                    tag: Tag("oscillators".into()),
                },
                tagged,
            ),
            change(
                "2",
                Edit::MergeTags {
                    from: vec![Tag("notes".into())],
                    into: Tag("note".into()),
                },
                Vec::new(),
            ),
        ];
        restructure::queue(&archive, &proposal).unwrap();
        proposal.id.to_string()
    }

    #[test]
    fn a_proposal_is_listed_previewed_and_approved_in_part() {
        let sample = Sample::new();
        let id = queued(&sample);
        let listed = sample.handle.restructure_proposals().unwrap();
        let proposal = listed.iter().find(|p| p.id == id).unwrap();
        assert_eq!(proposal.kind, "tags");
        assert_eq!(proposal.changes.len(), 2);
        let retire = &proposal.changes[0];
        assert_eq!(retire.description, "retire “oscillators”");
        assert_eq!(retire.invasiveness, Invasiveness::Rewriting);
        assert_eq!(
            retire.invasiveness_words,
            "rewriting design documents and tags"
        );

        let none = sample
            .handle
            .restructure_preview(id.clone(), Vec::new())
            .unwrap();
        assert!(none.tags_before.iter().any(|t| t.tag == "oscillators"));
        assert_eq!(
            none.tags_before, none.tags_after,
            "nothing chosen, nothing changes"
        );
        let first = sample
            .handle
            .restructure_preview(id.clone(), vec!["1".into()])
            .unwrap();
        assert!(first.tags_after.iter().all(|t| t.tag != "oscillators"));
        assert_eq!(first.before, first.after, "tags move no files");

        let approved = sample
            .handle
            .approve_restructure(id.clone(), Some(vec!["1".into()]))
            .unwrap();
        assert_eq!(approved.approved, ["1"]);
        assert_eq!(approved.rejected, ["2"]);
        assert!(approved.commit.is_some());
        assert_eq!(sample.handle.restructure_proposals().unwrap(), []);
    }

    #[test]
    fn a_rejected_proposal_leaves_the_queue_and_nothing_else() {
        let sample = Sample::new();
        let id = queued(&sample);
        sample.handle.reject_restructure(id.clone()).unwrap();
        assert_eq!(sample.handle.restructure_proposals().unwrap(), []);
        assert!(sample.handle.approve_restructure(id, None).is_err());
    }
}
