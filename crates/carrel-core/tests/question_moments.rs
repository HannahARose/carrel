//! Questions at natural moments (`docs/design/librarian.md`, "Questions"): which items to ask
//! about while browsing, wandering and talking to the librarian, how often, and the day's few
//! on Status.

use std::path::Path;

use carrel_core::archive::{Archive, ItemRef};
use carrel_core::card::{Card, Id, Kind};
use carrel_core::ingest::card::Glossary;
use carrel_core::questions::moments::{
    self, Conversing, DAILY, Daily, WANDER_EVERY, open_about, wander_asks,
};
use carrel_core::questions::{QuestionEntry, QuestionQueue, answer};

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

/// A photo with a camera's name and no description, and a person nobody has explained, made
/// from one of the sample archive's cards.
fn photo() -> (ItemRef, Card) {
    let (_dir, archive) = sample();
    let items = archive.items().unwrap();
    let mut item = items[0].clone();
    item.item = Some("Reading/IMG_2041.jpg".into());
    item.kind = Kind::File;
    let text = std::fs::read_to_string(archive.root().join(&items[0].card)).unwrap();
    let mut card = carrel_core::card_file::CardFile::parse(&text)
        .unwrap()
        .card()
        .clone();
    card.title = "IMG_2041".into();
    card.summary = Some("With J. Okafor at the lab.".into());
    card.dates = None;
    (item, card)
}

fn today() -> jiff::civil::Date {
    jiff::civil::date(2026, 11, 1)
}

fn now() -> (jiff::Timestamp, u32) {
    (jiff::Timestamp::now(), 30)
}

#[test]
fn an_item_shows_its_queued_questions_first_then_new_ones_but_never_declined_or_resting() {
    let (item, card) = photo();
    let glossary = Glossary::default();
    let fresh = open_about(&[], &item, &card, &glossary, today(), now());
    let asked: Vec<&str> = fresh.iter().map(|q| q.question.as_str()).collect();
    assert_eq!(asked, ["What does this photo show?", "Who is J. Okafor?"]);

    // Once one is in the queue, it comes first, with its suggestions, and isn't asked twice.
    let mut queued = fresh[1].clone();
    queued.suggestions = vec![carrel_core::questions::Suggestion::new("A friend", None)];
    let open = open_about(
        std::slice::from_ref(&queued),
        &item,
        &card,
        &glossary,
        today(),
        now(),
    );
    assert_eq!(open.len(), 2);
    assert_eq!(open[0].id, queued.id);
    assert_eq!(open[1].question, "What does this photo show?");

    // A declined one is never asked again, and a skipped one rests.
    let mut declined = queued.clone();
    declined.declined = Some(carrel_core::questions::now());
    let open = open_about(&[declined], &item, &card, &glossary, today(), now());
    assert_eq!(open.len(), 1, "{open:?}");
    let mut skipped = queued;
    skipped.skipped = Some(carrel_core::questions::now());
    let open = open_about(&[skipped], &item, &card, &glossary, today(), now());
    assert_eq!(open.len(), 1, "{open:?}");
    assert_eq!(open[0].question, "What does this photo show?");
}

#[test]
fn a_wander_asks_at_most_every_few_steps_and_not_on_the_first() {
    assert!(
        !wander_asks(1, None),
        "the first step is for looking around"
    );
    assert!(wander_asks(2, None));
    assert!(!wander_asks(3, Some(2)));
    assert!(wander_asks(2 + WANDER_EVERY, Some(2)));
}

fn question(item: Id, words: &str) -> QuestionEntry {
    let mut q = QuestionEntry::new(carrel_core::questions::QuestionKind::Item, "x", words);
    q.about = vec![item];
    q
}

#[test]
fn the_librarian_asks_once_an_answer_never_twice_in_a_row_or_about_the_same_item() {
    let (a, b) = (Id::new_v7(), Id::new_v7());
    let open = vec![
        (a, vec![question(a, "Who is A?")]),
        (b, vec![question(b, "Who is B?")]),
    ];
    let mut conversing = Conversing::default();
    assert_eq!(conversing.next(&open).unwrap().question, "Who is A?");
    assert!(
        conversing.next(&open).is_none(),
        "not after two answers in a row"
    );
    assert_eq!(
        conversing.next(&open).unwrap().question,
        "Who is B?",
        "A was asked about already"
    );
    assert!(conversing.next(&open).is_none());
    assert!(conversing.next(&open).is_none(), "nothing left to ask");
}

#[test]
fn asking_puts_a_question_in_the_queue_once() {
    let (_dir, archive) = sample();
    let (item, card) = photo();
    let fresh = open_about(&[], &item, &card, &Glossary::default(), today(), now());
    let first = moments::ask(&archive, &fresh[0]).unwrap();
    let again = moments::ask(&archive, &fresh[0]).unwrap();
    assert_eq!(
        first.id, again.id,
        "the same question, asked again, is the one queued"
    );
    let (queued, _) = QuestionQueue::of(&archive).list().unwrap();
    assert_eq!(
        queued
            .iter()
            .filter(|q| q.question == fresh[0].question)
            .count(),
        1
    );
}

#[test]
fn status_names_a_few_new_questions_a_day_the_same_all_day() {
    let (_dir, archive) = sample();
    let mut daily = Daily::default();
    let (questions, changed) = daily.today(&archive, today()).unwrap();
    assert!(changed);
    assert!(
        !questions.is_empty() && questions.len() <= DAILY,
        "{questions:?}"
    );
    let (queued, _) = QuestionQueue::of(&archive).list().unwrap();
    for q in &questions {
        assert!(queued.iter().any(|e| e.id == q.id), "each joins the queue");
    }

    // The same all day, and nothing new is raised.
    let (again, changed) = daily.today(&archive, today()).unwrap();
    assert!(!changed);
    assert_eq!(
        again.iter().map(|q| q.id).collect::<Vec<_>>(),
        questions.iter().map(|q| q.id).collect::<Vec<_>>()
    );
    assert_eq!(
        QuestionQueue::of(&archive).list().unwrap().0.len(),
        queued.len()
    );

    // One answered (here, settled) drops out.
    answer::settled(&archive, &questions[0]).unwrap();
    let (left, _) = daily.today(&archive, today()).unwrap();
    assert_eq!(left.len(), questions.len() - 1);

    // The record is kept on the device, and read back as it was.
    let dir = tempfile::tempdir().unwrap();
    let path = Daily::path_in(dir.path(), "archive");
    daily.save(&path).unwrap();
    assert_eq!(Daily::load(&path), daily);
}
