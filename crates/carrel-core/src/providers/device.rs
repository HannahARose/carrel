//! The providers on this device, as the CLI and the apps start them: the built-in ones, every
//! provider program found beside the running program, in an app bundle's `Helpers` folder or
//! on `PATH`, and the text models the owner chose in [`Prefs`]. Provider programs run on
//! computers; on iOS only the built-in providers and the app's own are there.

use std::path::PathBuf;

use super::{Capability, ProviderError, Registry};
use crate::local::Prefs;

#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
mod discovered;

#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
#[doc(hidden)]
pub use discovered::retry_after;

/// A provider program found on this device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The provider's name: the program's name without `carrel-provider-` or `.exe`.
    pub name: String,
    /// Where the program is.
    pub path: PathBuf,
}

/// A provider program that couldn't start, and why.
#[derive(Debug)]
pub struct Failed {
    /// What was found.
    pub found: Found,
    /// Why it didn't start.
    pub error: ProviderError,
}

/// The registry for this device: the built-in providers and every provider program found in
/// `folders` then the default ones (`process::default_folders`), with those that couldn't start.
///
/// The text model chosen in the preferences (`model`) is tried first. If it isn't available,
/// as when its file was removed, the others are used as if none were chosen.
#[must_use]
pub fn registry(folders: &[PathBuf]) -> (Registry, Vec<Failed>) {
    registry_with(folders, &Prefs::load().unwrap_or_default())
}

/// The registry for this device, as [`registry`], with the models chosen in `prefs`.
#[must_use]
pub fn registry_with(folders: &[PathBuf], prefs: &Prefs) -> (Registry, Vec<Failed>) {
    BUILT_HERE.set(BUILT_HERE.get() + 1);
    let mut registry = Registry::with_built_ins();
    let failed = start_programs(&mut registry, folders);
    if let Some(Ok(model)) = prefs
        .model
        .as_ref()
        .map(|name| resolve_model(&registry, name))
    {
        registry.prefer(Capability::TextModel, &model.provider, &model.offer);
    }
    let second = second_model(&registry, prefs.second_model.as_deref());
    registry.second_opinion(
        Capability::TextModel,
        second
            .as_ref()
            .map(|m| (m.provider.as_str(), m.offer.as_str())),
    );
    (registry, failed)
}

thread_local! {
    static BUILT_HERE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many registries this thread has built, so a test can check that a thread that must
/// stay quick, such as the interactive mode's, builds none.
#[must_use]
pub fn registries_built_here() -> usize {
    BUILT_HERE.get()
}

/// Forgets what the provider programs offered, so the next registry starts them all again
/// to ask. What they offer is kept for as long as the programs and the models folder are
/// unchanged, which covers downloads; this is for anything else, such as the owner asking
/// to look again.
pub fn forget_providers() {
    #[cfg(not(any(
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos"
    )))]
    discovered::forget();
}

/// Adds the provider programs in `folders` then the default folders to `registry`; returns
/// those that couldn't start. The first time, they're started together, and what each
/// offers is kept; later, while nothing has changed, they're added from what was kept and
/// started only when used (#333).
#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
fn start_programs(registry: &mut Registry, folders: &[PathBuf]) -> Vec<Failed> {
    use super::process::{default_folders, discover};
    let mut all = folders.to_vec();
    all.extend(default_folders());
    let mut failed = Vec::new();
    for provider in discovered::providers(&discover(&all)) {
        match provider {
            Ok(provider) => registry.add(Box::new(provider)),
            Err(failure) => failed.push(failure),
        }
    }
    failed
}

/// There are no provider programs to start here.
#[cfg(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
))]
fn start_programs(_registry: &mut Registry, _folders: &[PathBuf]) -> Vec<Failed> {
    Vec::new()
}

/// The model asked for a second opinion: the one `setting` names, none for `off`, or, unset,
/// the best installed model the benchmark recommends, other than the first choice.
#[must_use]
pub fn second_model(registry: &Registry, setting: Option<&str>) -> Option<AvailableModel> {
    match setting.map(str::trim) {
        Some("off") => None,
        Some(name) if !name.is_empty() => resolve_model(registry, name).ok(),
        _ => {
            let models = text_models(registry);
            let first = models.first()?.name();
            models.into_iter().find(|m| {
                m.name() != first && crate::models::find(&m.offer).is_some_and(|c| c.recommended)
            })
        }
    }
}

/// An available text model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableModel {
    /// The provider's name, such as `llama`.
    pub provider: String,
    /// The offer's identifier, such as `qwen3.5-4b`.
    pub offer: String,
    /// `provider/label`, for people.
    pub label: String,
}

impl AvailableModel {
    /// `provider/offer`, as the `model` preference stores it.
    #[must_use]
    pub fn name(&self) -> String {
        format!("{}/{}", self.provider, self.offer)
    }
}

/// The available text models, best first.
#[must_use]
pub fn text_models(registry: &Registry) -> Vec<AvailableModel> {
    registry
        .ranked(Capability::TextModel)
        .into_iter()
        .map(|(provider, offer)| AvailableModel {
            provider: provider.name().to_owned(),
            offer: offer.id.clone(),
            label: format!(
                "{}/{}",
                provider.name(),
                offer.label.as_deref().unwrap_or(&offer.id)
            ),
        })
        .collect()
}

/// The available text model `name` names: `provider/offer`, the offer alone, or a provider
/// alone for its best offer (`apple`).
///
/// # Errors
///
/// A message naming the available models, if none matches.
pub fn resolve_model(registry: &Registry, name: &str) -> Result<AvailableModel, String> {
    let available = text_models(registry);
    let name = name.trim();
    let found = match name.split_once('/') {
        Some((provider, offer)) => available
            .iter()
            .find(|m| m.provider == provider && m.offer == offer),
        None => available
            .iter()
            .find(|m| m.offer == name)
            .or_else(|| available.iter().find(|m| m.provider == name)),
    };
    found.cloned().ok_or_else(|| {
        let names: Vec<String> = available.iter().map(AvailableModel::name).collect();
        if names.is_empty() {
            format!("no text model {name} is available; there are none (see `carrel models`)")
        } else {
            format!(
                "no text model {name} is available; available: {}",
                names.join(", ")
            )
        }
    })
}
