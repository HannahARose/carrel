//! `carrel-provider-llama`: Carrel's portable text-model provider (`docs/providers.md`).
//!
//! It runs open models locally through llama.cpp, on any platform: with Metal on Macs, and
//! optionally Vulkan or CUDA elsewhere. Models are GGUF files in Carrel's models folder
//! ([`carrel_core::local::models_dir`]); each one is offered as a text model. Answers that
//! must follow a JSON Schema are constrained with a grammar, so they're always valid JSON.

mod llama;

use carrel_core::providers::serve::{About, serve};

fn main() {
    let provider = llama::LlamaProvider::new(carrel_core::local::models_dir().as_deref());
    let about = About {
        label: "llama.cpp",
        version: env!("CARGO_PKG_VERSION"),
    };
    if let Err(e) = serve(
        &provider,
        about,
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    ) {
        eprintln!("carrel-provider-llama: {e}");
        std::process::exit(1);
    }
}
