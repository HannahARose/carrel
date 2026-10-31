//! `carrel-provider-extract`: Carrel's portable text-extraction provider (`docs/providers.md`).
//!
//! It reads a PDF's text layer in Rust, on any platform, and recognizes text in images and
//! scanned PDFs with the `tesseract` program when it's installed (scanned PDFs also need
//! `pdftoppm`, from Poppler). On Apple devices the Apple provider's native tools come
//! first; this fills in everywhere else. It also reads what 3MF, G-code, STEP and IGES files
//! say about themselves, everywhere.

mod extract;
mod models;

use carrel_core::providers::serve::{About, serve};

fn main() {
    let provider = extract::Extractor::new();
    let about = About {
        label: "Portable extraction",
        version: env!("CARGO_PKG_VERSION"),
    };
    if let Err(e) = serve(
        &provider,
        about,
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    ) {
        eprintln!("carrel-provider-extract: {e}");
        std::process::exit(1);
    }
}
