//! Models and providers, as Settings shows them: Carrel's model catalog with what's
//! downloaded, the providers this device has, and downloading a catalog model through the
//! network helper as `carrel models download` does. A download is always the owner's
//! confirmed action: the app shows the size first and asks.

use std::sync::Arc;

use carrel_core::models::{CATALOG, Model, installed, installed_mlx, part_path};
use carrel_core::providers::Capability;

use crate::{ArchiveHandle, CarrelError, ProgressReporter, text};

/// A model in Carrel's catalog.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CatalogModel {
    /// Its short name, used to download it.
    pub name: String,
    /// What it is, in words.
    pub title: String,
    /// Whether it's an embedding model, for search by meaning; otherwise a text model.
    pub embedding: bool,
    /// The download's size in bytes; 0 for one that comes with a provider.
    pub size: u64,
    /// Its license's SPDX identifier.
    pub license: String,
    /// The memory a machine should have to run it, in gigabytes.
    pub memory_gb: u8,
    /// Whether the benchmark recommends it.
    pub recommended: bool,
    /// The provider it comes with, when there's nothing to download.
    pub comes_with: Option<String>,
    /// Whether it's here.
    pub state: ModelState,
    /// Whether it can be downloaded: its file is pinned to a checksum.
    pub downloadable: bool,
}

/// Whether a catalog model is on this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ModelState {
    /// It comes with a provider: there's nothing to download.
    BuiltIn,
    /// It's in the models folder.
    Downloaded,
    /// A download was interrupted, with this much of it there.
    Partial {
        /// Bytes downloaded.
        bytes: u64,
    },
    /// It isn't here.
    NotDownloaded,
}

/// What a provider offers.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ProviderOffer {
    /// What it does: `text-model`, `extract-text` or `embed`.
    pub capability: String,
    /// Its identifier within the provider, such as a model's name.
    pub id: String,
    /// A name for people.
    pub label: String,
    /// Higher wins when several can do the same thing.
    pub priority: i32,
    /// Whether it can be used now.
    pub available: bool,
    /// Why it can't, when it isn't.
    pub reason: Option<String>,
}

/// A provider on this device.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ProviderInfo {
    /// Its name, as in `carrel-provider-<name>`.
    pub name: String,
    /// Its program, for a provider program.
    pub path: Option<String>,
    /// Its version, when it says.
    pub version: Option<String>,
    /// What it offers.
    pub offers: Vec<ProviderOffer>,
    /// Why it couldn't start, when it couldn't; then it offers nothing.
    pub error: Option<String>,
}

