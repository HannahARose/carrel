//! Questions for the owner and the glossary (FORMAT.md §7.3, §7.9).

use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::ingest::card::Glossary;
use carrel_core::ingest::questions::ask;
use carrel_core::questions::{QuestionKind, QuestionQueue, add_fact, clear_waiting, mark_waiting};
use carrel_core::review::{Confidence, REVIEW_DIR, ReviewEntry, State};

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

fn proposal(archive: &Archive, name: &str, confidence: Confidence) -> ReviewEntry {
    let text = std::fs::read_to_string(
        archive
            .root()
            .join(REVIEW_DIR)
            .join("01a18701-0500-776a-9867-08f000000001.json"),
    )
    .unwrap();
    let mut entry: ReviewEntry = serde_json::from_str(&text).unwrap();
    entry.id = Id::new_v7();
    entry.source.path = Path::new("/in").join(name);
    entry.destination.confidence = confidence;
    entry
}

#[test]
fn a_recurring_unknown_term_is_one_question_naming_its_items() {
    let (_dir, archive) = sample();
    let collections = archive.collections().unwrap();
    let entries = [
        proposal(&archive, "scope-ds-plan.md", Confidence::Medium),
        proposal(&archive, "scope-ds-notes.txt", Confidence::Low),
        proposal(&archive, "scope-ds budget.csv", Confidence::High),
        proposal(&archive, "lamp.txt", Confidence::Low),
        // Known already: a collection's folder name, and a glossary term, each in two names.
        proposal(&archive, "Hardware-v1.txt", Confidence::Low),
        proposal(&archive, "Hardware-v2.txt", Confidence::Low),
        proposal(&archive, "oscillator-sim run.csv", Confidence::High),
        proposal(&archive, "oscillator-sim plot.png", Confidence::High),
    ];
    let questions = ask(&archive, &collections, &Glossary::of(&archive), &entries);
    assert_eq!(questions.len(), 1, "{questions:#?}");
    let question = &questions[0];
    assert_eq!(question.kind, QuestionKind::Term);
    assert_eq!(question.subject, "scope-ds");
    assert_eq!(question.question, "What is “scope-ds”?");
    assert_eq!(
        question.items,
        [entries[0].id, entries[1].id, entries[2].id]
    );
}

#[test]
fn a_known_term_whose_items_cant_be_placed_asks_where_they_go() {
    let (_dir, archive) = sample();
    let collections = archive.collections().unwrap();
    let entries = [
        proposal(&archive, "oscillator-sim run.csv", Confidence::Low),
        proposal(&archive, "oscillator-sim plot.png", Confidence::Low),
        proposal(&archive, "oscillator-sim notes.md", Confidence::High),
    ];
    let questions = ask(&archive, &collections, &Glossary::of(&archive), &entries);
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].kind, QuestionKind::Collection);
    assert_eq!(questions[0].items, [entries[0].id, entries[1].id]);
    assert!(
        questions[0]
            .choices
            .contains(&"Projects/Hardware".to_owned())
    );
    assert!(!questions[0].choices.contains(&"Holding".to_owned()));

    let queue = QuestionQueue::of(&archive);
    queue.save(&questions[0]).unwrap();
    let (saved, bad) = queue.list().unwrap();
    assert!(bad.is_empty());
    assert_eq!(saved, questions, "questions round-trip through the queue");
}

#[test]
fn waiting_proposals_need_the_owner_until_the_question_is_settled() {
    let (_dir, archive) = sample();
    let collections = archive.collections().unwrap();
    let mut entries = vec![
        proposal(&archive, "ABX-1 results.csv", Confidence::High),
        proposal(&archive, "ABX summary.pdf", Confidence::High),
    ];
    let questions = ask(&archive, &collections, &Glossary::of(&archive), &entries);
    assert_eq!(questions[0].subject, "ABX");
    for entry in &mut entries {
        mark_waiting(entry, &questions);
        assert_eq!(entry.state, State::NeedsOwner);
        assert!(entry.problems[0].message.contains("What is “ABX”?"));
    }
    clear_waiting(&mut entries[0], &questions[0]);
    assert_eq!(entries[0].state, State::Proposed);
    assert!(entries[0].problems.is_empty(), "{:?}", entries[0].problems);
}

#[test]
fn facts_go_into_the_glossary_newest_first() {
    let (_dir, archive) = sample();
    add_fact(
        &archive,
        "scope-ds",
        "The scope-ds repositories\n are part of Cognitogram.",
    )
    .unwrap();
    let glossary = Glossary::of(&archive);
    assert_eq!(
        glossary.facts[0],
        (
            "scope-ds".to_owned(),
            "The scope-ds repositories are part of Cognitogram.".to_owned()
        )
    );
    assert_eq!(glossary.facts[1].0, "oscillator-sim");
    let text = std::fs::read_to_string(archive.root().join("_System/glossary.md")).unwrap();
    assert!(text.starts_with("# Glossary\n\n### scope-ds\n"));
    assert!(text.contains("-->\n\n### oscillator-sim"));

    let dir = tempfile::tempdir().unwrap();
    let fresh = Archive::init(
        &dir.path().join("a"),
        &carrel_core::archive::InitOptions::new("Fresh"),
    )
    .unwrap();
    std::fs::remove_file(fresh.root().join("_System/glossary.md")).ok();
    add_fact(&fresh, "ABX", "An assay.").unwrap();
    assert_eq!(Glossary::of(&fresh).facts.len(), 1);
}
