//! `carrel-provider-extract`: Carrel's portable text-extraction provider. The extractors are
//! in the library ([`carrel_provider_extract`]).

use carrel_core::providers::serve::{About, serve};

fn main() {
    let provider = carrel_provider_extract::extract::Extractor::new();
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
// scratch
