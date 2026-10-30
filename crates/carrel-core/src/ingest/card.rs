//! Stage 7: drafting an item's card with a model, and checking the draft
//! (`docs/design/ingest.md`).

use std::fmt::Write as _;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::archive::{Archive, Collection};
use crate::card::{DateOrTime, Id, Kind, Provenance, QualityLevel, Tag};
use crate::providers::{Capability, CompletionRequest, Offer, Provider, Registry, estimate_tokens};
use crate::review::Attempt;

use super::Candidate;
use super::text::{ItemText, fit};

/// Room left in the context for the model's answer, in tokens.
pub const ANSWER_TOKENS: u32 = 400;

/// The context assumed for a model that doesn't report its size.
const DEFAULT_CONTEXT: u64 = 4096;

/// The longest title a card gets, in characters.
pub const MAX_TITLE: usize = 120;

/// The longest summary, in characters.
pub const MAX_SUMMARY: usize = 600;

/// The problem raised for a file whose title is just its machine-made name, such as `IMG_2041`.
/// Worth one retry, but accepted on the last attempt: a card titled with the file name beats no
/// card. A name a person chose, such as `amplifier build log`, is often already the right title.
const REPEATS_NAME: &str = "the title only repeats the file name; say what the item is";

/// The most tags a draft may have.
pub const MAX_TAGS: usize = 5;

/// How much of the glossary goes into a prompt, in bytes.
const GLOSSARY_BUDGET: usize = 2000;

/// Facts the owner has taught Carrel (`_System/glossary.md`, FORMAT.md §7.3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Glossary {
    /// Each fact as its term and text, newest first.
    pub facts: Vec<(String, String)>,
}

impl Glossary {
    /// Reads the facts: each level-3 heading is a term, and the text below it (without the
    /// `<!-- id … -->` comment) is the fact.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut facts: Vec<(String, String)> = Vec::new();
        for line in text.lines() {
            if let Some(term) = line.strip_prefix("### ") {
                facts.push((term.trim().to_owned(), String::new()));
            } else if let Some((_, fact)) = facts.last_mut()
                && !line.trim_start().starts_with("<!--")
                && !line.trim().is_empty()
            {
                if !fact.is_empty() {
                    fact.push(' ');
                }
                fact.push_str(line.trim());
            }
        }
        facts.retain(|(_, fact)| !fact.is_empty());
        Self { facts }
    }

    /// The archive's glossary; empty if it has none.
    #[must_use]
    pub fn of(archive: &Archive) -> Self {
        std::fs::read_to_string(archive.root().join("_System/glossary.md"))
            .map(|text| Self::parse(&text))
            .unwrap_or_default()
    }
}

/// The tags an item may get, with what each means.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Vocabulary {
    /// Each tag and its description, sorted by tag.
    pub tags: Vec<(Tag, String)>,
}

impl Vocabulary {
    /// Every tag defined in `ARCHIVE.md` and any collection. Until the item has a destination,
    /// all of them are candidates; routing narrows them to those that apply there (§6.5).
    #[must_use]
    pub fn of(archive: &Archive, collections: &[Collection]) -> Self {
        let mut tags: Vec<(Tag, String)> = archive
            .doc()
            .tags
            .iter()
            .chain(collections.iter().flat_map(|c| c.doc.tags.iter()))
            .map(|t| (t.name.clone(), t.description.clone()))
            .collect();
        tags.sort_by(|a, b| a.0.0.cmp(&b.0.0));
        tags.dedup_by(|a, b| a.0 == b.0);
        Self { tags }
    }

    /// Whether `tag` is in the vocabulary.
    #[must_use]
    pub fn contains(&self, tag: &str) -> bool {
        self.tags.iter().any(|(t, _)| t.0 == tag)
    }
}

/// What a draft is written with besides the item.
#[derive(Debug, Clone)]
pub struct Context<'a> {
    /// The archive's name.
    pub archive_name: &'a str,
    /// The tags available.
    pub vocabulary: &'a Vocabulary,
    /// What the owner has taught.
    pub glossary: &'a Glossary,
    /// The device drafting, for provenance.
    pub device: Id,
}

