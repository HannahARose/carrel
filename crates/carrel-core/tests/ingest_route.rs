//! Ingest's routing stage and proposals: owner rules, model routing with a second pass, and
//! the review entries that result.

use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::card::{Checksum, Id, Kind, Sensitivity, Tag};
use carrel_core::ingest::card::Draft;
use carrel_core::ingest::route::{Route, Router, RoutingRules, tags_for};
use carrel_core::ingest::sensitivity::floor;
use carrel_core::ingest::{Batch, Candidate};
use carrel_core::providers::{ProviderError, Registry};
use carrel_core::review::{Confidence, ReviewQueue, State};
use serde_json::json;

mod common;
use common::Scripted;

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

fn sample() -> (tempfile::TempDir, Archive) {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
        &dir.path().join("archive"),
    );
    let archive = Archive::open(&dir.path().join("archive")).unwrap();
    (dir, archive)
}

fn candidate(path: PathBuf, kind: Kind) -> Candidate {
    Candidate {
        path,
        kind,
        sealed: None,
        markers: Vec::new(),
        checksum: Checksum::of_bytes(b"x"),
        size: 1,
        duplicate_of: None,
        likely: Vec::new(),
        same_as: None,
    }
}

fn draft(title: &str, tags: &[&str]) -> Draft {
    Draft {
        title: title.into(),
        summary: Some("Something to file.".into()),
        tags: tags.iter().map(|t| Tag((*t).into())).collect(),
        new_tag: None,
        created: None,
        generated: None,
        attempts: Vec::new(),
        problems: Vec::new(),
    }
}

#[allow(clippy::unnecessary_wraps)] // scripted answers are results
fn choice(path: &str) -> Result<serde_json::Value, ProviderError> {
    fitting(path, "well")
}

#[allow(clippy::unnecessary_wraps)]
fn fitting(path: &str, fit: &str) -> Result<serde_json::Value, ProviderError> {
    Ok(json!({"choice": path, "fit": fit, "reason": format!("it fits {path}")}))
}

fn route_with(archive: &Archive, answers: Vec<Result<serde_json::Value, ProviderError>>) -> Route {
    let collections = archive.collections().unwrap();
    let rules = RoutingRules::default();
    let router = Router::new(archive, &collections, &rules);
    let mut registry = Registry::new();
    registry.add(Box::new(Scripted::new("m", 100, 8192, answers)));
    router.route(
        &registry,
        &candidate(PathBuf::from("/in/notes.txt"), Kind::File),
        Path::new("/in"),
        &draft("Lamp notes", &[]),
    )
}

#[test]
fn owner_rules_decide_first() {
    let (dir, archive) = sample();
    let collections = archive.collections().unwrap();
    let rules = RoutingRules::of(&archive, &collections);
    assert_eq!(rules.rules.len(), 2);
    assert_eq!(rules.problems, Vec::<String>::new());
    let router = Router::new(&archive, &collections, &rules);
    let registry = Registry::new();

    let kicad = dir.path().join("lamp-pcb");
    std::fs::create_dir(&kicad).unwrap();
    std::fs::write(kicad.join("lamp.kicad_pro"), "{}").unwrap();
    let route = router.route(
        &registry,
        &candidate(kicad, Kind::Bundle),
        dir.path(),
        &draft("Lamp PCB", &[]),
    );
    assert_eq!(route.path, "Projects/Hardware");
    assert_eq!(route.confidence, Confidence::High);
    assert_eq!(route.rule.as_deref(), Some("KiCad projects"));
    assert!(!route.auto_file);
    assert_eq!(route.attempts, [], "no model was asked");

    let rfc = dir.path().join("RFC 4122 notes.pdf");
    std::fs::write(&rfc, "x").unwrap();
    let route = router.route(
        &registry,
        &candidate(rfc, Kind::File),
        dir.path(),
        &draft("UUIDs", &[]),
    );
    assert_eq!(route.rule.as_deref(), Some("Standards and RFCs"));
    assert!(route.auto_file);
}

