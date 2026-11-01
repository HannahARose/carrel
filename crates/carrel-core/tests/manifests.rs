//! Checksums and manifests (FORMAT.md §2.6, §7.7).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use carrel_core::card::{Card, Checksum, DateOrTime, Id, Kind};
use carrel_core::card_file::CardFile;
use carrel_core::detect::Rules;
use carrel_core::manifest::{Diff, Manifest};

fn sample_archive() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive")
}

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// Sets a file's modification time, as a coarse file system clock would.
fn set_modified(path: &Path, time: SystemTime) {
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(time).unwrap();
}

fn paths(m: &Manifest) -> Vec<&str> {
    m.entries.iter().map(|e| e.path.as_str()).collect()
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| (*x).to_owned()).collect()
}

#[test]
fn builds_a_sorted_manifest_of_every_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(&root.join("README.md"), "hello\n");
    write(&root.join("src/main.rs"), "fn main() {}\n");
    write(&root.join("src/a-b.rs"), "");
    write(&root.join("src-old/x"), "x");
    write(&root.join(".DS_Store"), "junk");
    write(&root.join("src/._main.rs"), "junk");
    write(&root.join("target/debug/app"), "binary");
    write(&root.join("web/node_modules/x/index.js"), "x");
    write(&root.join(".git/HEAD"), "ref: refs/heads/main\n");

    let m = Manifest::build(root, Id::new_v7(), &Rules::default(), None).unwrap();
    assert_eq!(
        paths(&m),
        [
            ".git/HEAD",
            "README.md",
            "src-old/x",
            "src/a-b.rs",
            "src/main.rs"
        ],
        "byte order; ignored files and regenerable folders left out; .git kept"
    );
    let readme = &m.entries[1];
    assert_eq!(readme.checksum, Checksum::of_bytes(b"hello\n"));
    assert_eq!(readme.size, 6);
    assert!(
        readme.modified.is_valid() && readme.modified.has_time(),
        "{:?}",
        readme.modified
    );
    assert_eq!(m.total_size(), 21 + 6 + 1 + 13);
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_recorded_not_followed() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("real/big.bin"), "data");
    std::os::unix::fs::symlink("real", dir.path().join("link")).unwrap();
    let m = Manifest::build(dir.path(), Id::new_v7(), &Rules::default(), None).unwrap();
    assert_eq!(paths(&m), ["link", "real/big.bin"]);
    assert_eq!(m.entries[0].checksum, Checksum::of_bytes(b"real"));
    assert_eq!(m.entries[0].size, 4);
}

#[test]
fn tsv_round_trips_including_awkward_names() {
    let dir = tempfile::tempdir().unwrap();
    // Windows can't hold a tab or line feed in a name, so those are checked elsewhere.
    let control = !cfg!(windows);
    write(&dir.path().join("100% done.md"), "a");
    if control {
        write(&dir.path().join("tab\tname"), "b");
        write(&dir.path().join("new\nline"), "c");
    }
    write(&dir.path().join("café/naïve.txt"), "d");
    let m = Manifest::build(dir.path(), Id::new_v7(), &Rules::default(), None).unwrap();
    let tsv = m.to_tsv();
    assert!(tsv.starts_with(&format!("# carrel manifest 1.0\tid={}\tbuilt=", m.item)));
    assert_eq!(
        tsv.lines().count(),
        if control { 5 } else { 3 },
        "one header and one line per file:\n{tsv}"
    );
    assert!(tsv.contains("100%25 done.md\t"));
    assert!(!control || (tsv.contains("tab%09name\t") && tsv.contains("new%0Aline\t")));
    assert_eq!(Manifest::parse(&tsv).unwrap(), m);
    let mut content = String::new();
    for e in &m.entries {
        use std::fmt::Write as _;
        let path = carrel_core::manifest::encode_path(&e.path);
        writeln!(content, "{path}\t{}\t{}", e.checksum.0, e.size).unwrap();
    }
    assert_eq!(
        m.checksum(),
        Checksum::of_bytes(content.as_bytes()),
        "content only (FORMAT.md §7.7)"
    );
}

