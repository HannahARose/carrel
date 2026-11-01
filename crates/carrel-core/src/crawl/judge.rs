//! What the crawler's model is told about one item, the JSON form it answers in, and reading
//! that answer: whether the item fits its collection's design document, where it would go
//! instead, and how its card could be better.

use std::fmt::Write as _;

use serde_json::{Value, json};

use crate::card::Card;

/// The most of an item's text the model reads.
const TEXT_CHARS: usize = 3_000;
/// The most of a design document the model reads.
const DESIGN_CHARS: usize = 2_000;
/// How many collections the prompt lists.
const PLACES_LISTED: usize = 80;
/// How many missing tags the model may suggest.
const MAX_TAGS: usize = crate::ingest::card::MAX_TAGS;
/// How sure the model must be that an item doesn't fit for it to count as misplaced: a model
/// less sure than this is taken to say it fits, since a move it isn't sure of costs the owner
/// more than an item left where it is.
pub const MISFIT_CONFIDENCE: f64 = 0.5;

/// A collection the item could go to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// Its path.
    pub path: String,
    /// What it's for: its description, else its title.
    pub purpose: String,
}

/// What the model is shown of the item.
#[derive(Debug, Clone, Copy)]
pub struct Item<'a> {
    /// Its card.
    pub card: &'a Card,
    /// The collection it's in, by path.
    pub collection: &'a str,
    /// That collection's design document, without its frontmatter.
    pub design: &'a str,
    /// Its text, as search read it, if any.
    pub text: Option<&'a str>,
}

/// What the model concluded about an item.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Judgment {
    /// Whether it fits where it is.
    pub fits: bool,
    /// Where it would go instead, by path, when it doesn't fit.
    pub better: Option<String>,
    /// How sure the model is, from 0 to 1.
    pub confidence: f64,
    /// Why, in a sentence.
    pub reason: String,
    /// A better title, if the card's could be clearer.
    pub title: Option<String>,
    /// A better summary, if the card's could be better.
    pub summary: Option<String>,
    /// The date it was made, as `YYYY-MM-DD`, if the text gives one the card doesn't have.
    pub created: Option<String>,
    /// Tags it's missing.
    pub tags: Vec<String>,
}

/// The crawler's instructions.
#[must_use]
pub fn system() -> String {
    "You keep a personal archive in order. Each item is described by a card and filed in a collection, whose design document says what belongs in it. You check one item at a time: which collection it belongs in, and whether its card could be better.\n\n\
     - Name the collection the item belongs in, from the list, exactly as written. The collection it's in now is on the list: name it when it's a reasonable place for the item, even if another would do as well. Name another only when the item plainly belongs there instead. Leave it empty only when no collection on the list is right.\n\
     - Judge the item by what it is, against each collection's purpose, not by which collection's words its card happens to share.\n\
     - Then read the card again. Suggest a clearer title, a better summary of at most two sentences, the date it was made when the text gives one the card lacks, and up to five tags it's missing. Leave a field empty when the card's is fine, as it usually is. Only suggest what the text and card show; don't guess.\n\
     - Give how sure you are of the collection you named, from 0 to 1, and why, in one sentence of at most 20 words.\n"
        .to_owned()
}

/// The prompt for `item`, with the collections it could go to.
#[must_use]
pub fn prompt(item: &Item<'_>, places: &[Place]) -> String {
    let card = item.card;
    let mut text = format!("The item is in the collection {}.\n", item.collection);
    let design = shorten(item.design.trim(), DESIGN_CHARS);
    if design.is_empty() {
        text.push_str("Its design document says nothing more than its name.\n");
    } else {
        writeln!(text, "Its design document:\n{design}").unwrap();
    }
    text.push_str("\nThe collections:\n");
    for place in places.iter().take(PLACES_LISTED) {
        writeln!(text, "- {}: {}", place.path, place.purpose).unwrap();
    }
    text.push_str("\nThe card:\n");
    writeln!(text, "Title: {}", card.title).unwrap();
    if let Some(item) = &card.item {
        writeln!(text, "File: {item}").unwrap();
    }
    writeln!(
        text,
        "Summary: {}",
        card.summary.as_deref().unwrap_or("(none)")
    )
    .unwrap();
    let tags: Vec<&str> = card.tags.iter().map(|t| t.0.as_str()).collect();
    writeln!(
        text,
        "Tags: {}",
        if tags.is_empty() {
            "(none)".to_owned()
        } else {
            tags.join(", ")
        }
    )
    .unwrap();
    let created = card
        .dates
        .as_ref()
        .and_then(|d| d.created.as_ref())
        .map(|d| d.0.clone());
    writeln!(
        text,
        "Created: {}",
        created.as_deref().unwrap_or("(unknown)")
    )
    .unwrap();
    match item.text.map(str::trim).filter(|t| !t.is_empty()) {
        Some(body) => writeln!(text, "\nIts text:\n{}", shorten(body, TEXT_CHARS)).unwrap(),
        None => text.push_str("\nNo text could be read from it.\n"),
    }
    // The question last, naming where it is, so the model judges that collection and not
    // whichever it read about last.
    writeln!(
        text,
        "\nWhich collection does this item belong in? It's in {} now.",
        item.collection
    )
    .unwrap();
    text
}