/// A drafted card, or as much of one as could be settled.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    /// The title.
    pub title: String,
    /// The summary.
    pub summary: Option<String>,
    /// Tags from the vocabulary.
    pub tags: Vec<Tag>,
    /// A new tag the model proposed, when none fitted.
    pub new_tag: Option<Tag>,
    /// When the item was made, if the text says.
    pub created: Option<DateOrTime>,
    /// How the fields were produced, when a model produced them.
    pub generated: Option<Provenance>,
    /// Every model call made.
    pub attempts: Vec<Attempt>,
    /// What's left for the owner to settle; empty when the draft is complete.
    pub problems: Vec<String>,
}

/// A model's answer, before checking.
#[derive(Debug, Deserialize)]
struct Answer {
    #[serde(default)]
    title: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    new_tag: Option<String>,
    #[serde(default)]
    created: Option<String>,
}

/// Words in file names that cameras, scanners, browsers and programs make up, which say nothing
/// about what a file is.
const GENERIC_WORDS: [&str; 31] = [
    "attachment",
    "copy",
    "dcim",
    "doc",
    "document",
    "download",
    "downloads",
    "draft",
    "dsc",
    "dscn",
    "export",
    "file",
    "final",
    "image",
    "img",
    "mov",
    "mvimg",
    "new",
    "output",
    "page",
    "photo",
    "pic",
    "picture",
    "pxl",
    "scan",
    "scanned",
    "screen",
    "screenshot",
    "shot",
    "unnamed",
    "untitled",
];

/// Whether a file's name (without its extension) was made by a device or program rather than
/// chosen by a person: nothing in it but numbers, dates, short fragments and generic words, as
/// in `IMG_2041`, `scan0003`, `Screenshot 2025-01-01 at 10.21.33` or `document (3)`.
#[must_use]
pub fn machine_made(stem: &str) -> bool {
    let mut words = Vec::new();
    let mut word = String::new();
    for c in stem.chars() {
        if c.is_alphabetic() {
            word.extend(c.to_lowercase());
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    !words
        .iter()
        .any(|w| w.chars().count() >= 3 && !GENERIC_WORDS.contains(&w.as_str()))
}

/// The JSON Schema the model's answer must follow. Tags are an enum of the vocabulary, so a
/// model can only add new ones through `new_tag`.
#[must_use]
pub fn schema(vocabulary: &Vocabulary) -> Value {
    let mut properties = json!({
        "title": {"type": "string", "description": "A short, specific title for the item, not just its file name"},
        "summary": {"type": "string", "description": "One to three sentences saying what the item is"},
        "new_tag": {"type": "string", "description": "Only if none of the tags fit: one new tag, lowercase, with words joined by -"},
        "created": {"type": "string", "description": "When the item was made, as YYYY-MM-DD, only if the text says"}
    });
    let mut required = vec!["title", "summary"];
    if !vocabulary.tags.is_empty() {
        let names: Vec<&str> = vocabulary.tags.iter().map(|(t, _)| t.0.as_str()).collect();
        properties["tags"] = json!({
            "type": "array",
            "items": {"enum": names},
            "maxItems": MAX_TAGS,
            "description": "Up to five tags from the list that describe the item"
        });
        required.push("tags");
    }
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": required,
        "properties": properties
    })
}

