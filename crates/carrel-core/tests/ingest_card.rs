//! Ingest's card stage: prompts, schema, validation, retries and fallback, with scripted
//! models.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use carrel_core::archive::Archive;
use carrel_core::card::{Checksum, DateOrTime, Id, Kind, QualityLevel, Tag};
use carrel_core::ingest::Candidate;
use carrel_core::ingest::card::{Context, Glossary, Vocabulary, draft, schema, system_prompt};
use carrel_core::ingest::text::ItemText;
use carrel_core::providers::{ProviderError, Registry};
use serde_json::json;

mod common;
use common::Scripted;

fn sample() -> Archive {
    Archive::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"))
        .unwrap()
}

fn candidate(name: &str) -> Candidate {
    Candidate {
        path: PathBuf::from("/incoming").join(name),
        kind: Kind::File,
        sealed: None,
        markers: Vec::new(),
        checksum: Checksum::of_bytes(b""),
        size: 10,
        duplicate_of: None,
        likely: Vec::new(),
        same_as: None,
    }
}

fn text(body: &str) -> ItemText {
    ItemText {
        text: body.into(),
        media_type: Some("text/plain".into()),
        method: "embedded".into(),
        pages: None,
        created: None,
        located: false,
        attempts: Vec::new(),
        note: None,
    }
}

fn good() -> serde_json::Value {
    json!({
        "title": "Desk lamp diffuser print notes",
        "summary": "Notes on printing the lamp's frosted diffuser.",
        "tags": ["3d-printing", "not-a-real-tag", "3d-printing"],
        "new_tag": "Lighting",
        "created": "2026-09-14"
    })
}

struct Fixture {
    vocabulary: Vocabulary,
    glossary: Glossary,
}

impl Fixture {
    fn new() -> Self {
        let archive = sample();
        let collections = archive.collections().unwrap();
        Self {
            vocabulary: Vocabulary::of(&archive, &collections),
            glossary: Glossary::of(&archive),
        }
    }
    fn context(&self) -> Context<'_> {
        Context {
            archive_name: "Sample archive",
            vocabulary: &self.vocabulary,
            glossary: &self.glossary,
            device: Id::new_v7(),
        }
    }
}

fn registry_of(models: Vec<Scripted>) -> Registry {
    let mut registry = Registry::new();
    for model in models {
        registry.add(Box::new(model));
    }
    registry
}

#[test]
fn the_glossary_vocabulary_and_schema_come_from_the_archive() {
    let fixture = Fixture::new();
    assert_eq!(fixture.glossary.facts.len(), 1);
    assert_eq!(fixture.glossary.facts[0].0, "oscillator-sim");
    assert!(
        fixture.glossary.facts[0]
            .1
            .contains("coupled oscillators note")
    );
    assert!(fixture.vocabulary.contains("3d-printing"));
    assert!(fixture.vocabulary.contains("electronics"));

    let schema = schema(&fixture.vocabulary);
    let tags = schema["properties"]["tags"]["items"]["enum"]
        .as_array()
        .unwrap();
    assert!(tags.contains(&json!("3d-printing")));
    assert_eq!(
        schema["required"],
        json!(["title", "summary", "created", "tags"])
    );

    let system = system_prompt(&fixture.context());
    assert!(system.contains("Sample archive"));
    assert!(
        system.contains("- oscillator-sim: The oscillator-sim project"),
        "{system}"
    );
    assert!(
        system.contains("- 3d-printing: Printed parts and print files"),
        "{system}"
    );
}

#[test]
fn a_good_answer_becomes_the_draft() {
    let fixture = Fixture::new();
    let registry = registry_of(vec![Scripted::new("a", 100, 8192, vec![Ok(good())])]);
    let d = draft(
        &registry,
        &fixture.context(),
        &candidate("diffuser.txt"),
        &text("Print at 0.2 mm."),
    );
    assert_eq!(d.problems, Vec::<String>::new());
    assert_eq!(d.title, "Desk lamp diffuser print notes");
    assert_eq!(
        d.tags,
        [Tag("3d-printing".into())],
        "unknown and repeated tags are dropped"
    );
    assert_eq!(
        d.new_tag,
        Some(Tag("lighting".into())),
        "a new tag is lowercased"
    );
    assert_eq!(d.created.unwrap().0, "2026-09-14");
    let generated = d.generated.unwrap();
    assert_eq!(generated.model.as_deref(), Some("a/m"));
    assert_eq!(generated.level, Some(QualityLevel::Preferred));
    assert_eq!(d.attempts.len(), 1);
    assert_eq!(d.attempts[0].result, "ok");
}

