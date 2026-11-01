//! Typed corrections (`docs/design/corrections.md`): the owner says in plain words what's wrong
//! with a card, narrowly ("it's from 2019") or broadly ("it's a picture of A, not B"), and a
//! text model turns that into field changes, each marked as dictated by the owner or inferred
//! by the model. Lines of `field: value` are applied without a model.
//!
//! This module works out the changes and applies them to a card; [`prepare`] holds a
//! correction until the owner's yes, then saves it, refiling and committing a filed item.
//! Showing it and asking are the caller's.

pub mod prepare;

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::card::{Card, DateOrTime, Dates, Provenance, Sensitivity, Tag};
use crate::ingest::card::{MAX_SUMMARY, MAX_TAGS, MAX_TITLE, Vocabulary};
use crate::providers::{Capability, CompletionRequest, Registry};
use crate::review::Attempt;
use crate::review::CardDraft;

/// How much of the item's text the model sees, in bytes.
const TEXT_BYTES: usize = 6_000;
/// Room for the model's answer, in tokens.
const ANSWER_TOKENS: u32 = 500;
/// The schema's word for a field left as it is.
const UNCHANGED: &str = "unchanged";

/// A field's new value, and whether the owner stated it outright.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field<T> {
    /// The new value.
    pub value: T,
    /// Whether the owner dictated it, rather than the model inferring it from what they said.
    /// Only dictated fields stop being model-written.
    pub dictated: bool,
}

/// Tags to add and remove.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagEdit {
    /// Tags to add, from the vocabulary.
    pub add: Vec<Tag>,
    /// Tags to remove.
    pub remove: Vec<Tag>,
    /// A new tag, not yet in the vocabulary, proposed as drafting proposes one.
    pub new: Option<Tag>,
}

/// What a correction changes. Fields left `None` stay as they are.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Changes {
    /// The title.
    pub title: Option<Field<String>>,
    /// The summary.
    pub summary: Option<Field<String>>,
    /// When the item was made.
    pub created: Option<Field<DateOrTime>>,
    /// The tags.
    pub tags: Option<Field<TagEdit>>,
    /// The sensitivity.
    pub sensitivity: Option<Field<Sensitivity>>,
    /// A collection the owner named, by path.
    pub collection: Option<Field<String>>,
    /// Whether what the item is changed enough that it should be routed again.
    #[serde(default)]
    pub reroute: bool,
    /// Something true beyond this item, to offer for the glossary.
    pub fact: Option<Fact>,
}

/// Something the owner said that's true beyond the item, offered for the glossary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact {
    /// The word or name it's about: the glossary entry's heading.
    pub about: String,
    /// What's true, as one sentence.
    pub fact: String,
}

impl Changes {
    /// Whether it changes nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.summary.is_none()
            && self.created.is_none()
            && self.tags.is_none()
            && self.sensitivity.is_none()
            && self.collection.is_none()
            && !self.reroute
            && self.fact.is_none()
    }

    /// The same changes, each marked as worked out by a model rather than dictated, as the
    /// librarian's are.
    #[must_use]
    pub fn inferred(mut self) -> Self {
        fn undictate<T>(field: &mut Option<Field<T>>) {
            if let Some(field) = field {
                field.dictated = false;
            }
        }
        undictate(&mut self.title);
        undictate(&mut self.summary);
        undictate(&mut self.created);
        undictate(&mut self.tags);
        undictate(&mut self.sensitivity);
        undictate(&mut self.collection);
        self
    }
}

/// The fields a correction can change, as a filed card and a proposal's draft both have them.
#[derive(Debug, Clone, PartialEq)]
pub struct Fields {
    /// The title.
    pub title: String,
    /// The summary.
    pub summary: Option<String>,
    /// The tags.
    pub tags: Vec<Tag>,
    /// The dates.
    pub dates: Option<Dates>,
    /// The sensitivity.
    pub sensitivity: Sensitivity,
    /// How program-written fields were produced, with the corrections kept.
    pub generated: Option<Provenance>,
}

/// A card or draft whose fields can be corrected.
pub trait Correctable {
    /// Its correctable fields.
    fn fields(&self) -> Fields;
    /// Replaces them.
    fn set_fields(&mut self, fields: Fields);
}