/// The instructions every card prompt starts with.
#[must_use]
pub fn system_prompt(context: &Context<'_>) -> String {
    let mut text = format!(
        "You write catalog cards for \u{201c}{}\u{201d}, a personal archive. Describe what each item is, \
         using only what the item itself shows; don't invent details. The summary says what the item \
         contains or is for, beyond its title. Choose only tags that clearly describe the item: one or \
         two is usual, and none is fine. Never repeat account numbers, passwords or identity numbers \
         in a card.\n",
        context.archive_name
    );
    if !context.vocabulary.tags.is_empty() {
        text.push_str("\nTags:\n");
        for (tag, description) in &context.vocabulary.tags {
            writeln!(text, "- {}: {description}", tag.0).unwrap();
        }
    }
    if !context.glossary.facts.is_empty() {
        text.push_str(
            "\nFacts the owner has told you, to help you understand items. Mention one only when the \
             item itself is about it:\n",
        );
        let mut used = 0;
        for (term, fact) in &context.glossary.facts {
            let line = format!("- {term}: {fact}\n");
            used += line.len();
            if used > GLOSSARY_BUDGET {
                break;
            }
            text.push_str(&line);
        }
    }
    text
}

fn describe(candidate: &Candidate, text: &ItemText) -> String {
    let name = candidate
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let kind = match candidate.kind {
        Kind::Bundle => "a project folder, kept whole".to_owned(),
        Kind::Folder => "a folder".to_owned(),
        _ => match (&text.media_type, text.pages) {
            (Some(media), Some(pages)) => format!("a file ({media}, {pages} pages)"),
            (Some(media), None) => format!("a file ({media})"),
            _ => "a file".to_owned(),
        },
    };
    format!("Name: {name}\nWhat it is: {kind}\n")
}

/// Checks a model's answer. Returns the problems a retry should fix, and fills `draft` with
/// what's usable.
fn check(
    answer: &Answer,
    candidate: &Candidate,
    vocabulary: &Vocabulary,
    draft: &mut Draft,
) -> Vec<String> {
    let mut problems = Vec::new();
    let title = answer.title.trim();
    let name = candidate
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let stem = candidate
        .path
        .file_stem()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if title.is_empty() {
        problems.push("the title is empty".to_owned());
    } else if title.chars().count() > MAX_TITLE {
        problems.push(format!("the title is longer than {MAX_TITLE} characters"));
    } else if candidate.kind == Kind::File
        && (title.to_lowercase() == name || title.to_lowercase() == stem)
        && machine_made(&stem)
    {
        // A project's name is often its right title, so this only applies to files.
        problems.push(REPEATS_NAME.to_owned());
    }
    let summary = answer.summary.trim();
    if summary.is_empty() {
        problems.push("the summary is empty".to_owned());
    } else if summary.chars().count() > MAX_SUMMARY {
        problems.push(format!(
            "the summary is longer than {MAX_SUMMARY} characters"
        ));
    } else if summary
        .trim_end_matches('.')
        .eq_ignore_ascii_case(title.trim_end_matches('.'))
    {
        problems.push(
            "the summary only repeats the title; say what the item contains or is for".to_owned(),
        );
    }

    title.clone_into(&mut draft.title);
    draft.summary = (!summary.is_empty()).then(|| summary.to_owned());
    draft.tags.clear();
    for tag in &answer.tags {
        if vocabulary.contains(tag) && !draft.tags.iter().any(|t| &t.0 == tag) {
            draft.tags.push(Tag(tag.clone()));
        }
    }
    draft.tags.truncate(MAX_TAGS);
    draft.new_tag = answer
        .new_tag
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .and_then(|t| {
            let tag = Tag(t.to_lowercase());
            if vocabulary.contains(&tag.0) {
                if !draft.tags.contains(&tag) && draft.tags.len() < MAX_TAGS {
                    draft.tags.push(tag);
                }
                None
            } else {
                // A malformed new tag isn't worth a retry; it's dropped.
                tag.is_well_formed().then_some(tag)
            }
        });
    draft.created = answer
        .created
        .as_deref()
        .map(|d| DateOrTime(d.trim().to_owned()))
        .filter(DateOrTime::is_valid);
    problems
}

