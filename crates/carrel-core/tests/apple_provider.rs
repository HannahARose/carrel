//! The Apple provider (`apple/CarrelProviders`) end to end, through the provider protocol.
//! Runs only when `CARREL_TEST_APPLE_PROVIDER` names the built `carrel-provider-apple`, as
//! CI's Swift job does; elsewhere it passes without checking anything.

fn start() -> Option<ProcessProvider> {
    let Some(program) = std::env::var_os("CARREL_TEST_APPLE_PROVIDER").map(PathBuf::from) else {
        eprintln!("CARREL_TEST_APPLE_PROVIDER isn't set; skipping");
        return None;
    };
    Some(
        ProcessProvider::start(&Found {
            name: "apple".into(),
            path: program,
        })
        .unwrap(),
    )
}

use std::path::{Path, PathBuf};

use carrel_core::providers::process::{Found, ProcessProvider};
use carrel_core::providers::{
    Capability, CompletionRequest, ExtractionRequest, Provider, Registry, priority,
};

#[test]
fn the_apple_provider_reads_pdfs_through_the_registry() {
    let Some(apple) = start() else { return };
    assert_eq!(apple.label(), Some("Apple"));

    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(apple));
    let best = registry.best(Capability::ExtractText).unwrap();
    assert_eq!(
        (best.provider.as_str(), best.offer.as_str()),
        ("apple", "pdfkit")
    );
    assert_eq!(best.priority, priority::NATIVE);

    let pdf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/sample-archive/Reading/Coupled oscillators note.pdf")
        .canonicalize()
        .unwrap();
    let (extraction, source, attempts) = registry
        .extract(&ExtractionRequest {
            path: pdf,
            media_type: "application/pdf".into(),
        })
        .unwrap();
    assert_eq!(source.provider, "apple");
    assert_eq!(attempts, []);
    assert_eq!(extraction.method, "embedded");
    assert_ne!(extraction.text.trim(), "");
}

#[test]
fn the_apple_model_answers_or_says_why_it_cant() {
    let Some(apple) = start() else { return };
    let offer = apple
        .offers()
        .iter()
        .find(|o| o.capability == Capability::TextModel)
        .cloned()
        .expect("the Apple provider offers a text model");
    assert_eq!(offer.id, "afm");
    assert_eq!(offer.priority, priority::NATIVE);
    if !offer.available {
        // CI's virtual Macs have no Apple Intelligence.
        assert!(offer.reason.is_some(), "an unavailable offer says why");
        return;
    }

    let mut registry = Registry::new();
    registry.add(Box::new(apple));
    let (answer, source, _) = registry
        .complete(&CompletionRequest {
            prompt: "Give a short title for a bill of materials for a desk lamp.".into(),
            schema: Some(serde_json::json!({
                "type": "object",
                "required": ["title"],
                "properties": {"title": {"type": "string"}}
            })),
            ..CompletionRequest::default()
        })
        .unwrap();
    assert_eq!(
        (source.provider.as_str(), source.offer.as_str()),
        ("apple", "afm")
    );
    assert!(answer.model.starts_with("apple/"), "{}", answer.model);
    let (tokens, exact) = registry.count_tokens(&source, "A bill of materials for a desk lamp.");
    assert!(exact, "Apple's model counts its own tokens");
    assert!(tokens > 0);
    let title = answer
        .json
        .as_ref()
        .and_then(|j| j["title"].as_str())
        .unwrap_or_default();
    assert_ne!(title, "", "{}", answer.text);
}

#[test]
fn scanned_pdfs_fall_back_from_pdfkit_to_vision() {
    let Some(apple) = start() else { return };
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(apple));
    let scan = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/scanned.pdf")
        .canonicalize()
        .unwrap();
    let (extraction, source, attempts) = registry
        .extract(&ExtractionRequest {
            path: scan,
            media_type: "application/pdf".into(),
        })
        .unwrap();
    assert_eq!(
        (source.provider.as_str(), source.offer.as_str()),
        ("apple", "vision")
    );
    assert_eq!(extraction.method, "ocr");
    assert!(
        extraction.text.to_lowercase().contains("scanned receipt"),
        "{}",
        extraction.text
    );
    assert_eq!(attempts.len(), 1, "PDFKit was tried first");
    assert_eq!(attempts[0].source.offer, "pdfkit");
    assert!(matches!(
        attempts[0].error,
        carrel_core::providers::ProviderError::Unsupported(_)
    ));
}
