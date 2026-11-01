//! The embedding index, as Settings shows it (`docs/design/search.md`): its model, how many
//! items are embedded and how many are behind, a switch to another model under way, and
//! rebuilding it or switching models with progress, as `carrel index` does
//! ([`carrel_core::index::manage`]).

use std::sync::Arc;

use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::index::manage::{self, Built, ManageError, Stage, Switched};
use carrel_core::models::embedding_models;
use carrel_core::search::run::{ModelSource, how_to_get};

use crate::models::{CatalogModel, catalog_model};

use crate::{ArchiveHandle, CarrelError, ProgressListener};

/// A switch of embedding model under way, and how far it has got.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct IndexSwitch {
    /// The archive's model until the switch finishes, if it has one.
    pub from: Option<String>,
    /// The model it's switching to.
    pub to: String,
    /// How many items the archive has.
    pub items: u64,
    /// How many are embedded with the new model.
    pub embedded: u64,
    /// How many are still to embed.
    pub behind: u64,
}

/// The archive's embedding index, as `carrel index status` reports it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct IndexStatus {
    /// The archive's embedding model; without one, it's searched by words only.
    pub model: Option<String>,
    /// Whether the model has an index yet.
    pub exists: bool,
    /// How many items the archive has.
    pub items: u64,
    /// How many are embedded and up to date.
    pub embedded: u64,
    /// How many are missing or out of date.
    pub behind: u64,
    /// How many vectors the index holds.
    pub vectors: u64,
    /// Whether a rebuild stopped part way; rebuilding again carries on from there.
    pub rebuilding: bool,
    /// The switch to another model under way, if any.
    pub switch: Option<IndexSwitch>,
}

/// An embedding model this device can embed with.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct EmbeddingChoice {
    /// The model's identifier.
    pub model: String,
    /// The provider that runs it.
    pub provider: String,
    /// How many numbers its vectors have, if the provider says.
    pub dimensions: Option<u64>,
}

/// An embedding model in Carrel's catalog that this device doesn't have.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct EmbeddingElsewhere {
    /// The model's identifier.
    pub model: String,
    /// How to get it, in words that name no interface's command.
    pub how: String,
    /// The catalog model to download for it, when it's downloaded; download it with
    /// [`ArchiveHandle::download_model`] once the owner has seen its size.
    pub download: Option<CatalogModel>,
    /// The provider it comes with instead, which only Apple devices have.
    pub comes_with: Option<String>,
}

/// The embedding models to switch to, as `carrel index model` lists them.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct EmbeddingModels {
    /// Those a provider here offers, best first.
    pub here: Vec<EmbeddingChoice>,
    /// The catalog's others, and how to get them.
    pub elsewhere: Vec<EmbeddingElsewhere>,
}

/// How a rebuild or a switch left the index.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct IndexBuilt {
    /// The model it's of.
    pub model: String,
    /// How many items were embedded.
    pub embedded: u64,
    /// How many items it holds.
    pub items: u64,
    /// How many vectors it holds.
    pub vectors: u64,
    /// Whether it carried on from a rebuild or switch that stopped part way.
    pub resumed: bool,
}

/// How a switch of embedding model left things.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum IndexSwitched {
    /// The archive already used the model, and wasn't switching.
    Already,
    /// The archive already used the model; the switch to `to` was stopped and its index
    /// removed.
    Stopped {
        /// The model it had been switching to.
        to: String,
    },
    /// The archive uses the model now, on every device, as one change in its history.
    Done {
        /// The model it used before, whose index was removed.
        from: Option<String>,
        /// A switch to another model that was given up to start this one.
        replaced: Option<String>,
        /// The new model's index.
        built: IndexBuilt,
    },
}

impl From<ManageError> for CarrelError {
    fn from(error: ManageError) -> Self {
        match error {
            ManageError::Lease(e) => e.into(),
            e @ ManageError::NotOffered { .. } => Self::invalid(e),
            e => Self::failed(e),
        }
    }
}

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

fn built(model: &str, built: Built) -> IndexBuilt {
    IndexBuilt {
        model: model.to_owned(),
        embedded: count(built.embedded),
        items: count(built.items),
        vectors: count(built.vectors),
        resumed: built.resumed,
    }
}