macro_rules! correctable {
    ($type:ty) => {
        impl Correctable for $type {
            fn fields(&self) -> Fields {
                Fields {
                    title: self.title.clone(),
                    summary: self.summary.clone(),
                    tags: self.tags.clone(),
                    dates: self.dates.clone(),
                    sensitivity: self.sensitivity,
                    generated: self.generated.clone(),
                }
            }

            fn set_fields(&mut self, fields: Fields) {
                self.title = fields.title;
                self.summary = fields.summary;
                self.tags = fields.tags;
                self.dates = fields.dates;
                self.sensitivity = fields.sensitivity;
                self.generated = fields.generated;
            }
        }
    };
}

correctable!(Card);
correctable!(CardDraft);

/// The card being corrected, as the model sees it.
#[derive(Debug, Clone)]
pub struct Subject<'a> {
    /// The card.
    pub card: &'a Fields,
    /// The collection it's in or proposed for, by path.
    pub collection: &'a str,
    /// The item's text, as read for drafting.
    pub text: &'a str,
    /// The archive's collections, by path, with what each is for.
    pub collections: &'a [(String, String)],
    /// The tags available.
    pub vocabulary: &'a Vocabulary,
}

/// A correction worked out: the changes, the model that made them if one did, and every
/// model call.
#[derive(Debug, Clone, Default)]
pub struct Worked {
    /// The changes.
    pub changes: Changes,
    /// The model, as `provider/offer`, when one turned the words into changes.
    pub model: Option<String>,
    /// Every model call made.
    pub attempts: Vec<Attempt>,
}

/// Why a correction couldn't be worked out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorrectionError {
    /// A `field: value` line names a field corrections can't set, or a value that isn't
    /// valid for it.
    BadLine(String),
    /// Plain words need a text model, and there's none.
    NoModel,
    /// No model gave usable changes.
    Unusable(Vec<String>),
}

impl std::fmt::Display for CorrectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadLine(why) => write!(f, "{why}"),
            Self::NoModel => write!(
                f,
                "a correction in words needs a text model; write `field: value` lines instead, \
                 such as `created: 2019-03-31`"
            ),
            Self::Unusable(problems) if problems.is_empty() => {
                write!(f, "no model could work out the correction")
            }
            Self::Unusable(problems) => {
                write!(f, "no model gave usable changes ({})", problems.join("; "))
            }
        }
    }
}

impl std::error::Error for CorrectionError {}

/// Works out what `words` change: directly when every line is `field: value`, otherwise
/// with the best text model, each with one retry, falling back to the next.
///
/// # Errors
///
/// [`CorrectionError`] when a field line is wrong, there's no model for plain words, or no
/// model gives usable changes.
pub fn work_out(
    registry: &Registry,
    subject: &Subject<'_>,
    words: &str,
    earlier: &[String],
) -> Result<Worked, CorrectionError> {
    if let Some(changes) = parse_lines(words, subject)? {
        return Ok(Worked {
            changes,
            model: None,
            attempts: Vec::new(),
        });
    }
    let ranked = registry.ranked(Capability::TextModel);
    if ranked.is_empty() {
        return Err(CorrectionError::NoModel);
    }
    let schema = schema(subject);
    let system = system_prompt();
    let prompt = prompt(subject, words, earlier);
    let mut worked = Worked::default();
    let mut last = Vec::new();
    for (provider, offer) in ranked {
        let mut feedback: Option<String> = None;
        for _ in 0..2 {
            let mut text = prompt.clone();
            if let Some(feedback) = &feedback {
                write!(
                    text,
                    "\n\nYour previous answer had these problems; fix them: {feedback}"
                )
                .unwrap();
            }
            let request = CompletionRequest {
                system: Some(system.clone()),
                prompt: text,
                schema: Some(schema.clone()),
                max_tokens: Some(ANSWER_TOKENS),
            };
            let attempt = |result: &str, message: Option<String>| Attempt {
                stage: "correction".into(),
                provider: provider.name().to_owned(),
                offer: offer.id.clone(),
                result: result.into(),
                message,
            };
            let completion = match provider.complete(&offer.id, &request) {
                Ok(c) => c,
                Err(error) => {
                    worked
                        .attempts
                        .push(attempt(error.result(), Some(error.to_string())));
                    break;
                }
            };
            let Some(answer) = completion
                .json
                .as_ref()
                .and_then(|j| serde_json::from_value::<Answer>(j.clone()).ok())
            else {
                worked.attempts.push(attempt(
                    "failed",
                    Some("the answer wasn't the schema".into()),
                ));
                feedback = Some("answer with a JSON object following the schema".into());
                continue;
            };
            match check(&answer, subject).map(|c| grounded(c, words, subject)) {
                Ok(changes) => {
                    worked.attempts.push(attempt("ok", None));
                    worked.changes = changes;
                    worked.model = Some(format!("{}/{}", provider.name(), offer.id));
                    return Ok(worked);
                }
                Err(problems) => {
                    worked
                        .attempts
                        .push(attempt("failed", Some(problems.join("; "))));
                    feedback = Some(problems.join("; "));
                    last = problems;
                }
            }
        }
    }
    Err(CorrectionError::Unusable(last))
}

