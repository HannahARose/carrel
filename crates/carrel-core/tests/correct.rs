//! Typed corrections (`docs/design/corrections.md`).

mod common;

use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::card::{DateOrTime, Provenance, Sensitivity, Tag};
use carrel_core::card_file::CardFile;
use carrel_core::correct::{Correctable, CorrectionError, Fields, Subject, apply, notes, work_out};
use carrel_core::ingest::card::Vocabulary;
use carrel_core::providers::Registry;
use serde_json::{Value, json};

use common::Scripted;

fn sample() -> Archive {
    Archive::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"))
        .unwrap()
}

/// A model-drafted card for a photo, as ingest would write it.
fn photo_card() -> Fields {
    let archive = sample();
    let item = archive.items().unwrap().into_iter().next().unwrap();
    let text = std::fs::read_to_string(archive.root().join(&item.card)).unwrap();
    let mut card = CardFile::parse(&text).unwrap().card().clone();
    card.title = "Lake Geneva at dusk".into();
    card.summary = Some("A photo of Lake Geneva at dusk from the shore.".into());
    card.tags = Vec::new();
    card.sensitivity = Sensitivity::Personal;
    card.generated = Some(Provenance {
        fields: vec!["title".into(), "summary".into(), "tags".into()],
        by: "carrel 0.2.0".into(),
        model: Some("apple/afm".into()),
        device: None,
        at: DateOrTime("2026-10-30".into()),
        level: None,
        extra: carrel_core::card::Extra::new(),
    });
    card.fields()
}

fn collections() -> Vec<(String, String)> {
    vec![
        ("Reading".into(), "Books and papers".into()),
        ("Travel".into(), "Trips and places".into()),
    ]
}

/// A full answer that changes nothing, to adjust per test.
fn unchanged() -> Value {
    json!({
        "title": "", "title_dictated": false, "summary": "", "summary_dictated": false,
        "created": "", "created_dictated": false, "add_tags": [], "remove_tags": [],
        "new_tag": "", "tags_dictated": false, "sensitivity": "unchanged",
        "sensitivity_dictated": false, "collection": "unchanged", "collection_dictated": false,
        "reroute": false, "fact": "", "fact_about": ""
    })
}

fn registry_of(answers: Vec<Value>) -> Registry {
    let mut registry = Registry::new();
    registry.add(Box::new(Scripted::new(
        "m",
        100,
        8192,
        answers.into_iter().map(Ok).collect(),
    )));
    registry
}

#[test]
fn a_broad_correction_rewrites_fields_but_they_stay_model_written() {
    let archive = sample();
    let collections_list = collections();
    let vocabulary = Vocabulary::of(&archive, &archive.collections().unwrap());
    let mut card = photo_card();
    let mut answer = unchanged();
    answer["title"] = json!("Lake Annecy at dusk");
    answer["summary"] = json!("A photo of Lake Annecy at dusk from the shore.");
    answer["reroute"] = json!(true);
    let registry = registry_of(vec![answer]);
    let subject = Subject {
        card: &card,
        collection: "Holding",
        text: "Image: photo, 4032 × 3024",
        collections: &collections_list,
        vocabulary: &vocabulary,
    };
    let words = "This is a picture of Lake Annecy, not Geneva";
    let worked = work_out(&registry, &subject, words, &[]).unwrap();
    assert!(worked.changes.reroute);
    assert_eq!(worked.model.as_deref(), Some("m/m"));

    let at = DateOrTime("2026-10-31".into());
    let changed = apply(&mut card, &worked, words, &at);
    assert_eq!(changed, vec!["title", "summary"]);
    assert_eq!(card.title, "Lake Annecy at dusk");
    let generated = card.generated.as_ref().unwrap();
    assert!(
        generated.fields.contains(&"title".to_owned()),
        "inferred, so still the model's"
    );
    assert!(generated.fields.contains(&"summary".to_owned()));
    assert_eq!(generated.model.as_deref(), Some("m/m"));
    assert_eq!(notes(&card)[0].said, words);
}

