//! The generated listings (FORMAT.md §6.2, §6.3).

use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::listing::refresh;

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
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
        &dir.path().join("archive"),
    );
    let archive = Archive::open(&dir.path().join("archive")).unwrap();
    (dir, archive)
}

fn read(archive: &Archive, path: &str) -> String {
    std::fs::read_to_string(archive.root().join(path)).unwrap()
}

#[test]
fn the_sample_archives_listings_are_exactly_what_carrel_writes() {
    let (_dir, archive) = sample();
    let refreshed = refresh(&archive).unwrap();
    assert_eq!(
        refreshed.changed,
        Vec::<String>::new(),
        "regenerate the fixture's listings"
    );
    assert_eq!(refreshed.problems, Vec::<String>::new());
}

#[test]
fn a_new_item_and_subcollection_appear_everywhere_they_should() {
    let (_dir, archive) = sample();
    let hardware = archive.root().join("Projects/Hardware");
    std::fs::write(hardware.join("wiring (v2).txt"), "wiring\n").unwrap();
    std::fs::write(
        hardware.join("wiring (v2).txt.md"),
        "---\ncarrel: card\nformat: \"1.0\"\nid: 01a18701-0500-776a-9867-0899f0000001\nkind: file\n\
         title: \"Wiring [v2] | final\"\nitem: \"wiring (v2).txt\"\ndates:\n  added: \"2026-10-30\"\n\
         sensitivity: personal\nchecksum: \"sha256:0000000000000000000000000000000000000000000000000000000000000000\"\n\
         size: 7\n---\n",
    )
    .unwrap();
    let prints = hardware.join("Prints");
    std::fs::create_dir_all(&prints).unwrap();
    std::fs::write(
        prints.join("_collection.md"),
        "---\ncarrel: collection\nformat: \"1.0\"\nid: 01a18701-0500-776a-9867-0899f0000002\n\
         title: \"Prints\"\ndescription: \"Files for the 3D printer\"\n---\n\n# Prints\n",
    )
    .unwrap();

    let refreshed = refresh(&archive).unwrap();
    assert_eq!(
        refreshed.changed,
        [
            "Projects/_collection.md",
            "Projects/Hardware/_collection.md",
            "Projects/Hardware/Prints/_collection.md",
            "ARCHIVE.md"
        ]
    );
    let listing = read(&archive, "Projects/Hardware/_collection.md");
    assert!(
        listing.contains(
            "| [Wiring \\[v2\\] \\| final](wiring%20%28v2%29.txt.md) | file | 2026-10-30 |"
        ),
        "{listing}"
    );
    assert!(listing.contains("| [Prints](Prints/_collection.md) | 0 | Files for the 3D printer |"));
    let projects = read(&archive, "Projects/_collection.md");
    assert!(projects.contains("| [Hardware](Hardware/_collection.md) | 5 | "));
    let new = read(&archive, "Projects/Hardware/Prints/_collection.md");
    assert!(
        new.starts_with("---\ncarrel: collection"),
        "owner text is kept"
    );
    assert!(new.contains("# Prints\n\n<!-- carrel:listing -->\n## Contents\n\nPart of [Hardware](../_collection.md)\n"));
    assert!(read(&archive, "ARCHIVE.md").contains("| 3 (+5 in subcollections) |"));

    let again = refresh(&archive).unwrap();
    assert_eq!(again.changed, Vec::<String>::new(), "nothing changes twice");
}

#[test]
fn broken_markers_are_reported_and_left_alone() {
    let (_dir, archive) = sample();
    let path = archive.root().join("Reading/_collection.md");
    let text = read(&archive, "Reading/_collection.md").replace("<!-- /carrel:listing -->\n", "");
    std::fs::write(&path, &text).unwrap();
    let refreshed = refresh(&archive).unwrap();
    assert!(refreshed.problems[0].starts_with("Reading/_collection.md: "));
    assert!(
        !refreshed
            .changed
            .contains(&"Reading/_collection.md".to_owned())
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
}
