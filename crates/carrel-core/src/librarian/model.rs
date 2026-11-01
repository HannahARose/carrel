//! Which model the librarian runs on (`docs/design/librarian.md`, decision 2): the one the
//! `librarian.model` preference names, else the strongest local text model the device has:
//! the largest installed model the librarian benchmark recommends that fits in its memory
//! (the catalog's `librarian` flag, set from `docs/bench/`), else the largest installed open
//! model that fits, else Apple's model, else the best text model by priority.
//!
//! When there's no model to run on, [`choose_or_explain`] says why and what to do: the text
//! models the device has, or the catalog model to download for its memory ([`to_download`]).
//!
//! How much memory the device has is the caller's to find out, since the core has no
//! platform-specific code.

use std::fmt;
use std::path::Path;

use crate::models::Model;
use crate::providers::Registry;
use crate::providers::device::{AvailableModel, resolve_model, text_models};

/// The provider Apple's on-device model comes with.
const APPLE: &str = "apple";

/// The model the librarian runs on: the one `setting` names when it's available, else the
/// largest catalog model installed that fits in `memory_gb` (any size when that's unknown),
/// taking those the librarian benchmark recommends first, else Apple's model, else the best
/// text model. `None` when there's no text model at all.
///
/// # Errors
///
/// When `setting` names a model that isn't available, a message naming those that are.
pub fn choose(
    registry: &Registry,
    setting: Option<&str>,
    memory_gb: Option<u64>,
) -> Result<Option<AvailableModel>, String> {
    if let Some(name) = setting.map(str::trim).filter(|n| !n.is_empty()) {
        return resolve_model(registry, name).map(Some);
    }
    let available = text_models(registry);
    let catalog = crate::models::CATALOG;
    Ok(largest_fitting(&available, memory_gb, true, catalog)
        .or_else(|| largest_fitting(&available, memory_gb, false, catalog))
        .or_else(|| available.iter().find(|m| m.provider == APPLE).cloned())
        .or_else(|| available.into_iter().next()))
}

/// Why the librarian has no model to run on, and what would let it: the text models this
/// device has, or the one to download. Each interface words its own way of acting on it,
/// such as `carrel models download NAME` or a Download button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoModel {
    /// What `librarian.model` names, when it's a model that isn't available.
    pub gone: Option<String>,
    /// The text models this device has, best first, which the librarian can run on.
    pub available: Vec<AvailableModel>,
    /// The one it runs on once `librarian.model` is cleared: the strongest of `available`.
    pub instead: Option<AvailableModel>,
    /// When there's no text model at all, the catalog's to download for this device. `None`
    /// when one is downloaded already, since then it's a provider to run it that's missing.
    pub download: Option<&'static Model>,
    /// The catalog's text models in the models folder, which no provider here offers.
    pub downloaded: Vec<&'static Model>,
    /// This device's memory in gigabytes, when known.
    pub memory_gb: Option<u64>,
}

impl fmt::Display for NoModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.gone, &self.instead) {
            (Some(gone), Some(instead)) => {
                let names: Vec<&str> = self.available.iter().map(|m| m.label.as_str()).collect();
                write!(
                    f,
                    "librarian.model names {gone}, which isn't available here. This device \
                     has {}; without the setting, the librarian runs on {}.",
                    names.join(", "),
                    instead.label
                )
            }
            (Some(gone), None) => write!(
                f,
                "librarian.model names {gone}, which isn't available here, and this device \
                 has no other text model."
            )
            .and_then(|()| self.write_download(f)),
            (None, _) if !self.downloaded.is_empty() => {
                let names: Vec<&str> = self.downloaded.iter().map(|m| m.name).collect();
                write!(
                    f,
                    "The librarian needs a text model, and none runs here: {} {} downloaded, \
                     but no provider that runs {} was found. Put carrel-provider-llama (or \
                     carrel-provider-mlx) beside carrel, or in a folder on PATH such as \
                     ~/.local/bin.",
                    names.join(", "),
                    if names.len() == 1 { "is" } else { "are" },
                    if names.len() == 1 { "it" } else { "them" },
                )
            }
            (None, _) => write!(
                f,
                "The librarian needs a text model, and this device has none."
            )
            .and_then(|()| self.write_download(f)),
        }
    }
}

impl NoModel {
    /// Says which model to download, or that none in the catalog fits.
    fn write_download(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let memory = self
            .memory_gb
            .map_or_else(String::new, |gb| format!("'s {gb} GB of memory"));
        match self.download {
            Some(model) => write!(
                f,
                " {} ({}, {}) suits this device{memory}.",
                model.title,
                model.name,
                crate::human_size(model.size)
            ),
            None => write!(
                f,
                " No model in Carrel's catalog fits in this device{memory}."
            ),
        }
    }
}

