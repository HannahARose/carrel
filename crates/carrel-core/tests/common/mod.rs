//! Helpers shared by the ingest tests.

#![allow(dead_code)]

pub mod archives;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use carrel_core::providers::{
    Capability, Completion, CompletionRequest, Offer, Provider, ProviderError,
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
