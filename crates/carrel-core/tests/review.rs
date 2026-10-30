//! The review queue (FORMAT.md §7.9).

use std::path::{Path, PathBuf};

use carrel_core::archive::{Archive, InitOptions};
use carrel_core::card::Id;
use carrel_core::review::{REVIEW_DIR, ReviewEntry, ReviewError, ReviewQueue, State};

fn sample_entry_text() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/sample-archive")
        .join(REVIEW_DIR)
        .join("01a18701-0500-776a-9867-08f000000001.json");
    std::fs::read_to_string(path).unwrap()
}

fn sample_entry() -> ReviewEntry {
    serde_json::from_str(&sample_entry_text()).unwrap()
}

fn new_archive() -> (tempfile::TempDir, Archive) {
    let dir = tempfile::tempdir().unwrap();
    let archive = Archive::init(dir.path(), &InitOptions::new("Review test")).unwrap();
    (dir, archive)
}

fn with_id(mut entry: ReviewEntry) -> ReviewEntry {
    entry.id = Id::new_v7();
    entry
}

#[test]
fn entries_round_trip_and_keep_unknown_keys() {
    let mut value: serde_json::Value = serde_json::from_str(&sample_entry_text()).unwrap();
    value["future_field"] = serde_json::json!({"kept": true});
    value["destination"]["future"] = serde_json::json!(1);
    let entry: ReviewEntry = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(entry.state, State::Proposed);
    assert_eq!(entry.card.title, "Desk lamp diffuser (print file)");
    assert_eq!(entry.proposed_tags[0].0, "lighting");
    let back = serde_json::to_value(&entry).unwrap();
    assert_eq!(back["future_field"], value["future_field"]);
    assert_eq!(back["destination"]["future"], 1);
}

#[test]
fn the_queue_saves_lists_finds_and_removes_entries() {
    let (_dir, archive) = new_archive();
    let queue = ReviewQueue::of(&archive);
    assert!(
        queue.list().unwrap().0.is_empty(),
        "no queue folder yet is fine"
    );

    let first = with_id(sample_entry());
    let second = with_id(sample_entry());
    queue.save(&second).unwrap();
    queue.save(&first).unwrap();
    let (entries, bad) = queue.list().unwrap();
    assert_eq!(bad.len(), 0);
    let ids: Vec<Id> = entries.iter().map(|e| e.id).collect();
    let mut expected = vec![first.id, second.id];
    expected.sort_by_key(|id| id.0);
    assert_eq!(ids, expected, "oldest first");

    let found = queue.find(&first.id.to_string()[..30]).unwrap();
    assert_eq!(found.id, first.id, "a unique prefix is enough");
    let short = carrel_core::review::short_id(first.id);
    assert_eq!(short.len(), 8);
    assert_eq!(
        queue.find(&short).unwrap().id,
        first.id,
        "so is the short id"
    );
    assert!(matches!(queue.find("01"), Err(ReviewError::Ambiguous(..))));
    assert!(matches!(queue.find("ffff"), Err(ReviewError::NotFound(..))));

    let mut changed = found.clone();
    changed.state = State::Rejected;
    queue.save(&changed).unwrap();
    assert_eq!(
        queue.find(&first.id.to_string()).unwrap().state,
        State::Rejected
    );

    queue.remove(first.id).unwrap();
    assert_eq!(queue.list().unwrap().0.len(), 1);
    assert!(matches!(
        queue.remove(first.id),
        Err(ReviewError::NotFound(..))
    ));
    let leftovers: Vec<PathBuf> = std::fs::read_dir(archive.root().join(REVIEW_DIR))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".tmp"))
        .collect();
    assert_eq!(leftovers, Vec::<PathBuf>::new());
}

#[test]
fn bad_entries_are_reported_not_fatal() {
    let (_dir, archive) = new_archive();
    let queue = ReviewQueue::of(&archive);
    queue.save(&with_id(sample_entry())).unwrap();
    let dir = archive.root().join(REVIEW_DIR);
    std::fs::write(dir.join("broken.json"), "{ not json").unwrap();
    std::fs::write(dir.join("notes.txt"), "ignored").unwrap();
    let mut misnamed = with_id(sample_entry());
    misnamed.state = State::Held;
    std::fs::write(
        dir.join("01a18701-0000-7000-8000-000000000000.json"),
        serde_json::to_string(&misnamed).unwrap(),
    )
    .unwrap();
    let mut two_tags: serde_json::Value = serde_json::to_value(with_id(sample_entry())).unwrap();
    two_tags["proposed_tags"] = serde_json::json!(["one", "two"]);
    let id = two_tags["id"].as_str().unwrap().to_owned();
    std::fs::write(dir.join(format!("{id}.json")), two_tags.to_string()).unwrap();

    let (entries, bad) = queue.list().unwrap();
    assert_eq!(entries.len(), 1);
    let messages: Vec<String> = bad.iter().map(ToString::to_string).collect();
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(
        messages
            .iter()
            .any(|m| m.contains("named for another entry"))
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("at most one proposed tag"))
    );
}

#[test]
fn read_only_archives_cant_be_changed() {
    let (dir, _) = new_archive();
    let path = dir.path().join("_System/archive.json");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace("\"format\": \"1.0\"", "\"format\": \"2.0\""),
    )
    .unwrap();
    let queue = ReviewQueue::of(&Archive::open(dir.path()).unwrap());
    let entry = with_id(sample_entry());
    assert!(matches!(queue.save(&entry), Err(ReviewError::ReadOnly)));
    assert!(matches!(queue.remove(entry.id), Err(ReviewError::ReadOnly)));
}
