//! The extraction provider as a real program, through Carrel's provider registry.
//!
//! OCR tests need `tesseract` (and `pdftoppm` for scanned PDFs). They're skipped when it isn't
//! installed, unless `CARREL_REQUIRE_TESSERACT` is set, as in CI.

use std::path::{Path, PathBuf};

use carrel_core::providers::process::{Found, ProcessProvider, default_folders, find_program};
use carrel_core::providers::{ExtractionRequest, ProviderError, Registry};

fn registry() -> Registry {
    let found = Found {
        name: "extract".into(),
        path: PathBuf::from(env!("CARGO_BIN_EXE_carrel-provider-extract")),
    };
    let mut registry = Registry::new();
    registry.add(Box::new(ProcessProvider::start(&found).unwrap()));
    registry
}

fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(name)
        .canonicalize()
        .unwrap()
}

fn extract(
    path: PathBuf,
    media_type: &str,
) -> Result<(String, String, Vec<String>), ProviderError> {
    let (extraction, source, attempts) = registry().extract(&ExtractionRequest {
        path,
        media_type: media_type.into(),
        context: None,
    })?;
    Ok((
        extraction.text,
        format!("{}/{}", source.offer, extraction.method),
        attempts.iter().map(|a| a.source.offer.clone()).collect(),
    ))
}

/// Whether OCR can run here; fails the test run if it must and can't.
fn have(program: &str) -> bool {
    let found = find_program(program, &default_folders()).is_some();
    assert!(
        found || std::env::var_os("CARREL_REQUIRE_TESSERACT").is_none(),
        "{program} is required but isn't installed"
    );
    found
}

#[test]
fn a_pdfs_text_layer_is_read() {
    let (text, how, attempts) = extract(
        data("../../fixtures/sample-archive/Reading/Coupled oscillators note.pdf"),
        "application/pdf",
    )
    .unwrap();
    assert_eq!(how, "pdf-text/embedded");
    assert_eq!(attempts, Vec::<String>::new());
    assert!(text.to_lowercase().contains("oscillator"), "{text}");
}

#[test]
fn images_are_recognized_with_tesseract() {
    if !have("tesseract") {
        return;
    }
    let (text, how, _) = extract(data("tests/data/scanned.png"), "image/png").unwrap();
    assert_eq!(how, "tesseract/ocr");
    assert!(text.contains("Scanned receipt 2026"), "{text}");
}

#[test]
fn a_scanned_pdf_falls_back_from_its_missing_text_layer_to_ocr() {
    let scanned = data("../carrel-core/tests/data/scanned.pdf");
    if !have("tesseract") || !have("pdftoppm") {
        let error = extract(scanned, "application/pdf").unwrap_err();
        assert!(error.to_string().contains("OCR"), "{error}");
        return;
    }
    let (text, how, attempts) = extract(scanned, "application/pdf").unwrap();
    assert_eq!(how, "tesseract/ocr");
    assert_eq!(attempts, ["pdf-text"]);
    assert!(text.contains("Scanned receipt 2026"), "{text}");
}

#[test]
fn a_step_files_header_and_parts_are_read_with_its_date() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample holder.step");
    std::fs::write(
        &path,
        "ISO-10303-21;\nHEADER;\nFILE_NAME('sample holder','2026-10-11T14:22:05',(''),(''),'Onshape','Onshape','');\nENDSEC;\nDATA;\n#10=PRODUCT('Rear Body','Rear Body','',(#5));\nENDSEC;\n",
    )
    .unwrap();
    let request = ExtractionRequest {
        path: path.canonicalize().unwrap(),
        media_type: carrel_core::ingest::text::media_type(&path),
        context: None,
    };
    let (extraction, source, _) = registry().extract(&request).unwrap();
    assert_eq!(source.offer, "model-details");
    assert_eq!(extraction.method, "embedded");
    assert_eq!(extraction.created.as_deref(), Some("2026-10-11"));
    assert!(
        extraction.text.ends_with("Parts: Rear Body"),
        "{}",
        extraction.text
    );
}
