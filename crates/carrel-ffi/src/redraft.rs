//! Drafting waiting proposals again with the models this device has now (#445), as
//! `carrel review redraft` and the interactive mode's `d` do, through
//! [`carrel_core::ingest::redraft`].

use std::sync::Arc;

use carrel_core::ingest::redraft::{RedraftError, failed_drafts, no_drafting_model, redraft};
use carrel_core::providers::Registry;

use crate::correct::{FromSwift, ModelProvider};
use crate::review::Proposal;
use crate::{ArchiveHandle, CarrelError, ProgressListener};

/// A proposal drafted again.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RedraftedProposal {
    /// The proposal, as saved.
    pub proposal: Proposal,
    /// What the owner had set, which was kept, such as `title` or `collection`.
    pub kept: Vec<String>,
}

/// What drafting proposals again did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RedraftOutcome {
    /// The proposals drafted again.
    pub redrafted: Vec<RedraftedProposal>,
    /// Those that weren't, each with why.
    pub failures: Vec<String>,
}

impl ArchiveHandle {
    /// The providers for drafting: the device's, with the app's model when it gives one.
    fn drafting_registry(&self, model: Option<Arc<dyn ModelProvider>>) -> Registry {
        let mut registry = self.registry();
        if let Some(model) = model {
            registry.add(Box::new(FromSwift::new(model)));
        }
        registry
    }
}

/// Why drafting can't run here, and what would let it, for a device with `memory_gb`.
fn no_model_words(registry: &Registry, memory_gb: Option<u64>) -> String {
    let mut text = format!("{}.", RedraftError::NoTextModel);
    if let Some(missing) = no_drafting_model(
        registry,
        memory_gb,
        carrel_core::local::models_dir().as_deref(),
    ) {
        text.push(' ');
        text.push_str(&missing.explain_for("Drafting cards"));
        if missing.download.is_some() {
            text.push_str(" Download it in Settings, under Models.");
        }
    }
    text
}

#[uniffi::export]
impl ArchiveHandle {
    /// Drafts the waiting proposals `ids` again with the device's models, and `model` when
    /// the app gives one, keeping each one's identifier and what the owner set. `memory_gb`
    /// is this device's memory, to say which model to download when there's none.
    /// `progress` hears how many are done. It takes as long as the models do, so call it off
    /// the main thread.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] when there's no text model, saying what to download, or when
    /// none could be drafted again, saying why; [`CarrelError::Failed`] if the queue can't be
    /// read.
    pub fn redraft_proposals(
        &self,
        ids: Vec<String>,
        model: Option<Arc<dyn ModelProvider>>,
        memory_gb: Option<u64>,
        progress: Option<Arc<dyn ProgressListener>>,
    ) -> Result<RedraftOutcome, CarrelError> {
        let archive = self.fresh()?;
        let device = self.holder()?.device;
        let registry = self.drafting_registry(model);
        let total = ids.len() as u64;
        let mut outcome = RedraftOutcome {
            redrafted: Vec::new(),
            failures: Vec::new(),
        };
        for (done, id) in ids.iter().enumerate() {
            let done = done as u64;
            let result = redraft(&archive, &registry, id, device, &mut |p| {
                if let Some(progress) = &progress {
                    progress.update(done, total, p.item.map(str::to_owned));
                }
            });
            match result {
                Ok(redrafted) => outcome.redrafted.push(RedraftedProposal {
                    proposal: (&redrafted.entry).into(),
                    kept: redrafted.kept.iter().map(|k| (*k).to_owned()).collect(),
                }),
                Err(RedraftError::NoTextModel) => {
                    return Err(CarrelError::invalid(no_model_words(&registry, memory_gb)));
                }
                Err(RedraftError::Action(e)) => outcome.failures.push(e.to_string()),
            }
        }
        if let Some(progress) = &progress {
            progress.update(total, total, None);
        }
        if outcome.redrafted.is_empty() && !outcome.failures.is_empty() {
            return Err(CarrelError::invalid(outcome.failures.join("\n")));
        }
        Ok(outcome)
    }

    /// The identifiers of the waiting proposals drafted here whose card or collection
    /// couldn't be settled, which Draft Again All drafts again.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the queue can't be read.
    pub fn failed_drafts(&self) -> Result<Vec<String>, CarrelError> {
        let archive = self.fresh()?;
        let device = self.holder()?.device;
        Ok(failed_drafts(&archive, device)?
            .iter()
            .map(|e| e.id.to_string())
            .collect())
    }
}