#[test]
fn the_content_checksum_ignores_times_but_not_contents() {
    let dir = tempfile::tempdir().unwrap();
    write(&dir.path().join("a.txt"), "aaa");
    write(&dir.path().join("sub/b.txt"), "bbb");
    let rules = Rules::default();
    let m = Manifest::build(dir.path(), Id::new_v7(), &rules, None).unwrap();

    let mut retimed = m.clone();
    for e in &mut retimed.entries {
        e.modified = DateOrTime("1999-12-31T23:59:59+00:00".into());
    }
    retimed.item = Id::new_v7();
    assert_ne!(retimed.to_tsv(), m.to_tsv());
    assert_eq!(
        retimed.checksum(),
        m.checksum(),
        "times and the header don't count"
    );

    let mut edited = m.clone();
    edited.entries[0].checksum = Checksum::of_bytes(b"other");
    assert_ne!(edited.checksum(), m.checksum());
    let mut resized = m.clone();
    resized.entries[0].size += 1;
    assert_ne!(resized.checksum(), m.checksum());
    let mut moved = m.clone();
    moved.entries[0].path = "z.txt".into();
    assert_ne!(moved.checksum(), m.checksum());

    let empty = Manifest {
        item: m.item,
        entries: vec![],
        built: None,
        not_downloaded: Vec::new(),
    };
    assert_eq!(empty.checksum(), Checksum::of_bytes(b""));
}

#[test]
fn parse_rejects_malformed_manifests() {
    let id = Id::new_v7();
    let ok = format!(
        "# carrel manifest 1.0\tid={id}\na\tsha256:{}\t1\t2026-10-29T12:00:00-07:00\n",
        "0".repeat(64)
    );
    assert!(Manifest::parse(&ok).is_ok());
    for (bad, why) in [
        (String::new(), "empty"),
        ("# something else\n".to_owned(), "header"),
        ("# carrel manifest 1.0\n".to_owned(), "no id"),
        (ok.replace("\t1\t", "\tone\t"), "size"),
        (ok.replace("sha256:", "md5:"), "checksum"),
        (ok.replace("\t2026", ""), "columns"),
        (
            format!("{ok}{}", ok.lines().nth(1).unwrap()),
            "repeated path",
        ),
    ] {
        assert!(Manifest::parse(&bad).is_err(), "should reject: {why}");
    }
}

#[test]
fn diffs_find_added_removed_changed_and_renamed_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(&root.join("keep.txt"), "same");
    write(&root.join("edit.txt"), "before");
    write(&root.join("gone.txt"), "bye");
    write(&root.join("old/name.txt"), "moving");
    write(&root.join("dup1.txt"), "twin");
    write(&root.join("dup2.txt"), "twin");
    let rules = Rules::default();
    let before = Manifest::build(root, Id::new_v7(), &rules, None).unwrap();

    write(&root.join("edit.txt"), "after");
    std::fs::remove_file(root.join("gone.txt")).unwrap();
    std::fs::create_dir_all(root.join("new")).unwrap();
    std::fs::rename(root.join("old/name.txt"), root.join("new/name.txt")).unwrap();
    std::fs::remove_file(root.join("dup1.txt")).unwrap();
    std::fs::remove_file(root.join("dup2.txt")).unwrap();
    write(&root.join("twin.txt"), "twin"); // one of two identical files reappears
    write(&root.join("fresh.txt"), "hello");
    let after = Manifest::build(root, before.item, &rules, Some(&before)).unwrap();

    assert_eq!(
        before.diff(&after),
        Diff {
            added: s(&["fresh.txt"]),
            removed: s(&["dup2.txt", "gone.txt"]),
            changed: s(&["edit.txt"]),
            renamed: vec![
                ("dup1.txt".into(), "twin.txt".into()),
                ("old/name.txt".into(), "new/name.txt".into())
            ],
        }
    );
    assert!(after.diff(&after).is_empty());
}

