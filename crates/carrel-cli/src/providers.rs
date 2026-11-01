//! `carrel providers`: which providers this device has and which Carrel would use.

use std::fmt::Write as _;

use carrel_core::providers::{Capability, Offer, Registry};
use serde_json::{Value, json};

use crate::{Failure, Report};

const CAPABILITIES: [Capability; 2] = [Capability::TextModel, Capability::ExtractText];

pub use carrel_core::providers::device::{
    AvailableModel as TextModel, Failed, second_model, text_models,
};

/// The registry for this device ([`carrel_core::providers::device::registry`]).
pub fn registry() -> (Registry, Vec<Failed>) {
    carrel_core::providers::device::registry(&[])
}

/// The available text model `name` names: `provider/offer`, the offer alone, or a provider
/// alone for its best offer (`apple`).
///
/// # Errors
///
/// [`Failure`] naming the available models, if none matches.
pub fn resolve_model(registry: &Registry, name: &str) -> Result<TextModel, Failure> {
    carrel_core::providers::device::resolve_model(registry, name).map_err(Failure::new)
}

/// `carrel providers`. It always succeeds: a provider that fails to start is listed with its
/// error.
pub fn list() -> Report {
    let (registry, failed) = registry();
    list_of(&registry, &failed)
}

/// `carrel providers`, for the providers in `registry` and those that `failed`.
pub fn list_of(registry: &Registry, failed: &[Failed]) -> Report {
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
    for Failed { found, error } in failed {
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
