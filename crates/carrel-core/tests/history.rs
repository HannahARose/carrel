//! The metadata history repository (FORMAT.md §8).

use std::path::{Path, PathBuf};

use carrel_core::archive::{Archive, InitOptions};
use carrel_core::card::Id;
use carrel_core::card_file::CardFile;
use carrel_core::coordinator::{Coordinator, DEFAULT_LEASE, DiskLock, Holder, WriterLease};
use carrel_core::history::{Change, History, HistoryError, Move, Operation};

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

/// Takes the writer lock as a new person and device, returning the lease and who holds it.
fn writer(archive: &Archive) -> (WriterLease, Id, Id) {
    let holder = Holder {
        person: Id::new_v7(),
        device: Id::new_v7(),
    };
    let lease = DiskLock::for_archive(archive)
        .acquire(holder, "test", DEFAULT_LEASE)
        .unwrap();
    (lease, holder.person, holder.device)
}

/// Files in the HEAD commit, via Git itself.
fn committed_files(root: &Path) -> Vec<String> {
    let repo = git2::Repository::open(root.join("_System/history.git")).unwrap();
    let tree = repo.head().unwrap().peel_to_tree().unwrap();
    let mut files = Vec::new();
    tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
        if entry.kind() == Some(git2::ObjectType::Blob) {
            files.push(format!("{dir}{}", entry.name().unwrap()));
        }
        git2::TreeWalkResult::Ok
    })
    .unwrap();
    files.sort();
    files
}

#[test]
fn a_new_archive_starts_with_one_create_commit() {
    let dir = tempfile::tempdir().unwrap();
    let archive = Archive::init(dir.path(), &InitOptions::new("History test")).unwrap();
    let (lease, person, device) = writer(&archive);
    let history = History::init(&archive, &lease).unwrap();

    let log = history.log().unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].operation, Some(Operation::Create));
    assert_eq!(log[0].summary, "Create archive “History test”");
    assert_eq!((log[0].person, log[0].device), (Some(person), Some(device)));

    let files = committed_files(dir.path());
    for expected in [
        "ARCHIVE.md",
        "README.md",
        "Active/_index.md",
        "Holding/_collection.md",
        "Holding/CHANGELOG.md",
        "_System/archive.json",
        "_System/settings.json",
        "_System/FORMAT.md",
        "_System/glossary.md",
        "_System/rules/markers.yaml",
    ] {
        assert!(
            files.contains(&expected.to_owned()),
            "{expected} not committed: {files:?}"
        );
    }
    assert!(!files.iter().any(|f| f.starts_with(".obsidian")
        || f.starts_with("_System/history.git")
        || f.starts_with("_System/writer.lock")));
    assert!(matches!(
        History::init(&archive, &lease),
        Err(HistoryError::AlreadyInitialized)
    ));
}

#[test]
fn only_metadata_is_tracked_never_item_contents() {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&sample_archive(), dir.path());
    std::fs::write(dir.path().join("Reading/uncataloged notes.md"), "# Mine\n").unwrap();
    let archive = Archive::open(dir.path()).unwrap();
    let (lease, _, _) = writer(&archive);
    History::init(&archive, &lease).unwrap();

    let files = committed_files(dir.path());
    for card in [
        "Projects/oscillator-sim.md",
        "Projects/Hardware/notes.card.md",
        "Projects/Hardware/notes.md.md",
        "Reading/On the Origin of Species.md",
        "Reading/Coupled oscillators note (preprint).pdf.md",
        "Reading/_collection.md",
        "Projects/CHANGELOG.md",
        "Journal/2026/2026-10.md",
        "Active/Checked out/oscillator-sim.md",
        "_System/glossary.md",
        "_System/rules/routing.yaml",
    ] {
        assert!(files.contains(&card.to_owned()), "{card} should be tracked");
    }
    for content in [
        "Reading/Coupled oscillators note.pdf",
        "Projects/Hardware/parts-list.csv",
        "Projects/Hardware/notes.md", // an item that happens to be Markdown
        "Projects/oscillator-sim/README.md", // inside a sealed bundle
        "Projects/field-notes-2025/2025-06-14 pond.md",
        "Reading/uncataloged notes.md",
        ".obsidian/app.json",
    ] {
        assert!(
            !files.contains(&content.to_owned()),
            "{content} must not be tracked"
        );
    }
    assert!(
        !files.iter().any(|f| f.starts_with("_System/manifests/")),
        "manifests stay out (FORMAT.md §8.1)"
    );
}