/// The model's answer: every field present, with `unchanged` or an empty string for none. Its
/// flags mirror the schema, one per field.
#[derive(Debug, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // one flag per field, as the schema has
struct Answer {
    #[serde(default)]
    title: String,
    #[serde(default)]
    title_dictated: bool,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    summary_dictated: bool,
    #[serde(default)]
    created: String,
    #[serde(default)]
    created_dictated: bool,
    #[serde(default)]
    add_tags: Vec<String>,
    #[serde(default)]
    remove_tags: Vec<String>,
    #[serde(default)]
    new_tag: String,
    #[serde(default)]
    tags_dictated: bool,
    #[serde(default)]
    sensitivity: String,
    #[serde(default)]
    sensitivity_dictated: bool,
    #[serde(default)]
    collection: String,
    #[serde(default)]
    collection_dictated: bool,
    #[serde(default)]
    reroute: bool,
    #[serde(default)]
    fact: String,
    #[serde(default)]
    fact_about: String,
}

const DICTATED: &str = "true only if the owner stated this value outright; false if you worked it out from what they said";

/// The JSON Schema the model's answer follows. Every field is required, with `unchanged` or
/// an empty string for none: models constrained by a grammar otherwise stop early.
#[must_use]
pub fn schema(subject: &Subject<'_>) -> Value {
    let tags: Vec<&str> = subject
        .vocabulary
        .tags
        .iter()
        .map(|(t, _)| t.0.as_str())
        .collect();
    let mut collections = vec![UNCHANGED];
    collections.extend(subject.collections.iter().map(|(p, _)| p.as_str()));
    let flag = json!({"type": "boolean", "description": DICTATED});
    let tag_list = |what: &str| {
        if tags.is_empty() {
            json!({"type": "array", "items": {"type": "string"}, "maxItems": 0, "description": what})
        } else {
            json!({"type": "array", "items": {"enum": tags}, "maxItems": MAX_TAGS, "description": what})
        }
    };
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "title", "title_dictated", "summary", "summary_dictated", "created", "created_dictated",
            "add_tags", "remove_tags", "new_tag", "tags_dictated", "sensitivity",
            "sensitivity_dictated", "collection", "collection_dictated", "reroute", "fact",
            "fact_about"
        ],
        "properties": {
            "title": {"type": "string", "description": "The new title, or an empty string to keep it"},
            "title_dictated": flag,
            "summary": {"type": "string", "description": "The new summary, or an empty string to keep it"},
            "summary_dictated": flag,
            "created": {"type": "string", "description": "The new date it was made, as YYYY-MM-DD, or an empty string to keep it"},
            "created_dictated": flag,
            "add_tags": tag_list("Tags to add"),
            "remove_tags": tag_list("Tags to remove"),
            "new_tag": {"type": "string", "description": "Only if no tag fits: one new tag, lowercase, words joined by -; otherwise an empty string"},
            "tags_dictated": flag,
            "sensitivity": {"enum": [UNCHANGED, "public", "personal", "sensitive", "restricted"]},
            "sensitivity_dictated": flag,
            "collection": {"enum": collections, "description": "Only if the owner named where it belongs"},
            "collection_dictated": flag,
            "reroute": {"type": "boolean", "description": "true only if the correction says the item is a different kind of thing or about a different subject, so where it belongs should be chosen again; false for dates, names, details and small fixes"},
            "fact": {"type": "string", "description": "Something the owner said that is true beyond this item, such as who a name refers to, as one sentence; otherwise an empty string"},
            "fact_about": {"type": "string", "description": "The word or name that fact is about; an empty string when there's no fact"}
        }
    })
}