#[test]
fn bad_rules_are_reported_and_skipped() {
    let (_dir, archive) = sample();
    std::fs::write(
        archive.root().join("_System/rules/routing.yaml"),
        "rules:\n  - name: \"broken\"\n    match: { name: \"([\" }\n    collection: 01a18701-0500-776a-9867-0804857e5ab6\n  - name: \"nowhere\"\n    match: { extension: pdf }\n    collection: 01a18701-0000-7000-8000-000000000000\n",
    )
    .unwrap();
    let rules = RoutingRules::of(&archive, &archive.collections().unwrap());
    assert_eq!(rules.rules.len(), 0);
    assert_eq!(rules.problems.len(), 2, "{:?}", rules.problems);
}

#[test]
fn the_model_routes_level_by_level_and_must_agree_with_itself() {
    let (_dir, archive) = sample();
    let both = route_with(
        &archive,
        vec![
            choice("Projects"),
            choice("Projects"),
            choice("Projects/Hardware"),
            choice("Projects/Hardware"),
        ],
    );
    assert_eq!(both.path, "Projects/Hardware");
    assert_eq!(both.confidence, Confidence::Medium);
    assert_eq!(both.attempts.len(), 4, "two passes per level");
    assert_eq!(
        both.reasons,
        ["it fits Projects", "it fits Projects/Hardware"]
    );

    let here = route_with(
        &archive,
        vec![
            choice("Projects"),
            choice("Projects"),
            choice("(here)"),
            choice("(here)"),
        ],
    );
    assert_eq!(here.path, "Projects");
    assert_eq!(here.confidence, Confidence::Medium);

    let wavering = route_with(&archive, vec![choice("Projects"), choice("Reading")]);
    assert_eq!(wavering.path, "Holding");
    assert_eq!(wavering.confidence, Confidence::Low);
    assert!(wavering.reasons[0].contains("wavered"));

    let wavering_below = route_with(
        &archive,
        vec![
            choice("Projects"),
            choice("Projects"),
            choice("Projects/Hardware"),
            choice("(here)"),
        ],
    );
    assert_eq!(
        wavering_below.path, "Projects",
        "stops at the level it was sure of"
    );
    assert_eq!(wavering_below.confidence, Confidence::Low);

    let poor = route_with(
        &archive,
        vec![fitting("Reading", "poorly"), choice("Reading")],
    );
    assert_eq!(
        poor.path, "Holding",
        "a poor fit at the top goes to Holding"
    );
    assert_eq!(poor.confidence, Confidence::Low);
    assert!(poor.reasons[0].contains("poorly"));

    let partly = route_with(
        &archive,
        vec![fitting("Reading", "partly"), choice("Reading")],
    );
    assert_eq!(partly.path, "Reading", "a partial fit keeps the collection");
    assert_eq!(partly.confidence, Confidence::Low);

    let poor_below = route_with(
        &archive,
        vec![
            choice("Projects"),
            choice("Projects"),
            fitting("Projects/Hardware", "poorly"),
            choice("Projects/Hardware"),
        ],
    );
    assert_eq!(
        poor_below.path, "Projects",
        "a poor fit below means the parent"
    );

    let failing = route_with(&archive, vec![Err(ProviderError::Failed("down".into()))]);
    assert_eq!(failing.path, "Holding");

    let collections = archive.collections().unwrap();
    let rules = RoutingRules::default();
    let none = Router::new(&archive, &collections, &rules).route(
        &Registry::new(),
        &candidate(PathBuf::from("/in/x.txt"), Kind::File),
        Path::new("/in"),
        &draft("X", &[]),
    );
    assert_eq!(none.path, "Holding");
    assert!(none.reasons[0].contains("no text model"));
}

