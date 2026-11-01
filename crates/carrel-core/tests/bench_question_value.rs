//! Question value (`docs/design/librarian.md`, "Measuring it"): answering the questions a batch
//! raises with their known answers, re-routing, and comparing the gains with the queue's
//! ranking. Driven by a scripted model that drafts, routes and rates from what its prompt says.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::bench::Answers;
use carrel_core::bench::question_value::{
    KnownAnswer, Proposal, known_answer, send_to, summarize_value, value_each, value_questions,
};
use carrel_core::card::Id;
use carrel_core::providers::{
    Capability, Completion, CompletionRequest, Offer, Provider, ProviderError, Registry,
};
use carrel_core::questions::rank::{Ranked, Weighed};
use carrel_core::questions::{QuestionEntry, QuestionKind};
use carrel_core::review::Confidence;
use serde_json::{Value, json};

fn bench() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/bench")
}

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

/// A model that understands "ABX" only once the glossary explains it: it summarizes an ABX
/// item as research when its instructions carry the fact, and files what it understands.
struct Understands {
    offers: Vec<Offer>,
}

impl Understands {
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

    fn answer(request: &CompletionRequest) -> Value {
        let schema = request.schema.clone().unwrap_or(Value::Null);
        let properties = &schema["properties"];
        let system = request.system.as_deref().unwrap_or("");
        if properties.get("choice").is_some() {
            let names: Vec<&str> = properties["choice"]["enum"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let wanted = if request.prompt.contains("Lab work on ABX") {
                Some("Research")
            } else if request.prompt.contains("kettle") {
                Some("Receipts")
            } else {
                None
            };
            return match wanted.filter(|w| names.contains(w)) {
                Some(choice) => json!({"choice": choice, "fit": "well", "reason": "it fits"}),
                None => json!({"choice": names[0], "fit": "poorly", "reason": "not sure"}),
            };
        }
        if properties.get("level").is_some() {
            return json!({"level": "personal", "reason": "nothing private"});
        }
        let summary = if system.contains("- ABX:") && request.prompt.contains("ABX") {
            "Lab work on ABX."
        } else if request.prompt.contains("kettle") {
            "A receipt for a kettle."
        } else {
            "An item."
        };
        json!({"title": "An item", "summary": summary, "created": "", "tags": []})
    }
}

impl Provider for Understands {
    fn name(&self) -> &'static str {
        "understands"
    }
    fn offers(&self) -> &[Offer] {
        &self.offers
    }
    fn complete(
        &self,
        _offer: &str,
        request: &CompletionRequest,
    ) -> Result<Completion, ProviderError> {
        let answer = Self::answer(request);
        Ok(Completion {
            text: answer.to_string(),
            json: Some(answer),
            model: "understands/m".into(),
            usage: None,
        })
    }
}

fn answers() -> Answers {
    serde_saphyr::from_str(
        r#"
items:
  "ABX-plan.txt":
    destinations: [Research]
    sensitivity: personal
    date: null
  "ABX-results.txt":
    destinations: [Research]
    sensitivity: personal
    date: null
  "receipt kettle.txt":
    destinations: [Receipts]
    sensitivity: personal
    date: null
"#,
    )
    .unwrap()
}

