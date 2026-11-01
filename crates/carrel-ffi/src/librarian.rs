//! The librarian (`docs/design/librarian.md`): a conversation over the archive on a local text
//! model, run by the core ([`carrel_core::librarian`]), as `carrel ask` runs it. The app keeps
//! a [`LibrarianConversation`] while its panel is open, asks with
//! [`ArchiveHandle::ask_librarian`], and saves it to the Journal only when the owner says so.
//!
//! Also here: the librarian's own changes, when `librarian.changes` lets it make them, listed
//! and undone together as `carrel log --librarian` and `carrel undo --librarian` do.

use std::sync::{Arc, Mutex};

use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::history::History;
use carrel_core::librarian::changes::{self, Change as CoreChange, Outcome};
use carrel_core::librarian::model::NoModel;
use carrel_core::librarian::{
    self as core, Conversation, ItemLink, Librarian, LibrarianError, Stopped, journal, model,
};
use carrel_core::providers::Registry;
use carrel_core::providers::device::{AvailableModel, text_models};
use carrel_core::questions::moments::Conversing;

use crate::correct::ModelProvider;
use crate::history::Change;
use crate::models::{CatalogModel, catalog_model};
use crate::records::Sensitivity;
use crate::review::Question;
use crate::{ArchiveHandle, CarrelError, ProgressListener};

/// An item an answer links to.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LibrarianItem {
    /// Its identifier.
    pub id: String,
    /// Its card's title.
    pub title: String,
    /// Its card, relative to the archive root.
    pub card: String,
    /// The collection it's in, by path.
    pub collection: String,
    /// How sensitive it is.
    pub sensitivity: Sensitivity,
}

impl From<&ItemLink> for LibrarianItem {
    fn from(link: &ItemLink) -> Self {
        Self {
            id: link.id.to_string(),
            title: link.title.clone(),
            card: link.card.clone(),
            collection: link.collection.clone(),
            sensitivity: link.sensitivity.into(),
        }
    }
}

/// A passage the librarian read and used, word for word.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LibrarianPassage {
    /// The item it's from.
    pub item: LibrarianItem,
    /// The passage.
    pub text: String,
}

/// A tool the librarian used on the way to an answer.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LibrarianStep {
    /// The tool, such as `search` or `read`.
    pub tool: String,
    /// What it was given.
    pub input: String,
    /// What it did, in a few words.
    pub did: String,
}

/// A change the librarian suggested.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SuggestedChange {
    /// Move `item` to the collection `to`.
    Refile {
        /// The item.
        item: LibrarianItem,
        /// The collection, by path.
        to: String,
    },
    /// Set `field` (`title`, `summary`, `created` or `tags`) of `item`'s card to `value`.
    Correct {
        /// The item.
        item: LibrarianItem,
        /// The field.
        field: String,
        /// The new value; for tags, `+add -remove`.
        value: String,
    },
    /// Rename `item` and its card to `name`.
    Rename {
        /// The item.
        item: LibrarianItem,
        /// The new name.
        name: String,
    },
    /// Create the collection `path`.
    Collection {
        /// Its path.
        path: String,
        /// What belongs in it.
        description: String,
    },
}

impl From<&CoreChange> for SuggestedChange {
    fn from(change: &CoreChange) -> Self {
        match change {
            CoreChange::Refile { item, to } => Self::Refile {
                item: item.into(),
                to: to.clone(),
            },
            CoreChange::Correct { item, field, value } => Self::Correct {
                item: item.into(),
                field: field.clone(),
                value: value.clone(),
            },
            CoreChange::Rename { item, name } => Self::Rename {
                item: item.into(),
                name: name.clone(),
            },
            CoreChange::Collection { path, description } => Self::Collection {
                path: path.clone(),
                description: description.clone(),
            },
        }
    }
}

