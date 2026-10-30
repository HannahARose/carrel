//! Ingest's stages before any model: intake, kind, fingerprint and duplicates.

use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::card::{Kind, Sealing};
use carrel_core::ingest::{ChecksumIndex, IngestError, IntakeOptions, examine, intake, plan};

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

/// A copy of the sample archive, and a separate folder of things to ingest.
fn setup() -> (tempfile::TempDir, Archive, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&sample_archive(), &dir.path().join("archive"));
    let incoming = dir.path().join("incoming");
    std::fs::create_dir(&incoming).unwrap();
    let archive = Archive::open(&dir.path().join("archive")).unwrap();
    (dir, archive, incoming.canonicalize().unwrap())
}

fn each() -> IntakeOptions {
    IntakeOptions {
        each: true,
        whole: false,
    }
}

#[test]
fn each_argument_is_an_item_and_each_splits_folders() {
    let (_dir, archive, incoming) = setup();
    std::fs::write(incoming.join("b.txt"), "b").unwrap();
    std::fs::write(incoming.join("a.pdf"), "a").unwrap();
    std::fs::write(incoming.join(".hidden"), "h").unwrap();
    std::fs::write(incoming.join(".DS_Store"), "").unwrap();
    std::fs::create_dir(incoming.join("photos")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(incoming.join("a.pdf"), incoming.join("link.pdf")).unwrap();

    let (items, _) = intake(
        &archive,
        std::slice::from_ref(&incoming),
        IntakeOptions::default(),
    )
    .unwrap();
    assert_eq!(
        items,
        std::slice::from_ref(&incoming),
        "a folder is one item by default"
    );

    let (items, skipped) = intake(&archive, std::slice::from_ref(&incoming), each()).unwrap();
    assert_eq!(
        items,
        [
            incoming.join("a.pdf"),
            incoming.join("b.txt"),
            incoming.join("photos")
        ]
    );
    let reasons: Vec<(String, &str)> = skipped
        .iter()
        .map(|s| {
            (
                s.path.file_name().unwrap().to_string_lossy().into_owned(),
                s.reason.as_str(),
            )
        })
        .collect();
    assert!(
        reasons.contains(&(".hidden".into(), "hidden")),
        "{reasons:?}"
    );
    assert!(
        reasons.contains(&(".DS_Store".into(), "ignored (§2.8)")),
        "{reasons:?}"
    );
    #[cfg(unix)]
    assert!(
        reasons.contains(&("link.pdf".into(), "a link")),
        "{reasons:?}"
    );

    let file = incoming.join("a.pdf");
    let (items, _) = intake(&archive, &[file.clone(), file.clone()], each()).unwrap();
    assert_eq!(items, [file], "a path given twice is one item");
}

#[test]
fn pile_folders_need_a_choice() {
    let (_dir, archive, incoming) = setup();
    let downloads = incoming.join("Downloads");
    std::fs::create_dir(&downloads).unwrap();
    std::fs::write(downloads.join("x.txt"), "x").unwrap();
    assert!(matches!(
        intake(
            &archive,
            std::slice::from_ref(&downloads),
            IntakeOptions::default()
        ),
        Err(IngestError::NeedsChoice(_))
    ));
    let whole = IntakeOptions {
        each: false,
        whole: true,
    };
    assert_eq!(
        intake(&archive, std::slice::from_ref(&downloads), whole)
            .unwrap()
            .0,
        std::slice::from_ref(&downloads)
    );
    assert_eq!(
        intake(&archive, std::slice::from_ref(&downloads), each())
            .unwrap()
            .0,
        [downloads.join("x.txt")]
    );
}

#[test]
fn paths_inside_the_archive_or_missing_are_refused() {
    let (_dir, archive, incoming) = setup();
    let inside = archive.root().join("Reading");
    assert!(matches!(
        intake(&archive, &[inside], IntakeOptions::default()),
        Err(IngestError::InsideArchive(_))
    ));
    assert!(matches!(
        intake(&archive, &[incoming.join("nope")], IntakeOptions::default()),
        Err(IngestError::Missing(_))
    ));
}

#[test]
fn examine_finds_the_kind_and_fingerprint() {
    let (_dir, archive, incoming) = setup();
    let file = incoming.join("notes.txt");
    std::fs::write(&file, "hello").unwrap();
    let candidate = examine(&archive, &file).unwrap();
    assert_eq!(candidate.kind, Kind::File);
    assert_eq!(candidate.size, 5);
    assert_eq!(
        candidate.checksum.0,
        "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );

    let project = incoming.join("tool");
    std::fs::create_dir_all(project.join("target/debug")).unwrap();
    std::fs::write(project.join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(project.join("target/debug/big"), vec![0u8; 10_000]).unwrap();
    let candidate = examine(&archive, &project).unwrap();
    assert_eq!(candidate.kind, Kind::Bundle);
    assert_eq!(candidate.sealed, Some(Sealing::Hard));
    assert_eq!(candidate.markers, ["Cargo.toml"]);
    assert_eq!(candidate.size, 10, "regenerable target/ isn't counted");

    let folder = incoming.join("photos");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("a.jpg"), "jpg").unwrap();
    assert_eq!(examine(&archive, &folder).unwrap().kind, Kind::Folder);
}

#[test]
fn duplicates_of_archived_items_and_within_a_batch_are_marked() {
    let (_dir, archive, incoming) = setup();
    let index = ChecksumIndex::build(&archive).unwrap();
    assert!(
        index.len() >= 5,
        "the sample's files and bundles are indexed"
    );

    std::fs::copy(
        archive.root().join("Projects/Hardware/parts-list.csv"),
        incoming.join("parts-list copy.csv"),
    )
    .unwrap();
    copy_dir(
        &archive.root().join("Projects/oscillator-sim"),
        &incoming.join("oscillator-sim"),
    );
    std::fs::write(incoming.join("one.txt"), "same").unwrap();
    std::fs::write(incoming.join("two.txt"), "same").unwrap();

    let (candidates, _) = plan(&archive, std::slice::from_ref(&incoming), each()).unwrap();
    let by_name = |name: &str| {
        candidates
            .iter()
            .position(|c| c.path.file_name().unwrap() == name)
            .unwrap()
    };
    let csv = &candidates[by_name("parts-list copy.csv")];
    assert_eq!(
        csv.duplicate_of.as_ref().unwrap().card,
        "Projects/Hardware/parts-list.csv.md"
    );
    let bundle = &candidates[by_name("oscillator-sim")];
    assert_eq!(
        bundle.duplicate_of.as_ref().unwrap().card,
        "Projects/oscillator-sim.md",
        "bundles match by manifest checksum"
    );
    assert_eq!(
        candidates[by_name("two.txt")].same_as,
        Some(by_name("one.txt"))
    );
    assert_eq!(candidates[by_name("one.txt")].same_as, None);
    assert_eq!(candidates[by_name("one.txt")].duplicate_of, None);
}

#[test]
fn a_run_reports_each_item_as_it_starts_and_each_stage_as_it_ends() {
    use carrel_core::ingest::run::{Stage, run_with_progress};
    use carrel_core::providers::Registry;

    let (_dir, archive, incoming) = setup();
    let a = incoming.join("a.txt");
    let b = incoming.join("b.txt");
    std::fs::write(&a, "first\n").unwrap();
    std::fs::write(&b, "second\n").unwrap();
    let mut seen = Vec::new();
    run_with_progress(
        &archive,
        &Registry::with_built_ins(),
        &[a, b],
        IntakeOptions::default(),
        carrel_core::card::Id::new_v7(),
        &mut |p| seen.push((p.stage, p.done, p.total, p.item.map(str::to_owned))),
    )
    .unwrap();
    let item = |s: &str| Some(s.to_owned());
    let mut expected = Vec::new();
    for stage in [
        Stage::Reading,
        Stage::Drafting,
        Stage::Routing,
        Stage::Rating,
    ] {
        expected.push((stage, 0, 2, item("a.txt")));
        expected.push((stage, 1, 2, item("b.txt")));
        expected.push((stage, 2, 2, None));
    }
    assert_eq!(seen, expected);
}
