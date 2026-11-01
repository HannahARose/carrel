//! Carrel's questions beyond Review's (`docs/design/librarian.md`, "Questions"): every kind in
//! the queue with its suggested answers, skipping and the skipped ones, a routing rule after a
//! sender's answer, an item question answered through a correction, and questions at natural
//! moments: about the item shown, and the day's few on Status. A wander step and the
//! librarian ask theirs in [`crate::discover`] and [`crate::librarian`].

use std::sync::Arc;

use carrel_core::card::Id;
use carrel_core::correct::prepare;
use carrel_core::inbox::{Answer, Request};
use carrel_core::questions::answer::{self as core_answer, Target};
use carrel_core::questions::moments::{self, Daily};
use carrel_core::questions::{QuestionKind, QuestionQueue, Suggestion};

use crate::correct::{Correction, ModelProvider};
use crate::review::Question;
use crate::{ArchiveHandle, CarrelError};

/// Which kind of question it is, and so how it's answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum QuestionAsks {
    /// What a recurring term means; the answer becomes a glossary fact.
    Term,
    /// Which collection items about a term go in: a collection's path.
    Collection,
    /// Where a group of items goes, together: a collection's path.
    Group,
    /// Whether a sender or project should have its own collection: yes, no or a collection,
    /// then whether to add a routing rule.
    Sender,
    /// Something about one item; the answer is a correction of its card.
    Item,
}

impl From<QuestionKind> for QuestionAsks {
    fn from(kind: QuestionKind) -> Self {
        match kind {
            QuestionKind::Term => Self::Term,
            QuestionKind::Collection => Self::Collection,
            QuestionKind::Group => Self::Group,
            QuestionKind::Sender => Self::Sender,
            QuestionKind::Item => Self::Item,
        }
    }
}

/// A likely answer to pick.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct QuestionSuggestion {
    /// The answer, as the owner would give it.
    pub answer: String,
    /// Why it's likely, for people.
    pub why: Option<String>,
    /// For a collection answer, whether picking it creates the collection.
    pub new_collection: bool,
    /// Whether picking it settles the question without changing anything.
    pub no_change: bool,
}

impl From<&Suggestion> for QuestionSuggestion {
    fn from(suggestion: &Suggestion) -> Self {
        Self {
            answer: suggestion.answer.clone(),
            why: suggestion.why.clone(),
            new_collection: suggestion.new_collection,
            no_change: suggestion.no_change,
        }
    }
}

/// Today, on this device.
fn today() -> jiff::civil::Date {
    jiff::Zoned::now().date()
}

#[uniffi::export]
impl ArchiveHandle {
    /// The questions resting after a skip, the soonest back first. They come back by
    /// themselves after the archive's `questions.skip_days`, and can be answered before.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the queue can't be read.
    pub fn skipped_questions(&self) -> Result<Vec<Question>, CarrelError> {
        let archive = self.fresh()?;
        let ranked = core_answer::tidy(&archive)?;
        Ok(ranked
            .resting
            .iter()
            .map(|w| (&w.question).into())
            .collect())
    }

    /// Skips the question `id`: it rests out of the queue for the archive's
    /// `questions.skip_days`, and its proposals keep waiting on it. Returns it as saved.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if it isn't found or can't be saved.
    pub fn skip_question(&self, id: String) -> Result<Question, CarrelError> {
        let archive = self.fresh()?;
        let question = QuestionQueue::of(&archive).find(&id)?;
        self.through_inbox(&archive, || {
            Ok(Request::Answer {
                question: question.id,
                answer: Answer::Skip,
            })
        })?;
        Ok((&core_answer::skip(&archive, &question)?).into())
    }

    /// Adds a routing rule filing future items named for `subject` into the collection at
    /// `collection`, once the owner agreed after answering a sender's question. Returns the
    /// commit.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for a collection that doesn't exist, [`CarrelError::Busy`]
    /// if another writer holds the archive, or [`CarrelError::Failed`].
    pub fn add_question_rule(
        &self,
        subject: String,
        collection: String,
    ) -> Result<Option<String>, CarrelError> {
        let archive = self.fresh()?;
        let collections = archive.collections()?;
        let target = carrel_core::actions::collection(&collections, &collection)?;
        self.through_inbox(&archive, || {
            Ok(Request::AddRule {
                subject: subject.clone(),
                collection: target.path.clone(),
            })
        })?;
        Ok(core_answer::add_rule(
            &archive,
            &subject,
            target,
            self.holder()?,
        )?)
    }

