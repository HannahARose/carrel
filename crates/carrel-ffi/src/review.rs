//! Review and Questions (`docs/design/interactive.md`): the proposals waiting for the owner,
//! grouped by the ingest batch they came from, and the decisions on them, through
//! [`carrel_core::actions`] as `carrel review` and the interactive mode make them.

use std::collections::BTreeMap;

use carrel_core::actions::{self, Kept};
use carrel_core::backup::{Backups, Kopia};
use carrel_core::compressed::plan::Choice;
use carrel_core::ingest::filing::{Outcome, Placement};
use carrel_core::questions::{QuestionEntry, QuestionKind, QuestionQueue};
use carrel_core::review::{Confidence as CoreConfidence, ReviewEntry, ReviewQueue, State};

use crate::records::CardFields;
use crate::{ArchiveHandle, CarrelError, ItemKind, text};

/// Where a proposal stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ProposalState {
    /// Waiting for the owner.
    Proposed,
    /// Something couldn't be settled; see its problems.
    NeedsOwner,
    /// Approved, to be filed where proposed.
    Approved,
    /// To go to Holding.
    Held,
    /// To stay where it is.
    Rejected,
    /// Filed.
    Archived,
}

impl From<State> for ProposalState {
    fn from(state: State) -> Self {
        match state {
            State::Proposed => Self::Proposed,
            State::NeedsOwner => Self::NeedsOwner,
            State::Approved => Self::Approved,
            State::Held => Self::Held,
            State::Rejected => Self::Rejected,
            State::Archived => Self::Archived,
        }
    }
}

/// How sure a proposal is of where it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Confidence {
    /// The passes disagreed, the model chose Holding, or validation needed a retry.
    Low,
    /// A model chose it and a second pass agreed.
    Medium,
    /// A routing rule decided.
    High,
}

impl From<CoreConfidence> for Confidence {
    fn from(confidence: CoreConfidence) -> Self {
        match confidence {
            CoreConfidence::Low => Self::Low,
            CoreConfidence::Medium => Self::Medium,
            CoreConfidence::High => Self::High,
        }
    }
}

/// What happens to a compressed file when its proposal is approved.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CompressedChoice {
    /// Whether it's expanded on approval; otherwise kept compressed, or asking.
    pub expand: bool,
    /// Whether the owner has to choose before it's approved.
    pub asking: bool,
    /// Why it's kept or asked about, in words.
    pub reason: Option<String>,
    /// How many files expanding makes, when known.
    pub files: Option<u64>,
}

/// An archived item a proposal duplicates, or likely duplicates.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Duplicate {
    /// The archived item's identifier.
    pub id: String,
    /// Its card's path in the archive.
    pub card: String,
    /// Why it matched: `same checksum`, or the likely reasons.
    pub why: Vec<String>,
}

/// A proposal to file an item.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Proposal {
    /// Its identifier.
    pub id: String,
    /// Its identifier's start, as people see it.
    pub short_id: String,
    /// The ingest batch it came from.
    pub batch: String,
    /// When it was proposed.
    pub created: String,
    /// Where it stands.
    pub state: ProposalState,
    /// Where the item is now.
    pub source: String,
    /// The item's kind.
    pub kind: ItemKind,
    /// The card it would get, as the owner edits it: its new tag among its tags.
    pub fields: CardFields,
    /// The new tag it would add to the destination's tags, if any.
    pub new_tag: Option<String>,
    /// The collection it would go to.
    pub destination: String,
    /// How sure that is.
    pub confidence: Confidence,
    /// Why it was chosen.
    pub reasons: Vec<String>,
    /// What needs settling, in words.
    pub problems: Vec<String>,
    /// Why its sensitivity is what it is, when it's sensitive or restricted.
    pub sensitivity_reasons: Vec<String>,
    /// Archived items it duplicates, the exact duplicate first.
    pub duplicates: Vec<Duplicate>,
    /// For a compressed file, what approving does with it.
    pub compressed: Option<CompressedChoice>,
    /// Whether "approve all with high confidence" approves it.
    pub high: bool,
}

