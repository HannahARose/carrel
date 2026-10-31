//! `carrel-provider-cad`: draws STEP and IGES models for Carrel (`docs/design/models-3d.md`).
//!
//! It reads a model through `OpenCascade`, meshes it coarsely, and draws four views of it with a
//! small software renderer, so it needs no GPU and draws the same everywhere. The views go
//! back as a picture for an image reader to describe; the text has the file's header and
//! parts, its size, solids and volume. It builds without `OpenCascade` too, and then offers
//! drawing as unavailable, with how to install it.

mod cad;
mod occt;
mod png;
mod render;

use carrel_core::providers::serve::{About, serve};

fn main() {
    let provider = cad::Cad::new();
    let about = About {
        label: "CAD models",
        version: env!("CARGO_PKG_VERSION"),
    };
    if let Err(e) = serve(
        &provider,
        about,
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    ) {
        eprintln!("carrel-provider-cad: {e}");
        std::process::exit(1);
    }
}