#[test]
fn rebuilding_reuses_checksums_of_unchanged_files() {
    let dir = tempfile::tempdir().unwrap();
    let an_hour_ago = SystemTime::now() - Duration::from_secs(3600);
    for (name, contents) in [("a.txt", "aaa"), ("b.txt", "bbb")] {
        write(&dir.path().join(name), contents);
        set_modified(&dir.path().join(name), an_hour_ago);
    }
    let rules = Rules::default();
    let first = Manifest::build(dir.path(), Id::new_v7(), &rules, None).unwrap();

    // Plant a wrong checksum in the previous manifest: if the file isn't re-read, it survives.
    let mut planted = first.clone();
    let marker = Checksum(format!("sha256:{}", "f".repeat(64)));
    planted.entries[0].checksum = marker.clone();
    let reused = Manifest::build(dir.path(), first.item, &rules, Some(&planted)).unwrap();
    assert_eq!(
        reused.entries[0].checksum, marker,
        "unchanged size and time: checksum reused"
    );

    // A different modification time means the file is read again.
    planted.entries[0].modified = DateOrTime("2000-01-01T00:00:00+00:00".into());
    let reread = Manifest::build(dir.path(), first.item, &rules, Some(&planted)).unwrap();
    assert_eq!(reread.entries[0].checksum, Checksum::of_bytes(b"aaa"));

    // The build time survives writing and reading the manifest, and still allows reuse.
    let mut planted = Manifest::parse(&first.to_tsv()).unwrap();
    assert_eq!(planted.built, first.built);
    planted.entries[0].checksum = marker.clone();
    let reused = Manifest::build(dir.path(), first.item, &rules, Some(&planted)).unwrap();
    assert_eq!(reused.entries[0].checksum, marker, "after a round trip");

    // Without a build time, nothing is known to be safe to reuse.
    planted.built = None;
    let reread = Manifest::build(dir.path(), first.item, &rules, Some(&planted)).unwrap();
    assert_eq!(reread.entries[0].checksum, Checksum::of_bytes(b"aaa"));
}

#[test]
fn files_modified_around_the_previous_build_are_read_again() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    write(&path, "aaa");
    set_modified(&path, SystemTime::now() - Duration::from_secs(3600));
    let rules = Rules::default();
    let first = Manifest::build(dir.path(), Id::new_v7(), &rules, None).unwrap();
    let marker = Checksum(format!("sha256:{}", "f".repeat(64)));
    let built = first.built.unwrap();
    // Within the window either side of the build, a same-size edit could hide behind an
    // unchanged modification time, so the planted checksum must not survive.
    let second = Duration::from_secs(1);
    for (time, when) in [
        (built - second, "a second before"),
        (built, "at"),
        (built + second, "a second after"),
    ] {
        set_modified(&path, time);
        let mut planted = Manifest::build(dir.path(), first.item, &rules, None).unwrap();
        planted.built = Some(built);
        planted.entries[0].checksum = marker.clone();
        let rebuilt = Manifest::build(dir.path(), first.item, &rules, Some(&planted)).unwrap();
        assert_eq!(
            rebuilt.entries[0].checksum,
            Checksum::of_bytes(b"aaa"),
            "modified {when} the build"
        );
    }
}

#[test]
fn a_same_size_edit_within_the_same_second_is_noticed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("note.txt");
    // A coarse clock (about 16 ms on Windows, a second or two on some file systems) gives
    // both writes the same modification time; pin it so every platform sees that case.
    let tick = SystemTime::now();
    let write_at_tick = |contents: &str| {
        write(&path, contents);
        set_modified(&path, tick);
    };
    write_at_tick("version 1");
    let rules = Rules::default();
    let first = Manifest::build(dir.path(), Id::new_v7(), &rules, None).unwrap();
    write_at_tick("version 2"); // same size, same modification time
    let second = Manifest::build(dir.path(), first.item, &rules, Some(&first)).unwrap();
    assert_eq!(first.diff(&second).changed, ["note.txt"]);
    assert_eq!(second.entries[0].checksum, Checksum::of_bytes(b"version 2"));
}

