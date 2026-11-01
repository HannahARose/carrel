//! Questions for the owner and the glossary (FORMAT.md §7.3, §7.9, and the "Questions" section
//! of `docs/design/librarian.md`).

mod common;

use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::ingest::card::Glossary;
use carrel_core::ingest::questions::ask;
use carrel_core::questions::{
    QuestionEntry, QuestionKind, QuestionQueue, about, add_fact, answer, clear_waiting,
    mark_waiting, suggest,
};
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
    assert_eq!(
        questions[0].suggestions[0].answer, "Projects/Hardware",
        "where they were going is suggested first"
    );

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

#[test]
fn a_group_split_between_collections_asks_where_it_goes_with_likely_answers() {
    let (_dir, archive) = sample();
    let collections = archive.collections().unwrap();
    let mut entries = vec![
        proposal(&archive, "Daves Chicken receipt 1.pdf", Confidence::Medium),
        proposal(&archive, "Daves Chicken receipt 2.pdf", Confidence::Medium),
        proposal(&archive, "Daves Chicken receipt 3.pdf", Confidence::Low),
        proposal(&archive, "lamp.txt", Confidence::Low),
    ];
    entries[2].destination.path = "Reading".into();
    let questions = ask(&archive, &collections, &Glossary::of(&archive), &entries);
    assert_eq!(questions.len(), 1, "{questions:#?}");
    let group = &questions[0];
    assert_eq!(group.kind, QuestionKind::Group);
    assert_eq!(group.subject, "Daves Chicken");
    assert_eq!(group.items.len(), 3);
    let answers: Vec<(&str, bool)> = group
        .suggestions
        .iter()
        .map(|s| (s.answer.as_str(), s.new_collection))
        .collect();
    assert_eq!(
        answers,
        [
            ("Projects/Hardware", false),
            ("Reading", false),
            ("Projects/Hardware/Daves Chicken", true)
        ]
    );
    assert_eq!(
        group.question,
        "These 3 items about “Daves Chicken”: Projects/Hardware, Reading, or a new \
         Projects/Hardware/Daves Chicken collection?"
    );
}

#[test]
fn the_question_settling_the_most_unsure_items_comes_first() {
    let (_dir, archive) = sample();
    let collections = archive.collections().unwrap();
    let entries = [
        proposal(&archive, "ABX-1 results.csv", Confidence::High),
        proposal(&archive, "ABX summary.pdf", Confidence::High),
        proposal(&archive, "ABX plot.png", Confidence::High),
        proposal(&archive, "Cog2 a.txt", Confidence::Low),
        proposal(&archive, "Cog2 b.txt", Confidence::Low),
    ];
    let questions = ask(&archive, &collections, &Glossary::of(&archive), &entries);
    let subjects: Vec<&str> = questions.iter().map(|q| q.subject.as_str()).collect();
    assert_eq!(
        subjects,
        ["Cog2", "ABX"],
        "two unsure items outweigh three sure ones"
    );
}

#[test]
fn a_recurring_sender_mostly_in_one_collection_asks_for_its_own() {
    let (_dir, archive) = sample();
    let items = archive.items().unwrap();
    let hardware: Vec<_> = items
        .iter()
        .filter(|i| i.card.starts_with("Projects/Hardware/"))
        .cloned()
        .collect();
    assert!(hardware.len() >= 3);
    let named: Vec<_> = hardware
        .iter()
        .enumerate()
        .map(|(n, i)| (i.clone(), format!("Northbridge Bank statement {n}.pdf")))
        .collect();
    let proposals = vec![
        proposal(&archive, "Northbridge Bank letter.pdf", Confidence::Low),
        proposal(
            &archive,
            "Northbridge Bank statement 9.pdf",
            Confidence::Medium,
        ),
    ];
    let context = carrel_core::questions::rank::Context::of(&archive).unwrap();
    let asked = about::senders(&named, &proposals, &context);
    assert_eq!(asked.len(), 1, "{asked:#?}");
    let sender = &asked[0];
    assert_eq!(sender.kind, QuestionKind::Sender);
    assert_eq!(
        sender.question,
        "Should “Northbridge Bank” have its own collection?"
    );
    assert_eq!(sender.about.len(), hardware.len());
    assert_eq!(sender.items.len(), 2);
    assert_eq!(
        sender.suggestions[0].answer,
        "Projects/Hardware/Northbridge Bank"
    );
    assert!(sender.suggestions[0].new_collection);
    assert!(sender.suggestions[1].no_change);

    // Too few, and nothing is asked.
    assert_eq!(
        about::senders(&named[..1], &proposals[..1], &context).len(),
        0
    );
}