impl From<&ReviewEntry> for Proposal {
    fn from(entry: &ReviewEntry) -> Self {
        let mut duplicates = Vec::new();
        if let Some(id) = entry.duplicate_of {
            duplicates.push(Duplicate {
                id: id.to_string(),
                card: String::new(),
                why: vec!["same checksum".into()],
            });
        }
        duplicates.extend(entry.likely_duplicates.iter().map(|l| Duplicate {
            id: l.id.to_string(),
            card: l.card.clone(),
            why: l.why.iter().map(ToString::to_string).collect(),
        }));
        Self {
            id: entry.id.to_string(),
            short_id: carrel_core::review::short_id(entry.id),
            batch: entry.batch.to_string(),
            created: entry.created.0.clone(),
            state: entry.state.into(),
            source: text(&entry.source.path),
            kind: entry.kind.into(),
            fields: (&actions::draft_fields(entry)).into(),
            new_tag: entry.proposed_tags.first().map(|t| t.0.clone()),
            destination: entry.destination.path.clone(),
            confidence: entry.destination.confidence.into(),
            reasons: entry.destination.reasons.clone(),
            problems: entry.problems.iter().map(|p| p.message.clone()).collect(),
            sensitivity_reasons: entry.sensitivity_reasons.clone(),
            duplicates,
            compressed: entry.compressed.as_ref().map(|plan| CompressedChoice {
                expand: plan.action == Choice::Expand,
                asking: plan.action == Choice::Ask,
                reason: plan.reason.clone(),
                files: plan.files,
            }),
            high: actions::is_high(entry),
        }
    }
}

/// The proposals from one ingest batch, which approve together.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReviewBatch {
    /// The batch's identifier.
    pub id: String,
    /// When its first proposal was made.
    pub created: String,
    /// Its proposals still waiting, by destination.
    pub proposals: Vec<Proposal>,
}

/// What the Review screen shows.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ReviewList {
    /// The batches with proposals waiting, oldest first.
    pub batches: Vec<ReviewBatch>,
    /// Proposals that couldn't be read, and why.
    pub unreadable: Vec<String>,
}

/// An item filed.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FiledItem {
    /// The proposal it came from.
    pub proposal: String,
    /// The new item's identifier.
    pub item: String,
    /// Its title.
    pub title: String,
    /// The item's path in the archive.
    pub path: String,
    /// Its card's path in the archive.
    pub card: String,
    /// Where the original was left, when it was copied rather than moved.
    pub kept_original: Option<String>,
}

/// A proposal that couldn't be filed.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NotFiled {
    /// The proposal.
    pub proposal: String,
    /// Its title.
    pub title: String,
    /// Why, in words.
    pub error: String,
}

/// What filing a batch did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FilingOutcome {
    /// The items filed, in order.
    pub filed: Vec<FiledItem>,
    /// The proposals that couldn't be.
    pub failed: Vec<NotFiled>,
    /// The history commit, if anything was filed.
    pub commit: Option<String>,
    /// The backup snapshot taken first, when backups are set up.
    pub snapshot: Option<String>,
    /// Why no snapshot was taken, when backups are set up but failed.
    pub snapshot_error: Option<String>,
    /// Listings that couldn't be brought up to date, and why.
    pub listing_problems: Vec<String>,
}

impl From<Outcome> for FilingOutcome {
    fn from(outcome: Outcome) -> Self {
        Self {
            filed: outcome
                .filed
                .iter()
                .map(|f| FiledItem {
                    proposal: f.entry.to_string(),
                    item: f.item.to_string(),
                    title: f.title.clone(),
                    path: f.path.clone(),
                    card: f.card.clone(),
                    kept_original: f.kept_original.as_deref().map(text),
                })
                .collect(),
            failed: outcome
                .failed
                .iter()
                .map(|(entry, error)| NotFiled {
                    proposal: entry.id.to_string(),
                    title: entry.card.title.clone(),
                    error: error.to_string(),
                })
                .collect(),
            commit: outcome.commit,
            snapshot: outcome.snapshot.map(|s| s.id),
            snapshot_error: outcome.snapshot_error.map(|e| e.to_string()),
            listing_problems: outcome.listing_problems,
        }
    }
}

