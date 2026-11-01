//! Corrections (`docs/design/corrections.md`): the owner types what's wrong with a proposal
//! or a filed item, a model works out the changes ([`carrel_core::correct::prepare`]), the app
//! shows them, and they're saved only once the owner agrees.
//!
//! The model is the device's, through its provider programs, or one the app supplies as a
//! [`ModelProvider`] written in Swift, which the core calls like any other provider.

use std::sync::{Arc, Mutex};

use carrel_core::actions;
use carrel_core::correct::prepare::{self, Prepared};
use carrel_core::providers::{
    Capability, Completion, CompletionRequest, Offer, Provider, ProviderError, Registry, priority,
};

use crate::records::CardFields;
use crate::{ArchiveHandle, CarrelError};

/// A prompt for a text model, as the core asks it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ModelRequest {
    /// Instructions that frame the prompt.
    pub system: Option<String>,
    /// The prompt.
    pub prompt: String,
    /// A JSON Schema the answer must follow, as JSON text.
    pub schema: Option<String>,
    /// The longest answer wanted, in tokens.
    pub max_tokens: Option<u32>,
}

/// A text model's answer.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ModelAnswer {
    /// The answer. When a schema was given, the JSON that follows it.
    pub text: String,
    /// The exact model and version, for provenance, such as `apple/afm-3`.
    pub model: String,
}

/// A text model the app supplies, implemented in Swift, as a provider program would answer
/// `complete` (`docs/providers.md`).
#[uniffi::export(with_foreign)]
pub trait ModelProvider: Send + Sync {
    /// Answers `request`.
    ///
    /// # Errors
    ///
    /// Whatever the model reports, as [`CarrelError::Callback`].
    fn complete(&self, request: ModelRequest) -> Result<ModelAnswer, CarrelError>;
}

/// A [`ModelProvider`] as the core's [`Provider`]: one text model, called `app/model`.
struct FromSwift {
    model: Arc<dyn ModelProvider>,
    offers: Vec<Offer>,
}

impl FromSwift {
    fn new(model: Arc<dyn ModelProvider>) -> Self {
        Self {
            model,
            offers: vec![Offer {
                capability: Capability::TextModel,
                id: "model".into(),
                label: Some("the app's model".into()),
                priority: priority::NATIVE,
                available: true,
                reason: None,
                media_types: Vec::new(),
                extra: serde_json::Map::new(),
            }],
        }
    }
}

impl Provider for FromSwift {
    fn name(&self) -> &'static str {
        "app"
    }

    fn offers(&self) -> &[Offer] {
        &self.offers
    }

    fn complete(
        &self,
        _offer: &str,
        request: &CompletionRequest,
    ) -> Result<Completion, ProviderError> {
        let answer = self
            .model
            .complete(ModelRequest {
                system: request.system.clone(),
                prompt: request.prompt.clone(),
                schema: request.schema.as_ref().map(ToString::to_string),
                max_tokens: request.max_tokens,
            })
            .map_err(|e| ProviderError::Failed(e.to_string()))?;
        let json = match &request.schema {
            Some(_) => Some(serde_json::from_str(&answer.text).map_err(|e| {
                ProviderError::Failed(format!("the answer isn't the JSON asked for: {e}"))
            })?),
            None => None,
        };
        Ok(Completion {
            text: answer.text,
            json,
            model: answer.model,
            usage: None,
        })
    }
}

/// A lasting fact the owner stated, offered for the glossary once a correction is saved.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GlossaryFact {
    /// The word or name it's about.
    pub about: String,
    /// What's true, as one sentence.
    pub fact: String,
}

/// What a correction would change, for the app to show before the owner agrees.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CorrectionChanges {
    /// The card's title, for a heading.
    pub title: String,
    /// The card's fields before.
    pub before: CardFields,
    /// Its fields after.
    pub after: CardFields,
    /// The fields changed, by name.
    pub changed: Vec<String>,
    /// The collection it moves from, when it moves.
    pub moved_from: Option<String>,
    /// The collection it moves to, when it moves.
    pub moved_to: Option<String>,
    /// Whether a lower sensitivity was kept at sensitive, because a rule finds a restricted
    /// value in the item.
    pub kept_sensitive: bool,
    /// Tags left off because they don't apply where it moves.
    pub dropped: Vec<String>,
    /// A fact to offer for the glossary once it's saved.
    pub fact: Option<GlossaryFact>,
    /// The model that worked it out, as `provider/offer`.
    pub model: Option<String>,
    /// Whether it changes nothing anyone would see; then there's nothing to save.
    pub is_empty: bool,
}

/// A correction worked out and held until the owner agrees: save it with
/// [`ArchiveHandle::save_correction`], or drop it to change nothing.
#[derive(Debug, uniffi::Object)]
pub struct Correction {
    changes: CorrectionChanges,
    prepared: Mutex<Option<Prepared>>,
    /// The item question it answers, which leaves the queue once it's saved.
    answering: Option<carrel_core::card::Id>,
}

