//! Preparing typed corrections: worked out without saving, then saved to a proposal or, for a
//! filed item, as one change that refiles it.

use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
use carrel_core::correct::prepare::{PrepareError, item_text, prepare_filed, prepare_review, save};
use carrel_core::history::History;
use carrel_core::providers::Registry;
use carrel_core::review::ReviewQueue;

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
    let root = dir.path().join("archive");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
        &root,
    );
    (dir, Archive::open(&root).unwrap())
}

fn holder() -> Holder {
    Holder {
        person: Id::new_v7(),
        device: Id::new_v7(),
    }
}

#[test]
fn a_proposals_correction_is_worked_out_then_saved_only_when_asked() {
    let (_dir, archive) = sample();
    let registry = Registry::with_built_ins();
    let lines = "created: 2019-03-31\nsensitivity: public\ncollection: Reading";
    let prepared = prepare_review(&archive, &registry, "00000001", lines).unwrap();
    assert!(!prepared.is_empty());
    assert!(
        prepared.changed.contains(&"created"),
        "{:?}",
        prepared.changed
    );
    assert_eq!(
        prepared.moved,
        Some(("Projects/Hardware".into(), "Reading".into()))
    );
    assert_eq!(prepared.to(), Some("Reading"));
    assert_eq!(
        prepared
            .after
            .dates
            .as_ref()
            .and_then(|d| d.created.as_ref())
            .map(|d| d.0.as_str()),
        Some("2019-03-31")
    );
    assert!(prepared.proposal().is_some());
    let queue = ReviewQueue::of(&archive);
    assert_eq!(
        queue.find("00000001").unwrap().destination.path,
        "Projects/Hardware",
        "nothing is saved yet"
    );

    let saved = save(&archive, prepared, || -> Result<Holder, PrepareError> {
        panic!("a proposal is saved without the writer lease")
    })
    .unwrap();
    assert!(!saved.filed);
    assert_eq!(saved.commit, None);
    assert_eq!(queue.find("00000001").unwrap().destination.path, "Reading");
}

#[test]
fn a_bad_line_or_a_missing_proposal_is_an_error() {
    let (_dir, archive) = sample();
    let registry = Registry::with_built_ins();
    let bad = prepare_review(&archive, &registry, "00000001", "created: soon").unwrap_err();
    assert!(bad.to_string().contains("isn't a date"), "{bad}");
    let missing = prepare_review(&archive, &registry, "ffffffff", "created: 2019").unwrap_err();
    assert_ne!(missing.to_string(), "");
}

#[test]
fn a_correction_to_what_it_is_already_is_empty() {
    let (_dir, archive) = sample();
    let registry = Registry::with_built_ins();
    let entry = ReviewQueue::of(&archive).find("00000001").unwrap();
    let same = format!(
        "sensitivity: {}",
        carrel_core::correct::level(entry.card.sensitivity)
    );
    let prepared = prepare_review(&archive, &registry, "00000001", &same).unwrap();
    assert!(prepared.is_empty(), "{:?}", prepared.worked.changes);
}

#[test]
fn a_filed_items_correction_refiles_it_in_one_change() {
    let (_dir, archive) = sample();
    let holder = holder();
    let lease = DiskLock::for_archive(&archive)
        .acquire(holder, "test", DEFAULT_LEASE)
        .unwrap();
    History::start(&archive, &lease).unwrap();
    lease.release().unwrap();
    let registry = Registry::with_built_ins();
    let item = archive
        .items()
        .unwrap()
        .into_iter()
        .find(|i| i.item.as_deref() == Some("Projects/Hardware/parts-list.csv"))
        .unwrap();
    let text = item_text(
        &archive,
        &registry,
        &archive.root().join("Projects/Hardware/parts-list.csv"),
        item.kind,
    );
    assert!(!text.is_empty(), "the built-in reader reads a CSV");

    let lines = "created: 2019-03-31\ncollection: Reading";
    let prepared = prepare_filed(&archive, &registry, item, lines).unwrap();
    assert_eq!(prepared.to(), Some("Reading"));
    assert!(prepared.proposal().is_none());
    assert!(
        archive
            .root()
            .join("Projects/Hardware/parts-list.csv")
            .exists()
    );

    let saved = save(&archive, prepared, || Ok::<_, PrepareError>(holder)).unwrap();
    assert!(saved.filed);
    assert_eq!(saved.card.as_deref(), Some("Reading/parts-list.csv.md"));
    assert!(saved.commit.is_some());
    assert!(archive.root().join("Reading/parts-list.csv").exists());
    let card = std::fs::read_to_string(archive.root().join("Reading/parts-list.csv.md")).unwrap();
    assert!(card.contains("created: 2019-03-31"), "{card}");
    let log = History::open(&archive).unwrap().log().unwrap();
    assert!(
        log[0].summary.contains("Owner: created: 2019-03-31"),
        "{}",
        log[0].summary
    );
}