/// A proposal pointed at another collection.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MovedProposal {
    /// The proposal.
    pub proposal: String,
    /// The collection it now goes to.
    pub to: String,
    /// Tags left off because they don't apply there.
    pub dropped: Vec<String>,
}

/// One row of a side-by-side comparison of a proposal and an archived item.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ComparedField {
    /// The field, such as `title`.
    pub label: String,
    /// The proposal's value.
    pub proposed: String,
    /// The archived item's value.
    pub archived: String,
    /// Whether they differ.
    pub differs: bool,
}

/// A proposal beside an archived item it duplicates or likely duplicates.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Comparison {
    /// The archived item.
    pub duplicate: Duplicate,
    /// The fields side by side.
    pub rows: Vec<ComparedField>,
}

/// What a question is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum QuestionTopic {
    /// What a recurring term means; the answer becomes a glossary fact.
    Term,
    /// Which collection items about a term belong in; the answer is a collection's path.
    Collection,
}

/// A question for the owner.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Question {
    /// Its identifier.
    pub id: String,
    /// What it's about.
    pub topic: QuestionTopic,
    /// The term it's about, as it appears in the items' names.
    pub subject: String,
    /// The question, in words.
    pub question: String,
    /// The proposals waiting on the answer.
    pub proposals: Vec<String>,
    /// For a collection question, the collections to choose from.
    pub choices: Vec<String>,
}

impl From<&QuestionEntry> for Question {
    fn from(entry: &QuestionEntry) -> Self {
        Self {
            id: entry.id.to_string(),
            topic: match entry.kind {
                QuestionKind::Term => QuestionTopic::Term,
                QuestionKind::Collection => QuestionTopic::Collection,
            },
            subject: entry.subject.clone(),
            question: entry.question.clone(),
            proposals: entry.items.iter().map(ToString::to_string).collect(),
            choices: entry.choices.clone(),
        }
    }
}

/// What answering a question did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Answered {
    /// For a term, the glossary's history commit.
    pub commit: Option<String>,
    /// For a term, the intake entries queued to be drafted again with the answer. The
    /// worker drafts them; the app or the agent starts it.
    pub requeued: Vec<String>,
    /// For a collection, the proposals pointed there.
    pub moved: Vec<MovedProposal>,
}

