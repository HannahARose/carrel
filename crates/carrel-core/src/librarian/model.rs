//! Which model the librarian runs on (`docs/design/librarian.md`, decision 2): the one the
//! `librarian.model` preference names, else the strongest local text model the device has:
//! the largest installed model the librarian benchmark recommends that fits in its memory
//! (the catalog's `librarian` flag, set from `docs/bench/`), else the largest installed open
//! model that fits, else Apple's model, else the best text model by priority.
//!
//! How much memory the device has is the caller's to find out, since the core has no
//! platform-specific code.

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
}