    /// Works out the correction `answer` to the item question `id` makes to its item's
    /// card (a proposal's or a filed item's), with `model` or, when it's `None`, the device's
    /// models. Nothing is saved; once the owner agrees, saving it with
    /// [`ArchiveHandle::save_correction`] also takes the question off the queue.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for a question that isn't about an item, or
    /// [`CarrelError::Failed`] if it isn't found or the answer can't be worked out.
    pub fn prepare_question_correction(
        &self,
        id: String,
        answer: String,
        model: Option<Arc<dyn ModelProvider>>,
    ) -> Result<Arc<Correction>, CarrelError> {
        let archive = self.fresh()?;
        let question = QuestionQueue::of(&archive).find(&id)?;
        let words = core_answer::item_words(&question, &answer);
        let registry = self.correction_registry(model);
        let prepared = match core_answer::target(&question) {
            Some(Target::Proposal(proposal)) => {
                prepare::prepare_review(&archive, &registry, &proposal.to_string(), &words)?
            }
            Some(Target::Filed(item)) => {
                let item = Self::item(&archive, &item.to_string())?;
                prepare::prepare_filed(&archive, &registry, item, &words)?
            }
            None => return Err(CarrelError::invalid("the question isn't about an item")),
        };
        Ok(Correction::answering(prepared, question.id))
    }

    /// Takes the question `id` off the queue as answered, for an item question whose answer
    /// changes nothing on the card.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if it isn't found or the queue can't be written.
    pub fn settle_question(&self, id: String) -> Result<(), CarrelError> {
        let archive = self.fresh()?;
        let question = QuestionQueue::of(&archive).find(&id)?;
        self.through_inbox(&archive, || {
            Ok(Request::Answer {
                question: question.id,
                answer: Answer::Settle,
            })
        })?;
        core_answer::settled(&archive, &question)?;
        Ok(())
    }

    /// The open questions about the filed item `item`, to show beside it: those already in
    /// the queue first, then those Carrel would ask, which aren't added to the queue until
    /// the owner answers one ([`ArchiveHandle::ask_about_item`]).
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if there's no such item, or [`CarrelError::Failed`] if its
    /// card or the queue can't be read.
    pub fn questions_about(&self, item: String) -> Result<Vec<Question>, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &item)?;
        Ok(moments::about_item(&archive, &item, today())?
            .iter()
            .map(Into::into)
            .collect())
    }

    /// Puts the question `question` (its words, as [`ArchiveHandle::questions_about`] gave
    /// them) about the filed item `item` in the queue, so it can be answered, and returns it
    /// as the queue keeps it.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if there's no such item or the question isn't open about it
    /// any more, or [`CarrelError::Failed`].
    pub fn ask_about_item(&self, item: String, question: String) -> Result<Question, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &item)?;
        let open = moments::about_item(&archive, &item, today())?;
        let wanted = open
            .iter()
            .find(|q| q.question == question)
            .ok_or_else(|| CarrelError::invalid("that question isn't open any more"))?;
        Ok((&moments::ask(&archive, wanted)?).into())
    }

    /// The day's few new questions for Status, the same all day: chosen the first time
    /// they're asked for each day, added to the queue, and kept on this device in Carrel's
    /// folder. Those answered or skipped since drop out.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive or the queue can't be read.
    pub fn questions_today(&self) -> Result<Vec<Question>, CarrelError> {
        let archive = self.fresh()?;
        let path = Daily::path_in(&self.config_dir()?, &archive.info().id.to_string());
        let mut daily = Daily::load(&path);
        let (questions, changed) = daily.today(&archive, today())?;
        if changed {
            // The record is a convenience: a device that can't save it still asks.
            let _ = daily.save(&path);
        }
        Ok(questions.iter().map(Into::into).collect())
    }
}