/// Drafts a card for `candidate`, trying each available text model in turn, best first. Each
/// gets one retry with its answer's problems stated. If none succeeds, the draft falls back
/// to the file name as its title and lists what's left for the owner.
#[must_use]
pub fn draft(
    registry: &Registry,
    context: &Context<'_>,
    candidate: &Candidate,
    text: &ItemText,
) -> Draft {
    let file_stem = candidate
        .path
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut draft = Draft {
        title: file_stem.clone(),
        summary: None,
        tags: Vec::new(),
        new_tag: None,
        created: None,
        generated: None,
        attempts: Vec::new(),
        problems: Vec::new(),
    };
    let ranked = registry.ranked(Capability::TextModel);
    let Some(best_priority) = ranked.first().map(|(_, o)| o.priority) else {
        draft
            .problems
            .push("no text model on this device; write the card by hand".to_owned());
        return draft;
    };
    let system = system_prompt(context);
    let schema = schema(context.vocabulary);
    let mut last_problems = Vec::new();

    for (provider, offer) in ranked {
        let asking = Asking {
            registry,
            provider,
            offer,
            system: &system,
            schema: &schema,
            context,
            candidate,
            text,
        };
        match asking.ask(&mut draft) {
            Ok(model) => {
                draft.generated = Some(provenance(
                    &draft,
                    model,
                    context.device,
                    offer.priority < best_priority,
                ));
                return draft;
            }
            Err(problems) => last_problems = problems,
        }
    }
    // Nothing usable: keep the file name as the title, and say why.
    draft.title = file_stem;
    draft.summary = None;
    draft.tags.clear();
    draft.new_tag = None;
    draft.created = None;
    let declined: Vec<String> = draft
        .attempts
        .iter()
        .filter(|a| a.result == "declined")
        .map(|a| format!("{}/{}", a.provider, a.offer))
        .collect();
    draft.problems.push(if !declined.is_empty() && last_problems.is_empty() {
        format!(
            "{} declined to draft it (a safety refusal); write the card yourself, or add another model with `carrel models`",
            declined.join(" and ")
        )
    } else if last_problems.is_empty() {
        "no model could draft a card".to_owned()
    } else {
        format!(
            "no model drafted a usable card ({})",
            last_problems.join("; ")
        )
    });
    draft
}

/// One model's turn at drafting.
struct Asking<'a> {
    registry: &'a Registry,
    provider: &'a dyn Provider,
    offer: &'a Offer,
    system: &'a str,
    schema: &'a Value,
    context: &'a Context<'a>,
    candidate: &'a Candidate,
    text: &'a ItemText,
}

impl Asking<'_> {
    /// Asks this model, with one retry. Returns the model's name on success, or the last
    /// answer's problems.
    fn ask(&self, draft: &mut Draft) -> Result<String, Vec<String>> {
        let (provider, offer) = (self.provider, self.offer);
        let prompt = match fitted_prompt(
            self.registry,
            provider,
            offer,
            self.system,
            self.candidate,
            self.text,
        ) {
            Ok(prompt) => prompt,
            Err(why) => {
                draft
                    .attempts
                    .push(attempt(provider, offer, "unsupported", Some(why)));
                return Err(Vec::new());
            }
        };
        let mut feedback: Option<String> = None;
        let mut problems = Vec::new();
        for round in 0..2 {
            let mut request_prompt = prompt.clone();
            if let Some(feedback) = &feedback {
                write!(
                    request_prompt,
                    "\n\nYour previous answer had these problems; fix them: {feedback}"
                )
                .unwrap();
            }
            let request = CompletionRequest {
                system: Some(self.system.to_owned()),
                prompt: request_prompt,
                schema: Some(self.schema.clone()),
                max_tokens: Some(ANSWER_TOKENS),
            };
            let completion = match provider.complete(&offer.id, &request) {
                Ok(completion) => completion,
                Err(error) => {
                    let result = error.result();
                    draft
                        .attempts
                        .push(attempt(provider, offer, result, Some(error.to_string())));
                    return Err(problems);
                }
            };
            let answer: Option<Answer> = completion
                .json
                .as_ref()
                .and_then(|j| serde_json::from_value(j.clone()).ok());
            let Some(answer) = answer else {
                draft.attempts.push(attempt(
                    provider,
                    offer,
                    "failed",
                    Some("the answer wasn't a card".into()),
                ));
                feedback = Some("the answer must be a JSON object with title and summary".into());
                continue;
            };
            problems = check(&answer, self.candidate, self.context.vocabulary, draft);
            if round == 1 {
                problems.retain(|p| p != REPEATS_NAME);
            }
            if problems.is_empty() {
                draft.attempts.push(attempt(provider, offer, "ok", None));
                draft.problems.clear();
                return Ok(completion.model);
            }
            draft.attempts.push(attempt(
                provider,
                offer,
                "failed",
                Some(problems.join("; ")),
            ));
            feedback = Some(problems.join("; "));
        }
        Err(problems)
    }
}

