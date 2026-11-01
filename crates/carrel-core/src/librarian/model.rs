//! Which model the librarian runs on (`docs/design/librarian.md`, decision 2): the one the
//! `librarian.model` preference names, else the strongest local text model the device has:
//! the largest installed open model that fits in its memory, else Apple's model, else the best
//! text model by priority.
//!
//! How much memory the device has is the caller's to find out, since the core has no
//! platform-specific code.

use crate::providers::Registry;
use crate::providers::device::{AvailableModel, resolve_model, text_models};

/// The provider Apple's on-device model comes with.
const APPLE: &str = "apple";

/// The model the librarian runs on: the one `setting` names when it's available, else the
/// largest catalog model installed that fits in `memory_gb` (any size when that's unknown),
/// else Apple's model, else the best text model. `None` when there's no text model at all.
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
    let largest = available
        .iter()
        .filter_map(|m| {
            let catalog = crate::models::find(&m.offer)?;
            let open = catalog.embedding.is_none() && catalog.comes_with.is_none();
            let fits = memory_gb.is_none_or(|gb| u64::from(catalog.memory_gb) <= gb);
            (open && fits).then_some((catalog.size, m))
        })
        .max_by_key(|(size, _)| *size)
        .map(|(_, m)| m.clone());
    Ok(largest
        .or_else(|| available.iter().find(|m| m.provider == APPLE).cloned())
        .or_else(|| available.into_iter().next()))
}
