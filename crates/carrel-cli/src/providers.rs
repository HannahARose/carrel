//! `carrel providers`: which providers this device has and which Carrel would use.

use std::fmt::Write as _;

use carrel_core::providers::process::{ProcessProvider, default_folders, discover};
use carrel_core::providers::{Capability, Offer, Provider, Registry};
use serde_json::{Value, json};

use crate::Report;

const CAPABILITIES: [Capability; 2] = [Capability::TextModel, Capability::ExtractText];

/// `carrel providers`. It always succeeds: a provider that fails to start is listed with its
/// error.
pub fn list() -> Report {
    let mut registry = Registry::with_built_ins();
    let mut entries = vec![json!({
        "name": "built-in",
        "path": null,
        "offers": offers_json(registry.providers().next().map_or(&[], |p| p.offers())),
    })];
    let mut text = String::new();
    let mut failed = Vec::new();

    for found in discover(&default_folders()) {
        match ProcessProvider::start(&found) {
            Ok(provider) => {
                entries.push(json!({
                    "name": found.name,
                    "path": found.path,
                    "label": provider.label(),
                    "version": provider.version(),
                    "offers": offers_json(provider.offers()),
                }));
                registry.add(Box::new(provider));
            }
            Err(e) => {
                entries.push(json!({
                    "name": found.name,
                    "path": found.path,
                    "error": e.to_string(),
                }));
                failed.push((found, e));
            }
        }
    }

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
    for (found, error) in &failed {
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
