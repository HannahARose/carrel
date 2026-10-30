//! Creating collections with design documents (FORMAT.md §6).

use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::collections::{CollectionError, NewCollection, create};
use carrel_core::coordinator::{Coordinator, DEFAULT_LEASE, DiskLock, Holder, WriterLease};
use carrel_core::history::{History, Operation};

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

fn setup() -> (tempfile::TempDir, Archive, WriterLease) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("archive");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
        &root,
    );
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

#[test]
fn a_new_collection_has_a_design_document_and_is_linked_and_recorded() {
    let (_dir, archive, lease) = setup();
    let new = NewCollection {
        description: Some("Bank accounts, tax and pensions".into()),
        tags: vec![("tax".into(), "Income tax".into())],
    };
    let created = create(&archive, "Finances", &new, &lease).unwrap();
    assert_eq!(created.path, "Finances");
    assert_eq!(created.design_document, "Finances/_collection.md");

    let collection = archive
        .collections()
        .unwrap()
        .into_iter()
        .find(|c| c.path == "Finances")
        .unwrap();
    assert_eq!(collection.doc.id, created.id);
    assert_eq!(collection.doc.title, "Finances");
    assert_eq!(
        collection.doc.description.as_deref(),
        Some("Bank accounts, tax and pensions")
    );
    assert_eq!(collection.doc.tags[0].name.0, "tax");

    let root = archive.root();
    let document = std::fs::read_to_string(root.join("Finances/_collection.md")).unwrap();
    for section in [
        "## Purpose and scope\n\nBank accounts, tax and pensions.\n",
        "## Not here",
        "## Organization",
        "Part of [Sample archive](../ARCHIVE.md)",
    ] {
        assert!(document.contains(section), "{section}\n---\n{document}");
    }
    assert!(root.join("Finances/CHANGELOG.md").is_file());
    let index = std::fs::read_to_string(root.join("ARCHIVE.md")).unwrap();
    assert!(
        index.contains("[Finances](Finances/_collection.md)"),
        "{index}"
    );

    let log = History::open(&archive).unwrap().log().unwrap();
    assert_eq!(log[0].operation, Some(Operation::Restructure));
    assert!(log[0].summary.contains("Finances"));
}

#[test]
fn subcollections_go_inside_collections_and_link_to_their_parent() {
    let (_dir, archive, lease) = setup();
    create(
        &archive,
        "Projects/Software/",
        &NewCollection::default(),
        &lease,
    )
    .unwrap();
    let document =
        std::fs::read_to_string(archive.root().join("Projects/Software/_collection.md")).unwrap();
    assert!(
        document.contains("Part of [Projects](../_collection.md)"),
        "{document}"
    );
    assert!(
        document.contains("What belongs here"),
        "a prompt to fill in"
    );
    assert!(
        archive
            .collections()
            .unwrap()
            .iter()
            .any(|c| c.path == "Projects/Software")
    );
}

#[test]
fn paths_that_cant_be_collections_are_refused() {
    let (_dir, archive, lease) = setup();
    let none = NewCollection::default();
    let refused = |path: &str| create(&archive, path, &none, &lease).unwrap_err();
    assert!(matches!(refused("Journal"), CollectionError::Reserved(_)));
    assert!(matches!(refused("Holding"), CollectionError::Reserved(_)));
    assert!(matches!(refused("Reading"), CollectionError::Exists(_)));
    assert!(
        matches!(refused("reading"), CollectionError::Exists(_)),
        "case-insensitive disks"
    );
    assert!(matches!(
        refused("Nowhere/Child"),
        CollectionError::NoParent(_)
    ));
    assert!(matches!(refused("_Private"), CollectionError::BadPath(_)));
    assert!(matches!(refused(""), CollectionError::BadPath(_)));
    assert!(matches!(
        refused("Q3: plans"),
        CollectionError::NotPortable(_)
    ));
    let tagged = NewCollection {
        description: None,
        tags: vec![("Bad Tag".into(), "x".into())],
    };
    assert!(matches!(
        create(&archive, "Tagged", &tagged, &lease).unwrap_err(),
        CollectionError::BadTag(_)
    ));
    assert!(!archive.root().join("Tagged").exists(), "nothing half-made");
}
