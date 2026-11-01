//! What a question is about, shown beside it (`carrel_core::questions::subjects`, #446).

use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::card::{Id, Sensitivity};
use carrel_core::questions::QuestionEntry;
use carrel_core::questions::subjects::subjects;
use carrel_core::review::{REVIEW_DIR, ReviewEntry, ReviewQueue};

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

#[test]
fn a_question_names_its_items_and_their_files_and_keeps_restricted_ones_marked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("archive");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
        &root,
    );
    let archive = Archive::open(&root).unwrap();
    let items = archive.items().unwrap();
    let parts = items
        .iter()
        .find(|i| i.card == "Projects/Hardware/parts-list.csv.md")
        .unwrap();
    let mut question: QuestionEntry = serde_json::from_value(serde_json::json!({
        "carrel": "question",
        "format": "1.0",
        "id": Id::new_v7(),
        "created": "2026-11-01T10:00:00-07:00",
        "kind": "item",
        "subject": "parts-list.csv",
        "question": "What is this list for?",
        "about": [parts.id],
    }))
    .unwrap();

    // And a waiting proposal it holds up, restricted, whose file is outside the archive.
    let scan = dir.path().join("passport.pdf");
    std::fs::write(&scan, "scan").unwrap();
    let text = std::fs::read_to_string(
        root.join(REVIEW_DIR)
            .join("01a18701-0500-776a-9867-08f000000001.json"),
    )
    .unwrap();
    let mut entry: ReviewEntry = serde_json::from_str(&text).unwrap();
    entry.id = Id::new_v7();
    entry.source.path.clone_from(&scan);
    entry.card.title = "Passport".into();
    entry.card.sensitivity = Sensitivity::Restricted;
    ReviewQueue::of(&archive).save(&entry).unwrap();
    question.items = vec![entry.id, Id::new_v7()];

    let found = subjects(&archive, &question).unwrap();
    assert_eq!(found.len(), 2, "a proposal that's gone is left out");
    assert_eq!(found[0].id, parts.id);
    assert!(!found[0].proposal);
    assert_eq!(found[0].card.as_deref(), Some(parts.card.as_str()));
    assert_eq!(
        found[0].path.as_deref(),
        Some(root.join("Projects/Hardware/parts-list.csv").as_path())
    );
    assert!(!found[0].restricted);
    assert_eq!(found[1].title, "Passport");
    assert!(found[1].proposal);
    assert_eq!(found[1].path.as_deref(), Some(scan.as_path()));
    assert!(
        found[1].restricted,
        "its preview waits until it's asked for"
    );

    std::fs::remove_file(&scan).unwrap();
    let found = subjects(&archive, &question).unwrap();
    assert_eq!(found[1].path, None, "a file that's gone has no path");
}