fn system_prompt() -> String {
    "You apply the owner's corrections to a catalog card in their personal archive. The owner \
     is right: change every field their correction affects, and nothing else. A broad correction \
     (what the item is or shows) usually changes the title and summary, and perhaps the tags. \
     Leave the tags alone unless the correction names a tag or changes what the item is. A \
     field is dictated only when the owner gave its exact value. A fact is only something about \
     a person, organisation, place, project or term that would help file other items, never \
     about this item. Keep the card's style: a short, specific title, and a summary of one to \
     three sentences saying what the item is. Never repeat account numbers, passwords or \
     identity numbers."
        .to_owned()
}

/// Lowercased words and numbers, for comparing what the owner said with a value.
fn words_of(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether the owner's words contain `value`, ignoring case and punctuation.
fn said(words: &str, value: &str) -> bool {
    let value = words_of(value);
    !value.is_empty() && format!(" {} ", words_of(words)).contains(&format!(" {value} "))
}

/// Holds the model's answer to what the owner actually said. A field counts as dictated only
/// when its value is in the owner's words (a date, when its year is); models mark inferred
/// fields as dictated too readily, and dictated fields stop being model-written for good. A
/// date the model worked out must appear in the item's text, and only a level the owner states
/// can lower the sensitivity. Tags change only when the owner talks about tags and names them.
/// A lasting fact must be about something the owner named.
fn grounded(mut changes: Changes, words: &str, subject: &Subject<'_>) -> Changes {
    if let Some(f) = &mut changes.title {
        f.dictated &= said(words, &f.value);
    }
    if let Some(f) = &mut changes.summary {
        f.dictated &= said(words, &f.value);
    }
    if let Some(f) = &mut changes.created {
        f.dictated &= said(words, &f.value.0[..4.min(f.value.0.len())]);
    }
    // A date the owner didn't give must come from the item: models invent plausible ones.
    if changes.created.as_ref().is_some_and(|f| {
        let day = &f.value.0[..10.min(f.value.0.len())];
        !f.dictated && !subject.text.contains(day)
    }) {
        changes.created = None;
    }
    if let Some(f) = &mut changes.sensitivity {
        f.dictated &= said(words, level(f.value));
    }
    // Only the owner's stated level can lower it; a model may still raise it.
    if changes
        .sensitivity
        .as_ref()
        .is_some_and(|f| !f.dictated && f.value < subject.card.sensitivity)
    {
        changes.sensitivity = None;
    }
    if let Some(f) = &mut changes.collection {
        let last = f.value.rsplit('/').next().unwrap_or(&f.value);
        f.dictated &= said(words, last);
    }
    if let Some(f) = &mut changes.tags {
        // Models fill tag lists given the chance, so tags are added only when the owner talks
        // about tags and names them. A broad correction may still drop tags that no longer fit.
        let about_tags = words_of(words).split(' ').any(|w| w.starts_with("tag"));
        let named = |t: &Tag| about_tags && said(words, &t.0.replace('-', " "));
        f.value.add.retain(|t| named(t));
        f.value.new = f.value.new.take().filter(|t| named(t));
        f.value.remove.retain(|t| named(t));
        // What's left was named as a tag by the owner, so it's theirs.
        f.dictated = true;
        if f.value == TagEdit::default() {
            changes.tags = None;
        }
    }
    changes.fact = changes.fact.take().filter(|fact| {
        let about = fact.about.to_lowercase();
        let first = words_of(&fact.fact);
        let first = first.split(' ').next().unwrap_or("");
        said(words, &fact.about)
            && !matches!(first, "this" | "these" | "it" | "its" | "the")
            && !["this item", "the item", "this file", "this document"]
                .iter()
                .any(|w| about.contains(w) || fact.fact.to_lowercase().contains(w))
    });
    changes
}

fn prompt(subject: &Subject<'_>, words: &str, earlier: &[String]) -> String {
    let card = subject.card;
    let mut text = String::from("The card now:\n");
    writeln!(text, "Title: {}", card.title).unwrap();
    writeln!(text, "Summary: {}", card.summary.as_deref().unwrap_or("")).unwrap();
    let tags: Vec<&str> = card.tags.iter().map(|t| t.0.as_str()).collect();
    writeln!(text, "Tags: {}", tags.join(", ")).unwrap();
    let created = card.dates.as_ref().and_then(|d| d.created.as_ref());
    writeln!(text, "Created: {}", created.map_or("", |c| c.0.as_str())).unwrap();
    writeln!(text, "Sensitivity: {}", level(card.sensitivity)).unwrap();
    writeln!(text, "Collection: {}", subject.collection).unwrap();
    if !subject.collections.is_empty() {
        text.push_str("\nCollections:\n");
        for (path, purpose) in subject.collections {
            writeln!(text, "- {path}: {purpose}").unwrap();
        }
    }
    if !subject.vocabulary.tags.is_empty() {
        text.push_str("\nTags:\n");
        for (tag, description) in &subject.vocabulary.tags {
            writeln!(text, "- {}: {description}", tag.0).unwrap();
        }
    }
    let excerpt = &subject.text[..subject.text.floor_char_boundary(TEXT_BYTES)];
    if !excerpt.trim().is_empty() {
        writeln!(text, "\nThe item's text:\n{excerpt}").unwrap();
    }
    if !earlier.is_empty() {
        text.push_str("\nThe owner corrected it before; these still hold:\n");
        for words in earlier {
            writeln!(text, "- {words}").unwrap();
        }
    }
    writeln!(text, "\nThe owner's correction: {words}").unwrap();
    text
}

/// Checks the model's answer against the card and archive.
fn check(answer: &Answer, subject: &Subject<'_>) -> Result<Changes, Vec<String>> {
    let mut problems = Vec::new();
    let mut changes = Changes {
        reroute: answer.reroute,
        ..Changes::default()
    };
    let title = answer.title.trim();
    if !title.is_empty() && title != subject.card.title {
        if title.chars().count() > MAX_TITLE {
            problems.push(format!("the title is longer than {MAX_TITLE} characters"));
        }
        changes.title = Some(Field {
            value: title.to_owned(),
            dictated: answer.title_dictated,
        });
    }
    let summary = answer.summary.trim();
    if !summary.is_empty() && Some(summary) != subject.card.summary.as_deref() {
        if summary.chars().count() > MAX_SUMMARY {
            problems.push(format!(
                "the summary is longer than {MAX_SUMMARY} characters"
            ));
        }
        let title = changes
            .title
            .as_ref()
            .map_or(subject.card.title.as_str(), |f| f.value.as_str());
        if summary
            .trim_end_matches('.')
            .eq_ignore_ascii_case(title.trim_end_matches('.'))
        {
            problems.push("the summary only repeats the title".to_owned());
        }
        changes.summary = Some(Field {
            value: summary.to_owned(),
            dictated: answer.summary_dictated,
        });
    }
    let created = answer.created.trim();
    if !created.is_empty() {
        let date = DateOrTime(created.to_owned());
        if date.is_valid() {
            changes.created = Some(Field {
                value: date,
                dictated: answer.created_dictated,
            });
        } else {
            problems.push(format!("{created} isn't a date as YYYY-MM-DD"));
        }
    }
    let edit = tag_edit(answer, subject.vocabulary, &subject.card.tags);
    if edit != TagEdit::default() {
        changes.tags = Some(Field {
            value: edit,
            dictated: answer.tags_dictated,
        });
    }
    if let Some(level) = parse_level(&answer.sensitivity)
        && level != subject.card.sensitivity
    {
        changes.sensitivity = Some(Field {
            value: level,
            dictated: answer.sensitivity_dictated,
        });
    }
    let collection = answer.collection.trim();
    if !collection.is_empty() && collection != UNCHANGED && collection != subject.collection {
        if subject.collections.iter().any(|(p, _)| p == collection) {
            changes.collection = Some(Field {
                value: collection.to_owned(),
                dictated: answer.collection_dictated,
            });
        } else {
            problems.push(format!("there's no collection {collection}"));
        }
    }
    let (fact, about) = (answer.fact.trim(), answer.fact_about.trim());
    changes.fact = (!fact.is_empty() && !about.is_empty()).then(|| Fact {
        about: about.to_owned(),
        fact: fact.to_owned(),
    });
    if problems.is_empty() && changes.is_empty() {
        problems
            .push("the answer changes nothing; change the fields the correction affects".into());
    }
    if problems.is_empty() {
        Ok(changes)
    } else {
        Err(problems)
    }
}

fn tag_edit(answer: &Answer, vocabulary: &Vocabulary, current: &[Tag]) -> TagEdit {
    let mut edit = TagEdit::default();
    for tag in &answer.add_tags {
        let tag = Tag(tag.trim().to_lowercase());
        if vocabulary.contains(&tag.0) && !current.contains(&tag) && !edit.add.contains(&tag) {
            edit.add.push(tag);
        }
    }
    for tag in &answer.remove_tags {
        let tag = Tag(tag.trim().to_lowercase());
        if current.contains(&tag) && !edit.remove.contains(&tag) {
            edit.remove.push(tag);
        }
    }
    let new = Tag(answer.new_tag.trim().to_lowercase());
    if !new.0.is_empty() {
        if vocabulary.contains(&new.0) {
            if !current.contains(&new) && !edit.add.contains(&new) {
                edit.add.push(new);
            }
        } else if new.is_well_formed() {
            edit.new = Some(new);
        }
    }
    edit
}

fn parse_level(word: &str) -> Option<Sensitivity> {
    match word.trim().to_lowercase().as_str() {
        "public" => Some(Sensitivity::Public),
        "personal" => Some(Sensitivity::Personal),
        "sensitive" => Some(Sensitivity::Sensitive),
        "restricted" => Some(Sensitivity::Restricted),
        _ => None,
    }
}

/// A level's name.
#[must_use]
pub fn level(level: Sensitivity) -> &'static str {
    match level {
        Sensitivity::Public => "public",
        Sensitivity::Personal => "personal",
        Sensitivity::Sensitive => "sensitive",
        Sensitivity::Restricted => "restricted",
    }
}