#[test]
fn dictated_fields_stop_being_model_written_and_earlier_corrections_are_given() {
    let archive = sample();
    let collections_list = collections();
    let vocabulary = Vocabulary::of(&archive, &archive.collections().unwrap());
    let mut card = photo_card();
    let mut answer = unchanged();
    answer["title"] = json!("Annecy, last evening");
    answer["title_dictated"] = json!(true);
    answer["summary"] = json!("A photo of Lake Annecy on the last evening of the trip.");
    answer["collection"] = json!("Travel");
    answer["collection_dictated"] = json!(true);
    let registry = registry_of(vec![answer]);
    let earlier = vec!["This is Lake Annecy, not Geneva".to_owned()];
    let subject = Subject {
        card: &card,
        collection: "Holding",
        text: "",
        collections: &collections_list,
        vocabulary: &vocabulary,
    };
    let worked = work_out(
        &registry,
        &subject,
        "Call it Annecy, last evening; it goes in Travel",
        &earlier,
    )
    .unwrap();
    assert_eq!(worked.changes.collection.as_ref().unwrap().value, "Travel");
    apply(
        &mut card,
        &worked,
        "Call it Annecy, last evening",
        &DateOrTime("2026-10-31".into()),
    );
    let generated = card.generated.as_ref().unwrap();
    assert!(
        !generated.fields.contains(&"title".to_owned()),
        "dictated, so the owner's"
    );
    assert!(generated.fields.contains(&"summary".to_owned()));
}

#[test]
fn the_prompt_carries_earlier_corrections_and_the_card() {
    let archive = sample();
    let collections_list = collections();
    let vocabulary = Vocabulary::of(&archive, &archive.collections().unwrap());
    let card = photo_card();
    let mut answer = unchanged();
    answer["summary"] = json!("A photo of the lake at dusk.");
    let scripted = Scripted::new("m", 100, 8192, vec![Ok(answer)]);
    let asked = scripted.asked.clone();
    let mut registry = Registry::new();
    registry.add(Box::new(scripted));
    let subject = Subject {
        card: &card,
        collection: "Holding",
        text: "",
        collections: &collections_list,
        vocabulary: &vocabulary,
    };
    work_out(
        &registry,
        &subject,
        "shorter summary",
        &["It's Annecy".to_owned()],
    )
    .unwrap();
    let prompt = &asked.lock().unwrap()[0].prompt;
    assert!(prompt.contains("Title: Lake Geneva at dusk"), "{prompt}");
    assert!(prompt.contains("- It's Annecy"), "{prompt}");
    assert!(prompt.contains("- Travel: Trips and places"), "{prompt}");
}

#[test]
fn an_answer_that_changes_nothing_or_names_no_collection_is_retried() {
    let archive = sample();
    let collections_list = collections();
    let vocabulary = Vocabulary::of(&archive, &archive.collections().unwrap());
    let card = photo_card();
    let mut bad = unchanged();
    bad["created"] = json!("last spring");
    let mut good = unchanged();
    good["created"] = json!("2025-04-12");
    good["created_dictated"] = json!(true);
    let registry = registry_of(vec![unchanged(), bad]);
    let subject = Subject {
        card: &card,
        collection: "Holding",
        text: "",
        collections: &collections_list,
        vocabulary: &vocabulary,
    };
    let err = work_out(&registry, &subject, "taken last spring", &[]).unwrap_err();
    assert!(
        matches!(err, CorrectionError::Unusable(ref p) if p[0].contains("isn't a date")),
        "{err}"
    );

    let registry = registry_of(vec![unchanged(), good]);
    let worked = work_out(&registry, &subject, "taken 2025-04-12", &[]).unwrap();
    assert_eq!(worked.attempts.len(), 2);
    assert!(worked.changes.created.unwrap().dictated);
}