#[test]
fn commits_record_trailers_and_skip_empty_changes() {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&sample_archive(), dir.path());
    let archive = Archive::open(dir.path()).unwrap();
    let (lease, person, device) = writer(&archive);
    let history = History::init(&archive, &lease).unwrap();

    let nothing = Change::new(Operation::Edit, "No-op", person, device);
    assert_eq!(history.commit(&archive, &nothing, &lease).unwrap(), None);

    let card_path = dir.path().join("Reading/On the Origin of Species.md");
    let mut file = CardFile::parse(&std::fs::read_to_string(&card_path).unwrap()).unwrap();
    file.card_mut().title = "On the Origin of Species, first edition".into();
    std::fs::write(&card_path, file.to_text().unwrap()).unwrap();

    let mut change = Change::new(Operation::Edit, "Retitle the book card", person, device);
    change.body = Some("Clarify which edition is on the shelf.".into());
    change.snapshot = Some("kopia:k123".into());
    change.items = vec![file.card().id];
    let id = history
        .commit(&archive, &change, &lease)
        .unwrap()
        .expect("a change");

    let log = history.log().unwrap();
    assert_eq!(log.len(), 2);
    let entry = &log[0];
    assert_eq!(entry.id, id);
    assert_eq!(entry.summary, "Retitle the book card");
    assert_eq!(entry.operation, Some(Operation::Edit));
    assert_eq!(entry.snapshot.as_deref(), Some("kopia:k123"));
    assert_eq!(entry.items, [file.card().id]);
    assert!(entry.reverts.is_none());
}

#[test]
fn undo_moves_items_back_and_records_what_it_reverted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    copy_dir(&sample_archive(), root);
    let archive = Archive::open(root).unwrap();
    let (lease, person, device) = writer(&archive);
    let history = History::init(&archive, &lease).unwrap();

    // Move the parts list (item and card) from Hardware up to Projects.
    let card_text =
        std::fs::read_to_string(root.join("Projects/Hardware/parts-list.csv.md")).unwrap();
    let item_id = CardFile::parse(&card_text).unwrap().card().id;
    std::fs::rename(
        root.join("Projects/Hardware/parts-list.csv"),
        root.join("Projects/parts-list.csv"),
    )
    .unwrap();
    std::fs::rename(
        root.join("Projects/Hardware/parts-list.csv.md"),
        root.join("Projects/parts-list.csv.md"),
    )
    .unwrap();
    let mut change = Change::new(
        Operation::Move,
        "Move the parts list to Projects",
        person,
        device,
    );
    change.items = vec![item_id];
    change.moves = vec![Move {
        item: item_id,
        from: "Projects/Hardware/parts-list.csv".into(),
        to: "Projects/parts-list.csv".into(),
    }];
    let moved = history
        .commit(&Archive::open(root).unwrap(), &change, &lease)
        .unwrap()
        .unwrap();
    assert!(committed_files(root).contains(&"Projects/parts-list.csv.md".to_owned()));

    let undo = history
        .undo(&Archive::open(root).unwrap(), &moved, &lease)
        .unwrap();
    assert!(
        root.join("Projects/Hardware/parts-list.csv").is_file(),
        "item moved back"
    );
    assert!(
        root.join("Projects/Hardware/parts-list.csv.md").is_file(),
        "card restored"
    );
    assert!(
        !root.join("Projects/parts-list.csv").exists()
            && !root.join("Projects/parts-list.csv.md").exists()
    );
    assert_eq!(
        std::fs::read_to_string(root.join("Projects/Hardware/parts-list.csv.md")).unwrap(),
        card_text,
        "card restored byte for byte"
    );

    let log = history.log().unwrap();
    assert_eq!(log[0].id, undo);
    assert_eq!(log[0].summary, "Undo: Move the parts list to Projects");
    assert_eq!(log[0].reverts.as_deref(), Some(moved.as_str()));
    assert_eq!(log[0].operation, Some(Operation::Move));
    assert_eq!(log[0].moves[0].to, "Projects/Hardware/parts-list.csv");
    assert_eq!(log.len(), 3, "history only grows");
}

#[test]
fn undo_refuses_rather_than_overwriting_later_work() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    copy_dir(&sample_archive(), root);
    let archive = Archive::open(root).unwrap();
    let (lease, person, device) = writer(&archive);
    let history = History::init(&archive, &lease).unwrap();
    let card_path = root.join("Reading/On the Origin of Species.md");
    let set_title = |title: &str| {
        let mut file = CardFile::parse(&std::fs::read_to_string(&card_path).unwrap()).unwrap();
        file.card_mut().title = title.into();
        std::fs::write(&card_path, file.to_text().unwrap()).unwrap();
    };

    set_title("First retitle");
    let first = history
        .commit(
            &archive,
            &Change::new(Operation::Edit, "First", person, device),
            &lease,
        )
        .unwrap()
        .unwrap();
    set_title("Second retitle");
    history
        .commit(
            &archive,
            &Change::new(Operation::Edit, "Second", person, device),
            &lease,
        )
        .unwrap()
        .unwrap();
    let before = std::fs::read_to_string(&card_path).unwrap();

    let err = history.undo(&archive, &first, &lease).unwrap_err();
    assert!(
        matches!(err, HistoryError::UndoConflict(ref paths) if paths == &["Reading/On the Origin of Species.md"]),
        "{err}"
    );
    assert_eq!(
        std::fs::read_to_string(&card_path).unwrap(),
        before,
        "nothing changed"
    );
    assert_eq!(history.log().unwrap().len(), 3);

    assert!(matches!(
        history.undo(&archive, "deadbeef", &lease),
        Err(HistoryError::UnknownCommit(_))
    ));
}

