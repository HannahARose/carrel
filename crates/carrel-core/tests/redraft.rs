//! Drafting a waiting proposal again (`carrel_core::ingest::redraft`, #445).

use std::path::{Path, PathBuf};

use carrel_core::actions;
use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::ingest::IntakeOptions;
use carrel_core::ingest::redraft::{
    RedraftError, failed_drafts, no_drafting_model, redraft, wants_redraft,
};
use carrel_core::providers::{
    Capability, Completion, CompletionRequest, Offer, Provider, ProviderError, Registry,
};
use carrel_core::review::{ReviewEntry, ReviewQueue, State};
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

/// A text model that answers whatever it's asked sensibly: a card titled "Lamp parts list",
/// the first collection offered, and personal.
struct Answers {
    offers: Vec<Offer>,
}

impl Answers {
    fn new() -> Self {
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
        }
    }
}

impl Provider for Answers {
    fn name(&self) -> &'static str {
        "answers"
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
            json!({
                "title": "Lamp parts list",
                "summary": "The parts for the desk lamp.",
                "created": "",
                "tags": []
            })
        };
        Ok(Completion {
            text: answer.to_string(),
            json: Some(answer),
            model: "answers/m".into(),
            usage: None,
        })
    }
}

fn with_model() -> Registry {
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(Answers::new()));
    registry
}

struct Setup {
    _dir: tempfile::TempDir,
    archive: Archive,
    device: Id,
    file: PathBuf,
}

/// The sample archive, and a proposal for a file outside it drafted with no text model.
fn drafted_without_a_model() -> (Setup, ReviewEntry) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("archive");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
        &root,
    );
    let archive = Archive::open(&root).unwrap();
    let file = dir.path().join("parts.txt");
    std::fs::write(&file, "LED strip, diffuser, 12 V supply, switch.").unwrap();
    let device = Id::new_v7();
    let run = carrel_core::ingest::run::run(
        &archive,
        &Registry::with_built_ins(),
        std::slice::from_ref(&file),
        IntakeOptions::default(),
        device,
    )
    .unwrap();
    run.save(&archive).unwrap();
    let entry = run.entries[0].clone().unwrap();
    (
        Setup {
            _dir: dir,
            archive,
            device,
            file,
        },
        entry,
    )
}

#[test]
fn a_proposal_drafted_without_a_model_is_offered_again() {
    let (s, entry) = drafted_without_a_model();
    assert_eq!(entry.state, State::NeedsOwner);
    assert!(
        entry
            .problems
            .iter()
            .any(|p| p.message.contains("no text model")),
        "{:?}",
        entry.problems
    );
    assert!(wants_redraft(&entry));
    let failed = failed_drafts(&s.archive, s.device).unwrap();
    assert_eq!(
        failed.iter().map(|e| e.id).collect::<Vec<_>>(),
        vec![entry.id]
    );
    assert!(failed_drafts(&s.archive, Id::new_v7()).unwrap().is_empty());
}

#[test]
fn drafting_again_without_a_model_fails_clearly_and_changes_nothing() {
    let (s, entry) = drafted_without_a_model();
    let registry = Registry::with_built_ins();
    let error = redraft(
        &s.archive,
        &registry,
        &entry.id.to_string(),
        s.device,
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(error, RedraftError::NoTextModel), "{error:?}");
    assert_eq!(
        error.to_string(),
        "no text model on this device; write the card by hand"
    );
    let help = no_drafting_model(&registry, Some(16), None).expect("no model here");
    let words = help.explain_for("Drafting cards");
    assert!(
        words.starts_with("Drafting cards needs a text model"),
        "{words}"
    );
    let saved = ReviewQueue::of(&s.archive)
        .find(&entry.id.to_string())
        .unwrap();
    assert_eq!(saved, entry);
}

#[test]
fn drafting_again_keeps_the_id_batch_and_the_owners_fields() {
    let (s, mut entry) = drafted_without_a_model();
    let text = actions::draft_text(&s.archive, &entry, &[]).unwrap();
    let edited = text.replace("title: parts", "title: My lamp parts");
    assert_ne!(edited, text, "{text}");
    assert_eq!(
        actions::apply_draft(&s.archive, &mut entry, &edited).unwrap(),
        actions::Edited::Changed
    );
    assert_eq!(entry.owner_fields, vec!["title".to_owned()]);
    ReviewQueue::of(&s.archive).save(&entry).unwrap();

    let redrafted = redraft(
        &s.archive,
        &with_model(),
        &carrel_core::review::short_id(entry.id),
        s.device,
        &mut |_| {},
    )
    .unwrap();
    let again = &redrafted.entry;
    assert_eq!(again.id, entry.id);
    assert_eq!(again.batch, entry.batch);
    assert_eq!(again.created, entry.created);
    assert_eq!(again.source.path, entry.source.path);
    assert_eq!(again.card.title, "My lamp parts", "the owner's title stays");
    assert_eq!(
        again.card.summary.as_deref(),
        Some("The parts for the desk lamp."),
        "the model drafts the rest"
    );
    let generated = again.card.generated.as_ref().expect("model-written fields");
    assert!(!generated.fields.contains(&"title".to_owned()));
    assert!(generated.fields.contains(&"summary".to_owned()));
    assert!(redrafted.kept.contains(&"title"));
    assert!(!again.problems.iter().any(|p| p.stage == "card"));
    assert!(
        !again
            .destination
            .reasons
            .iter()
            .any(|r| r.contains("no text model")),
        "{:?}",
        again.destination
    );
    assert!(again.attempts.iter().any(|a| a.provider == "answers"));
    assert!(!wants_redraft(again));
    let saved = ReviewQueue::of(&s.archive)
        .find(&entry.id.to_string())
        .unwrap();
    assert_eq!(&saved, again);
    let (all, _) = ReviewQueue::of(&s.archive).list().unwrap();
    assert_eq!(
        all.iter()
            .filter(|e| e.source.path == again.source.path)
            .count(),
        1,
        "drafted again in place, not added"
    );
}

#[test]
fn drafting_again_keeps_a_collection_the_owner_chose() {
    let (s, entry) = drafted_without_a_model();
    actions::move_to(&s.archive, &entry.id.to_string(), "Reading").unwrap();
    let redrafted = redraft(
        &s.archive,
        &with_model(),
        &entry.id.to_string(),
        s.device,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(redrafted.entry.destination.path, "Reading");
    assert!(redrafted.kept.contains(&"collection"));
    assert_eq!(redrafted.entry.card.title, "Lamp parts list");
}

#[test]
fn only_a_waiting_proposal_on_this_device_is_drafted_again() {
    let (s, entry) = drafted_without_a_model();
    let other = redraft(
        &s.archive,
        &with_model(),
        &entry.id.to_string(),
        Id::new_v7(),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(other.to_string().contains("another device"), "{other}");

    std::fs::remove_file(&s.file).unwrap();
    let gone = redraft(
        &s.archive,
        &with_model(),
        &entry.id.to_string(),
        s.device,
        &mut |_| {},
    )
    .unwrap_err();
    assert!(gone.to_string().contains("no longer there"), "{gone}");
}