fn provenance(draft: &Draft, model: String, device: Id, below_preferred: bool) -> Provenance {
    let mut fields = vec!["title".to_owned(), "summary".to_owned(), "tags".to_owned()];
    if draft.created.is_some() {
        fields.push("dates".to_owned());
    }
    Provenance {
        fields,
        by: format!("carrel {}", crate::CARREL_VERSION),
        model: Some(model),
        device: Some(device),
        at: now(),
        level: Some(if below_preferred {
            QualityLevel::BelowPreferred
        } else {
            QualityLevel::Preferred
        }),
        extra: crate::card::Extra::new(),
    }
}

/// The prompt with the item's text fitted to this model's context.
fn fitted_prompt(
    registry: &Registry,
    provider: &dyn Provider,
    offer: &Offer,
    system: &str,
    candidate: &Candidate,
    text: &ItemText,
) -> Result<String, String> {
    let context = offer
        .extra
        .get("context_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_CONTEXT);
    let source = crate::providers::Source {
        provider: provider.name().to_owned(),
        offer: offer.id.clone(),
        priority: offer.priority,
    };
    let count = |t: &str| registry.count_tokens(&source, t).0;
    let mut prompt = describe(candidate, text);
    let overhead = count(system) + count(&prompt) + u64::from(ANSWER_TOKENS) + 64;
    let Some(budget) = context.checked_sub(overhead).filter(|b| *b >= 64) else {
        return Err(format!(
            "{}'s context of {context} tokens is too small",
            offer.id
        ));
    };
    if text.text.trim().is_empty() {
        prompt.push_str("\nNo text could be read from it.\n");
    } else {
        let (fitted, cut) = fit(&text.text, budget, &count);
        writeln!(
            prompt,
            "\nText{}:\n{fitted}",
            if cut { " (shortened)" } else { "" }
        )
        .unwrap();
    }
    // A sanity check that also covers counters that can't count.
    if estimate_tokens(&prompt) == 0 {
        return Err("nothing to send".into());
    }
    Ok(prompt)
}

fn attempt(
    provider: &dyn Provider,
    offer: &Offer,
    result: &str,
    message: Option<String>,
) -> Attempt {
    Attempt {
        stage: "card".into(),
        provider: provider.name().to_owned(),
        offer: offer.id.clone(),
        result: result.into(),
        message,
    }
}

fn now() -> DateOrTime {
    DateOrTime(
        jiff::Timestamp::now()
            .to_zoned(jiff::tz::TimeZone::system())
            .strftime("%Y-%m-%dT%H:%M:%S%:z")
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_people_choose_are_told_apart_from_made_up_ones() {
        for made in [
            "IMG_2041",
            "scan0003",
            "Screenshot 2025-01-01 at 10.21.33",
            "document (3)",
            "PXL_20250314_101112",
            "01928f3a-7c41-7d2e-9b6a-3f0c5e2a1b44",
            "20250314",
            "Untitled",
        ] {
            assert!(machine_made(made), "{made}");
        }
        for chosen in [
            "amplifier build log",
            "Q3 plan",
            "invoice_2025-03",
            "cv alex morgan",
            "Liu2022",
        ] {
            assert!(!machine_made(chosen), "{chosen}");
        }
    }
}
