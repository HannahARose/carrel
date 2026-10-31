//! Helpers shared by the ingest tests.

#![allow(dead_code)]

pub mod archives;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use carrel_core::providers::{
    Capability, Completion, CompletionRequest, EmbeddingRequest, Embeddings, Offer, Provider,
    ProviderError,
};
use serde_json::json;

/// A model that gives queued answers and remembers what it was asked.
pub struct Scripted {
    pub name: &'static str,
    pub offers: Vec<Offer>,
    pub answers: Mutex<VecDeque<Result<serde_json::Value, ProviderError>>>,
    pub asked: Arc<Mutex<Vec<CompletionRequest>>>,
}

impl Scripted {
    pub fn new(
        name: &'static str,
        priority: i32,
        context: u64,
        answers: Vec<Result<serde_json::Value, ProviderError>>,
    ) -> Self {
        let mut extra = serde_json::Map::new();
        extra.insert("context_tokens".into(), json!(context));
        Self {
            name,
            offers: vec![Offer {
                capability: Capability::TextModel,
                id: "m".into(),
                label: None,
                priority,
                available: true,
                reason: None,
                media_types: Vec::new(),
                extra,
            }],
            answers: Mutex::new(answers.into()),
            asked: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Provider for Scripted {
    fn name(&self) -> &str {
        self.name
    }
    fn offers(&self) -> &[Offer] {
        &self.offers
    }
    fn complete(
        &self,
        _offer: &str,
        request: &CompletionRequest,
    ) -> Result<Completion, ProviderError> {
        self.asked.lock().unwrap().push(request.clone());
        let answer = self
            .answers
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Err(ProviderError::Failed("out of answers".into())))?;
        Ok(Completion {
            text: answer.to_string(),
            json: Some(answer),
            model: format!("{}/m", self.name),
            usage: None,
        })
    }
}

/// An embedding model that answers with vectors made from each text's length, or with a
/// scripted failure, and remembers what it was asked.
pub struct ScriptedEmbed {
    pub name: &'static str,
    pub offers: Vec<Offer>,
    /// The model identifier it says the vectors are from; normally the offer's.
    pub answers_as: Option<String>,
    /// How many numbers its vectors have.
    pub dimensions: usize,
    pub failure: Option<ProviderError>,
    pub asked: Arc<Mutex<Vec<(String, EmbeddingRequest)>>>,
}

impl ScriptedEmbed {
    /// A provider called `name` offering the embedding model `model`.
    pub fn new(name: &'static str, model: &str, priority: i32, dimensions: usize) -> Self {
        let mut extra = serde_json::Map::new();
        extra.insert("dimensions".into(), json!(dimensions));
        Self {
            name,
            offers: vec![Offer {
                capability: Capability::Embed,
                id: model.into(),
                label: None,
                priority,
                available: true,
                reason: None,
                media_types: Vec::new(),
                extra,
            }],
            answers_as: None,
            dimensions,
            failure: None,
            asked: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Provider for ScriptedEmbed {
    fn name(&self) -> &str {
        self.name
    }
    fn offers(&self) -> &[Offer] {
        &self.offers
    }
    fn embed(&self, offer: &str, request: &EmbeddingRequest) -> Result<Embeddings, ProviderError> {
        self.asked
            .lock()
            .unwrap()
            .push((offer.to_owned(), request.clone()));
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        let vectors = request
            .texts
            .iter()
            .map(|t| {
                let mut v = vec![0.0; self.dimensions];
                v[t.len() % self.dimensions] = 1.0;
                v
            })
            .collect();
        Ok(Embeddings {
            vectors,
            model: self.answers_as.clone().unwrap_or_else(|| offer.to_owned()),
            dimensions: self.dimensions,
        })
    }
}