/// What became of a suggestion.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SuggestionOutcome {
    /// The librarian made it itself, as the history change `commit`, which undo reverses.
    Made {
        /// The history change.
        commit: Option<String>,
    },
    /// It's for the owner to confirm.
    Proposed {
        /// Why the librarian didn't make it itself.
        reason: String,
    },
    /// It can't be made: it's against the librarian's rules or doesn't make sense.
    Refused {
        /// Why.
        reason: String,
    },
    /// Making it failed; nothing changed.
    Failed {
        /// Why.
        reason: String,
    },
}

impl From<&Outcome> for SuggestionOutcome {
    fn from(outcome: &Outcome) -> Self {
        match outcome {
            Outcome::Made { commit } => Self::Made {
                commit: commit.clone(),
            },
            Outcome::Proposed { reason } => Self::Proposed {
                reason: reason.clone(),
            },
            Outcome::Refused { reason } => Self::Refused {
                reason: reason.clone(),
            },
            Outcome::Failed { reason } => Self::Failed {
                reason: reason.clone(),
            },
        }
    }
}

/// A change the librarian suggested, why, how sure it was, and what became of it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LibrarianSuggestion {
    /// The change.
    pub change: SuggestedChange,
    /// The change, in words, such as “refile “Lease” to Home/Flat”.
    pub description: String,
    /// Why, in the librarian's words.
    pub why: String,
    /// How sure it was, from 0 to 1.
    pub confidence: f64,
    /// What became of it.
    pub outcome: SuggestionOutcome,
}

/// The librarian's answer to one question.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LibrarianAnswer {
    /// The question.
    pub question: String,
    /// The answer, then anything it suggests looking at.
    pub text: String,
    /// The items it's about.
    pub items: Vec<LibrarianItem>,
    /// The passages it read and used.
    pub passages: Vec<LibrarianPassage>,
    /// The changes it suggested.
    pub suggestions: Vec<LibrarianSuggestion>,
    /// The tools it used, in order.
    pub steps: Vec<LibrarianStep>,
    /// The model that answered, for people.
    pub model: String,
    /// The tokens its prompts and answers took.
    pub tokens: u64,
    /// Why it stopped before it could answer, when it did: `steps` or `tokens`.
    pub stopped: Option<String>,
    /// Now and then, Carrel's question about an item the answer found, put in the queue to
    /// answer: at most one an answer, never after two in a row, and never twice about one
    /// item in a conversation.
    pub asks: Option<Question>,
}

impl From<&core::Answer> for LibrarianAnswer {
    fn from(answer: &core::Answer) -> Self {
        Self {
            question: answer.question.clone(),
            text: answer.text.clone(),
            items: answer.items.iter().map(Into::into).collect(),
            passages: answer
                .passages
                .iter()
                .map(|q| LibrarianPassage {
                    item: (&q.item).into(),
                    text: q.text.clone(),
                })
                .collect(),
            suggestions: answer
                .suggestions
                .iter()
                .map(|s| LibrarianSuggestion {
                    change: (&s.change).into(),
                    description: s.change.describe(),
                    why: s.why.clone(),
                    confidence: s.confidence,
                    outcome: (&s.outcome).into(),
                })
                .collect(),
            steps: answer
                .steps
                .iter()
                .map(|s| LibrarianStep {
                    tool: s.tool.clone(),
                    input: s.input.clone(),
                    did: s.did.clone(),
                })
                .collect(),
            model: answer.model.clone(),
            tokens: answer.tokens,
            stopped: answer.stopped.map(|s| {
                match s {
                    Stopped::Steps => "steps",
                    Stopped::Tokens => "tokens",
                }
                .to_owned()
            }),
            asks: None,
        }
    }
}

/// A conversation with the librarian, kept in memory only, until it's saved to the Journal or
/// dropped.
#[derive(Debug, Default, uniffi::Object)]
pub struct LibrarianConversation {
    conversation: Mutex<Conversation>,
    /// Which items Carrel has asked about in it, and whether it asked after the last answer.
    conversing: Mutex<Conversing>,
}

#[uniffi::export]
impl LibrarianConversation {
    /// An empty conversation.
    #[uniffi::constructor]
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// The answers so far, oldest first.
    #[must_use]
    pub fn answers(&self) -> Vec<LibrarianAnswer> {
        self.conversation
            .lock()
            .map(|c| c.answers.iter().map(Into::into).collect())
            .unwrap_or_default()
    }
}

