//! The Apple provider (`apple/CarrelProviders`) end to end, through the provider protocol.
//! Runs only when `CARREL_TEST_APPLE_PROVIDER` names the built `carrel-provider-apple`, as
//! CI's Swift job does; elsewhere it passes without checking anything.

use std::path::{Path, PathBuf};

use carrel_core::providers::process::{Found, ProcessProvider};
use carrel_core::providers::{Capability, ExtractionRequest, Registry, priority};

#[test]
fn the_apple_provider_reads_pdfs_through_the_registry() {
    let Some(program) = std::env::var_os("CARREL_TEST_APPLE_PROVIDER").map(PathBuf::from) else {
        eprintln!("CARREL_TEST_APPLE_PROVIDER isn't set; skipping");
        return;
    };
    let apple = ProcessProvider::start(&Found {
        name: "apple".into(),
        path: program,
    })
    .unwrap();
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