#[test]
fn an_item_gets_questions_about_what_it_shows_and_who_it_names() {
    let (_dir, archive) = sample();
    let items = archive.items().unwrap();
    let mut item = items[0].clone();
    item.item = Some("Reading/IMG_2041.jpg".into());
    item.kind = carrel_core::card::Kind::File;
    let text = std::fs::read_to_string(archive.root().join(&items[0].card)).unwrap();
    let mut card = carrel_core::card_file::CardFile::parse(&text)
        .unwrap()
        .card()
        .clone();
    card.title = "IMG_2041".into();
    card.summary = Some("With J. Okafor at the lab.".into());
    card.dates = None;
    let today = jiff::civil::date(2026, 11, 1);
    let asked = about::item(&item, &card, &Glossary::of(&archive), today);
    let words: Vec<&str> = asked.iter().map(|q| q.question.as_str()).collect();
    assert_eq!(words, ["What does this photo show?", "Who is J. Okafor?"]);
    assert!(asked.iter().all(|q| q.about == [item.id]));

    add_fact(&archive, "J. Okafor", "My lab partner at Northbridge.").unwrap();
    let asked = about::item(&item, &card, &Glossary::of(&archive), today);
    assert_eq!(asked.len(), 1, "the glossary already says who J. Okafor is");
}

#[test]
fn raising_merges_into_what_was_asked_and_never_asks_a_declined_question_again() {
    let (_dir, archive) = sample();
    let lamp = proposal(&archive, "lamp.txt", Confidence::Low);
    let mut first = QuestionEntry::new(QuestionKind::Term, "scope-ds", "What is “scope-ds”?");
    first.items = vec![lamp.id];
    let saved = answer::raise(&archive, vec![first.clone()]).unwrap();
    assert_eq!(saved[0].id, first.id);

    let mut again = QuestionEntry::new(QuestionKind::Term, "Scope DS", "What is “Scope DS”?");
    again.items = vec![Id::new_v7()];
    let saved = answer::raise(&archive, vec![again.clone()]).unwrap();
    assert_eq!(saved[0].id, first.id, "merged into the first");
    assert_eq!(saved[0].items, [lamp.id, again.items[0]]);
    let (all, _) = QuestionQueue::of(&archive).list().unwrap();
    assert_eq!(all.len(), 1);

    answer::decline(&archive, &all[0]).unwrap();
    let saved = answer::raise(&archive, vec![again]).unwrap();
    assert!(saved.is_empty(), "declined, so not asked again");
}

#[test]
fn tidying_drops_what_the_glossary_answers_and_frees_its_proposals() {
    let (_dir, archive) = sample();
    let review = carrel_core::review::ReviewQueue::of(&archive);
    let (mut entries, _) = review.list().unwrap();
    let mut question = QuestionEntry::new(QuestionKind::Term, "ABX", "What is “ABX”?");
    question.items = vec![entries[0].id];
    QuestionQueue::of(&archive).save(&question).unwrap();
    mark_waiting(&mut entries[0], std::slice::from_ref(&question));
    review.save(&entries[0]).unwrap();

    let ranked = answer::tidy(&archive).unwrap();
    assert_eq!(ranked.open.len(), 1);
    assert_eq!(ranked.open[0].settles, 1);

    add_fact(&archive, "ABX", "An assay kit.").unwrap();
    let ranked = answer::tidy(&archive).unwrap();
    assert!(ranked.open.is_empty());
    assert!(
        ranked.answered[0].1.contains("glossary"),
        "{:?}",
        ranked.answered
    );
    assert_eq!(QuestionQueue::of(&archive).list().unwrap().0.len(), 0);
    let entry = review.find(&entries[0].id.to_string()).unwrap();
    assert_eq!(entry.state, State::Proposed, "{:?}", entry.problems);
}

#[test]
fn a_text_model_suggests_answers_and_none_means_no_suggestions() {
    use carrel_core::providers::Registry;
    let question = QuestionEntry::new(QuestionKind::Item, "IMG_2041", "What does this photo show?");
    let mut registry = Registry::new();
    registry.add(Box::new(common::Scripted::new(
        "m",
        100,
        8192,
        vec![Ok(serde_json::json!({
            "answers": ["Lake Annecy at dusk", "lake annecy at dusk", "A lake in the Alps"]
        }))],
    )));
    let suggested = suggest::with_model(&registry, &question, "Title: IMG_2041");
    let answers: Vec<&str> = suggested.iter().map(|s| s.answer.as_str()).collect();
    assert_eq!(answers, ["Lake Annecy at dusk", "A lake in the Alps"]);
    assert_eq!(
        suggest::with_model(&Registry::new(), &question, "").len(),
        0
    );
}