/// `text` cut to at most `chars` characters, marked when cut.
fn shorten(text: &str, chars: usize) -> String {
    let mut cut: String = text.chars().take(chars).collect();
    if cut.len() < text.len() {
        cut.push_str(" …");
    }
    cut
}

/// The JSON Schema the model answers to: the collection the item belongs in, one of `paths`
/// (its own among them) or empty for none, first, so the verdict comes before anything long.
/// It's asked where the item belongs rather than whether it fits, since small models judged
/// "fits" against whichever collection they had in mind. Every field is required, with an
/// empty value for none, since models constrained by a grammar otherwise stop early; the tags
/// are capped, so a small model can't list them until it runs out of room.
#[must_use]
pub fn schema(paths: &[String]) -> Value {
    let mut collections: Vec<&str> = vec![""];
    collections.extend(paths.iter().map(String::as_str));
    json!({
        "type": "object",
        "properties": {
            "collection": {"type": "string", "enum": collections, "description": "The collection it belongs in: the one it's in now when that's a reasonable place; empty only when none is right"},
            "confidence": {"type": "number", "description": "How sure you are of that collection, from 0 to 1"},
            "reason": {"type": "string", "description": "Why, in one sentence of at most 20 words"},
            "title": {"type": "string", "description": "A clearer title; empty when the card's is fine"},
            "summary": {"type": "string", "description": "A better summary; empty when the card's is fine"},
            "created": {"type": "string", "description": "When it was made, YYYY-MM-DD, if the text says and the card doesn't; else empty"},
            "tags": {"type": "array", "items": {"type": "string"}, "maxItems": MAX_TAGS, "description": "Tags it's missing; usually empty"}
        },
        "required": ["collection", "confidence", "reason", "title", "summary", "created", "tags"]
    })
}

/// The model's answer, read, against the card it's about and the collection it's in. The item
/// fits when the model names the collection it's in; an answer without a collection may say
/// `fits` instead, as the crawler's first answers did. `None` when it's in neither form.
/// Suggestions that change nothing are dropped, and a misfit it's less sure of than
/// [`MISFIT_CONFIDENCE`] counts as fitting.
#[must_use]
pub fn read(answer: &Value, card: &Card, collection: &str) -> Option<Judgment> {
    // Its white space run together: a suggestion is one line, whatever the model wrote.
    let text = |key: &str| {
        answer
            .get(key)
            .and_then(Value::as_str)
            .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|t| !t.is_empty())
    };
    let named = answer.get("collection").and_then(Value::as_str);
    let said_fits = answer.get("fits").and_then(Value::as_bool);
    let fits = match (named.map(str::trim).filter(|c| !c.is_empty()), said_fits) {
        (Some(named), _) => named.eq_ignore_ascii_case(collection),
        (None, Some(fits)) => fits,
        // A collection left empty: nowhere on the list is right.
        (None, None) => {
            named?;
            false
        }
    };
    let stated = answer
        .get("confidence")
        .and_then(Value::as_f64)
        .filter(|c| c.is_finite())
        .map(|c| c.clamp(0.0, 1.0));
    let confidence = stated.unwrap_or(0.0);
    let better = text("collection").filter(|c| !c.eq_ignore_ascii_case(collection));
    let created = text("created").filter(|c| {
        crate::card::DateOrTime(c.clone()).is_valid()
            && card
                .dates
                .as_ref()
                .and_then(|d| d.created.as_ref())
                .is_none_or(|have| have.0 != *c)
    });
    let have: Vec<&str> = card.tags.iter().map(|t| t.0.as_str()).collect();
    let mut tags: Vec<String> = answer
        .get("tags")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(|t| t.trim().to_lowercase())
                .collect()
        })
        .unwrap_or_default();
    tags.retain(|t| !t.is_empty() && !have.contains(&t.as_str()));
    tags.sort();
    tags.dedup();
    // A misfit the model isn't sure of counts as fitting. Nothing is meant to stay in
    // Holding, whatever the model says.
    let unsure = stated.is_some_and(|c| c < MISFIT_CONFIDENCE);
    let fits = (fits || unsure) && collection != "Holding";
    Some(Judgment {
        fits,
        // An item that doesn't fit but names nowhere better still doesn't fit.
        better: if fits { None } else { better },
        confidence,
        reason: text("reason").unwrap_or_default(),
        title: text("title").filter(|t| *t != card.title),
        summary: text("summary").filter(|s| card.summary.as_deref() != Some(s.as_str())),
        created,
        tags,
    })
}
