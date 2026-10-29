//! Creating and opening archives (FORMAT.md §3, §7.1, §12).

use std::path::{Path, PathBuf};

use carrel_core::archive::{Archive, ArchiveError, InitOptions};
use carrel_core::card::DateOrTime;
use carrel_core::documents::OutsideEdits;
use carrel_core::{Access, FORMAT_VERSION, frontmatter, sections};

fn sample_archive() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive")
}

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
fn init_creates_an_archive_that_opens() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("My Archive");
    let mut options = InitOptions::new("My Archive");
    options.now = Some(DateOrTime("2026-10-29T15:00:00-07:00".into()));
    let archive = Archive::init(&root, &options).unwrap();

    assert_eq!(archive.access(), Access::ReadWrite);
    assert_eq!(archive.info().format, FORMAT_VERSION);
    assert!(archive.info().id.is_v7());
    assert_eq!(archive.doc().id, archive.info().id);
    assert_eq!(archive.doc().name, "My Archive");
    assert_eq!(archive.info().created.0, "2026-10-29T15:00:00-07:00");
    assert_eq!(archive.settings().outside_edits, OutsideEdits::Flag);
    assert_eq!(archive.settings().sessions.idle_minutes, 45);

    for file in [
        "README.md",
        "_System/settings.json",
        "_System/glossary.md",
        "_System/rules/routing.yaml",
        "_System/rules/markers.yaml",
        "_System/rules/regenerable.yaml",
        "Holding/CHANGELOG.md",
        "Active/_index.md",
        ".obsidian/app.json",
    ] {
        assert!(root.join(file).is_file(), "{file} missing");
    }
    assert!(root.join("Journal").is_dir());
    let spec = std::fs::read_to_string(root.join("_System/FORMAT.md")).unwrap();
    let repo_spec =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../FORMAT.md"))
            .unwrap();
    assert_eq!(spec, repo_spec, "the archive gets this build's FORMAT.md");

    let collections = archive.collections().unwrap();
    assert_eq!(collections.len(), 1);
    assert_eq!(collections[0].path, "Holding");
    assert!(collections[0].doc.id.is_v7());
    assert_ne!(collections[0].doc.id, archive.info().id);

    // ARCHIVE.md links Holding, and Holding links back.
    let archive_md = std::fs::read_to_string(root.join("ARCHIVE.md")).unwrap();
    let body = frontmatter::split(&archive_md).unwrap().body;
    assert!(
        sections::get(body, "collections")
            .unwrap()
            .unwrap()
            .contains("(Holding/_collection.md)")
    );
    let holding = std::fs::read_to_string(root.join("Holding/_collection.md")).unwrap();
    let body = frontmatter::split(&holding).unwrap().body;
    assert!(
        sections::get(body, "listing")
            .unwrap()
            .unwrap()
            .contains("Part of [My Archive](../ARCHIVE.md)")
    );
}

#[test]
fn names_with_brackets_make_valid_links() {
    assert_eq!(
        carrel_core::archive::escape_link_text(r"A [b] \c"),
        r"A \[b\] \\c"
    );
    let dir = tempfile::tempdir().unwrap();
    Archive::init(dir.path(), &InitOptions::new("Fresh [test] archive")).unwrap();
    let holding = std::fs::read_to_string(dir.path().join("Holding/_collection.md")).unwrap();
    assert!(
        holding.contains(r"Part of [Fresh \[test\] archive](../ARCHIVE.md)"),
        "{holding}"
    );
    let reopened = Archive::open(dir.path()).unwrap();
    assert_eq!(reopened.doc().name, "Fresh [test] archive");
}

#[test]
fn init_refuses_existing_archives_and_folders_with_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("notes.txt"), "hello").unwrap();
    assert!(matches!(
        Archive::init(root, &InitOptions::new("A")),
        Err(ArchiveError::NotEmpty(_))
    ));
    assert_eq!(
        std::fs::read_dir(root).unwrap().count(),
        1,
        "a refused init writes nothing"
    );

    let mut options = InitOptions::new("A");
    options.allow_existing_files = true;
    Archive::init(root, &options).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("notes.txt")).unwrap(),
        "hello"
    );
    assert!(matches!(
        Archive::init(root, &options),
        Err(ArchiveError::AlreadyAnArchive(_))
    ));
}