#[test]
fn routing_shows_the_model_each_collections_design_document() {
    let (_dir, archive) = sample();
    let collections = archive.collections().unwrap();
    let rules = RoutingRules::default();
    let model = Scripted::new("m", 100, 8192, vec![choice("Reading"), choice("Reading")]);
    let asked = std::sync::Arc::clone(&model.asked);
    let mut registry = Registry::new();
    registry.add(Box::new(model));
    let _ = Router::new(&archive, &collections, &rules).route(
        &registry,
        &candidate(PathBuf::from("/in/paper.pdf"), Kind::File),
        Path::new("/in"),
        &draft("A paper", &["biology"]),
    );
    let asked = asked.lock().unwrap();
    let first = &asked[0].prompt;
    assert!(first.contains("Item: \u{201c}A paper\u{201d}"));
    assert!(first.contains("Projects: Projects"), "{first}");
    assert!(
        first.contains("Not here: Reference material goes in Reading."),
        "{first}"
    );
    assert!(
        !first.contains("Holding"),
        "Holding isn't a choice: {first}"
    );
    assert!(
        !first.contains("Projects/Hardware"),
        "only one level at a time: {first}"
    );
    let names = &asked[0].schema.as_ref().unwrap()["properties"]["choice"]["enum"];
    let reversed = &asked[1].schema.as_ref().unwrap()["properties"]["choice"]["enum"];
    let mut flipped = names.as_array().unwrap().clone();
    flipped.reverse();
    assert_eq!(
        reversed.as_array().unwrap(),
        &flipped,
        "the second pass is in the opposite order"
    );
}

#[test]
fn proposals_carry_the_card_destination_and_applicable_tags() {
    let (_dir, archive) = sample();
    let collections = archive.collections().unwrap();
    assert_eq!(
        tags_for(
            &archive,
            &collections,
            "Projects/Hardware",
            &[Tag("electronics".into()), Tag("biology".into())]
        ),
        [Tag("electronics".into())]
    );

    let route = route_with(
        &archive,
        vec![
            choice("Projects"),
            choice("Projects"),
            choice("Projects/Hardware"),
            choice("Projects/Hardware"),
        ],
    );
    let mut card = draft("Lamp notes", &["electronics", "biology"]);
    card.new_tag = Some(Tag("lighting".into()));
    let item = candidate(PathBuf::from("/in/notes.txt"), Kind::File);
    let batch = Batch {
        archive: &archive,
        collections: &collections,
        id: Id::new_v7(),
        device: Id::new_v7(),
    };
    let assessment = floor("notes.txt", "", batch.default_sensitivity(&route));
    assert_eq!(assessment.level, Sensitivity::Personal);
    let entry = batch
        .propose(&item, &card, &route, &assessment, &[])
        .unwrap();
    assert_eq!(entry.state, State::Proposed);
    assert_eq!(entry.destination.path, "Projects/Hardware");
    assert_eq!(entry.card.tags, [Tag("electronics".into())]);
    assert_eq!(entry.proposed_tags, [Tag("lighting".into())]);
    assert!(entry.problems[0].message.contains("biology"));
    assert_eq!(entry.attempts.len(), 4);

    let queue = ReviewQueue::of(&archive);
    queue.save(&entry).unwrap();
    let (saved, bad) = queue.list().unwrap();
    assert_eq!(bad.len(), 0);
    assert!(
        saved.iter().any(|e| e.id == entry.id),
        "a real, readable proposal"
    );

    let mut unsettled = draft("notes", &[]);
    unsettled
        .problems
        .push("no model drafted a usable card".into());
    let entry = batch
        .propose(&item, &unsettled, &route, &assessment, &[])
        .unwrap();
    assert_eq!(entry.state, State::NeedsOwner);

    // A card that repeats a card number from the item waits for the owner, restricted.
    let text = "Card 4111 1111 1111 1111 charged";
    let restricted = floor("receipt.txt", text, batch.default_sensitivity(&route));
    let leaky = draft("Receipt for 4111 1111 1111 1111", &[]);
    let entry = batch
        .propose(&item, &leaky, &route, &restricted, &[])
        .unwrap();
    assert_eq!(entry.state, State::NeedsOwner);
    assert_eq!(entry.card.sensitivity, Sensitivity::Restricted);
    assert!(entry.problems.iter().any(|p| p.stage == "sensitivity"));
    assert_eq!(
        entry.sensitivity_reasons,
        ["contains what looks like a card number"]
    );
    let json = serde_json::to_string(&entry).unwrap();
    assert_eq!(
        json.matches("4111 1111 1111 1111").count(),
        1,
        "only the card's own title holds the number; no reason or problem quotes it"
    );
}