#[test]
fn a_bad_answer_is_retried_with_its_problems() {
    let fixture = Fixture::new();
    let lazy = json!({"title": "IMG_2041", "summary": "", "tags": []});
    let model = Scripted::new("a", 100, 8192, vec![Ok(lazy), Ok(good())]);
    let registry = registry_of(vec![model]);
    let d = draft(
        &registry,
        &fixture.context(),
        &candidate("IMG_2041.txt"),
        &text("Print at 0.2 mm."),
    );
    assert_eq!(d.problems, Vec::<String>::new());
    assert_eq!(
        d.attempts
            .iter()
            .map(|a| a.result.as_str())
            .collect::<Vec<_>>(),
        ["failed", "ok"]
    );
    let message = d.attempts[0].message.as_deref().unwrap();
    assert!(
        message.contains("only repeats the file name") && message.contains("summary is empty"),
        "{message}"
    );
}

#[test]
fn a_name_a_person_chose_can_be_the_title() {
    let fixture = Fixture::new();
    let named =
        json!({"title": "Amplifier build log", "summary": "Notes from building it.", "tags": []});
    let model = Scripted::new("a", 100, 8192, vec![Ok(named)]);
    let registry = registry_of(vec![model]);
    let d = draft(
        &registry,
        &fixture.context(),
        &candidate("amplifier build log.txt"),
        &text("Soldered the op-amp socket."),
    );
    assert_eq!(d.problems, Vec::<String>::new());
    assert_eq!(d.title, "Amplifier build log");
    assert_eq!(d.attempts.len(), 1, "accepted the first time");
}

#[test]
fn after_two_bad_answers_the_next_model_drafts_below_preferred() {
    let fixture = Fixture::new();
    let bad = || Ok(json!({"title": "", "summary": "x"}));
    let first = Scripted::new("best", 100, 8192, vec![bad(), bad()]);
    let second = Scripted::new("fallback", 50, 8192, vec![Ok(good())]);
    let registry = registry_of(vec![first, second]);
    let d = draft(
        &registry,
        &fixture.context(),
        &candidate("diffuser.txt"),
        &text("Print at 0.2 mm."),
    );
    assert_eq!(d.problems, Vec::<String>::new());
    assert_eq!(d.attempts.len(), 3);
    assert_eq!(d.attempts[2].provider, "fallback");
    assert_eq!(
        d.generated.unwrap().level,
        Some(QualityLevel::BelowPreferred)
    );
}

#[test]
fn when_every_model_fails_the_owner_is_asked() {
    let fixture = Fixture::new();
    let broken = Scripted::new(
        "a",
        100,
        8192,
        vec![Err(ProviderError::Unsupported("too long".into()))],
    );
    let registry = registry_of(vec![broken]);
    let d = draft(
        &registry,
        &fixture.context(),
        &candidate("scan 0042.pdf"),
        &text("x"),
    );
    assert_eq!(d.title, "scan 0042", "the file name stands in");
    assert_eq!(d.generated, None);
    assert_eq!(d.problems.len(), 1);
    assert_eq!(d.attempts[0].result, "unsupported");

    let none = draft(
        &Registry::new(),
        &fixture.context(),
        &candidate("x.txt"),
        &text("x"),
    );
    assert!(none.problems[0].contains("no text model"));
}

#[test]
fn long_text_is_fitted_to_the_models_context() {
    let fixture = Fixture::new();
    let model = Scripted::new("small", 100, 2000, vec![Ok(good())]);
    let asked = Arc::clone(&model.asked);
    let registry = registry_of(vec![model]);
    let long = format!(
        "Abstract: the lamp build. {}In conclusion, it works.",
        "A sentence about the lamp build and its parts. ".repeat(2000)
    );
    let d = draft(
        &registry,
        &fixture.context(),
        &candidate("notes.txt"),
        &text(&long),
    );
    assert_eq!(d.problems, Vec::<String>::new());
    let asked = asked.lock().unwrap();
    let request = &asked[0];
    let used = carrel_core::providers::estimate_tokens(request.system.as_deref().unwrap())
        + carrel_core::providers::estimate_tokens(&request.prompt);
    assert!(used + 400 <= 2000, "{used} tokens of prompt");
    assert!(request.prompt.contains("Text (shortened):"));
    assert!(request.prompt.contains("Abstract: the lamp build."));
    assert!(
        request
            .prompt
            .trim_end()
            .ends_with("In conclusion, it works.")
    );
    assert!(
        request
            .prompt
            .starts_with("Name: notes.txt\nWhat it is: a file (text/plain)\n")
    );
}