/// Reports catching up to `progress`: the item, prefixed while its text is read.
fn reporter(
    progress: Option<Arc<dyn ProgressListener>>,
) -> impl FnMut(Stage, usize, usize, Option<&str>) {
    move |stage, done, total, item| {
        if let Some(progress) = &progress {
            let item = item.map(|item| match stage {
                Stage::Reading => format!("Reading {item}"),
                Stage::Embedding => item.to_owned(),
            });
            progress.update(count(done), count(total), item);
        }
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// The archive's embedding model, how many items are embedded with it and how many are
    /// behind, and a switch to another model under way.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive or its index can't be read.
    pub fn index_status(&self) -> Result<IndexStatus, CarrelError> {
        let status = manage::status(&self.fresh()?)?;
        Ok(IndexStatus {
            model: status.model,
            exists: status.exists,
            items: count(status.items),
            embedded: count(status.embedded),
            behind: count(status.behind),
            vectors: count(status.vectors),
            rebuilding: status.rebuilding,
            switch: status.switch.map(|s| IndexSwitch {
                from: s.from,
                to: s.to,
                items: count(s.items),
                embedded: count(s.embedded),
                behind: count(s.behind),
            }),
        })
    }

    /// The embedding models this device can embed with, and the catalog's others with how
    /// to get them. Starts the providers to find out.
    #[must_use]
    pub fn embedding_models(&self) -> EmbeddingModels {
        let here: Vec<EmbeddingChoice> = manage::offered_models(&self.registry())
            .into_iter()
            .map(|m| EmbeddingChoice {
                model: m.model,
                provider: m.provider,
                dimensions: m.dimensions,
            })
            .collect();
        let elsewhere = embedding_models()
            .filter(|m| !here.iter().any(|h| h.model == m.name))
            .filter_map(|m| {
                let how = how_to_get(m.name)?;
                Some(EmbeddingElsewhere {
                    model: m.name.to_owned(),
                    how: how.to_string(),
                    download: match how {
                        ModelSource::Download(found) => {
                            Some(catalog_model(found, self.models_dir().as_deref()))
                        }
                        ModelSource::ComesWith(_) => None,
                    },
                    comes_with: match how {
                        ModelSource::ComesWith(provider) => Some(provider.to_owned()),
                        ModelSource::Download(_) => None,
                    },
                })
            })
            .collect();
        EmbeddingModels { here, elsewhere }
    }

    /// Embeds every item again with the archive's embedding model, reporting items to
    /// `progress`. Interrupted, it carries on where it stopped when called again.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if the archive has no embedding model yet (switch to one
    /// instead), or [`CarrelError::Failed`] if no provider here runs it, it fails, or the
    /// index can't be written.
    pub fn rebuild_index(
        &self,
        progress: Option<Arc<dyn ProgressListener>>,
    ) -> Result<IndexBuilt, CarrelError> {
        let archive = self.fresh()?;
        let model = archive
            .settings()
            .embedding_model()
            .map(str::to_owned)
            .ok_or_else(|| {
                CarrelError::invalid("this archive has no embedding model yet; choose one first")
            })?;
        let rebuilt = manage::rebuild(&archive, &self.registry(), &model, &mut reporter(progress))?;
        Ok(built(&model, rebuilt))
    }

    /// Switches the archive to the embedding model `model`, which a provider here must offer,
    /// reporting items to `progress`: builds its index while search keeps using the current
    /// one, then makes it the archive's model for every device. `model` already the archive's,
    /// it stops a switch to another. Interrupted, it carries on when called again.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if no provider here offers it, [`CarrelError::Busy`] if
    /// another writer holds the archive, or [`CarrelError::Failed`].
    pub fn switch_index(
        &self,
        model: String,
        progress: Option<Arc<dyn ProgressListener>>,
    ) -> Result<IndexSwitched, CarrelError> {
        let archive = self.fresh()?;
        let holder = self.holder()?;
        let switched = manage::switch_to(
            &archive,
            &self.registry(),
            &model,
            &mut || DiskLock::for_archive(&archive).acquire(holder, "index model", DEFAULT_LEASE),
            &mut reporter(progress),
        )?;
        Ok(match switched {
            Switched::Already => IndexSwitched::Already,
            Switched::Stopped(to) => IndexSwitched::Stopped { to },
            Switched::Done {
                from,
                replaced,
                built: b,
            } => IndexSwitched::Done {
                from,
                replaced,
                built: built(model.trim(), b),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::Sample;

    #[test]
    fn a_model_not_here_says_how_to_get_it_as_data() {
        let sample = Sample::new();
        let models = sample.handle.embedding_models();
        let gemma = models
            .elsewhere
            .iter()
            .find(|m| m.model == "embeddinggemma-300m")
            .unwrap();
        let download = gemma.download.as_ref().unwrap();
        assert_eq!(download.name, "embeddinggemma-300m");
        assert!(download.size > 0);
        assert_eq!(gemma.comes_with, None);
        assert!(!gemma.how.contains("carrel models"), "{}", gemma.how);
        let apple = models
            .elsewhere
            .iter()
            .find(|m| m.model == "apple-nl-sentence-en")
            .unwrap();
        assert_eq!(apple.download, None);
        assert_eq!(apple.comes_with.as_deref(), Some("carrel-provider-apple"));
    }
}