/// The changes `words` make when every non-blank line is `field: value`, all dictated; `None`
/// when they're plain words for a model.
///
/// Fields: `title`, `summary`, `created` (or `date`), `tags` (`+add -remove`, or bare names to
/// add), `sensitivity` and `collection`.
///
/// # Errors
///
/// [`CorrectionError::BadLine`] for a value that isn't valid for its field.
pub fn parse_lines(words: &str, subject: &Subject<'_>) -> Result<Option<Changes>, CorrectionError> {
    let lines: Vec<&str> = words
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let mut parsed = Vec::new();
    for line in &lines {
        let Some((field, value)) = line.split_once(':') else {
            return Ok(None);
        };
        let field = field.trim().to_lowercase();
        if !matches!(
            field.as_str(),
            "title" | "summary" | "created" | "date" | "tags" | "sensitivity" | "collection"
        ) {
            return Ok(None);
        }
        parsed.push((field, value.trim()));
    }
    if parsed.is_empty() {
        return Ok(None);
    }
    let bad = |why: String| CorrectionError::BadLine(why);
    let mut changes = Changes::default();
    for (field, value) in parsed {
        match field.as_str() {
            "title" if !value.is_empty() => changes.title = Some(dictated(value.to_owned())),
            "summary" if !value.is_empty() => changes.summary = Some(dictated(value.to_owned())),
            "created" | "date" => {
                let date = DateOrTime(value.to_owned());
                if !date.is_valid() {
                    return Err(bad(format!("{value} isn't a date; write it as YYYY-MM-DD")));
                }
                changes.created = Some(dictated(date));
            }
            "tags" => {
                let mut edit = TagEdit::default();
                for word in value.split([' ', ',']).filter(|w| !w.is_empty()) {
                    let (remove, name) = match word.strip_prefix('-') {
                        Some(name) => (true, name),
                        None => (false, word.strip_prefix('+').unwrap_or(word)),
                    };
                    let tag = Tag(name.to_lowercase());
                    if remove {
                        edit.remove.push(tag);
                    } else if subject.vocabulary.contains(&tag.0) {
                        edit.add.push(tag);
                    } else if tag.is_well_formed() {
                        edit.new = Some(tag);
                    } else {
                        return Err(bad(format!("{name} isn't a tag")));
                    }
                }
                changes.tags = Some(dictated(edit));
            }
            "sensitivity" => {
                let level = parse_level(value).ok_or_else(|| {
                    bad(format!(
                        "{value} isn't a level; use public, personal, sensitive or restricted"
                    ))
                })?;
                changes.sensitivity = Some(dictated(level));
            }
            "collection" => {
                let path = value.trim_matches('/');
                if !subject.collections.iter().any(|(p, _)| p == path) {
                    return Err(bad(format!("there's no collection {path}")));
                }
                changes.collection = Some(dictated(path.to_owned()));
            }
            _ => return Err(bad(format!("{field} needs a value"))),
        }
    }
    Ok(Some(changes))
}