#[test]
fn a_title_that_repeats_a_made_up_file_name_is_accepted_after_a_retry() {
    let fixture = Fixture::new();
    let named = || {
        Ok(json!({"title": "scan0042", "summary": "A scanned shop receipt from 2026.", "tags": []}))
    };
    let registry = registry_of(vec![Scripted::new("a", 100, 8192, vec![named(), named()])]);
    let d = draft(
        &registry,
        &fixture.context(),
        &candidate("scan0042.pdf"),
        &text("Scanned receipt 2026"),
    );
    assert_eq!(d.problems, Vec::<String>::new());
    assert_eq!(d.title, "scan0042");
    assert_eq!(
        d.attempts
            .iter()
            .map(|a| a.result.as_str())
            .collect::<Vec<_>>(),
        ["failed", "ok"]
    );

    // A project folder's name is a fine title from the start.
    let registry = registry_of(vec![Scripted::new(
        "a",
        100,
        8192,
        vec![Ok(
            json!({"title": "oscillator-sim", "summary": "A Rust simulation of coupled oscillators.", "tags": []}),
        )],
    )]);
    let mut bundle = candidate("oscillator-sim");
    bundle.kind = Kind::Bundle;
    let d = draft(
        &registry,
        &fixture.context(),
        &bundle,
        &text("Folder: oscillator-sim"),
    );
    assert_eq!(d.attempts.len(), 1);
    assert_eq!(d.title, "oscillator-sim");
}

#[test]
fn a_summary_that_repeats_the_title_is_retried() {
    let fixture = Fixture::new();
    let echo = Ok(json!({"title": "Scanned receipt", "summary": "Scanned receipt.", "tags": []}));
    let registry = registry_of(vec![Scripted::new("a", 100, 8192, vec![echo, Ok(good())])]);
    let d = draft(
        &registry,
        &fixture.context(),
        &candidate("receipt.pdf"),
        &text("x"),
    );
    assert!(
        d.attempts[0]
            .message
            .as_deref()
            .unwrap()
            .contains("only repeats the title")
    );
    assert_eq!(d.problems, Vec::<String>::new());
}

#[test]
fn a_model_that_declines_is_named_so_the_owner_knows() {
    let fixture = Fixture::new();
    let refuses = Scripted::new(
        "a",
        100,
        8192,
        vec![Err(ProviderError::Declined(
            "Apple's on-device model declined: Detected content likely to be unsafe".into(),
        ))],
    );
    let registry = registry_of(vec![refuses]);
    let d = draft(
        &registry,
        &fixture.context(),
        &candidate("blood test results.txt"),
        &text("Ferritin 18 ug/L"),
    );
    assert_eq!(d.attempts[0].result, "declined");
    assert_eq!(d.problems.len(), 1);
    assert!(
        d.problems[0].contains("declined to draft it"),
        "{:?}",
        d.problems
    );
    assert_eq!(
        ProviderError::from_code(4, "x".into()),
        ProviderError::Declined("x".into())
    );
    assert_eq!(ProviderError::Declined("x".into()).result(), "declined");
}

#[test]
fn a_date_the_file_records_is_used_when_the_model_gives_none() {
    let fixture = Fixture::new();
    let mut photo = text("Image: photo, taken 2026-06-24 16:28 on an iPhone 15 Pro");
    photo.created = Some(DateOrTime("2026-06-24T16:28:05+02:00".into()));
    let mut undated = good();
    undated["created"] = json!("");
    let registry = registry_of(vec![Scripted::new("a", 100, 8192, vec![Ok(undated)])]);
    let d = draft(
        &registry,
        &fixture.context(),
        &candidate("IMG_0550.jpeg"),
        &photo,
    );
    assert_eq!(d.created, photo.created);
    // The date is the file's, not the model's.
    let generated = d.generated.unwrap();
    assert!(
        !generated.fields.contains(&"dates".to_owned()),
        "{:?}",
        generated.fields
    );

    // The model's own date stands.
    let registry = registry_of(vec![Scripted::new("a", 100, 8192, vec![Ok(good())])]);
    let d = draft(
        &registry,
        &fixture.context(),
        &candidate("IMG_0550.jpeg"),
        &photo,
    );
    assert_eq!(d.created, Some(DateOrTime("2026-09-14".into())));

    // And with no model, the file's.
    let d = draft(
        &Registry::new(),
        &fixture.context(),
        &candidate("IMG_0550.jpeg"),
        &photo,
    );
    assert_eq!(d.created, photo.created);
}