#[test]
fn the_sample_archive_manifests_match_their_folders() {
    let root = sample_archive();
    let rules = Rules::default();
    let mut checked = 0;
    for card_path in [
        "Projects/oscillator-sim.md",
        "Projects/field-notes-2025.md",
        "Projects/Hardware/lamp-build.md",
        "Projects/Hardware/notes.card.md",
    ] {
        let card_path = root.join(card_path);
        let file = CardFile::parse(&std::fs::read_to_string(&card_path).unwrap()).unwrap();
        let card: &Card = file.card();
        assert!(matches!(card.kind, Kind::Folder | Kind::Bundle));
        let manifest_ref = card.manifest.as_ref().unwrap();
        let text = std::fs::read_to_string(root.join(&manifest_ref.path)).unwrap();
        let stored = Manifest::parse(&text).unwrap();
        assert_eq!(
            stored.checksum(),
            manifest_ref.checksum,
            "{}",
            card_path.display()
        );
        assert_eq!(stored.item, card.id);
        let folder = card_path
            .parent()
            .unwrap()
            .join(card.item.as_ref().unwrap());
        let fresh = Manifest::build(&folder, card.id, &rules, None).unwrap();
        assert!(
            stored.diff(&fresh).is_empty(),
            "{}: {:?}",
            card_path.display(),
            stored.diff(&fresh)
        );
        assert_eq!(paths(&stored), paths(&fresh), "same order");
        assert_eq!(card.size, Some(fresh.total_size()));
        // A manifest rebuilt from the files, with today's modification times, still matches
        // the checksum recorded in the card.
        assert_eq!(
            fresh.checksum(),
            manifest_ref.checksum,
            "{}",
            card_path.display()
        );
        checked += 1;
    }
    assert_eq!(checked, 4);
}

#[test]
fn the_sample_archive_file_checksums_match() {
    let root = sample_archive();
    for card_path in [
        "Reading/Coupled oscillators note.pdf.md",
        "Projects/Hardware/parts-list.csv.md",
        "Holding/untitled scan.txt.md",
    ] {
        let card_path = root.join(card_path);
        let file = CardFile::parse(&std::fs::read_to_string(&card_path).unwrap()).unwrap();
        let item = card_path
            .parent()
            .unwrap()
            .join(file.card().item.as_ref().unwrap());
        assert_eq!(
            file.card().checksum.as_ref(),
            Some(&Checksum::of_file(&item).unwrap())
        );
    }
}

/// Run with `cargo test --release -p carrel-core --test manifests -- --ignored --nocapture`.
#[test]
#[ignore = "slow: builds a 100,000-file tree"]
fn large_trees_rebuild_quickly() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..100_000 {
        write(
            &dir.path()
                .join(format!("d{}/d{}/f{i}.txt", i % 100, i % 1000)),
            &format!("file {i}"),
        );
    }
    let rules = Rules::default();
    let t = std::time::Instant::now();
    let first = Manifest::build(dir.path(), Id::new_v7(), &rules, None).unwrap();
    let full = t.elapsed();
    let t = std::time::Instant::now();
    let second = Manifest::build(dir.path(), first.item, &rules, Some(&first)).unwrap();
    let incremental = t.elapsed();
    assert_eq!(first.entries, second.entries);
    let t = std::time::Instant::now();
    let tsv = first.to_tsv();
    let parsed = Manifest::parse(&tsv).unwrap();
    let io = t.elapsed();
    assert_eq!(parsed, first);
    println!(
        "100,000 files: full build {full:?}, incremental {incremental:?}, write+parse {io:?}, {} KB",
        tsv.len() / 1024
    );
}

#[test]
fn paths_with_tabs_and_line_breaks_encode_onto_one_line() {
    use carrel_core::manifest::{decode_path, encode_path};
    let awkward = "a\tb\nc\rd%e";
    assert_eq!(encode_path(awkward), "a%09b%0Ac%0Dd%25e");
    assert_eq!(decode_path(&encode_path(awkward)).as_deref(), Some(awkward));
}
