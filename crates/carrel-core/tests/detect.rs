//! Item kind detection, sealing and regenerable folders (FORMAT.md §4, §7.5).

use std::path::{Path, PathBuf};

use carrel_core::archive::{Archive, InitOptions};
use carrel_core::card::{Kind, Sealing};
use carrel_core::detect::{Rules, is_ignored};

fn sample_archive() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive")
}

fn touch(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "").unwrap();
}

#[test]
fn files_folders_and_bundles() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let rules = Rules::default();

    touch(&root.join("paper.pdf"));
    assert_eq!(
        rules.detect(&root.join("paper.pdf")).unwrap().kind,
        Kind::File
    );

    touch(&root.join("notes/a.txt"));
    touch(&root.join("notes/deep/Cargo.toml")); // markers only count at the top level
    let notes = rules.detect(&root.join("notes")).unwrap();
    assert_eq!((notes.kind, notes.sealed), (Kind::Folder, None));

    touch(&root.join("rust-app/Cargo.toml"));
    touch(&root.join("rust-app/src/main.rs"));
    let app = rules.detect(&root.join("rust-app")).unwrap();
    assert_eq!((app.kind, app.sealed), (Kind::Bundle, Some(Sealing::Hard)));
    assert_eq!(app.markers, ["Cargo.toml"]);

    std::fs::create_dir_all(root.join("mac-app/Mac App.xcodeproj")).unwrap();
    touch(&root.join("mac-app/.git")); // a worktree's .git is a file, and still a marker
    let mac = rules.detect(&root.join("mac-app")).unwrap();
    assert_eq!(mac.kind, Kind::Bundle);
    assert_eq!(mac.markers, [".git", "Mac App.xcodeproj"]);

    touch(&root.join("board/Lamp.KICAD_PRO"));
    assert_eq!(
        rules.detect(&root.join("board")).unwrap().kind,
        Kind::Bundle,
        "case-insensitive"
    );

    touch(&root.join("junk/.DS_Store"));
    touch(&root.join("junk/._Cargo.toml"));
    assert_eq!(
        rules.detect(&root.join("junk")).unwrap().kind,
        Kind::Folder,
        "ignored files aren't markers"
    );
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_never_followed() {
    let dir = tempfile::tempdir().unwrap();
    touch(&dir.path().join("project/Cargo.toml"));
    std::os::unix::fs::symlink(dir.path().join("project"), dir.path().join("link")).unwrap();
    assert_eq!(
        Rules::default()
            .detect(&dir.path().join("link"))
            .unwrap()
            .kind,
        Kind::File
    );
}

#[test]
fn regenerable_folders_are_found_but_not_searched() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    touch(&root.join("Cargo.toml"));
    touch(&root.join("target/debug/app"));
    touch(&root.join("web/node_modules/react/node_modules/x/index.js"));
    touch(&root.join("web/src/build.rs")); // a file named like a regenerable folder isn't one
    touch(&root.join("py/__pycache__/m.pyc"));
    touch(&root.join(".git/objects/target/x")); // .git is never searched
    let found = Rules::default().regenerable_folders(root).unwrap();
    let expected: Vec<PathBuf> = ["py/__pycache__", "target", "web/node_modules"]
        .iter()
        .map(PathBuf::from)
        .collect();
    assert_eq!(found, expected);
}

#[test]
fn the_sample_archive_is_detected_as_its_cards_say() {
    let archive = Archive::open(&sample_archive()).unwrap();
    let detect = |p: &str| archive.detect(&sample_archive().join(p)).unwrap();
    let sim = detect("Projects/oscillator-sim");
    assert_eq!((sim.kind, sim.sealed), (Kind::Bundle, Some(Sealing::Hard)));
    assert_eq!(sim.markers, ["Cargo.toml"]);
    assert_eq!(detect("Projects/Hardware/lamp-build").kind, Kind::Folder);
    assert_eq!(detect("Projects/Hardware/parts-list.csv").kind, Kind::File);
    // Soft sealing is the owner's choice, so detection reports a plain folder.
    assert_eq!(detect("Projects/field-notes-2025").kind, Kind::Folder);
}

#[test]
fn archives_use_their_own_rules() {
    let dir = tempfile::tempdir().unwrap();
    let archive = Archive::init(dir.path(), &InitOptions::new("Rules test")).unwrap();
    assert_eq!(archive.rules().unwrap(), Rules::default());

    std::fs::write(
        dir.path().join("_System/rules/markers.yaml"),
        "markers: [\"*.blend\"]\nnote: kept\n",
    )
    .unwrap();
    touch(&dir.path().join("Holding/scene/city.blend"));
    touch(&dir.path().join("Holding/code/Cargo.toml"));
    let rules = archive.rules().unwrap();
    assert_eq!(rules.markers.len(), 1);
    assert_eq!(
        rules.regenerable,
        Rules::default().regenerable,
        "the other file keeps its defaults"
    );
    assert_eq!(
        archive
            .detect(&dir.path().join("Holding/scene"))
            .unwrap()
            .kind,
        Kind::Bundle
    );
    assert_eq!(
        archive
            .detect(&dir.path().join("Holding/code"))
            .unwrap()
            .kind,
        Kind::Folder
    );

    std::fs::remove_file(dir.path().join("_System/rules/regenerable.yaml")).unwrap();
    assert_eq!(
        archive.rules().unwrap().regenerable,
        Rules::default().regenerable,
        "missing file means defaults"
    );

    std::fs::write(
        dir.path().join("_System/rules/markers.yaml"),
        "markers: not-a-list\n",
    )
    .unwrap();
    let err = archive.rules().unwrap_err();
    assert!(err.path.ends_with("markers.yaml"), "{err}");
}

#[test]
fn system_and_tool_files_are_ignored() {
    for name in [".DS_Store", "._notes.md", "Icon\r", ".obsidian", ".trash"] {
        assert!(is_ignored(name), "{name:?}");
    }
    for name in [".git", ".gitignore", "obsidian", "trash", "Icon"] {
        assert!(!is_ignored(name), "{name:?}");
    }
}