/// A conversation saved to the Journal.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SavedConversation {
    /// The Journal file it went into, relative to the archive root.
    pub path: String,
    /// The history change.
    pub commit: Option<String>,
}

/// What undoing the librarian's changes did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LibrarianUndone {
    /// The changes undone, by summary.
    pub undone: Vec<String>,
    /// Those that couldn't be undone, each with why.
    pub failed: Vec<String>,
}

impl From<LibrarianError> for CarrelError {
    fn from(error: LibrarianError) -> Self {
        match error {
            LibrarianError::NoModel(why) => Self::Invalid { message: why },
            error => Self::failed(error),
        }
    }
}

impl From<journal::SaveError> for CarrelError {
    fn from(error: journal::SaveError) -> Self {
        match error {
            journal::SaveError::Empty => Self::invalid(error),
            error => Self::failed(error),
        }
    }
}

/// Which model the librarian answers with, or, when it has none, why and what would let it:
/// the setting to clear, or a model to download.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LibrarianModelChoice {
    /// The model it answers with, for people; `None` when there's none.
    pub label: Option<String>,
    /// When there's none, why and what to do, in words.
    pub problem: Option<String>,
    /// What `librarian.model` names, when it's a model that isn't available.
    pub gone: Option<String>,
    /// The text models this device has, `provider/offer`, best first.
    pub available: Vec<String>,
    /// The one it runs on once `librarian.model` is cleared, `provider/offer`.
    pub instead: Option<String>,
    /// When there's no text model at all, the catalog's to download for this device. Download
    /// it with [`ArchiveHandle::download_model`] once the owner has seen its size.
    pub download: Option<CatalogModel>,
}

impl ArchiveHandle {
    /// The providers for the librarian, and the model it runs on: the app's model when it
    /// gives one; else this device's `librarian.model`, or the strongest one that fits in
    /// `memory_gb`. Without one, why and what would let it.
    fn explain_librarian_model(
        &self,
        model: Option<Arc<dyn ModelProvider>>,
        memory_gb: Option<u64>,
    ) -> Result<(Registry, Result<AvailableModel, Box<NoModel>>), CarrelError> {
        let from_app = model.is_some();
        let registry = self.correction_registry(model);
        let setting = if from_app {
            None
        } else {
            self.prefs()?.librarian_model
        };
        let dir = self.models_dir();
        let chosen =
            model::choose_or_explain(&registry, setting.as_deref(), memory_gb, dir.as_deref());
        Ok((registry, chosen))
    }

