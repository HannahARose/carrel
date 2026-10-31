//! `carrel-provider-extract`: Carrel's portable text-extraction provider (`docs/providers.md`).
//!
//! It reads a PDF's text layer in Rust, on any platform, and recognizes text in images and
//! scanned PDFs with the `tesseract` program when it's installed (scanned PDFs also need
//! `pdftoppm`, from Poppler). On Apple devices the Apple provider's native tools come
//! first; this fills in everywhere else. It also reads what 3MF, G-code, STEP and IGES files
//! say about themselves, everywhere.

//!
//! The library holds the extractors, so other providers can reuse them: the CAD provider
//! reads STEP and IGES headers with [`models::read`].

pub mod extract;
pub mod models;