#[test]
fn a_second_model_is_asked_only_when_the_first_isnt_sure() {
    use carrel_core::providers::Capability;
    let (_dir, archive) = sample();
    let collections = archive.collections().unwrap();
    let rules = RoutingRules::default();
    let router = Router::new(&archive, &collections, &rules);
    let route = |first: Vec<Result<serde_json::Value, ProviderError>>,
                 second: Vec<Result<serde_json::Value, ProviderError>>| {
        let mut registry = Registry::new();
        registry.add(Box::new(Scripted::new("fast", 100, 8192, first)));
        registry.add(Box::new(Scripted::new("careful", 50, 8192, second)));
        registry.second_opinion(Capability::TextModel, Some(("careful", "m")));
        router.route(
            &registry,
            &candidate(PathBuf::from("/in/notes.txt"), Kind::File),
            Path::new("/in"),
            &draft("Lamp notes", &[]),
        )
    };
    let reading = || {
        vec![
            choice("Reading"),
            choice("Reading"),
            choice("(here)"),
            choice("(here)"),
        ]
    };

    // Sure the first time: the second model isn't asked.
    let sure = route(reading(), vec![]);
    assert_eq!(sure.path, "Reading");
    assert_eq!(sure.attempts.len(), 2, "only the first model");

    // Holding from the first, a place from the second: the second wins, and says why.
    let unsure = route(
        vec![fitting("Reading", "poorly"), choice("Reading")],
        reading(),
    );
    assert_eq!(unsure.path, "Reading");
    assert_eq!(unsure.confidence, Confidence::Medium);
    assert_eq!(unsure.attempts.len(), 4, "every call is kept");
    assert!(
        unsure.reasons[0].contains("fast/m wasn't sure") && unsure.reasons[0].contains("careful/m"),
        "{:?}",
        unsure.reasons
    );

    // Both unsure: Holding stays, and the reasons say both tried.
    let both = route(
        vec![fitting("Reading", "poorly"), choice("Reading")],
        vec![choice("Reading"), choice("Projects")],
    );
    assert_eq!(both.path, "Holding");
    assert!(
        both.reasons
            .last()
            .unwrap()
            .contains("careful/m wasn't sure either")
    );

    // The same model can't second itself.
    let mut registry = Registry::new();
    registry.add(Box::new(Scripted::new(
        "fast",
        100,
        8192,
        vec![fitting("Reading", "poorly"), choice("Reading")],
    )));
    registry.second_opinion(Capability::TextModel, Some(("fast", "m")));
    assert!(
        registry
            .second(Capability::TextModel, ("fast", "m"))
            .is_none()
    );
}

#[test]
fn a_pinned_model_falls_back_only_to_its_second_opinion() {
    use carrel_core::providers::Capability;
    let mut registry = Registry::new();
    registry.add(Box::new(Scripted::new("fast", 100, 8192, vec![])));
    registry.add(Box::new(Scripted::new("careful", 50, 8192, vec![])));
    registry.add(Box::new(Scripted::new("other", 60, 8192, vec![])));
    registry.pin(Capability::TextModel, "careful", "m");
    let names = |r: &Registry| -> Vec<String> {
        r.ranked(Capability::TextModel)
            .iter()
            .map(|(p, _)| p.name().to_owned())
            .collect()
    };
    assert_eq!(names(&registry), ["careful"]);
    registry.second_opinion(Capability::TextModel, Some(("fast", "m")));
    assert_eq!(
        names(&registry),
        ["careful", "fast"],
        "the pinned model first"
    );
}