#[test]
fn answering_a_term_teaches_the_model_and_routing_improves() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("archive");
    copy_dir(&bench().join("archive"), &root);
    let items = dir.path().join("items");
    std::fs::create_dir_all(&items).unwrap();
    let mut paths = Vec::new();
    for (name, text) in [
        (
            "ABX-plan.txt",
            "Plan for the ABX runs: three plates, two weeks.",
        ),
        (
            "ABX-results.txt",
            "ABX results: the second plate grew fastest.",
        ),
        ("receipt kettle.txt", "Hartley & Sons, kettle 39.99, paid."),
    ] {
        std::fs::write(items.join(name), text).unwrap();
        paths.push(items.join(name));
    }
    let archive = Archive::open(&root).unwrap();
    let glossary = std::fs::read(root.join("_System/glossary.md")).unwrap();
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(Understands::new()));

    let run = value_questions(&archive, &registry, &paths, &answers(), Id::new_v7()).unwrap();

    let kettle = run
        .proposals
        .iter()
        .find(|p| p.item == "receipt kettle.txt")
        .unwrap();
    assert_eq!(kettle.destination, "Receipts");
    assert!(
        run.proposals
            .iter()
            .filter(|p| p.item.starts_with("ABX"))
            .all(|p| p.destination != "Research"),
        "{:?}",
        run.proposals
    );
    let term = run
        .valued
        .iter()
        .find(|v| v.kind == QuestionKind::Term)
        .unwrap_or_else(|| panic!("{:?}", run.valued));
    assert_eq!(term.subject, "ABX");
    assert_eq!(term.rank, 1);
    assert!(matches!(
        &term.answer,
        Some(KnownAnswer::Fact { collection, .. }) if collection == "Research"
    ));
    assert_eq!(term.moved.len(), 2, "{term:?}");
    assert!((term.gain - 2.0).abs() < 1e-9);
    let value = &run.value;
    assert!((value.routing_before - 1.0 / 3.0).abs() < 1e-9);
    assert!((value.routing_after - 1.0).abs() < 1e-9);
    assert!((value.top_k[0].ranked - 1.0).abs() < 1e-9);
    // The glossary is as it was.
    assert_eq!(
        std::fs::read(root.join("_System/glossary.md")).unwrap(),
        glossary
    );
}

fn proposal(item: &str, destination: &str, confidence: Confidence) -> Proposal {
    Proposal {
        item: item.into(),
        id: Id::new_v7(),
        destination: destination.into(),
        confidence,
    }
}

fn question(kind: QuestionKind, subject: &str, about: &[&Proposal]) -> QuestionEntry {
    let mut q = QuestionEntry::new(kind, subject, format!("About {subject}?"));
    q.items = about.iter().map(|p| p.id).collect();
    q
}

#[test]
fn the_ranking_is_compared_with_what_each_answer_wins() {
    let answers: Answers = serde_saphyr::from_str(
        r"
items:
  a.txt: {destinations: [Research], sensitivity: personal, date: null}
  b.txt: {destinations: [Research], sensitivity: personal, date: null}
  c.txt: {destinations: [Travel], sensitivity: personal, date: null}
  d.txt: {destinations: [Receipts], sensitivity: personal, date: null}
  e.txt: {destinations: [Receipts, Manuals], ambiguous: true, sensitivity: personal, date: null}
",
    )
    .unwrap();
    let proposals = vec![
        proposal("a.txt", "Holding", Confidence::Low),
        proposal("b.txt", "Holding", Confidence::Low),
        proposal("c.txt", "Holding", Confidence::Low),
        proposal("d.txt", "Receipts", Confidence::Medium),
        proposal("e.txt", "Holding", Confidence::Low),
    ];
    let p = |i: usize| &proposals[i];
    // The ranking thinks the travel question is worth most, then research, then receipts.
    let ranked = Ranked {
        open: vec![
            Weighed {
                question: question(QuestionKind::Group, "trip", &[p(2), p(3)]),
                settles: 2,
                value: 3.0,
            },
            Weighed {
                question: question(QuestionKind::Collection, "lab", &[p(0), p(1)]),
                settles: 2,
                value: 2.0,
            },
            Weighed {
                question: question(QuestionKind::Item, "e", &[p(4)]),
                settles: 1,
                value: 0.5,
            },
        ],
        ..Ranked::default()
    };
    let mut asked = HashSet::new();
    let mut reroute = |q: &QuestionEntry, answer: &KnownAnswer, items: &[&Proposal]| {
        asked.insert(q.subject.clone());
        match answer {
            KnownAnswer::Collection { collection } => Ok(send_to(collection, items)),
            KnownAnswer::Fact { .. } => Ok(BTreeMap::new()),
        }
    };
    let valued = value_each(&ranked, &proposals, &answers, &[], &mut reroute).unwrap();
    // An item question has no known answer and isn't re-routed.
    assert_eq!(asked, HashSet::from(["trip".to_owned(), "lab".to_owned()]));
    // The trip's items split between Travel and Receipts: the tie goes to the first by path,
    // Receipts, so c.txt stays wrong and d.txt doesn't move.
    assert_eq!(
        valued[0].answer,
        Some(KnownAnswer::Collection {
            collection: "Receipts".into()
        })
    );
    assert!(valued[0].gain.abs() < 1e-9, "{:?}", valued[0]);
    assert!((valued[1].gain - 2.0).abs() < 1e-9);
    assert!(valued[2].answer.is_none());

    let value = summarize_value(&valued, &proposals, &answers, 1);
    // d.txt is right, and e.txt half right in Holding: 1.5 of 5.
    assert_eq!(
        (value.questions, value.answerable, value.set_aside),
        (3, 2, 1)
    );
    assert!((value.routing_before - 0.3).abs() < 1e-9);
    assert!((value.routing_after - 0.7).abs() < 1e-9);
    // Answering the top one wins nothing; the best one question wins 2 points.
    assert!((value.top_k[0].ranked - 0.3).abs() < 1e-9);
    assert!((value.top_k[0].best - 0.7).abs() < 1e-9);
    assert!((value.top_k[1].ranked - 0.7).abs() < 1e-9);
    // Ranked values 3, 2, 0.5 against gains 0, 2, 0: no relation.
    let correlation = value.correlation.unwrap();
    assert!(correlation.abs() < 1e-9, "{correlation}");
}

