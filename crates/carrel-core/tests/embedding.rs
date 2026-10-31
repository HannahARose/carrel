//! Embedding through providers (`docs/design/search.md`): only the archive's model is ever
//! used, and each answer is checked before it's trusted.

mod common;

use carrel_core::providers::{EmbeddingRequest, ProviderError, Purpose, Registry, priority};
use common::ScriptedEmbed;

fn request(texts: &[&str]) -> EmbeddingRequest {
    EmbeddingRequest {
        texts: texts.iter().map(|t| (*t).to_owned()).collect(),
        purpose: Purpose::Document,
    }
}

#[test]
fn texts_are_embedded_with_the_archives_model_only() {
    let mut registry = Registry::with_built_ins();
    let gemma = ScriptedEmbed::new("llama", "embeddinggemma-300m", priority::PORTABLE, 768);
    let gemma_asked = gemma.asked.clone();
    registry.add(Box::new(gemma));
    // A better-ranked provider with another model is never used for this archive.
    let other = ScriptedEmbed::new("apple", "apple-sentence", priority::NATIVE, 8);
    let other_asked = other.asked.clone();
    registry.add(Box::new(other));

    let (embeddings, source, attempts) = registry
        .embed("embeddinggemma-300m", &request(&["a lease", "a receipt"]))
        .unwrap();
    assert_eq!(embeddings.model, "embeddinggemma-300m");
    assert_eq!(embeddings.dimensions, 768);
    assert_eq!(embeddings.vectors.len(), 2);
    assert_eq!(source.provider, "llama");
    assert_eq!(attempts.len(), 0);
    assert_eq!(gemma_asked.lock().unwrap().len(), 1);
    assert_eq!(other_asked.lock().unwrap().len(), 0);
}

#[test]
fn with_no_provider_of_the_model_nothing_else_stands_in() {
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(ScriptedEmbed::new(
        "llama",
        "qwen3-embedding-0.6b",
        priority::PORTABLE,
        4,
    )));
    let Err(ProviderError::Unavailable(why)) =
        registry.embed("embeddinggemma-300m", &request(&["x"]))
    else {
        panic!("expected unavailable");
    };
    assert!(why.contains("embeddinggemma-300m"), "{why}");
}

#[test]
fn another_provider_of_the_same_model_is_tried_when_one_fails() {
    let mut registry = Registry::new();
    registry.add(Box::new(ScriptedEmbed::new(
        "llama",
        "embeddinggemma-300m",
        priority::PORTABLE,
        768,
    )));
    let mut mlx = ScriptedEmbed::new("mlx", "embeddinggemma-300m", priority::NATIVE, 768);
    mlx.failure = Some(ProviderError::Failed("out of memory".into()));
    registry.add(Box::new(mlx));

    let (_, source, attempts) = registry
        .embed("embeddinggemma-300m", &request(&["x"]))
        .unwrap();
    assert_eq!(source.provider, "llama");
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].source.provider, "mlx");
}

#[test]
fn answers_from_another_model_or_of_the_wrong_shape_are_refused() {
    let mut liar = ScriptedEmbed::new("liar", "embeddinggemma-300m", priority::PORTABLE, 4);
    liar.answers_as = Some("something-else".into());
    let mut registry = Registry::new();
    registry.add(Box::new(liar));
    let Err(ProviderError::Failed(why)) = registry.embed("embeddinggemma-300m", &request(&["x"]))
    else {
        panic!("expected a failure");
    };
    assert!(why.contains("something-else"), "{why}");

    let embeddings = carrel_core::providers::Embeddings {
        vectors: vec![vec![1.0, 0.0], vec![1.0]],
        model: "m".into(),
        dimensions: 2,
    };
    assert!(embeddings.check("m", &request(&["a", "b"])).is_err());
    assert!(embeddings.check("m", &request(&["a"])).is_err());
    let right = carrel_core::providers::Embeddings {
        vectors: vec![vec![1.0, 0.0]],
        model: "m".into(),
        dimensions: 2,
    };
    assert!(right.check("m", &request(&["a"])).is_ok());
    // A catalog model's vectors have the catalog's dimensions.
    let short = carrel_core::providers::Embeddings {
        model: "embeddinggemma-300m".into(),
        ..right
    };
    assert!(
        short
            .check("embeddinggemma-300m", &request(&["a"]))
            .is_err()
    );
}

#[test]
fn requests_and_answers_use_the_protocols_shape() {
    let query = EmbeddingRequest {
        texts: vec!["the lease".into()],
        purpose: Purpose::Query,
    };
    assert_eq!(
        serde_json::to_value(&query).unwrap(),
        serde_json::json!({"texts": ["the lease"], "purpose": "query"})
    );
    let default: EmbeddingRequest =
        serde_json::from_value(serde_json::json!({"texts": ["x"]})).unwrap();
    assert_eq!(default.purpose, Purpose::Document);
}
