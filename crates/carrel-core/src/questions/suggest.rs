//! Likely answers to a question, to pick from beside a free answer and Skip
//! (`docs/design/librarian.md`, "Questions"). Where items were heading says where a group
//! probably goes, without a model; what a term or an item probably is takes a text model, and
//! when there's none, the question is simply asked without suggestions.

use serde::Deserialize;
use serde_json::json;

use crate::providers::{Capability, CompletionRequest, Registry};
use crate::review::ReviewEntry;

use super::{QuestionEntry, QuestionKind, Suggestion};

/// The most suggestions a question offers.
pub const MAX_SUGGESTIONS: usize = 4;
/// Room for a model's suggestions, in tokens.
const ANSWER_TOKENS: u32 = 300;

/// Where `entries` were heading, most of them first, as suggestions: each collection with how
/// many were going there. Holding isn't suggested, since it's where items go undecided.
#[must_use]
pub fn destinations(entries: &[&ReviewEntry]) -> Vec<Suggestion> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for entry in entries {
        let path = &entry.destination.path;
        if path == "Holding" {
            continue;
        }
        match counts.iter_mut().find(|(p, _)| p == path) {
            Some((_, n)) => *n += 1,
            None => counts.push((path.clone(), 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1));
    counts
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(path, n)| {
            let why = if n == entries.len() && n > 1 {
                format!("where all {n} were going")
            } else if n == 1 {
                "where one was going".to_owned()
            } else {
                format!("where {n} of them were going")
            };
            Suggestion::new(path, Some(why))
        })
        .collect()
}

/// A new collection at `path`, as a suggestion: picking it creates the collection.
#[must_use]
pub fn new_collection(path: &str, why: impl Into<String>) -> Suggestion {
    Suggestion {
        new_collection: true,
        ..Suggestion::new(path, Some(why.into()))
    }
}

/// A suggestion that settles the question without changing anything.
#[must_use]
pub fn no_change(answer: &str, why: Option<String>) -> Suggestion {
    Suggestion {
        no_change: true,
        ..Suggestion::new(answer, why)
    }
}

/// The model's suggestions.
#[derive(Debug, Deserialize)]
struct Answers {
    #[serde(default)]
    answers: Vec<String>,
}

/// Likely answers to a term or item `question`, from the best text model in `registry` given
/// `context` (the items' names, titles and summaries, or the item's text): at most three, each
/// a short plain answer. Empty when there's no text model or none gives a usable answer, so
/// the question is asked without suggestions.
#[must_use]
pub fn with_model(registry: &Registry, question: &QuestionEntry, context: &str) -> Vec<Suggestion> {
    if !matches!(question.kind, QuestionKind::Term | QuestionKind::Item) {
        return Vec::new();
    }
    let schema = json!({
        "type": "object",
        "properties": {
            "answers": {
                "type": "array",
                "items": {"type": "string", "maxLength": 160},
                "maxItems": 3
            }
        },
        "required": ["answers"]
    });
    let request = CompletionRequest {
        system: Some(
            "You help the owner of a personal archive answer a question about their own \
             things. Offer up to three likely answers they could pick, each short and plain, \
             as they would say it. Use only what the context shows; when it shows nothing \
             useful, answer with an empty list."
                .into(),
        ),
        prompt: format!(
            "Question: {}\n\nWhat the archive knows:\n{}",
            question.question,
            context.chars().take(4_000).collect::<String>()
        ),
        schema: Some(schema),
        max_tokens: Some(ANSWER_TOKENS),
    };
    for (provider, offer) in registry.ranked(Capability::TextModel) {
        let Ok(completion) = provider.complete(&offer.id, &request) else {
            continue;
        };
        let Some(answers) = completion
            .json
            .and_then(|j| serde_json::from_value::<Answers>(j).ok())
        else {
            continue;
        };
        let mut suggestions: Vec<Suggestion> = Vec::new();
        for answer in answers.answers {
            let answer = answer.split_whitespace().collect::<Vec<_>>().join(" ");
            if answer.is_empty()
                || suggestions
                    .iter()
                    .any(|s| s.answer.eq_ignore_ascii_case(&answer))
            {
                continue;
            }
            suggestions.push(Suggestion::new(
                answer,
                Some(format!("a guess by {}", offer.id)),
            ));
        }
        suggestions.truncate(3);
        return suggestions;
    }
    Vec::new()
}