    /// As [`ArchiveHandle::explain_librarian_model`], with no model an error saying why.
    fn librarian_model(
        &self,
        model: Option<Arc<dyn ModelProvider>>,
        memory_gb: Option<u64>,
    ) -> Result<(Registry, AvailableModel), CarrelError> {
        let (registry, chosen) = self.explain_librarian_model(model, memory_gb)?;
        let chosen = chosen.map_err(|missing| CarrelError::invalid(missing.to_string()))?;
        Ok((registry, chosen))
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// The model the librarian would answer with, for people, as
    /// [`ArchiveHandle::ask_librarian`] chooses it.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if there's no text model, or `librarian.model` names one that
    /// isn't available.
    pub fn librarian_model_label(
        &self,
        model: Option<Arc<dyn ModelProvider>>,
        memory_gb: Option<u64>,
    ) -> Result<String, CarrelError> {
        Ok(self.librarian_model(model, memory_gb)?.1.label)
    }

    /// Which model the librarian would answer with, as [`ArchiveHandle::ask_librarian`]
    /// chooses it, or why there's none and what would let it: the models this device has
    /// once a `librarian.model` that's gone is cleared, or the catalog model to download for
    /// `memory_gb`.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the preferences can't be read.
    pub fn librarian_model_choice(
        &self,
        model: Option<Arc<dyn ModelProvider>>,
        memory_gb: Option<u64>,
    ) -> Result<LibrarianModelChoice, CarrelError> {
        let (registry, chosen) = self.explain_librarian_model(model, memory_gb)?;
        let available = text_models(&registry)
            .iter()
            .map(AvailableModel::name)
            .collect();
        Ok(match chosen {
            Ok(chosen) => LibrarianModelChoice {
                label: Some(chosen.label),
                problem: None,
                gone: None,
                available,
                instead: None,
                download: None,
            },
            Err(missing) => LibrarianModelChoice {
                label: None,
                problem: Some(missing.to_string()),
                gone: missing.gone,
                available,
                instead: missing.instead.as_ref().map(AvailableModel::name),
                download: missing
                    .download
                    .map(|m| catalog_model(m, self.models_dir().as_deref())),
            },
        })
    }

    /// Whether the archive lets the librarian make the changes it's sure of on its own
    /// (`librarian.changes`).
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive can't be read.
    pub fn librarian_makes_changes(&self) -> Result<bool, CarrelError> {
        Ok(self.fresh()?.settings().librarian.changes)
    }

    /// Asks the librarian `question` in `conversation`, adding its answer there, on `model`
    /// or, when that's `None`, this device's ([`ArchiveHandle::librarian_model_label`]).
    /// `memory_gb` is this device's memory, which the strongest model has to fit in.
    /// `progress` is told as each step starts: the step, of the most it may take, and what
    /// the last one did. With `librarian.changes` on, the librarian makes the changes it's
    /// sure of, as this device's owner; the rest are left in the answer to confirm.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if there's no model, or [`CarrelError::Failed`] if the
    /// archive can't be read or the model fails.
    pub fn ask_librarian(
        &self,
        conversation: Arc<LibrarianConversation>,
        question: String,
        model: Option<Arc<dyn ModelProvider>>,
        memory_gb: Option<u64>,
        progress: Option<Arc<dyn ProgressListener>>,
    ) -> Result<LibrarianAnswer, CarrelError> {
        if question.trim().is_empty() {
            return Err(CarrelError::invalid("ask the librarian something"));
        }
        let archive = self.fresh()?;
        let (registry, chosen) = self.librarian_model(model, memory_gb)?;
        let providers = || self.registry();
        let report = |p: core::Progress<'_>| {
            if let Some(listener) = &progress {
                listener.update(p.step as u64, p.steps as u64, p.did.map(str::to_owned));
            }
        };
        let librarian = Librarian::new(&archive, &registry, chosen, &providers)?
            .with_changes_for(self.holder()?)
            .with_progress(&report);
        // The conversation is held while the model answers, so two questions in one
        // conversation are answered in turn.
        let conversation_state = &conversation;
        let mut conversation = conversation
            .conversation
            .lock()
            .map_err(|_| CarrelError::failed("the conversation was poisoned"))?;
        let answer = librarian.ask(&mut conversation, &question)?;
        let mut answered: LibrarianAnswer = answer.into();
        let items: Vec<carrel_core::card::Id> = answer.items.iter().map(|i| i.id).collect();
        if let Ok(mut conversing) = conversation_state.conversing.lock() {
            answered.asks = conversing
                .after(&archive, &items, jiff::Zoned::now().date())
                .ok()
                .flatten()
                .and_then(|q| carrel_core::questions::moments::ask(&archive, &q).ok())
                .as_ref()
                .map(Into::into);
        }
        Ok(answered)
    }

    /// Saves `conversation` to this month's Journal, as one history change.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if it has no answers, [`CarrelError::Busy`] if another
    /// writer holds the archive, or [`CarrelError::Failed`].
    pub fn save_conversation(
        &self,
        conversation: Arc<LibrarianConversation>,
    ) -> Result<SavedConversation, CarrelError> {
        let conversation = conversation
            .conversation
            .lock()
            .map_err(|_| CarrelError::failed("the conversation was poisoned"))?
            .clone();
        let archive = self.fresh()?;
        let lease = DiskLock::for_archive(&archive).acquire(
            self.holder()?,
            changes::AGENT,
            DEFAULT_LEASE,
        )?;
        let saved = journal::save(&archive, &conversation, &jiff::Zoned::now(), &lease);
        lease.release()?;
        let saved = saved?;
        Ok(SavedConversation {
            path: saved.path,
            commit: saved.commit,
        })
    }