fn catalog_model(model: &Model, dir: Option<&std::path::Path>) -> CatalogModel {
    let found = dir
        .map(|dir| {
            let mut found = installed(dir).unwrap_or_default();
            found.extend(installed_mlx(dir).unwrap_or_default());
            found
        })
        .unwrap_or_default();
    let partial = dir
        .filter(|_| model.comes_with.is_none())
        .and_then(|dir| std::fs::metadata(part_path(model, dir)).ok())
        .map(|m| m.len());
    CatalogModel {
        name: model.name.to_owned(),
        title: model.title.to_owned(),
        embedding: model.embedding.is_some(),
        size: if model.comes_with.is_some() {
            0
        } else {
            model.size
        },
        license: model.license.to_owned(),
        memory_gb: model.memory_gb,
        recommended: model.recommended,
        comes_with: model.comes_with.map(str::to_owned),
        state: if model.comes_with.is_some() {
            ModelState::BuiltIn
        } else if found.iter().any(|f| f.name == model.name) {
            ModelState::Downloaded
        } else if let Some(bytes) = partial {
            ModelState::Partial { bytes }
        } else {
            ModelState::NotDownloaded
        },
        downloadable: model.comes_with.is_none() && model.is_pinned(),
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// Carrel's model catalog, text models first, with what's in this device's models
    /// folder.
    #[must_use]
    pub fn model_catalog(&self) -> Vec<CatalogModel> {
        let dir = carrel_core::local::models_dir();
        let mut models: Vec<CatalogModel> = CATALOG
            .iter()
            .map(|m| catalog_model(m, dir.as_deref()))
            .collect();
        models.sort_by_key(|m| m.embedding);
        models
    }

    /// The providers this device has, built-in ones first, and those that couldn't start.
    #[must_use]
    pub fn providers(&self) -> Vec<ProviderInfo> {
        let (registry, failed) = if self.options.start_providers {
            let folders: Vec<std::path::PathBuf> = self
                .options
                .provider_folders
                .iter()
                .map(std::path::PathBuf::from)
                .collect();
            carrel_core::providers::device::registry_with(
                &folders,
                &self.prefs().unwrap_or_default(),
            )
        } else {
            (
                carrel_core::providers::Registry::with_built_ins(),
                Vec::new(),
            )
        };
        let mut providers: Vec<ProviderInfo> = registry
            .providers()
            .map(|provider| {
                let program = provider.program();
                ProviderInfo {
                    name: provider.name().to_owned(),
                    path: program.as_ref().map(|p| text(&p.path)),
                    version: program.and_then(|p| p.version),
                    offers: provider
                        .offers()
                        .iter()
                        .map(|offer| ProviderOffer {
                            capability: offer.capability.as_str().to_owned(),
                            id: offer.id.clone(),
                            label: offer.label.clone().unwrap_or_else(|| offer.id.clone()),
                            priority: offer.priority,
                            available: offer.available,
                            reason: offer.reason.clone(),
                        })
                        .collect(),
                    error: None,
                }
            })
            .collect();
        providers.extend(failed.into_iter().map(|f| ProviderInfo {
            name: f.found.name,
            path: Some(text(&f.found.path)),
            version: None,
            offers: Vec::new(),
            error: Some(f.error.to_string()),
        }));
        providers
    }

    /// Whether a provider on this device offers `capability` (`text-model`,
    /// `extract-text` or `embed`).
    #[must_use]
    pub fn has_capability(&self, capability: String) -> bool {
        let wanted = [
            Capability::TextModel,
            Capability::ExtractText,
            Capability::Embed,
        ]
        .into_iter()
        .find(|c| c.as_str() == capability);
        wanted.is_some_and(|c| !self.registry().ranked(c).is_empty())
    }

    /// Downloads the catalog model `name` into this device's models folder through the
    /// network helper, `carrel-fetch`, reporting bytes to `progress`. Call it only once the
    /// owner has confirmed, having seen the size ([`CatalogModel::size`]). Returns where it
    /// was saved.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for a model that isn't in the catalog or can't be downloaded,
    /// or [`CarrelError::Failed`] if the helper is missing or the download fails.
    pub fn download_model(
        &self,
        name: String,
        progress: Option<Arc<dyn ProgressReporter>>,
    ) -> Result<String, CarrelError> {
        download(&name, progress)
    }
}

#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
fn download(
    name: &str,
    progress: Option<Arc<dyn ProgressReporter>>,
) -> Result<String, CarrelError> {
    use carrel_core::models::{Helper, ModelError, download, find, network_log};
    let model = find(name)
        .ok_or_else(|| CarrelError::invalid(format!("no model called {name} in the catalog")))?;
    if let Some(provider) = model.comes_with {
        return Err(CarrelError::invalid(ModelError::ComesWith(
            model.name.to_owned(),
            provider,
        )));
    }
    if !model.is_pinned() {
        return Err(CarrelError::invalid(ModelError::Unpinned(
            model.name.to_owned(),
        )));
    }
    let dir = carrel_core::local::models_dir()
        .ok_or_else(|| CarrelError::failed("no models folder: set CARREL_MODELS_DIR"))?;
    let helper = Helper::find()?;
    let log = network_log();
    let path = download(model, &dir, &helper, log.as_deref(), &mut |done, total| {
        if let Some(progress) = &progress {
            progress.update(done, total, Some(name.to_owned()));
        }
    })?;
    Ok(text(&path))
}

#[cfg(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
))]
fn download(
    _name: &str,
    _progress: Option<Arc<dyn ProgressReporter>>,
) -> Result<String, CarrelError> {
    Err(CarrelError::failed("models are downloaded on a Mac"))
}
