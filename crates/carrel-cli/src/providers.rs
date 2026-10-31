//! `carrel providers`: which providers this device has and which Carrel would use.

use std::fmt::Write as _;

use carrel_core::providers::process::{Found, ProcessProvider, default_folders, discover};
use carrel_core::providers::{Capability, Offer, ProviderError, Registry};
use serde_json::{Value, json};

use crate::{Failure, Report};

const CAPABILITIES: [Capability; 2] = [Capability::TextModel, Capability::ExtractText];

/// A provider program that couldn't start, and why.
pub struct Failed {
    /// What was found.
    pub found: Found,
    /// Why it didn't start.
    pub error: ProviderError,
}

/// The registry for this device: the built-in providers and every provider program found
/// beside `carrel` or on `PATH`, with those that couldn't start.
///
/// The text model chosen in settings (`model`) is tried first. If it isn't available, as
/// when its file was removed, the others are used as if none were chosen.
pub fn registry() -> (Registry, Vec<Failed>) {
    let mut registry = Registry::with_built_ins();
    let mut failed = Vec::new();
    for found in discover(&default_folders()) {
        match ProcessProvider::start(&found) {
            Ok(provider) => registry.add(Box::new(provider)),
            Err(error) => failed.push(Failed { found, error }),
        }
    }
    let prefs = crate::config::Prefs::load().unwrap_or_default();
    if let Some(Ok(model)) = prefs.model.map(|name| resolve_model(&registry, &name)) {
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

/// The model asked for a second opinion: the one `setting` names, none for `off`, or, unset,
/// the best installed model the benchmark recommends, other than the first choice.
#[must_use]
pub fn second_model(registry: &Registry, setting: Option<&str>) -> Option<TextModel> {
    match setting.map(str::trim) {
        Some("off") => None,
        Some(name) if !name.is_empty() => resolve_model(registry, name).ok(),
        _ => {
            let models = text_models(registry);
            let first = models.first()?.name();
            models.into_iter().find(|m| {
                m.name() != first
                    && carrel_core::models::find(&m.offer).is_some_and(|c| c.recommended)
            })
        }
    }
}

/// An available text model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextModel {
    /// The provider's name, such as `llama`.
    pub provider: String,
    /// The offer's identifier, such as `qwen3.5-4b`.
    pub offer: String,
    /// `provider/label`, for people.
    pub label: String,
}

impl TextModel {
    /// `provider/offer`, as the `model` setting stores it.
    #[must_use]
    pub fn name(&self) -> String {
        format!("{}/{}", self.provider, self.offer)
    }
}

/// The available text models, best first.
#[must_use]
pub fn text_models(registry: &Registry) -> Vec<TextModel> {
    registry
        .ranked(Capability::TextModel)
        .into_iter()
        .map(|(provider, offer)| TextModel {
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
/// [`Failure`] naming the available models, if none matches.
pub fn resolve_model(registry: &Registry, name: &str) -> Result<TextModel, Failure> {
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
        let names: Vec<String> = available.iter().map(TextModel::name).collect();
        Failure::new(if names.is_empty() {
            format!("no text model {name} is available; there are none (see `carrel models`)")
        } else {
            format!(
                "no text model {name} is available; available: {}",
                names.join(", ")
            )
        })
    })
}

/// `carrel providers`. It always succeeds: a provider that fails to start is listed with its
/// error.
pub fn list() -> Report {
    let (registry, failed) = registry();
    let mut text = String::new();
    let mut entries: Vec<Value> = registry
        .providers()
        .map(|provider| {
            let mut entry = json!({
                "name": provider.name(),
                "offers": offers_json(provider.offers()),
            });
            if let Some(program) = provider.program() {
                entry["path"] = json!(program.path);
                entry["label"] = json!(program.label);
                entry["version"] = json!(program.version);
            } else {
                entry["path"] = Value::Null;
            }
            entry
        })
        .collect();
    entries.extend(failed.iter().map(|f| {
        json!({
            "name": f.found.name,
            "path": f.found.path,
            "error": f.error.to_string(),
        })
    }));

    for provider in registry.providers() {
        writeln!(text, "{}", provider.name()).unwrap();
        for offer in provider.offers() {
            let state = if offer.available {
                String::new()
            } else {
                format!(
                    " (unavailable{})",
                    offer
                        .reason
                        .as_ref()
                        .map(|r| format!(": {r}"))
                        .unwrap_or_default()
                )
            };
            writeln!(
                text,
                "  {:<13}{}, priority {}{state}",
                offer.capability.as_str(),
                offer.label.as_deref().unwrap_or(&offer.id),
                offer.priority
            )
            .unwrap();
        }
    }
    for Failed { found, error } in &failed {
        writeln!(
            text,
            "{} ({}) couldn't start: {error}",
            found.name,
            found.path.display()
        )
        .unwrap();
    }

    let mut chosen = serde_json::Map::new();
    text.push_str("\nUsed for:\n");
    for capability in CAPABILITIES {
        let best = registry.best(capability);
        match &best {
            Some(source) => writeln!(
                text,
                "  {:<13}{} / {}",
                capability.as_str(),
                source.provider,
                source.offer
            ),
            None => writeln!(text, "  {:<13}nothing on this device", capability.as_str()),
        }
        .unwrap();
        chosen.insert(
            capability.as_str().to_owned(),
            best.map_or(Value::Null, |s| json!(s)),
        );
    }

    Report::new(text, json!({ "providers": entries, "chosen": chosen }))
}

fn offers_json(offers: &[Offer]) -> Value {
    serde_json::to_value(offers).expect("offers always serialize")
}

#[cfg(test)]
mod model_tests {
    use carrel_core::providers::{Capability, Offer, Provider, Registry};

    use super::{resolve_model, text_models};

    struct Models {
        name: &'static str,
        offers: Vec<Offer>,
    }

    impl Models {
        fn new(name: &'static str, ids: &[(&str, i32)]) -> Self {
            Self {
                name,
                offers: ids
                    .iter()
                    .map(|(id, priority)| Offer {
                        capability: Capability::TextModel,
                        id: (*id).into(),
                        label: None,
                        priority: *priority,
                        available: true,
                        reason: None,
                        media_types: Vec::new(),
                        extra: serde_json::Map::new(),
                    })
                    .collect(),
            }
        }
    }

    impl Provider for Models {
        fn name(&self) -> &str {
            self.name
        }
        fn offers(&self) -> &[Offer] {
            &self.offers
        }
    }

    #[test]
    fn a_model_is_named_by_provider_and_offer_the_offer_or_the_provider() {
        let mut registry = Registry::with_built_ins();
        registry.add(Box::new(Models::new("apple", &[("afm", 100)])));
        registry.add(Box::new(Models::new(
            "llama",
            &[("small.gguf", 50), ("big.gguf", 50)],
        )));
        let names: Vec<String> = text_models(&registry)
            .iter()
            .map(super::TextModel::name)
            .collect();
        assert_eq!(names[0], "apple/afm", "best first");

        assert_eq!(
            resolve_model(&registry, "llama/big.gguf").unwrap().name(),
            "llama/big.gguf"
        );
        assert_eq!(
            resolve_model(&registry, "big.gguf").unwrap().name(),
            "llama/big.gguf"
        );
        assert_eq!(
            resolve_model(&registry, " apple ").unwrap().name(),
            "apple/afm"
        );
        let missing = resolve_model(&registry, "llama/afm").unwrap_err().0;
        assert!(missing.contains("available: apple/afm"), "{missing}");

        registry.prefer(Capability::TextModel, "llama", "big.gguf");
        assert_eq!(
            text_models(&registry)[0].name(),
            "llama/big.gguf",
            "the choice comes first"
        );
    }
}
