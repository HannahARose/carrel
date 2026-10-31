//! Temporary (#221): the same-size edit check, copied in by `.github/workflows/flaky-221.yml`
//! and run in a loop on Windows, against dev's code and this branch's.

use std::path::Path;

use carrel_core::card::{Checksum, Id};
use carrel_core::detect::Rules;
use carrel_core::manifest::Manifest;

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
}

/// The test as it was on dev: two writes in a row, at whatever times the clock gives them.
#[test]
fn unpinned() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("note.txt");
    write(&path, "version 1");
    let rules = Rules::default();
    let first = Manifest::build(dir.path(), Id::new_v7(), &rules, None).unwrap();
    write(&path, "version 2");
    let second = Manifest::build(dir.path(), first.item, &rules, Some(&first)).unwrap();
    assert_eq!(first.diff(&second).changed, ["note.txt"]);
    assert_eq!(second.entries[0].checksum, Checksum::of_bytes(b"version 2"));
}

/// Both writes given the same modification time, as a coarse clock does.
#[test]
fn pinned() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("note.txt");
    let tick = std::time::SystemTime::now();
    let write_at_tick = |contents: &str| {
        write(&path, contents);
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(tick).unwrap();
    };
    write_at_tick("version 1");
    let rules = Rules::default();
    let first = Manifest::build(dir.path(), Id::new_v7(), &rules, None).unwrap();
    write_at_tick("version 2");
    let second = Manifest::build(dir.path(), first.item, &rules, Some(&first)).unwrap();
    assert_eq!(first.diff(&second).changed, ["note.txt"]);
    assert_eq!(second.entries[0].checksum, Checksum::of_bytes(b"version 2"));
}
