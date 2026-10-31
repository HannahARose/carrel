//! The CAD provider as a real program, through Carrel's provider registry. Without
//! `OpenCascade` it must say how to install it; with it, `CARREL_REQUIRE_OPENCASCADE` (set in
//! CI's job for this provider) makes the drawing tests fail rather than pass unchecked.

use std::path::{Path, PathBuf};

use carrel_core::providers::process::{Found, ProcessProvider};
use carrel_core::providers::{Capability, ExtractionRequest, Provider, ProviderError, Registry};

fn provider() -> ProcessProvider {
    ProcessProvider::start(&Found {
        name: "cad".into(),
        path: PathBuf::from(env!("CARGO_BIN_EXE_carrel-provider-cad")),
    })
    .unwrap()
}

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

/// Whether the provider was built with `OpenCascade`; fails the run if it must have been.
fn drawing(provider: &ProcessProvider) -> bool {
    let offer = &provider.offers()[0];
    if !offer.available {
        assert!(
            offer
                .reason
                .as_deref()
                .unwrap()
                .contains("brew install opencascade")
        );
        assert!(
            std::env::var_os("CARREL_REQUIRE_OPENCASCADE").is_none(),
            "OpenCascade is required but carrel-provider-cad was built without it"
        );
    }
    offer.available
}

#[test]
fn step_and_iges_models_are_measured_and_drawn() {
    let cad = provider();
    if !drawing(&cad) {
        return;
    }
    let mut registry = Registry::new();
    // The extraction provider reads the same headers; the CAD provider comes first.
    registry.add(Box::new(carrel_provider_extract::extract::Extractor::with(
        None,
        None,
        "eng".into(),
    )));
    registry.add(Box::new(cad));
    for (name, media_type, heading) in [
        ("bracket.step", "model/step", "CAD model (STEP, AP214)"),
        ("bracket.igs", "model/iges", "CAD model (IGES)"),
    ] {
        let (extraction, source, _) = registry
            .extract(&ExtractionRequest {
                path: data(name),
                media_type: media_type.into(),
                context: None,
            })
            .unwrap();
        assert_eq!(source.offer, "cad-model");
        assert_eq!(extraction.method, "geometry");
        let text = &extraction.text;
        assert!(text.starts_with(heading), "{text}");
        assert!(text.contains("\nSize: 63 × 20 × 30 mm"), "{text}");
        assert!(text.contains("\nSolids: 2"), "{text}");
        // The bracket is 40 × 20 × 3 plus 40 × 3 × 27, and the pin π × 3² × 25: 6,347 mm³.
        assert!(text.contains("\nVolume: 6.35 cm³"), "{text}");
        let [picture] = extraction.pictures.as_slice() else {
            panic!("one picture: {:?}", extraction.pictures);
        };
        assert!(picture.context.starts_with("four views of a CAD model"));
        assert!(picture.context.ends_with("63 × 20 × 30 mm"));
        let png = std::fs::read(&picture.path).unwrap();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        std::fs::remove_file(&picture.path).unwrap();
    }
}

#[test]
fn files_that_arent_models_fail_without_stopping_the_provider() {
    let cad = provider();
    if !drawing(&cad) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("broken.step");
    std::fs::write(&path, "ISO-10303-21;\nnot really\n").unwrap();
    let request = ExtractionRequest {
        path,
        media_type: "model/step".into(),
        context: None,
    };
    assert!(matches!(
        cad.extract("cad-model", &request),
        Err(ProviderError::Failed(_))
    ));
    assert!(
        cad.offers()
            .iter()
            .any(|o| o.capability == Capability::ExtractText)
    );
    // Still answering.
    let (good, _, _) = {
        let mut registry = Registry::new();
        registry.add(Box::new(cad));
        registry
            .extract(&ExtractionRequest {
                path: data("bracket.step"),
                media_type: "model/step".into(),
                context: None,
            })
            .unwrap()
    };
    for picture in &good.pictures {
        std::fs::remove_file(&picture.path).unwrap();
    }
}
