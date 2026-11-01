//! Readings of what an item is, when the drafting model isn't sure
//! (`carrel_core::ingest::readings`, #449).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::correct::prepare;
use carrel_core::ingest::IntakeOptions;
use carrel_core::ingest::readings::pick_words;
use carrel_core::providers::{
    Capability, Completion, CompletionRequest, Offer, Provider, ProviderError, Registry,
};
use carrel_core::questions::QuestionQueue;
use carrel_core::questions::answer::item_words;
use carrel_core::review::{Confidence, ReviewEntry, ReviewQueue, State};
use serde_json::{Value, json};

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A text model that drafts each card from a script, one answer a call, repeating the last,
/// and answers routing and sensitivity sensibly. It records the card prompts it's given.
struct Scripted {
    offers: Vec<Offer>,
    cards: Vec<Value>,
    asked: Mutex<Vec<String>>,
}

impl Scripted {
    fn new(cards: Vec<Value>) -> Self {
        let mut extra = serde_json::Map::new();
        extra.insert("context_tokens".into(), json!(8_000));
        Self {
            offers: vec![Offer {
                capability: Capability::TextModel,
                id: "m".into(),
                label: None,
                priority: 10,
                available: true,
                reason: None,
                media_types: Vec::new(),
                extra,
            }],
            cards,
            asked: Mutex::new(Vec::new()),
        }
    }
}

impl Provider for Scripted {
    fn name(&self) -> &'static str {
        "scripted"
    }
    fn offers(&self) -> &[Offer] {
        &self.offers
    }
    fn complete(
        &self,
        _offer: &str,
        request: &CompletionRequest,
    ) -> Result<Completion, ProviderError> {
        let schema = request.schema.clone().unwrap_or(Value::Null);
        let properties = &schema["properties"];
        let answer = if properties.get("choice").is_some() {
            let first = properties["choice"]["enum"][0].clone();
            json!({"choice": first, "fit": "well", "reason": "it fits"})
        } else if properties.get("level").is_some() {
            json!({"level": "personal", "reason": "nothing private"})
        } else {
            let mut asked = self.asked.lock().unwrap();
            asked.push(request.prompt.clone());
            self.cards[(asked.len() - 1).min(self.cards.len() - 1)].clone()
        };
        Ok(Completion {
            text: answer.to_string(),
            json: Some(answer),
            model: "scripted/m".into(),
            usage: None,
        })
    }
}

fn fish() -> Value {
    json!({
        "title": "3D-printed fish model with text",
        "summary": "A model of a fish with lettering on its side.",
        "created": "",
        "tags": [],
        "reason": "The outline looks like a fish.",
        "alternatives": [{
            "title": "Cognitogram keychain plate",
            "summary": "A keychain plate with the Cognitogram lettering.",
            "tags": [],
            "reason": "The file name says it's a keychain plate."
        }]
    })
}

struct Setup {
    _dir: tempfile::TempDir,
    archive: Archive,
    file: PathBuf,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("archive");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
        &root,
    );
    let archive = Archive::open(&root).unwrap();
    let downloads = dir.path().join("Downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let file = downloads.join("cognitogram_keychain_plate copy.stl");
    std::fs::write(&file, [0_u8; 84]).unwrap();
    Setup {
        _dir: dir,
        archive,
        file,
    }
}

fn ingest(s: &Setup, model: Scripted) -> (carrel_core::ingest::run::Run, ReviewEntry) {
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(model));
    let run = carrel_core::ingest::run::run(
        &s.archive,
        &registry,
        std::slice::from_ref(&s.file),
        IntakeOptions::default(),
        Id::new_v7(),
    )
    .unwrap();
    run.save(&s.archive).unwrap();
    let entry = run.entries[0].clone().unwrap();
    (run, entry)
}

#[test]
fn the_reading_the_file_name_fits_fills_the_card_and_the_other_is_kept() {
    let s = setup();
    let (run, entry) = ingest(&s, Scripted::new(vec![fish()]));
    assert_eq!(entry.card.title, "Cognitogram keychain plate");
    assert_eq!(
        entry.card.summary.as_deref(),
        Some("A keychain plate with the Cognitogram lettering.")
    );
    assert_eq!(entry.readings.len(), 2);
    assert_eq!(entry.readings[1].title, "3D-printed fish model with text");
    assert_eq!(entry.readings[1].reason, "The outline looks like a fish.");
    assert!(entry.readings[1].against[0].starts_with("the file name"));
    assert_eq!(entry.destination.confidence, Confidence::Low);
    assert!(
        entry
            .destination
            .reasons
            .iter()
            .any(|r| r.contains("isn't clear")),
        "{:?}",
        entry.destination.reasons
    );

    // A question asks what it is, with the readings to pick from, and the proposal waits.
    let question = run
        .questions
        .iter()
        .find(|q| q.items == [entry.id])
        .unwrap();
    assert_eq!(
        question.question,
        "What is “cognitogram_keychain_plate copy.stl”?"
    );
    assert_eq!(question.suggestions.len(), 2);
    assert!(
        question.suggestions[1]
            .answer
            .starts_with("3D-printed fish")
    );
    assert_eq!(entry.state, State::NeedsOwner);
    assert_eq!(item_words(question, "2"), pick_words(2));
    assert!(item_words(question, "a lamp").contains("a lamp"));
}

