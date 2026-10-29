//! `carrel providers`: which providers this device has and which Carrel would use.

use std::fmt::Write as _;

use carrel_core::providers::process::{Found, ProcessProvider, default_folders, discover};
use carrel_core::providers::{Capability, Offer, ProviderError, Registry};
use serde_json::{Value, json};

use crate::Report;

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
pub fn registry() -> (Registry, Vec<Failed>) {
    let mut registry = Registry::with_built_ins();
    let mut failed = Vec::new();
    for found in discover(&default_folders()) {
        match ProcessProvider::start(&found) {
            Ok(provider) => registry.add(Box::new(provider)),
            Err(error) => failed.push(Failed { found, error }),
        }
    }
    (registry, failed)
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