/// The question to ask about `item` on a wander step or after the librarian's answer, put in
/// the queue; none when there's none open, or it can't be read.
pub(crate) fn ask_about(archive: &carrel_core::archive::Archive, item: Id) -> Option<Question> {
    let item = archive.items().ok()?.into_iter().find(|i| i.id == item)?;
    moments::ask_about(archive, &item, today())
        .ok()
        .flatten()
        .as_ref()
        .map(Into::into)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::correct::{ModelAnswer, ModelRequest};
    use crate::tests::Sample;

    /// The sample item Carrel has a question about: a project with no description.
    const ASKED_ABOUT: &str = "Projects/Hardware/notes.card.md";

    /// Puts a question of `kind` about `subject` in the sample's queue, with `suggestions`.
    fn queue(sample: &Sample, id: &str, kind: &str, subject: &str, suggestions: serde_json::Value) {
        let dir = sample.root().join("_System/queue/questions");
        std::fs::create_dir_all(&dir).unwrap();
        let question = json!({
            "carrel": "question", "format": "1.0", "id": id,
            "created": "2026-10-29T16:21:00-07:00", "kind": kind, "subject": subject,
            "question": format!("What about {subject}?"),
            "items": [sample.first_proposal()], "choices": ["Projects/Northbridge"],
            "suggestions": suggestions,
        });
        std::fs::write(dir.join(format!("{id}.json")), question.to_string()).unwrap();
    }

    const SENDER: &str = "01a18701-0500-776a-9867-08f0000000d1";

    #[test]
    fn every_kind_is_listed_with_its_suggestions() {
        let sample = Sample::new();
        queue(
            &sample,
            SENDER,
            "sender",
            "Northbridge Bank",
            json!([
                {"answer": "Projects/Northbridge", "why": "12 statements", "new_collection": true},
                {"answer": "no", "no_change": true}
            ]),
        );
        let questions = sample.handle.questions().unwrap();
        let sender = questions.iter().find(|q| q.id == SENDER).unwrap();
        assert_eq!(sender.kind, QuestionAsks::Sender);
        assert_eq!(sender.suggestions.len(), 2);
        assert!(sender.suggestions[0].new_collection);
        assert_eq!(sender.suggestions[0].why.as_deref(), Some("12 statements"));
        assert!(sender.suggestions[1].no_change);
    }

    #[test]
    fn a_sender_answered_yes_gathers_its_items_and_offers_a_rule() {
        let sample = Sample::new();
        queue(
            &sample,
            SENDER,
            "sender",
            "Northbridge Bank",
            json!([{"answer": "Projects/Northbridge", "new_collection": true}]),
        );
        let answered = sample
            .handle
            .answer_question(SENDER.into(), "yes".into())
            .unwrap();
        assert_eq!(answered.collection.as_deref(), Some("Projects/Northbridge"));
        assert!(answered.created);
        assert_eq!(answered.moved.len(), 1);
        assert_eq!(answered.rule_for.as_deref(), Some("Northbridge Bank"));
        let commit = sample
            .handle
            .add_question_rule("Northbridge Bank".into(), "Projects/Northbridge".into())
            .unwrap();
        assert!(commit.is_some(), "the rule is a change in history");
    }

    #[test]
    fn a_suggestion_that_changes_nothing_settles_it() {
        let sample = Sample::new();
        queue(
            &sample,
            SENDER,
            "sender",
            "Northbridge Bank",
            json!([{"answer": "no, it doesn't need one", "no_change": true}]),
        );
        let answered = sample
            .handle
            .answer_question(SENDER.into(), "1".into())
            .unwrap();
        assert!(answered.declined);
        assert!(
            sample
                .handle
                .questions()
                .unwrap()
                .iter()
                .all(|q| q.id != SENDER)
        );
    }

    #[test]
    fn a_skipped_question_rests_with_the_skipped() {
        let sample = Sample::new();
        queue(&sample, SENDER, "sender", "Northbridge Bank", json!([]));
        let skipped = sample.handle.skip_question(SENDER.into()).unwrap();
        assert_eq!(skipped.skips, 1);
        assert!(skipped.skipped.is_some());
        assert!(
            sample
                .handle
                .questions()
                .unwrap()
                .iter()
                .all(|q| q.id != SENDER)
        );
        let resting = sample.handle.skipped_questions().unwrap();
        assert_eq!(
            resting.iter().map(|q| q.id.as_str()).collect::<Vec<_>>(),
            [SENDER]
        );
    }

    /// A model that answers every correction with a description, as a Swift model would.
    struct Describes;

    impl ModelProvider for Describes {
        fn complete(&self, _: ModelRequest) -> Result<ModelAnswer, CarrelError> {
            Ok(ModelAnswer {
                text: r#"{"summary": "Notes from building the lamp."}"#.into(),
                model: "swift/describes".into(),
            })
        }
    }

    fn asked_about(sample: &Sample) -> String {
        sample
            .handle
            .items()
            .unwrap()
            .into_iter()
            .find(|i| i.card == ASKED_ABOUT)
            .unwrap()
            .id
    }

    #[test]
    fn an_item_question_is_answered_through_a_correction_saved_once() {
        let sample = Sample::new();
        let item = asked_about(&sample);
        let open = sample.handle.questions_about(item.clone()).unwrap();
        let first = open.first().expect("the sample's project has a question");
        assert_eq!(first.kind, QuestionAsks::Item);
        let asked = sample
            .handle
            .ask_about_item(item.clone(), first.question.clone())
            .unwrap();
        assert!(
            sample
                .handle
                .questions()
                .unwrap()
                .iter()
                .any(|q| q.id == asked.id)
        );
        assert!(
            sample
                .handle
                .answer_question(asked.id.clone(), "a lamp".into())
                .is_err(),
            "an item question is answered through a correction"
        );
        let correction = sample
            .handle
            .prepare_question_correction(
                asked.id.clone(),
                "the lamp build".into(),
                Some(Arc::new(Describes)),
            )
            .unwrap();
        assert!(!correction.changes().is_empty);
        assert!(
            sample
                .handle
                .questions()
                .unwrap()
                .iter()
                .any(|q| q.id == asked.id),
            "nothing changes until it's saved"
        );
        let saved = sample.handle.save_correction(correction).unwrap();
        assert!(saved.filed);
        assert!(
            sample
                .handle
                .questions()
                .unwrap()
                .iter()
                .all(|q| q.id != asked.id)
        );
    }

    #[test]
    fn the_days_questions_are_the_same_all_day() {
        let sample = Sample::new();
        let today = sample.handle.questions_today().unwrap();
        assert!(!today.is_empty(), "the sample has an item to ask about");
        assert!(today.len() <= moments::DAILY);
        let again = sample.handle.questions_today().unwrap();
        assert_eq!(
            today.iter().map(|q| &q.id).collect::<Vec<_>>(),
            again.iter().map(|q| &q.id).collect::<Vec<_>>()
        );
        assert!(
            sample.dir.path().join("config/questions").is_dir(),
            "kept on the device"
        );
    }
}