#[test]
fn field_lines_need_no_model_and_are_all_dictated() {
    let archive = sample();
    let collections_list = collections();
    let vocabulary = Vocabulary::of(&archive, &archive.collections().unwrap());
    let original = photo_card();
    let mut card = original.clone();
    let tag = vocabulary.tags[0].0.0.clone();
    let words =
        format!("created: 2019-03-31\ntags: +{tag}\nsensitivity: public\ncollection: Travel");
    let subject = Subject {
        card: &original,
        collection: "Holding",
        text: "",
        collections: &collections_list,
        vocabulary: &vocabulary,
    };
    let worked = work_out(&Registry::new(), &subject, &words, &[]).unwrap();
    assert!(worked.model.is_none());
    let changed = apply(&mut card, &worked, &words, &DateOrTime("2026-10-31".into()));
    assert_eq!(
        changed,
        vec!["created", "tags", "sensitivity", "collection"]
    );
    assert_eq!(
        card.sensitivity,
        Sensitivity::Public,
        "the owner may lower it"
    );
    assert!(card.tags.contains(&Tag(tag)));
    assert_eq!(
        card.dates.as_ref().unwrap().created,
        Some(DateOrTime("2019-03-31".into()))
    );
    let generated = card.generated.as_ref().unwrap();
    assert!(!generated.fields.contains(&"tags".to_owned()));
    assert_eq!(
        generated.model.as_deref(),
        Some("apple/afm"),
        "no model wrote anything new"
    );

    let wrong = work_out(&Registry::new(), &subject, "created: last week", &[]).unwrap_err();
    assert!(matches!(wrong, CorrectionError::BadLine(_)));
    let nowhere = work_out(&Registry::new(), &subject, "collection: Nowhere", &[]).unwrap_err();
    assert!(nowhere.to_string().contains("no collection Nowhere"));
    let words_need_model = work_out(&Registry::new(), &subject, "it's a picture of Annecy", &[]);
    assert_eq!(words_need_model.unwrap_err(), CorrectionError::NoModel);
}

#[test]
fn a_lasting_fact_is_offered_with_what_it_is_about() {
    let archive = sample();
    let collections_list = collections();
    let vocabulary = Vocabulary::of(&archive, &archive.collections().unwrap());
    let card = photo_card();
    let mut answer = unchanged();
    answer["summary"] = json!("A letter from Acme Property, the owner's landlord, about rent.");
    answer["fact"] = json!("Acme Property is the owner's landlord.");
    answer["fact_about"] = json!("Acme Property");
    let registry = registry_of(vec![answer]);
    let subject = Subject {
        card: &card,
        collection: "Holding",
        text: "",
        collections: &collections_list,
        vocabulary: &vocabulary,
    };
    let worked = work_out(&registry, &subject, "Acme Property is my landlord", &[]).unwrap();
    let fact = worked.changes.fact.unwrap();
    assert_eq!(fact.about, "Acme Property");
    assert_eq!(fact.fact, "Acme Property is the owner's landlord.");
}

#[test]
fn the_models_answer_is_held_to_what_the_owner_said() {
    let archive = sample();
    let collections_list = collections();
    let vocabulary = Vocabulary::of(&archive, &archive.collections().unwrap());
    let card = photo_card();
    let tags: Vec<String> = vocabulary
        .tags
        .iter()
        .take(3)
        .map(|(t, _)| t.0.clone())
        .collect();
    // As a model answered in the interactive mode: everything "dictated", tags nobody named,
    // and a "fact" about the item itself.
    let mut answer = unchanged();
    answer["title"] = json!("Bedroom lamp at dusk");
    answer["title_dictated"] = json!(true);
    answer["created"] = json!("2019-03-01");
    answer["created_dictated"] = json!(true);
    answer["add_tags"] = json!(tags);
    answer["tags_dictated"] = json!(true);
    answer["fact"] = json!("This item was made for the bedroom lamp.");
    answer["fact_about"] = json!("bedroom lamp");
    let registry = registry_of(vec![answer]);
    let subject = Subject {
        card: &card,
        collection: "Holding",
        text: "",
        collections: &collections_list,
        vocabulary: &vocabulary,
    };
    let words = "it was made in March 2019 and it is for the bedroom lamp";
    let changes = work_out(&registry, &subject, words, &[]).unwrap().changes;
    assert!(
        !changes.title.unwrap().dictated,
        "the owner didn't give this title"
    );
    assert!(changes.created.unwrap().dictated, "the owner gave the year");
    assert!(
        changes.tags.is_none(),
        "a narrow correction names no tags, so none change"
    );
    assert!(
        changes.fact.is_none(),
        "a fact about the item itself isn't lasting"
    );

    // Asking for a tag by name adds it.
    let mut tagged = unchanged();
    tagged["add_tags"] = json!([tags[0].clone()]);
    let registry = registry_of(vec![tagged]);
    let asked = format!("tag it {}", tags[0].replace('-', " "));
    let changes = work_out(&registry, &subject, &asked, &[]).unwrap().changes;
    let edit = changes.tags.unwrap();
    assert_eq!(edit.value.add, vec![Tag(tags[0].clone())]);
    assert!(edit.dictated);
}