#[test]
fn a_term_is_answered_with_a_fact_naming_its_collection() {
    let answers = answers();
    let proposals = vec![
        proposal("ABX-plan.txt", "Holding", Confidence::Low),
        proposal("ABX-results.txt", "Holding", Confidence::Low),
    ];
    let archive = Archive::open(&bench().join("archive")).unwrap();
    let collections = archive.collections().unwrap();
    let q = question(QuestionKind::Term, "ABX", &[&proposals[0], &proposals[1]]);
    let Some(KnownAnswer::Fact {
        term,
        fact,
        collection,
    }) = known_answer(&q, &proposals, &answers, &collections)
    else {
        panic!("a term is answered with a fact");
    };
    assert_eq!((term.as_str(), collection.as_str()), ("ABX", "Research"));
    assert!(
        fact.starts_with("items named for it belong in Research"),
        "{fact}"
    );
    let unknown = question(QuestionKind::Collection, "nothing", &[]);
    assert!(known_answer(&unknown, &proposals, &answers, &collections).is_none());
}

/// The test set raises each kind of question ingest asks (#357), whatever model drafts it:
/// a term question for each unknown term its names share, and a group question for each
/// sender, would-be collection and uncertain placement, when the model isn't sure where they
/// go. The model here is never sure, as a small one often isn't.
#[test]
fn the_test_set_raises_each_kind_of_question() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("archive");
    copy_dir(&bench().join("archive"), &root);
    let archive = Archive::open(&root).unwrap();
    let mut items: Vec<PathBuf> = std::fs::read_dir(bench().join("items"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    items.sort();
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(Understands::new()));

    let run = carrel_core::ingest::run::run(
        &archive,
        &registry,
        &items,
        carrel_core::ingest::IntakeOptions::default(),
        Id::new_v7(),
    )
    .unwrap();

    let raised = |kind: QuestionKind| -> HashSet<&str> {
        run.questions
            .iter()
            .filter(|q| q.kind == kind)
            .map(|q| q.subject.as_str())
            .collect()
    };
    let terms = raised(QuestionKind::Term);
    for term in [
        "ORCA", "QPCR", "ISA", "DnD", "LIGO", "EHIC", "BrewLog", "CNC", "PhD", "OSM",
    ] {
        assert!(terms.contains(term), "no question about {term}: {terms:?}");
    }
    let groups = raised(QuestionKind::Group);
    for group in [
        "Calder Building Society",
        "Mariner Travel",
        "Allotment",
        "Larkspur Road",
    ] {
        assert!(
            groups.contains(group),
            "no question about {group}: {groups:?}"
        );
    }

    // Each has a known answer, so the benchmark can value it.
    let answers = Answers::load(&bench().join("answers.yaml")).unwrap();
    let proposals = carrel_core::bench::question_value::proposals_of(&run);
    let collections = archive.collections().unwrap();
    for question in run
        .questions
        .iter()
        .filter(|q| terms.contains(q.subject.as_str()) || groups.contains(q.subject.as_str()))
    {
        assert!(
            known_answer(question, &proposals, &answers, &collections).is_some(),
            "{} has no known answer",
            question.subject
        );
    }
    assert!(
        run.questions.len() >= 14,
        "{} questions",
        run.questions.len()
    );
}