impl ArchiveHandle {
    /// Files `entries` where `placement` says, as this device, with a snapshot first when
    /// backups are set up, as `carrel review approve` does.
    fn file(
        &self,
        archive: &carrel_core::archive::Archive,
        entries: Vec<ReviewEntry>,
        placement: Placement,
    ) -> Result<FilingOutcome, CarrelError> {
        let backups = Kopia::for_archive(archive).ok();
        Ok(actions::decide(
            archive,
            entries,
            placement,
            self.holder()?,
            backups.as_ref().map(|b| b as &dyn Backups),
        )?
        .into())
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// The proposals waiting for the owner, by batch, each batch's by destination.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the queue can't be read.
    pub fn review(&self) -> Result<ReviewList, CarrelError> {
        let archive = self.fresh()?;
        let (entries, unreadable) = ReviewQueue::of(&archive).list()?;
        let mut batches: BTreeMap<String, ReviewBatch> = BTreeMap::new();
        for entry in entries.iter().filter(|e| e.state.is_open()) {
            let batch = batches
                .entry(entry.batch.to_string())
                .or_insert_with(|| ReviewBatch {
                    id: entry.batch.to_string(),
                    created: entry.created.0.clone(),
                    proposals: Vec::new(),
                });
            if entry.created.0 < batch.created {
                batch.created.clone_from(&entry.created.0);
            }
            batch.proposals.push(entry.into());
        }
        let mut batches: Vec<ReviewBatch> = batches.into_values().collect();
        for batch in &mut batches {
            batch
                .proposals
                .sort_by(|a, b| (&a.destination, &a.created).cmp(&(&b.destination, &b.created)));
        }
        batches.sort_by(|a, b| a.created.cmp(&b.created));
        Ok(ReviewList {
            batches,
            unreadable: unreadable.iter().map(ToString::to_string).collect(),
        })
    }

    /// The proposal `id` (its identifier, or the start or end of it).
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if it isn't found.
    pub fn proposal(&self, id: String) -> Result<Proposal, CarrelError> {
        let archive = self.fresh()?;
        Ok((&ReviewQueue::of(&archive).find(&id)?).into())
    }

    /// Files the proposals `ids` where they're proposed, as one batch.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Busy`] if another writer holds the archive, [`CarrelError::Invalid`]
    /// for a proposal that's already decided, or [`CarrelError::Failed`].
    pub fn approve(&self, ids: Vec<String>) -> Result<FilingOutcome, CarrelError> {
        let archive = self.fresh()?;
        let entries = actions::proposals(&archive, &ids)?;
        self.file(&archive, entries, Placement::Destination)
    }

    /// Files every proposal approving with high confidence takes (see [`Proposal::high`]).
    ///
    /// # Errors
    ///
    /// As [`ArchiveHandle::approve`].
    pub fn approve_all_high(&self) -> Result<FilingOutcome, CarrelError> {
        let archive = self.fresh()?;
        let entries = actions::all_high(&archive)?;
        self.file(&archive, entries, Placement::Destination)
    }

    /// Files the proposal `id`, which duplicates an archived item, linked to that item: as a
    /// new version of it, or beside it. `target` names the archived item; `None` when there's
    /// only one it could be.
    ///
    /// # Errors
    ///
    /// As [`ArchiveHandle::approve`], and [`CarrelError::Invalid`] when `target` is needed or
    /// matches nothing.
    pub fn approve_duplicate(
        &self,
        id: String,
        as_version: bool,
        target: Option<String>,
    ) -> Result<FilingOutcome, CarrelError> {
        let archive = self.fresh()?;
        let mut entries = actions::proposals(&archive, &[id])?;
        let kept = if as_version {
            Kept::AsVersion
        } else {
            Kept::Both
        };
        for entry in &mut entries {
            actions::keep(&archive, entry, kept, target.as_deref())?;
        }
        self.file(&archive, entries, Placement::Destination)
    }

    /// Files the proposals `ids` in Holding, for the owner to place later.
    ///
    /// # Errors
    ///
    /// As [`ArchiveHandle::approve`].
    pub fn hold(&self, ids: Vec<String>) -> Result<FilingOutcome, CarrelError> {
        let archive = self.fresh()?;
        let entries = actions::proposals(&archive, &ids)?;
        self.file(&archive, entries, Placement::Holding)
    }

    /// Rejects the proposals `ids`: their items stay where they are. Returns how many.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for a proposal that's already decided, or
    /// [`CarrelError::Failed`].
    pub fn reject(&self, ids: Vec<String>) -> Result<u32, CarrelError> {
        let archive = self.fresh()?;
        let entries = actions::proposals(&archive, &ids)?;
        let rejected = actions::reject(&archive, entries)?;
        Ok(u32::try_from(rejected.len()).unwrap_or(u32::MAX))
    }

    /// Points the proposal `id` at the collection at `collection`. Nothing is filed until
    /// it's approved.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for a decided proposal or a collection that doesn't exist.
    pub fn move_proposal(
        &self,
        id: String,
        collection: String,
    ) -> Result<MovedProposal, CarrelError> {
        let archive = self.fresh()?;
        let moved = actions::move_to(&archive, &id, &collection)?;
        Ok(MovedProposal {
            proposal: moved.entry.id.to_string(),
            to: moved.to,
            dropped: moved.dropped,
        })
    }

    /// Says whether the compressed file proposal `id` is expanded on approval or kept
    /// compressed.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if it isn't a compressed file or can't be expanded.
    pub fn choose_compressed(&self, id: String, expand: bool) -> Result<Proposal, CarrelError> {
        let archive = self.fresh()?;
        let choice = if expand { Choice::Expand } else { Choice::Keep };
        Ok((&actions::choose_compressed(&archive, &id, choice)?).into())
    }

    /// The proposal `id` beside each archived item it duplicates, the exact one first.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if it isn't found or the archive can't be read.
    pub fn compare(&self, id: String) -> Result<Vec<Comparison>, CarrelError> {
        let archive = self.fresh()?;
        let entry = ReviewQueue::of(&archive).find(&id)?;
        Ok(actions::compare(&archive, &entry)?
            .into_iter()
            .map(|c| Comparison {
                duplicate: Duplicate {
                    id: c.id.to_string(),
                    card: c.card,
                    why: c.why,
                },
                rows: c
                    .rows
                    .into_iter()
                    .map(|r| ComparedField {
                        label: r.label.to_owned(),
                        proposed: r.proposed,
                        archived: r.archived,
                        differs: r.differs,
                    })
                    .collect(),
            })
            .collect())
    }

    /// The questions waiting for the owner, those holding up the most proposals first.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the queue can't be read.
    pub fn questions(&self) -> Result<Vec<Question>, CarrelError> {
        let archive = self.fresh()?;
        let (mut questions, _) = QuestionQueue::of(&archive).list()?;
        questions.sort_by_key(|q| std::cmp::Reverse(q.items.len()));
        Ok(questions.iter().map(Into::into).collect())
    }

    /// Answers the question `id`: a term's answer is added to the glossary and the
    /// proposals waiting on it are queued to be drafted again; a collection's answer is a
    /// collection's path, where the waiting proposals then go.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for an empty answer or an unknown collection,
    /// [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`].
    pub fn answer_question(&self, id: String, answer: String) -> Result<Answered, CarrelError> {
        let archive = self.fresh()?;
        let question = QuestionQueue::of(&archive).find(&id)?;
        let answer = answer.trim();
        if answer.is_empty() {
            return Err(CarrelError::invalid("give an answer"));
        }
        match question.kind {
            QuestionKind::Term => {
                let identity = self.identity()?;
                let commit = actions::teach(&archive, &question, answer, identity.holder())?;
                let queued = actions::requeue_waiting(&archive, &question, identity.device)?;
                Ok(Answered {
                    commit,
                    requeued: queued.iter().map(|e| e.id.to_string()).collect(),
                    moved: Vec::new(),
                })
            }
            QuestionKind::Collection => {
                let moved = actions::place(&archive, &question, answer)?;
                Ok(Answered {
                    commit: None,
                    requeued: Vec::new(),
                    moved: moved
                        .into_iter()
                        .map(|m| MovedProposal {
                            proposal: m.entry.id.to_string(),
                            to: m.to,
                            dropped: m.dropped,
                        })
                        .collect(),
                })
            }
        }
    }

    /// Dismisses the question `id`: its proposals stop waiting on it.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if it isn't found or the queues can't be written.
    pub fn dismiss_question(&self, id: String) -> Result<(), CarrelError> {
        let archive = self.fresh()?;
        let question = QuestionQueue::of(&archive).find(&id)?;
        actions::dismiss(&archive, &question)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Sample;

    #[test]
    fn the_sample_review_queue_is_listed_by_batch() {
        let sample = Sample::new();
        let review = sample.handle.review().unwrap();
        let proposals: Vec<&Proposal> = review.batches.iter().flat_map(|b| &b.proposals).collect();
        assert_ne!(proposals.len(), 0);
        for batch in &review.batches {
            assert!(batch.proposals.iter().all(|p| p.batch == batch.id));
        }
        let first = proposals[0];
        assert_eq!(
            sample.handle.proposal(first.short_id.clone()).unwrap().id,
            first.id
        );
    }

    #[test]
    fn rejecting_twice_says_it_is_already_decided() {
        let sample = Sample::new();
        let id = sample.first_proposal();
        assert_eq!(sample.handle.reject(vec![id.clone()]).unwrap(), 1);
        assert!(matches!(
            sample.handle.reject(vec![id]),
            Err(CarrelError::Invalid { message }) if message.contains("already rejected")
        ));
    }

    #[test]
    fn moving_to_a_collection_that_does_not_exist_is_invalid() {
        let sample = Sample::new();
        let id = sample.first_proposal();
        assert!(matches!(
            sample.handle.move_proposal(id, "Nowhere".into()),
            Err(CarrelError::Invalid { .. })
        ));
    }
}