/// The model the librarian runs on, as [`choose`] picks it, or, when there's none, why and
/// what would let it ([`NoModel`]). `models_dir` is this device's models folder, to tell a
/// model that's missing from one that's downloaded with no provider to run it.
///
/// # Errors
///
/// [`NoModel`] when `setting` names a model that isn't available, or there's no text model.
pub fn choose_or_explain(
    registry: &Registry,
    setting: Option<&str>,
    memory_gb: Option<u64>,
    models_dir: Option<&Path>,
) -> Result<AvailableModel, Box<NoModel>> {
    let gone = match choose(registry, setting, memory_gb) {
        Ok(Some(model)) => return Ok(model),
        Ok(None) => None,
        Err(_) => setting.map(|s| s.trim().to_owned()),
    };
    let instead = if gone.is_some() {
        choose(registry, None, memory_gb).ok().flatten()
    } else {
        None
    };
    let available = text_models(registry);
    let downloaded = if available.is_empty() {
        downloaded_text_models(models_dir)
    } else {
        Vec::new()
    };
    Err(Box::new(NoModel {
        download: (instead.is_none() && downloaded.is_empty())
            .then(|| to_download(memory_gb))
            .flatten(),
        downloaded,
        available,
        gone,
        instead,
        memory_gb,
    }))
}

/// The catalog's text models downloaded into `models_dir`, for llama.cpp or MLX.
fn downloaded_text_models(models_dir: Option<&Path>) -> Vec<&'static Model> {
    let Some(dir) = models_dir else {
        return Vec::new();
    };
    let found: Vec<String> = crate::models::installed(dir)
        .unwrap_or_default()
        .into_iter()
        .chain(crate::models::installed_mlx(dir).unwrap_or_default())
        .map(|f| f.name)
        .collect();
    crate::models::CATALOG
        .iter()
        .filter(|m| m.embedding.is_none() && m.comes_with.is_none())
        .filter(|m| found.iter().any(|f| f == m.name))
        .collect()
}

/// The catalog's text model to download for the librarian on a device with `memory_gb` of
/// memory: the largest that fits, taking those the librarian benchmark recommends first, then
/// those the model benchmark recommends, then any. `None` when none fits.
#[must_use]
pub fn to_download(memory_gb: Option<u64>) -> Option<&'static Model> {
    download_from(crate::models::CATALOG, memory_gb)
}

fn download_from(catalog: &[Model], memory_gb: Option<u64>) -> Option<&Model> {
    let fitting: Vec<&Model> = catalog
        .iter()
        .filter(|m| m.embedding.is_none() && m.comes_with.is_none())
        .filter(|m| memory_gb.is_none_or(|gb| u64::from(m.memory_gb) <= gb))
        .collect();
    let largest = |pick: &dyn Fn(&Model) -> bool| {
        fitting
            .iter()
            .copied()
            .filter(|m| pick(m))
            .max_by_key(|m| m.size)
    };
    largest(&|m| m.librarian)
        .or_else(|| largest(&|m| m.recommended))
        .or_else(|| largest(&|_| true))
}