#[test]
fn opens_the_sample_archive() {
    let archive = Archive::open(&sample_archive()).unwrap();
    assert_eq!(archive.access(), Access::ReadWrite);
    assert_eq!(archive.doc().name, "Sample archive");
    assert_eq!(archive.doc().tags.len(), 3);
    assert_eq!(archive.doc().link_types[0].name, "supersedes");
    assert_eq!(archive.settings().checkout.destination, "desktop");
    let paths: Vec<_> = archive
        .collections()
        .unwrap()
        .into_iter()
        .map(|c| c.path)
        .collect();
    assert_eq!(
        paths,
        ["Holding", "Projects", "Projects/Hardware", "Reading"]
    );
}

#[test]
fn collection_discovery_skips_item_folders() {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&sample_archive(), dir.path());
    // A repo that happens to contain a file named _collection.md is still just an item.
    std::fs::write(
        dir.path().join("Projects/oscillator-sim/_collection.md"),
        "---\ncarrel: collection\nformat: \"1.0\"\nid: 01a18701-0500-776a-9867-0802f3a80e99\ntitle: \"Not a collection\"\n---\n",
    )
    .unwrap();
    let paths: Vec<_> = Archive::open(dir.path())
        .unwrap()
        .collections()
        .unwrap()
        .into_iter()
        .map(|c| c.path)
        .collect();
    assert_eq!(
        paths,
        ["Holding", "Projects", "Projects/Hardware", "Reading"]
    );
}

#[test]
fn newer_major_formats_open_read_only() {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&sample_archive(), dir.path());
    let path = dir.path().join("_System/archive.json");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace("\"format\": \"1.0\"", "\"format\": \"2.0\""),
    )
    .unwrap();
    assert_eq!(
        Archive::open(dir.path()).unwrap().access(),
        Access::ReadOnly
    );

    std::fs::write(
        &path,
        text.replace("\"format\": \"1.0\"", "\"format\": \"1.7\""),
    )
    .unwrap();
    assert_eq!(
        Archive::open(dir.path()).unwrap().access(),
        Access::ReadWrite
    );
}

#[test]
fn open_reports_what_is_wrong() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        Archive::open(dir.path()),
        Err(ArchiveError::NotAnArchive(_))
    ));

    copy_dir(&sample_archive(), dir.path());
    let info = dir.path().join("_System/archive.json");
    let original = std::fs::read_to_string(&info).unwrap();

    std::fs::write(&info, "{ not json").unwrap();
    assert!(matches!(
        Archive::open(dir.path()),
        Err(ArchiveError::Invalid { .. })
    ));

    let other_id = original.replacen("01a18", "01b18", 1);
    std::fs::write(&info, other_id).unwrap();
    assert!(matches!(
        Archive::open(dir.path()),
        Err(ArchiveError::IdMismatch { .. })
    ));
}

#[test]
fn missing_settings_mean_defaults_and_unknown_settings_survive() {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&sample_archive(), dir.path());
    let settings = dir.path().join("_System/settings.json");

    std::fs::remove_file(&settings).unwrap();
    let archive = Archive::open(dir.path()).unwrap();
    assert_eq!(archive.settings().reminders, ["idle", "weekly", "menu-bar"]);

    std::fs::write(
        &settings,
        "{\"outside_edits\": \"drift\", \"future_setting\": {\"x\": 1}}",
    )
    .unwrap();
    let archive = Archive::open(dir.path()).unwrap();
    assert_eq!(archive.settings().outside_edits, OutsideEdits::Drift);
    assert_eq!(archive.settings().extra["future_setting"]["x"], 1);
    let written = serde_json::to_string(archive.settings()).unwrap();
    assert!(
        written.contains("\"future_setting\":{\"x\":1}"),
        "{written}"
    );
}

#[test]
fn find_walks_up_to_the_archive_root() {
    let nested = sample_archive().join("Projects/Hardware/lamp-build");
    let archive = Archive::find(&nested).unwrap();
    assert_eq!(
        archive.root(),
        std::fs::canonicalize(sample_archive()).unwrap()
    );

    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        Archive::find(dir.path()),
        Err(ArchiveError::NotAnArchive(_))
    ));
}