impl Correction {
    /// `prepared`, answering the item question `question`.
    pub(crate) fn answering(prepared: Prepared, question: carrel_core::card::Id) -> Arc<Self> {
        Arc::new(Self::made(prepared, Some(question)))
    }

    fn of(prepared: Prepared) -> Arc<Self> {
        Arc::new(Self::made(prepared, None))
    }

    fn made(prepared: Prepared, answering: Option<carrel_core::card::Id>) -> Self {
        let changes = CorrectionChanges {
            title: prepared.title.clone(),
            before: (&prepared.before).into(),
            after: (&prepared.after).into(),
            changed: prepared.changed.iter().map(|&c| c.to_owned()).collect(),
            moved_from: prepared.moved.as_ref().map(|(from, _)| from.clone()),
            moved_to: prepared.moved.as_ref().map(|(_, to)| to.clone()),
            kept_sensitive: prepared.kept_sensitive,
            dropped: prepared.dropped.clone(),
            fact: prepared.fact().map(|f| GlossaryFact {
                about: f.about.clone(),
                fact: f.fact.clone(),
            }),
            model: prepared.worked.model.clone(),
            is_empty: prepared.is_empty(),
        };
        Self {
            changes,
            prepared: Mutex::new(Some(prepared)),
            answering,
        }
    }
}

#[uniffi::export]
impl Correction {
    /// What it would change.
    #[must_use]
    pub fn changes(&self) -> CorrectionChanges {
        self.changes.clone()
    }
}

/// What saving a correction did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SavedCorrection {
    /// The card's title.
    pub title: String,
    /// Whether it was a filed item's, so it can be undone from History.
    pub filed: bool,
    /// The history commit, for a filed item.
    pub commit: Option<String>,
    /// The card's path after, for a filed item.
    pub card: Option<String>,
}

impl ArchiveHandle {
    /// The providers for a correction: the app's model when it gives one, otherwise the
    /// device's.
    pub(crate) fn correction_registry(&self, model: Option<Arc<dyn ModelProvider>>) -> Registry {
        match model {
            Some(model) => {
                let mut registry = Registry::with_built_ins();
                registry.add(Box::new(FromSwift::new(model)));
                registry
            }
            None => self.registry(),
        }
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// Works out what `words` change in the proposal `id`, with `model` or, when it's
    /// `None`, the device's models. Nothing is saved.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for a decided proposal, or [`CarrelError::Failed`] if it
    /// isn't found or the words can't be worked out.
    pub fn prepare_proposal_correction(
        &self,
        id: String,
        words: String,
        model: Option<Arc<dyn ModelProvider>>,
    ) -> Result<Arc<Correction>, CarrelError> {
        let archive = self.fresh()?;
        let registry = self.correction_registry(model);
        Ok(Correction::of(prepare::prepare_review(
            &archive, &registry, &id, &words,
        )?))
    }

    /// Works out what `words` change in the filed item `item`, as
    /// [`ArchiveHandle::prepare_proposal_correction`] does for a proposal.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if there's no such item, or [`CarrelError::Failed`].
    pub fn prepare_item_correction(
        &self,
        item: String,
        words: String,
        model: Option<Arc<dyn ModelProvider>>,
    ) -> Result<Arc<Correction>, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &item)?;
        let registry = self.correction_registry(model);
        Ok(Correction::of(prepare::prepare_filed(
            &archive, &registry, item, &words,
        )?))
    }

    /// Saves `correction`, as the owner agreed: a proposal back to the queue, or a filed
    /// item's card as one change, refiled when its collection changed. One answering an item
    /// question takes the question off the queue. A correction is saved once.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if it was saved already, [`CarrelError::Busy`] if another
    /// writer holds the archive, or [`CarrelError::Failed`].
    pub fn save_correction(
        &self,
        correction: Arc<Correction>,
    ) -> Result<SavedCorrection, CarrelError> {
        let prepared = correction
            .prepared
            .lock()
            .map_err(|_| CarrelError::failed("the correction was poisoned"))?
            .take()
            .ok_or_else(|| CarrelError::invalid("this correction was saved already"))?;
        let archive = self.fresh()?;
        let saved = prepare::save(&archive, prepared, || self.holder())?;
        if let Some(id) = correction.answering
            && let Ok(question) =
                carrel_core::questions::QuestionQueue::of(&archive).find(&id.to_string())
        {
            carrel_core::questions::answer::settled(&archive, &question)?;
        }
        Ok(SavedCorrection {
            title: saved.title,
            filed: saved.filed,
            commit: saved.commit,
            card: saved.card,
        })
    }

    /// Adds `fact` to the glossary, as one history change, when the owner agrees to the fact
    /// a correction offered. Returns the commit.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`].
    pub fn learn(&self, fact: GlossaryFact) -> Result<Option<String>, CarrelError> {
        let archive = self.fresh()?;
        Ok(actions::learn(
            &archive,
            &fact.about,
            &fact.fact,
            self.holder()?,
        )?)
    }
}
