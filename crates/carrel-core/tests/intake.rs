//! The intake queue: adding items, their order, and processing them one at a time.

use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::coordinator::Holder;
use carrel_core::ingest::IntakeOptions;
use carrel_core::intake::{add, move_to_front, preview, process, retry, waiting};
use carrel_core::providers::Registry;
use carrel_core::questions::QuestionQueue;
use carrel_core::review::{REVIEW_DIR, ReviewQueue, State};

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

struct Setup {
    _dir: tempfile::TempDir,
    archive: Archive,
    incoming: PathBuf,
    holder: Holder,
}

impl Setup {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("archive");
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
            &root,
        );
        for entry in std::fs::read_dir(root.join(REVIEW_DIR)).unwrap() {
            std::fs::remove_file(entry.unwrap().path()).unwrap();
        }
        let incoming = dir.path().join("incoming");
        std::fs::create_dir(&incoming).unwrap();
        Self {
            archive: Archive::open(&root).unwrap(),
            incoming: incoming.canonicalize().unwrap(),
            holder: Holder {
                person: Id::new_v7(),
                device: Id::new_v7(),
            },
            _dir: dir,
        }
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.incoming.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }
}

#[test]
fn adding_queues_each_item_once_and_skips_what_is_waiting_already() {
    let s = Setup::new();
    let a = s.file("a.txt", "first\n");
    let b = s.file("b.txt", "second\n");
    let copy = s.file("b copy.txt", "second\n");

    let planned = preview(
        &s.archive,
        &[a.clone(), b.clone()],
        IntakeOptions::default(),
        s.holder.device,
    )
    .unwrap();
    assert_eq!(planned.added.len(), 2);
    assert_eq!(
        waiting(&s.archive, s.holder.device, false).unwrap(),
        [],
        "a preview queues nothing"
    );

    let queued = add(
        &s.archive,
        &[a.clone(), b.clone(), copy],
        IntakeOptions::default(),
        s.holder.device,
    )
    .unwrap();
    assert_eq!(queued.added.len(), 2);
    assert_eq!(queued.skipped.len(), 1);
    assert!(
        queued.skipped[0].reason.contains("same as another item"),
        "{:?}",
        queued.skipped
    );

    let again = add(&s.archive, &[a], IntakeOptions::default(), s.holder.device).unwrap();
    assert_eq!(again.added, []);
    assert_eq!(again.skipped[0].reason, "already queued");

    let other_device = waiting(&s.archive, Id::new_v7(), true).unwrap();
    assert_eq!(other_device, [], "entries wait for their own device");

    let order: Vec<PathBuf> = waiting(&s.archive, s.holder.device, false)
        .unwrap()
        .into_iter()
        .map(|e| e.path)
        .collect();
    assert_eq!(order, [s.incoming.join("a.txt"), b.clone()]);
    let last = waiting(&s.archive, s.holder.device, false).unwrap()[1].id;
    move_to_front(&s.archive, &last.to_string(), s.holder.device).unwrap();
    assert_eq!(
        waiting(&s.archive, s.holder.device, false).unwrap()[0].path,
        b
    );
}

#[test]
fn processing_saves_the_proposal_and_takes_the_item_off_the_queue() {
    let s = Setup::new();
    let a = s.file("lamp notes.txt", "notes on the lamp\n");
    add(
        &s.archive,
        std::slice::from_ref(&a),
        IntakeOptions::default(),
        s.holder.device,
    )
    .unwrap();
    let entry = waiting(&s.archive, s.holder.device, false)
        .unwrap()
        .remove(0);
    let mut stages = Vec::new();
    let processed = process(
        &s.archive,
        &Registry::with_built_ins(),
        entry,
        s.holder,
        None,
        &mut |p| stages.push(p.stage),
    )
    .unwrap();
    assert!(processed.run.is_ok());
    assert!(!stages.is_empty(), "progress is reported");
    assert_eq!(waiting(&s.archive, s.holder.device, true).unwrap(), []);
    let (proposals, _) = ReviewQueue::of(&s.archive).list().unwrap();
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].source.path, a);
    let checksum = carrel_core::card::Checksum::of_file(&a).unwrap();
    assert_eq!(
        carrel_core::search::texts::TextCache::of(&s.archive)
            .get(&checksum)
            .as_deref(),
        Some("notes on the lamp\n"),
        "kept for search, so it's never read again"
    );
}

#[test]
fn a_failing_item_stays_until_it_has_failed_twice_then_waits_for_a_retry() {
    let s = Setup::new();
    let gone = s.file("gone.txt", "soon gone\n");
    add(
        &s.archive,
        std::slice::from_ref(&gone),
        IntakeOptions::default(),
        s.holder.device,
    )
    .unwrap();
    std::fs::remove_file(&gone).unwrap();
    for attempt in 1..=2 {
        let entry = waiting(&s.archive, s.holder.device, false)
            .unwrap()
            .remove(0);
        let processed = process(
            &s.archive,
            &Registry::with_built_ins(),
            entry,
            s.holder,
            None,
            &mut |_| {},
        )
        .unwrap();
        assert!(processed.run.unwrap_err().contains("no longer there"));
        assert_eq!(processed.entry.attempts, attempt);
    }
    assert_eq!(
        waiting(&s.archive, s.holder.device, false).unwrap(),
        [],
        "stuck"
    );
    let stuck = waiting(&s.archive, s.holder.device, true).unwrap();
    assert_eq!(stuck.len(), 1);
    let retried = retry(&s.archive, &stuck[0].id.to_string()).unwrap();
    assert_eq!((retried.attempts, retried.failure), (0, None));
    assert_eq!(
        waiting(&s.archive, s.holder.device, false).unwrap().len(),
        1
    );
}

#[test]
fn a_question_is_asked_once_items_processed_apart_share_an_unknown_term() {
    let s = Setup::new();
    let plan = s.file("scope-ds plan.txt", "the plan\n");
    let notes = s.file("scope-ds notes.txt", "the notes\n");
    add(
        &s.archive,
        &[plan, notes],
        IntakeOptions::default(),
        s.holder.device,
    )
    .unwrap();
    let registry = Registry::with_built_ins();
    let first = waiting(&s.archive, s.holder.device, false)
        .unwrap()
        .remove(0);
    let one = process(&s.archive, &registry, first, s.holder, None, &mut |_| {}).unwrap();
    assert_eq!(one.questions, [], "one item isn't a pattern yet");
    let second = waiting(&s.archive, s.holder.device, false)
        .unwrap()
        .remove(0);
    let two = process(&s.archive, &registry, second, s.holder, None, &mut |_| {}).unwrap();
    assert_eq!(two.questions.len(), 1, "{:?}", two.questions);
    assert_eq!(two.questions[0].items.len(), 2);

    let (asked, _) = QuestionQueue::of(&s.archive).list().unwrap();
    assert_eq!(asked.len(), 1);
    let (proposals, _) = ReviewQueue::of(&s.archive).list().unwrap();
    assert!(proposals.iter().all(|p| p.state == State::NeedsOwner));
    assert!(
        proposals
            .iter()
            .all(|p| p.problems.iter().filter(|x| x.stage == "question").count() == 1),
        "each is marked once"
    );
}