#[test]
fn picking_a_reading_rewrites_the_card_as_one_correction_and_answers_the_question() {
    let s = setup();
    let (_, entry) = ingest(&s, Scripted::new(vec![fish()]));
    let registry = Registry::with_built_ins();
    let prepared =
        prepare::prepare_review(&s.archive, &registry, &entry.id.to_string(), &pick_words(2))
            .unwrap();
    assert_eq!(prepared.picked, Some(2));
    assert_eq!(prepared.after.title, "3D-printed fish model with text");
    assert_eq!(
        prepared.after.summary.as_deref(),
        Some("A model of a fish with lettering on its side.")
    );
    assert!(prepared.changed.contains(&"title") && prepared.changed.contains(&"summary"));
    prepare::save(
        &s.archive,
        prepared,
        || -> Result<_, prepare::PrepareError> { unreachable!("a proposal needs no writer lease") },
    )
    .unwrap();

    let saved = ReviewQueue::of(&s.archive)
        .find(&entry.id.to_string())
        .unwrap();
    assert_eq!(saved.card.title, "3D-printed fish model with text");
    assert!(saved.owner_fields.contains(&"title".to_owned()));
    assert_eq!(saved.readings.len(), 2, "the readings stay to pick again");
    assert!(saved.readings.iter().all(|r| r.against.is_empty()));
    let notes =
        carrel_core::correct::notes(&carrel_core::correct::Correctable::fields(&saved.card));
    assert_eq!(
        notes.last().unwrap().said,
        "Picked reading 2: “3D-printed fish model with text”"
    );
    let (questions, _) = QuestionQueue::of(&s.archive).list().unwrap();
    assert!(questions.iter().all(|q| !q.items.contains(&entry.id)));
    assert!(saved.problems.iter().all(|p| p.stage != "question"));

    // A reading that isn't there is refused.
    let wrong =
        prepare::prepare_review(&s.archive, &registry, &entry.id.to_string(), &pick_words(4));
    assert!(wrong.unwrap_err().to_string().contains("no reading 4"));
}

#[test]
fn a_card_that_ignores_the_file_name_is_asked_again_once() {
    let s = setup();
    let mut sure = fish();
    sure["alternatives"] = json!([]);
    sure["reason"] = json!("");
    let model = Scripted::new(vec![sure.clone(), fish()]);
    let (_, entry) = ingest(&s, model);
    assert_eq!(entry.readings.len(), 2);
    assert_eq!(entry.card.title, "Cognitogram keychain plate");
    let retries: Vec<_> = entry
        .attempts
        .iter()
        .filter(|a| a.stage == "card" && a.result == "failed")
        .collect();
    assert_eq!(retries.len(), 1);
    assert!(
        retries[0]
            .message
            .as_deref()
            .unwrap()
            .contains("the file name (“cognitogram_keychain_plate copy”) doesn't fit the card")
    );

    // Still sure the second time: the card stands, without readings.
    let s = setup();
    let (_, entry) = ingest(&s, Scripted::new(vec![sure]));
    assert_eq!(entry.card.title, "3D-printed fish model with text");
    assert!(entry.readings.is_empty());
    assert!(entry.problems.is_empty(), "{:?}", entry.problems);
}

#[test]
fn a_broken_alternative_is_dropped_on_the_last_try_without_failing_the_card() {
    let s = setup();
    let mut broken = fish();
    broken["alternatives"][0]["title"] = json!("");
    let (_, entry) = ingest(&s, Scripted::new(vec![broken]));
    assert_eq!(entry.card.title, "3D-printed fish model with text");
    assert!(entry.readings.is_empty());
    assert!(entry.problems.is_empty(), "{:?}", entry.problems);
    assert!(
        entry
            .attempts
            .iter()
            .any(|a| a.message.as_deref() == Some("alternative 1: the title is empty"))
    );
}

#[test]
fn drafting_again_replaces_the_question_offering_the_old_readings() {
    let s = setup();
    let (_, entry) = ingest(&s, Scripted::new(vec![fish()]));
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(Scripted::new(vec![fish()])));
    let redrafted = carrel_core::ingest::redraft::redraft(
        &s.archive,
        &registry,
        &entry.id.to_string(),
        entry.source.device,
        &mut |_| {},
    )
    .unwrap();
    let (questions, _) = QuestionQueue::of(&s.archive).list().unwrap();
    let offering: Vec<_> = questions
        .iter()
        .filter(|q| !q.suggestions.is_empty())
        .collect();
    assert_eq!(offering.len(), 1, "{questions:#?}");
    assert_eq!(offering[0].items, [entry.id]);
    let waits: Vec<_> = redrafted
        .entry
        .problems
        .iter()
        .filter(|p| p.stage == "question")
        .collect();
    assert_eq!(waits.len(), 1, "{waits:?}");
    assert!(waits[0]
        .message
        .contains(&carrel_core::review::short_id(offering[0].id)));
}
