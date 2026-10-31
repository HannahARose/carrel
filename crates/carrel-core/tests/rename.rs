//! Portable names (FORMAT.md §2.5) and renaming items to them.

use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::card_file::CardFile;
use carrel_core::coordinator::{Coordinator, DEFAULT_LEASE, DiskLock, Holder, WriterLease};
use carrel_core::history::{History, Operation};
use carrel_core::names::check;
use carrel_core::rename::{RenameError, rename};

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

const PLAN: &str = "01a18701-0500-776a-9867-0899f0000010";

/// The sample archive with history, an item whose name isn't portable, and a card in another
/// collection that links to it.
fn setup() -> (tempfile::TempDir, Archive, WriterLease) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("archive");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
        &root,
    );
    // Windows can't create this name at all, which is the point; build it elsewhere.
    let name = if cfg!(windows) {
        "Q3- plan-.txt"
    } else {
        "Q3: plan?.txt"
    };
    let hardware = root.join("Projects/Hardware");
    std::fs::write(hardware.join(name), "plan\n").unwrap();
    std::fs::write(
        hardware.join(format!("{name}.md")),
        format!(
            "---\ncarrel: card\nformat: \"1.0\"\nid: {PLAN}\nkind: file\ntitle: \"Q3 plan\"\n\
             item: \"{name}\"\ndates:\n  added: \"2026-10-30\"\nsensitivity: personal\n\
             checksum: \"sha256:0000000000000000000000000000000000000000000000000000000000000000\"\n\
             size: 5\nplacement:\n  confidence: low\n  by: model\n  at: \"2026-10-30T10:00:00-04:00\"\n\
             ---\n\nThe owner's notes.\n"
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("Reading/Plan review.md"),
        format!(
            "---\ncarrel: card\nformat: \"1.0\"\nid: 01a18701-0500-776a-9867-0899f0000011\n\
             kind: physical\ntitle: \"Plan review\"\nlinks:\n  - type: related\n    id: {PLAN}\n\
             \x20   path: \"../Projects/Hardware/{name}.md\"\nsensitivity: personal\n\
             location: \"Desk drawer\"\n---\n"
        ),
    )
    .unwrap();
    let archive = Archive::open(&root).unwrap();
    let holder = Holder {
        person: Id::new_v7(),
        device: Id::new_v7(),
    };
    let lease = DiskLock::for_archive(&archive)
        .acquire(holder, "test", DEFAULT_LEASE)
        .unwrap();
    History::start(&archive, &lease).unwrap();
    (dir, archive, lease)
}

fn plan(archive: &Archive) -> carrel_core::archive::ItemRef {
    archive
        .items()
        .unwrap()
        .into_iter()
        .find(|i| i.id.to_string() == PLAN)
        .unwrap()
}

#[cfg(not(windows))]
#[test]
fn status_finds_the_name_and_suggests_one() {
    let (_dir, archive, _lease) = setup();
    let findings = check(&archive).unwrap();
    let item = findings
        .iter()
        .find(|f| f.path == "Projects/Hardware/Q3: plan?.txt")
        .unwrap();
    assert_eq!(item.problems, ["contains : ?"]);
    assert_eq!(item.suggestion.as_deref(), Some("Q3- plan-.txt"));
    assert!(
        !findings
            .iter()
            .any(|f| f.path == "Projects/Hardware/Q3: plan?.txt.md"),
        "the card follows its item, so it isn't reported separately"
    );
}

#[test]
fn renaming_moves_item_and_card_relinks_and_can_be_undone() {
    let (_dir, archive, lease) = setup();
    let item = plan(&archive);
    let old = item.item.clone().unwrap();
    let renamed = rename(&archive, &item, "Q3 plan.txt", &lease).unwrap();
    assert_eq!(renamed.from, old);
    assert_eq!(renamed.to, "Projects/Hardware/Q3 plan.txt");
    assert_eq!(renamed.card, "Projects/Hardware/Q3 plan.txt.md");
    assert_eq!(renamed.relinked, ["Reading/Plan review.md"]);

    let root = archive.root();
    assert!(!root.join(&old).exists());
    assert_eq!(
        std::fs::read_to_string(root.join("Projects/Hardware/Q3 plan.txt")).unwrap(),
        "plan\n"
    );
    let card = CardFile::parse(
        &std::fs::read_to_string(root.join("Projects/Hardware/Q3 plan.txt.md")).unwrap(),
    )
    .unwrap();
    assert_eq!(card.card().item.as_deref(), Some("Q3 plan.txt"));
    assert_eq!(card.card().placement, None, "the owner has placed it");
    assert_eq!(
        card.body(),
        "\nThe owner's notes.\n",
        "the owner's text is kept"
    );
    let review =
        CardFile::parse(&std::fs::read_to_string(root.join("Reading/Plan review.md")).unwrap())
            .unwrap();
    assert_eq!(
        review.card().links[0].path.as_deref(),
        Some("../Projects/Hardware/Q3 plan.txt.md")
    );
    let changelog = std::fs::read_to_string(root.join("Projects/Hardware/CHANGELOG.md")).unwrap();
    assert!(
        changelog.contains("to [Q3 plan](Q3%20plan.txt.md)"),
        "{changelog}"
    );
    let listing = std::fs::read_to_string(root.join("Projects/Hardware/_collection.md")).unwrap();
    assert!(listing.contains("| [Q3 plan](Q3%20plan.txt.md) | file | 2026-10-30 |"));
    assert!(
        check(&archive).unwrap().is_empty(),
        "every name is portable now"
    );

    let history = History::open(&archive).unwrap();
    let log = history.log().unwrap();
    assert_eq!(log[0].operation, Some(Operation::Move));
    assert_eq!(log[0].moves[0].to, "Projects/Hardware/Q3 plan.txt");

    history.undo(&archive, &log[0].id, &lease).unwrap();
    assert!(root.join(&old).exists(), "undo renames it back");
    assert!(!root.join("Projects/Hardware/Q3 plan.txt").exists());
}

#[test]
fn a_taken_or_unportable_name_changes_nothing() {
    let (_dir, archive, lease) = setup();
    let item = plan(&archive);
    let before = std::fs::read_to_string(archive.root().join(&item.card)).unwrap();
    assert!(matches!(
        rename(&archive, &item, "PARTS-LIST.CSV", &lease),
        Err(RenameError::Taken(_))
    ));
    assert!(matches!(
        rename(&archive, &item, "plan?.txt", &lease),
        Err(RenameError::NotPortable(_))
    ));
    assert!(matches!(
        rename(&archive, &item, "aux.txt", &lease),
        Err(RenameError::NotPortable(_))
    ));
    assert_eq!(
        std::fs::read_to_string(archive.root().join(&item.card)).unwrap(),
        before
    );
}

#[test]
fn a_rename_can_change_only_case() {
    let (_dir, archive, lease) = setup();
    let item = plan(&archive);
    let first = rename(&archive, &item, "q3 plan.txt", &lease).unwrap();
    let item = plan(&archive);
    let second = rename(&archive, &item, "Q3 Plan.txt", &lease).unwrap();
    assert_eq!(first.to, "Projects/Hardware/q3 plan.txt");
    assert_eq!(second.to, "Projects/Hardware/Q3 Plan.txt");
    let names: Vec<String> = std::fs::read_dir(archive.root().join("Projects/Hardware"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.to_lowercase().starts_with("q3"))
        .collect();
    let mut names = names;
    names.sort();
    assert_eq!(names, ["Q3 Plan.txt", "Q3 Plan.txt.md"]);
}
