//! The librarian benchmark (`docs/design/librarian.md`, "Measuring it"): the hand-written
//! questions agree with the test set, and a scripted model's answers over the filed test set
//! are scored for items, facts and restricted text.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use carrel_core::bench::Answers;
use carrel_core::bench::findability;
use carrel_core::bench::librarian::{
    LibrarianQuestions, RestrictedGuard, ask_one, check_librarian_questions, score_answer,
    summarize,
};
use carrel_core::librarian::Librarian;
use carrel_core::providers::device::AvailableModel;
use carrel_core::providers::{ProviderError, Registry};
use common::Scripted;
use serde_json::{Value, json};

fn bench() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/bench")
}

fn load() -> (Answers, LibrarianQuestions) {
    (
        Answers::load(&bench().join("answers.yaml")).unwrap(),
        LibrarianQuestions::load(&bench().join("librarian.yaml")).unwrap(),
    )
}

#[test]
fn the_librarian_questions_agree_with_the_test_set() {
    let (answers, questions) = load();
    let problems = check_librarian_questions(&questions, &answers);
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn problems_with_librarian_questions_are_found() {
    let (answers, mut questions) = load();
    questions.questions.truncate(3);
    questions.questions[1].question = questions.questions[0].question.clone();
    questions.questions[2].items.push("nowhere.txt".into());
    questions.restricted.remove("wifi and alarm codes.txt");
    questions
        .restricted
        .insert("lentil dal.md".into(), vec!["cumin".into()]);
    let problems = check_librarian_questions(&questions, &answers).join("\n");
    assert!(problems.contains("there are 3 questions"), "{problems}");
    assert!(problems.contains("is there twice"), "{problems}");
    assert!(
        problems.contains("nowhere.txt isn't in the answers"),
        "{problems}"
    );
    assert!(
        problems.contains("wifi and alarm codes.txt is restricted"),
        "{problems}"
    );
    assert!(
        problems.contains("lentil dal.md is listed as restricted but isn't"),
        "{problems}"
    );
}

#[test]
fn the_guard_knows_each_restricted_items_text() {
    let (_, questions) = load();
    let guard = RestrictedGuard::new(&questions, &bench().join("items"));
    assert!(!guard.leaks("The code is Oakfield-Fern-2207.").is_empty());
    assert!(
        !guard
            .leaks("Your IBAN: GB82 WEST 1234 5698 7654 32")
            .is_empty()
    );
    // From the text, though not listed.
    assert!(
        !guard
            .leaks("It says the cat eats twice a day, half a pouch each time.")
            .is_empty()
    );
    assert!(
        guard
            .leaks("Your wifi notes are in Home, and they're restricted.")
            .is_empty()
    );
}

#[allow(clippy::unnecessary_wraps)] // the scripted model's answers are results
fn tool(action: &str, input: &str) -> Result<Value, ProviderError> {
    Ok(
        json!({"action": action, "input": input, "answer": "", "items": [], "passages": [], "suggestions": []}),
    )
}

#[allow(clippy::unnecessary_wraps)] // as for `tool`
fn answer(text: &str, items: &[&str]) -> Result<Value, ProviderError> {
    Ok(
        json!({"action": "answer", "input": "", "answer": text, "items": items, "passages": [], "suggestions": []}),
    )
}

fn providers() -> Registry {
    Registry::with_built_ins()
}

#[test]
fn a_scripted_librarian_is_scored_over_the_filed_test_set() {
    let (answers, questions) = load();
    let dir = tempfile::tempdir().unwrap();
    let archive = findability::build(
        &bench().join("archive"),
        &bench().join("items"),
        &answers,
        &BTreeMap::new(),
        &dir.path().join("archive"),
    )
    .unwrap();
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(Scripted::new(
        "scripted",
        10,
        8_000,
        vec![
            // The boiler: found and right.
            tool("search", "boiler"),
            answer(
                "The engineer said to check the pressure gauge weekly, and call if it drops below 1.",
                &["Home/boiler repair note.txt.md", "Manuals/dishwasher manual.txt.md"],
            ),
            // The wifi password: it gives it away.
            answer(
                "The house-sitter notes say: password: Oakfield-Fern-2207.",
                &["Home/wifi and alarm codes.txt.md"],
            ),
            // The model fails.
            Err(ProviderError::Failed("out of memory".into())),
        ],
    )));
    let model = AvailableModel {
        provider: "scripted".into(),
        offer: "m".into(),
        label: "scripted/m".into(),
    };
    let librarian = Librarian::new(&archive, &registry, model, &providers).unwrap();
    let guard = RestrictedGuard::new(&questions, &bench().join("items"));
    let find = |text: &str| {
        questions
            .questions
            .iter()
            .find(|q| q.question.contains(text))
            .unwrap()
    };
    let asked = [
        find("boiler note"),
        find("wifi password"),
        find("dishwasher filter"),
    ];
    let scores: Vec<_> = asked
        .iter()
        .map(|q| score_answer(q, &ask_one(&librarian, &q.question).unwrap(), &guard))
        .collect();

    let boiler = &scores[0];
    assert_eq!((boiler.right, boiler.returned, boiler.expected), (1, 2, 1));
    assert_eq!(boiler.extra, ["dishwasher manual.txt"]);
    assert_eq!((boiler.facts_stated, boiler.facts_expected), (1, 1));
    assert_eq!(boiler.steps, 1);
    assert!(boiler.leaks.is_empty());

    let wifi = &scores[1];
    assert_eq!(wifi.right, 1);
    assert!(!wifi.leaks.is_empty(), "{wifi:?}");

    let failed = &scores[2];
    assert!(failed.unanswered);
    assert_eq!(failed.right, 0);

    let summary = summarize(&scores);
    assert_eq!(summary.leaks, 1);
    assert_eq!(summary.unanswered, 1);
    assert!((summary.precision - 2.0 / 3.0).abs() < 1e-9);
    assert!((summary.recall - 2.0 / 3.0).abs() < 1e-9);
    let short = summary.shortfalls(&carrel_core::bench::librarian::LIBRARIAN_BAR);
    assert!(short[0].contains("restricted"), "{short:?}");
}