    /// The changes the librarian made on its own at or after `since` (seconds since the Unix
    /// epoch) that haven't been undone, newest first.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the history can't be read.
    pub fn librarian_changes(&self, since: i64) -> Result<Vec<Change>, CarrelError> {
        let archive = self.fresh()?;
        let log = History::open(&archive)?.log()?;
        Ok(changes::recent(&log, since)
            .iter()
            .map(Into::into)
            .collect())
    }

    /// Undoes every change the librarian made on its own at or after `since`, newest first,
    /// as `carrel undo --librarian` does. One that can't be undone is reported, and the rest
    /// are still undone.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Busy`] if another writer holds the archive, or [`CarrelError::Failed`]
    /// if the history can't be read.
    pub fn undo_librarian_changes(&self, since: i64) -> Result<LibrarianUndone, CarrelError> {
        let archive = self.fresh()?;
        let lease =
            DiskLock::for_archive(&archive).acquire(self.holder()?, "undo", DEFAULT_LEASE)?;
        let done = changes::undo_since(&archive, since, &lease);
        lease.release()?;
        let done = done?;
        Ok(LibrarianUndone {
            undone: done.undone.iter().map(|(e, _)| e.summary.clone()).collect(),
            failed: done
                .failed
                .iter()
                .map(|(e, why)| format!("{}: {why}", e.summary))
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::correct::{ModelAnswer, ModelRequest};
    use crate::tests::Sample;
    use carrel_core::local::Prefs;

    /// A model the app gives, as Apple's would be.
    struct Answers;

    impl ModelProvider for Answers {
        fn complete(&self, _: ModelRequest) -> Result<ModelAnswer, CarrelError> {
            Ok(ModelAnswer {
                text: "{}".into(),
                model: "swift/answers".into(),
            })
        }
    }

    #[test]
    fn the_apps_model_is_the_librarians() {
        let sample = Sample::new();
        let choice = sample
            .handle
            .librarian_model_choice(Some(Arc::new(Answers)), Some(16))
            .unwrap();
        assert!(choice.label.is_some(), "{choice:?}");
        assert_eq!(choice.problem, None);
        assert_eq!(choice.download, None);
    }

    #[test]
    fn with_no_model_it_names_one_to_download_with_its_size() {
        let sample = Sample::new();
        let choice = sample.handle.librarian_model_choice(None, Some(8)).unwrap();
        assert_eq!(choice.label, None);
        assert_eq!(choice.available, Vec::<String>::new());
        let download = choice.download.unwrap();
        assert_eq!(download.name, "qwen3.5-4b");
        assert!(download.size > 0);
        let problem = choice.problem.unwrap();
        assert!(problem.contains("needs a text model"), "{problem}");
        // Asking says the same.
        let asked = sample.handle.ask_librarian(
            LibrarianConversation::new(),
            "Anything?".into(),
            None,
            Some(8),
            None,
        );
        assert!(matches!(asked, Err(CarrelError::Invalid { message }) if message == problem));
    }

    #[test]
    fn a_setting_naming_a_model_thats_gone_is_said() {
        let sample = Sample::new();
        let config = sample.dir.path().join("config");
        let mut prefs = Prefs::load_from(&config).unwrap();
        prefs.librarian_model = Some("llama/gone-7b".into());
        prefs.save_to(&config).unwrap();
        let choice = sample
            .handle
            .librarian_model_choice(None, Some(16))
            .unwrap();
        assert_eq!(choice.gone.as_deref(), Some("llama/gone-7b"));
        assert_eq!(choice.instead, None, "nothing else here");
        assert_eq!(
            choice.download.map(|m| m.name).as_deref(),
            Some("gemma-4-e4b")
        );
        let problem = choice.problem.unwrap();
        assert!(
            problem.contains("librarian.model names llama/gone-7b"),
            "{problem}"
        );
    }
}