/// The largest of `available` that's an open model in `catalog` fitting in `memory_gb`, and,
/// when `recommended`, that the librarian benchmark recommends.
fn largest_fitting(
    available: &[AvailableModel],
    memory_gb: Option<u64>,
    recommended: bool,
    catalog: &[Model],
) -> Option<AvailableModel> {
    available
        .iter()
        .filter_map(|m| {
            let catalog = catalog.iter().find(|c| c.name == m.offer)?;
            let open = catalog.embedding.is_none() && catalog.comes_with.is_none();
            let fits = memory_gb.is_none_or(|gb| u64::from(catalog.memory_gb) <= gb);
            (open && fits && (catalog.librarian || !recommended)).then_some((catalog.size, m))
        })
        .max_by_key(|(size, _)| *size)
        .map(|(_, m)| m.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(name: &str) -> AvailableModel {
        AvailableModel {
            provider: "llama".into(),
            offer: name.into(),
            label: name.into(),
        }
    }

    #[test]
    fn a_model_the_benchmark_recommends_comes_before_a_larger_one() {
        let available = [installed("qwen3.5-4b"), installed("gemma-4-e4b")];
        let mut catalog = crate::models::CATALOG.to_vec();
        let pick = |catalog: &[Model], memory| {
            largest_fitting(&available, memory, true, catalog)
                .or_else(|| largest_fitting(&available, memory, false, catalog))
                .map(|m| m.offer)
        };
        // Nothing recommended yet: the largest that fits.
        assert_eq!(pick(&catalog, None).as_deref(), Some("gemma-4-e4b"));
        assert_eq!(pick(&catalog, Some(8)).as_deref(), Some("qwen3.5-4b"));
        // Once the benchmark recommends the smaller one, it's chosen wherever it fits.
        for model in &mut catalog {
            model.librarian = model.name == "qwen3.5-4b";
        }
        assert_eq!(pick(&catalog, None).as_deref(), Some("qwen3.5-4b"));
        assert_eq!(pick(&catalog, Some(8)).as_deref(), Some("qwen3.5-4b"));
    }

    /// A provider offering the text models `ids`, as `carrel-provider-llama` does.
    struct Offering(Vec<crate::providers::Offer>);

    impl Offering {
        fn registry(ids: &[&str]) -> Registry {
            let offers = ids
                .iter()
                .map(|id| crate::providers::Offer {
                    capability: crate::providers::Capability::TextModel,
                    id: (*id).into(),
                    label: None,
                    priority: 0,
                    available: true,
                    reason: None,
                    media_types: Vec::new(),
                    extra: serde_json::Map::new(),
                })
                .collect();
            let mut registry = Registry::with_built_ins();
            registry.add(Box::new(Self(offers)));
            registry
        }
    }

    impl crate::providers::Provider for Offering {
        fn name(&self) -> &'static str {
            "llama"
        }

        fn offers(&self) -> &[crate::providers::Offer] {
            &self.0
        }
    }

    #[test]
    fn the_strongest_installed_model_is_chosen() {
        let registry = Offering::registry(&["qwen3.5-4b", "gemma-4-e4b"]);
        let chosen = choose_or_explain(&registry, None, Some(16), None).unwrap();
        assert_eq!(chosen.name(), "llama/gemma-4-e4b");
        let chosen =
            choose_or_explain(&registry, Some("llama/qwen3.5-4b"), Some(16), None).unwrap();
        assert_eq!(chosen.name(), "llama/qwen3.5-4b");
    }

    #[test]
    fn with_no_model_installed_it_names_one_to_download() {
        let registry = Registry::with_built_ins();
        let missing = choose_or_explain(&registry, None, Some(8), None).unwrap_err();
        assert_eq!(missing.gone, None);
        assert_eq!(missing.available, []);
        let download = missing.download.unwrap();
        assert_eq!(download.name, "qwen3.5-4b");
        let words = missing.to_string();
        assert!(
            words.starts_with("The librarian needs a text model"),
            "{words}"
        );
        assert!(words.contains("(qwen3.5-4b, 2.7 GB)"), "{words}");
        assert!(words.contains("8 GB of memory"), "{words}");
        // A larger machine gets the larger model the benchmark recommends.
        let download = choose_or_explain(&registry, None, Some(64), None)
            .unwrap_err()
            .download;
        assert_eq!(download.map(|m| m.name), Some("gemma-4-e4b"));
        // And a machine too small for any is told so.
        let tiny = choose_or_explain(&registry, None, Some(2), None).unwrap_err();
        assert_eq!(tiny.download, None);
        assert!(
            tiny.to_string()
                .contains("No model in Carrel's catalog fits")
        );
    }

    #[test]
    fn a_setting_naming_a_model_thats_gone_lists_those_installed() {
        let registry = Offering::registry(&["qwen3.5-4b", "granite-4.2-3b"]);
        let missing =
            choose_or_explain(&registry, Some("llama/gone-7b"), Some(16), None).unwrap_err();
        assert_eq!(missing.gone.as_deref(), Some("llama/gone-7b"));
        assert_eq!(missing.available.len(), 2);
        assert_eq!(
            missing.instead.map(|m| m.offer).as_deref(),
            Some("qwen3.5-4b")
        );
        assert_eq!(missing.download, None);
        // With nothing installed, it's a model to download instead.
        let registry = Registry::with_built_ins();
        let missing =
            choose_or_explain(&registry, Some("llama/gone-7b"), Some(16), None).unwrap_err();
        assert_eq!(missing.instead, None);
        assert_eq!(missing.download.map(|m| m.name), Some("gemma-4-e4b"));
        assert!(missing.to_string().contains("no other text model"));
    }

    #[test]
    fn the_librarian_benchmark_choice_comes_first_to_download() {
        let mut catalog = crate::models::CATALOG.to_vec();
        for model in &mut catalog {
            model.librarian = model.name == "qwen3.5-4b";
        }
        assert_eq!(
            download_from(&catalog, Some(64)).map(|m| m.name),
            Some("qwen3.5-4b")
        );
    }

    #[test]
    fn a_model_downloaded_with_no_provider_to_run_it_says_the_provider_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let qwen = crate::models::find("qwen3.5-4b").unwrap();
        std::fs::write(dir.path().join(qwen.file_name()), b"gguf").unwrap();
        let registry = Registry::with_built_ins();
        let missing = choose_or_explain(&registry, None, Some(16), Some(dir.path())).unwrap_err();
        assert_eq!(missing.downloaded, [qwen]);
        assert_eq!(missing.download, None, "it's downloaded already");
        let words = missing.to_string();
        assert!(words.contains("qwen3.5-4b is downloaded"), "{words}");
        assert!(words.contains("carrel-provider-llama"), "{words}");
    }
}
