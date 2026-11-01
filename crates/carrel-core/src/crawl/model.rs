//! Which model the crawler judges with: the largest installed model the crawler benchmark
//! recommends that fits in the device's memory (the catalog's `crawler` flag, set from
//! `docs/bench/`), else the librarian's choice without a preference, the strongest the device
//! has ([`crate::librarian::model::choose`]).
//!
//! How much memory the device has is the caller's to find out, since the core has no
//! platform-specific code.

use crate::models::Model;
use crate::providers::Registry;
use crate::providers::device::{AvailableModel, text_models};

/// The model the crawler judges with on a device with `memory_gb` of memory (any size when
/// that's unknown), or `None` when there's no text model at all.
#[must_use]
pub fn choose(registry: &Registry, memory_gb: Option<u64>) -> Option<AvailableModel> {
    recommended(&text_models(registry), memory_gb, crate::models::CATALOG).or_else(|| {
        crate::librarian::model::choose(registry, None, memory_gb)
            .ok()
            .flatten()
    })
}

/// The largest of `available` that `catalog` marks as the crawler benchmark's recommendation
/// and that fits in `memory_gb`.
fn recommended(
    available: &[AvailableModel],
    memory_gb: Option<u64>,
    catalog: &[Model],
) -> Option<AvailableModel> {
    available
        .iter()
        .filter_map(|m| {
            let model = catalog.iter().find(|c| c.name == m.offer)?;
            let fits = memory_gb.is_none_or(|gb| u64::from(model.memory_gb) <= gb);
            (model.crawler && fits).then_some((model.size, m))
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
    fn a_model_the_crawler_benchmark_recommends_is_chosen_where_it_fits() {
        let available = [installed("qwen3.5-4b"), installed("qwen3.5-9b")];
        let mut catalog = crate::models::CATALOG.to_vec();
        for model in &mut catalog {
            model.crawler = false;
        }
        assert_eq!(recommended(&available, None, &catalog), None, "none recommended");
        for model in &mut catalog {
            model.crawler = model.name.starts_with("qwen3.5");
        }
        assert_eq!(
            recommended(&available, None, &catalog).map(|m| m.offer),
            Some("qwen3.5-9b".into()),
            "the largest recommended"
        );
        let small = catalog
            .iter()
            .find(|m| m.name == "qwen3.5-4b")
            .unwrap()
            .memory_gb;
        assert_eq!(
            recommended(&available, Some(u64::from(small)), &catalog).map(|m| m.offer),
            Some("qwen3.5-4b".into()),
            "the largest that fits"
        );
    }
}