#[test]
fn plain_git_can_read_the_history() {
    if std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let archive = Archive::init(dir.path(), &InitOptions::new("Readable")).unwrap();
    let (lease, _, _) = writer(&archive);
    History::init(&archive, &lease).unwrap();
    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(["--git-dir=_System/history.git", "--work-tree=."])
            .args(args)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    };
    let log = run(&[
        "log",
        "--format=%s%n%(trailers:key=Carrel-Operation,valueonly)",
    ]);
    assert!(
        log.contains("Create archive “Readable”") && log.contains("create"),
        "{log}"
    );
    assert_eq!(
        run(&["status", "--porcelain"]),
        "",
        "Git sees a clean work tree"
    );

    // Without the explicit flags, the repository's own config still finds the work tree.
    let out = std::process::Command::new("git")
        .args(["--git-dir", "_System/history.git", "ls-files"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("ARCHIVE.md"));
}

#[test]
fn archives_opened_by_a_relative_path_record_history() {
    // Inside the build's own temporary folder, which is on the same drive as the current
    // folder on Windows too, so a relative path between them exists.
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let cwd = std::env::current_dir().unwrap();
    let target: Vec<_> = dir.path().components().collect();
    let here: Vec<_> = cwd.components().collect();
    let common = target.iter().zip(&here).take_while(|(a, b)| a == b).count();
    let relative: PathBuf =
        std::iter::repeat_n(std::path::Component::ParentDir, here.len() - common)
            .chain(target[common..].iter().copied())
            .collect::<PathBuf>()
            .join("Relative");
    assert!(relative.is_relative());
    assert!(relative.starts_with(".."), "{}", relative.display());
    let archive = Archive::init(&relative, &InitOptions::new("Relative")).unwrap();
    assert!(archive.root().is_absolute());
    let (lease, _, _) = writer(&archive);
    let history = History::init(&archive, &lease).unwrap();
    assert_eq!(history.log().unwrap().len(), 1);
}

#[test]
fn history_can_start_for_an_archive_made_without_it() {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&sample_archive(), dir.path());
    let archive = Archive::open(dir.path()).unwrap();
    assert!(matches!(
        History::open(&archive),
        Err(HistoryError::NotInitialized)
    ));
    let (lease, person, _) = writer(&archive);
    let history = History::start(&archive, &lease).unwrap();
    let log = history.log().unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].summary, "Start history of “Sample archive”");
    assert_eq!(log[0].operation, Some(Operation::Create));
    assert_eq!(log[0].person, Some(person));
    assert!(
        committed_files(dir.path()).contains(&"Reading/On the Origin of Species.md".to_owned())
    );
    assert!(matches!(
        History::start(&archive, &lease),
        Err(HistoryError::AlreadyInitialized)
    ));
}

#[test]
fn the_first_change_cant_be_undone_and_nothing_is_touched() {
    let dir = tempfile::tempdir().unwrap();
    let archive = Archive::init(&dir.path().join("First"), &InitOptions::new("First")).unwrap();
    let (lease, _, _) = writer(&archive);
    let history = History::init(&archive, &lease).unwrap();
    let first = history.log().unwrap()[0].id.clone();
    let result = history.undo(&archive, &first, &lease);
    assert!(
        matches!(result, Err(HistoryError::UndoBlocked(_))),
        "{result:?}"
    );
    assert!(archive.root().join("_System/archive.json").is_file());
    assert!(archive.root().join("ARCHIVE.md").is_file());
    assert_eq!(history.log().unwrap().len(), 1);
}

#[test]
fn a_changed_setting_is_recorded_and_can_be_undone() {
    use carrel_core::settings::{Key, change};

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("archive");
    let archive = carrel_core::archive::Archive::init(
        &root,
        &carrel_core::archive::InitOptions::new("Settings"),
    )
    .unwrap();
    let holder = carrel_core::coordinator::Holder {
        person: carrel_core::card::Id::new_v7(),
        device: carrel_core::card::Id::new_v7(),
    };
    let lease = carrel_core::coordinator::DiskLock::for_archive(&archive)
        .acquire(holder, "test", carrel_core::coordinator::DEFAULT_LEASE)
        .unwrap();
    History::start(&archive, &lease).unwrap();

    let changed = change(&archive, Key::Originals, "keep", &lease).unwrap();
    assert_eq!(
        (changed.from.as_str(), changed.to.as_str()),
        ("move", "keep")
    );
    let commit = changed.commit.expect("recorded");
    let reopened = carrel_core::archive::Archive::open(&root).unwrap();
    assert_eq!(Key::Originals.get(reopened.settings()), "keep");
    let again = change(&reopened, Key::Originals, "keep", &lease).unwrap();
    assert_eq!(again.commit, None, "the same value changes nothing");

    History::open(&reopened)
        .unwrap()
        .undo(&reopened, &commit, &lease)
        .unwrap();
    let undone = carrel_core::archive::Archive::open(&root).unwrap();
    assert_eq!(Key::Originals.get(undone.settings()), "move");
}