/// A value the owner stated outright.
fn dictated<T>(value: T) -> Field<T> {
    Field {
        value,
        dictated: true,
    }
}

/// A correction as kept in a card's provenance: the owner's words and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    /// The owner's words.
    pub said: String,
    /// When.
    pub at: DateOrTime,
}

/// The corrections kept in `card`'s provenance, oldest first.
#[must_use]
pub fn notes(card: &Fields) -> Vec<Note> {
    card.generated
        .as_ref()
        .and_then(|g| g.extra.get("corrections"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Applies `worked` to `card`, said as `words` on `at`: the fields change, dictated fields
/// leave the `generated` provenance, inferred ones stay model-written with the model and
/// time, and the words are kept in the provenance for later model work. The collection is
/// the caller's, since moving is. Returns the names of the fields changed.
pub fn apply(
    card: &mut Fields,
    worked: &Worked,
    words: &str,
    at: &DateOrTime,
) -> Vec<&'static str> {
    let wanted = &worked.changes;
    let mut changed = Vec::new();
    let mut dictated = Vec::new();
    let mut inferred = Vec::new();
    let mut note = |name: &'static str, is_dictated: bool, provenance: &'static str| {
        changed.push(name);
        if is_dictated {
            dictated.push(provenance);
        } else {
            inferred.push(provenance);
        }
    };
    if let Some(f) = &wanted.title {
        card.title.clone_from(&f.value);
        note("title", f.dictated, "title");
    }
    if let Some(f) = &wanted.summary {
        card.summary = Some(f.value.clone());
        note("summary", f.dictated, "summary");
    }
    if let Some(f) = &wanted.created {
        card.dates.get_or_insert_with(Dates::default).created = Some(f.value.clone());
        note("created", f.dictated, "dates");
    }
    if let Some(f) = &wanted.tags {
        card.tags.retain(|t| !f.value.remove.contains(t));
        for tag in f.value.add.iter().chain(&f.value.new) {
            if !card.tags.contains(tag) {
                card.tags.push(tag.clone());
            }
        }
        note("tags", f.dictated, "tags");
    }
    if let Some(f) = &wanted.sensitivity {
        card.sensitivity = f.value;
        note("sensitivity", f.dictated, "sensitivity");
    }
    if let Some(f) = &wanted.collection {
        note("collection", f.dictated, "placement");
    }

    let mut notes = notes(card);
    notes.push(Note {
        said: words.trim().to_owned(),
        at: at.clone(),
    });
    let generated = card.generated.get_or_insert_with(|| Provenance {
        fields: Vec::new(),
        by: format!("carrel {}", crate::CARREL_VERSION),
        model: None,
        device: None,
        at: at.clone(),
        level: None,
        extra: crate::card::Extra::new(),
    });
    generated.fields.retain(|f| !dictated.contains(&f.as_str()));
    let model_wrote = inferred.iter().any(|f| *f != "placement");
    if let (true, Some(model)) = (model_wrote, &worked.model) {
        for field in inferred.iter().filter(|f| **f != "placement") {
            if !generated.fields.iter().any(|f| f == field) {
                generated.fields.push((*field).to_owned());
            }
        }
        generated.model = Some(model.clone());
        generated.at = at.clone();
        generated.by = format!("carrel {}", crate::CARREL_VERSION);
    }
    generated.extra.insert(
        "corrections".into(),
        serde_json::to_value(&notes).unwrap_or(Value::Null),
    );
    changed
}
