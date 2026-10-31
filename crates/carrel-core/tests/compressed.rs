//! Reading inside compressed files and expanding them (`docs/design/compressed.md`).

mod common;

use std::path::{Path, PathBuf};

use carrel_core::compressed::{EntryKind, Error, Limits, Shape, expand, list, read_entry};
use carrel_core::detect::Compressed;
use common::archives::{Item, bz2, gz, tar, xz, zip, zst};

fn project() -> Vec<Item<'static>> {
    vec![
        Item::Dir("Spectrometer/"),
        Item::File(
            "Spectrometer/README.md",
            b"# Spectrometer\nCalibration runs.\n",
        ),
        Item::File("Spectrometer/data/run1.csv", b"nm,counts\n500,12\n"),
        Item::Exec("Spectrometer/tools/plot.sh", b"#!/bin/sh\necho plot\n"),
        Item::Link("Spectrometer/latest.csv", "data/run1.csv"),
        Item::File("__MACOSX/Spectrometer/._README.md", b"junk"),
        Item::File("Spectrometer/.DS_Store", b"junk"),
    ]
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn every_portable_archive_format_lists_the_same_contents() {
    let dir = tempfile::tempdir().unwrap();
    let zipped = dir.path().join("Spectrometer.zip");
    zip(&zipped, &project());
    let plain = tar(&project());
    let archives = [
        (zipped, Compressed::Zip),
        (write(dir.path(), "s.tar", &plain), Compressed::Tar),
        (
            write(dir.path(), "s.tar.gz", &gz(&plain)),
            Compressed::TarGz,
        ),
        (
            write(dir.path(), "s.tar.bz2", &bz2(&plain)),
            Compressed::TarBz2,
        ),
        (
            write(dir.path(), "s.tar.xz", &xz(&plain)),
            Compressed::TarXz,
        ),
        (
            write(dir.path(), "s.tar.zst", &zst(&plain)),
            Compressed::TarZst,
        ),
    ];
    for (path, format) in archives {
        assert_eq!(Compressed::of(&path), Some(format));
        let listing = list(&path, format, &Limits::DEFAULT).unwrap();
        assert_eq!(
            listing.shape(),
            Shape::OneFolder("Spectrometer".into()),
            "{format:?}"
        );
        assert_eq!(
            listing.top_level(),
            ["README.md", "data", "latest.csv", "tools"],
            "{format:?}: junk is dropped"
        );
        assert_eq!(listing.file_count(), 3, "{format:?}");
        let link = listing.find("latest.csv").unwrap();
        assert_eq!(link.kind, EntryKind::Link("data/run1.csv".into()));
        let script = listing.find("tools/plot.sh").unwrap();
        // Zips made on Windows don't record Unix permissions.
        assert!(script.executable || cfg!(windows), "{format:?}");
        let modified = listing.find("README.md").unwrap().modified.unwrap();
        assert!(modified.to_string().starts_with("2024-05-0"), "{modified}");
        assert_eq!(listing.unsafe_entry(), None);
        assert_eq!(listing.too_big(&Limits::DEFAULT), None);
        let readme = read_entry(&path, format, "Spectrometer/README.md", 1000).unwrap();
        assert!(String::from_utf8(readme).unwrap().contains("Calibration"));
    }
}

#[test]
fn single_compressed_files_list_one_file_with_its_size() {
    let dir = tempfile::tempdir().unwrap();
    let csv = b"a,b\n1,2\n3,4\n";
    for (name, bytes, format) in [
        ("data.csv.gz", gz(csv), Compressed::Gz),
        ("data.csv.bz2", bz2(csv), Compressed::Bz2),
        ("data.csv.xz", xz(csv), Compressed::Xz),
        ("data.csv.zst", zst(csv), Compressed::Zst),
    ] {
        let path = write(dir.path(), name, &bytes);
        let listing = list(&path, format, &Limits::DEFAULT).unwrap();
        assert_eq!(listing.shape(), Shape::OneFile("data.csv".into()), "{name}");
        assert_eq!(listing.total_size(), csv.len() as u64, "{name}");
        assert_eq!(read_entry(&path, format, "data.csv", 3).unwrap(), b"a,b");

        let into = tempfile::tempdir().unwrap();
        let expanded = expand(&path, format, into.path(), Some(&Limits::DEFAULT)).unwrap();
        assert_eq!(expanded.path, into.path().join("data.csv"), "{name}");
        assert_eq!(std::fs::read(&expanded.path).unwrap(), csv);
    }
}

#[test]
fn expanding_unwraps_keeps_times_and_permissions_and_drops_junk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Spectrometer export.zip");
    zip(&path, &project());
    let into = tempfile::tempdir().unwrap();
    let expanded = expand(&path, Compressed::Zip, into.path(), Some(&Limits::DEFAULT)).unwrap();
    let root = into.path().join("Spectrometer");
    assert_eq!(expanded.path, root, "the wrapping folder is the item");
    assert_eq!(expanded.files, 3);
    let names: Vec<_> = std::fs::read_dir(into.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["Spectrometer"], "nothing else is left behind");
    assert!(!root.join(".DS_Store").exists());
    assert!(root.join("README.md").is_file());
    #[cfg(unix)]
    assert_eq!(
        std::fs::read_link(root.join("latest.csv")).unwrap(),
        Path::new("data/run1.csv")
    );
    let modified = std::fs::metadata(root.join("data/run1.csv"))
        .unwrap()
        .modified()
        .unwrap();
    let year = jiff::Timestamp::try_from(modified).unwrap().to_string();
    assert!(year.starts_with("2024-05-0"), "{year}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = |p: &str| {
            std::fs::metadata(root.join(p))
                .unwrap()
                .permissions()
                .mode()
        };
        assert_ne!(mode("tools/plot.sh") & 0o111, 0);
        assert_eq!(mode("README.md") & 0o111, 0);
    }

    let loose = dir.path().join("Loose.tar.gz");
    std::fs::write(
        &loose,
        gz(&tar(&[
            Item::File("a.txt", b"a"),
            Item::File("b.txt", b"b"),
        ])),
    )
    .unwrap();
    let into = tempfile::tempdir().unwrap();
    let expanded = expand(
        &loose,
        Compressed::TarGz,
        into.path(),
        Some(&Limits::DEFAULT),
    )
    .unwrap();
    assert_eq!(
        expanded.path,
        into.path().join("Loose"),
        "named after the archive"
    );
    assert!(expanded.path.join("b.txt").is_file());

    let one = dir.path().join("report.pdf.zip");
    zip(&one, &[Item::File("report.pdf", b"%PDF-1.7")]);
    let into = tempfile::tempdir().unwrap();
    let expanded = expand(&one, Compressed::Zip, into.path(), Some(&Limits::DEFAULT)).unwrap();
    assert_eq!(
        expanded.path,
        into.path().join("report.pdf"),
        "one file inside"
    );
}

#[test]
fn unsafe_entries_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut cases: Vec<(&str, Vec<Item<'_>>)> = vec![
        ("slip.zip", vec![Item::File("../../escaped.txt", b"x")]),
        ("link.zip", vec![Item::Link("out", "../../etc/passwd")]),
    ];
    // Windows leaves links out, so nothing can be written through one there.
    if cfg!(unix) {
        cases.push((
            "through.zip",
            vec![
                Item::Link("dir", "."),
                Item::File("dir/x.txt", b"written through a link"),
            ],
        ));
    }
    for (name, items) in cases {
        let path = dir.path().join(name);
        zip(&path, &items);
        let into = tempfile::tempdir().unwrap();
        let error =
            expand(&path, Compressed::Zip, into.path(), Some(&Limits::DEFAULT)).unwrap_err();
        assert!(matches!(error, Error::Unsafe(_)), "{name}: {error}");
    }
    assert!(!dir.path().join("escaped.txt").exists());
    let path = dir.path().join("slip.zip");
    assert!(
        list(&path, Compressed::Zip, &Limits::DEFAULT)
            .unwrap()
            .unsafe_entry()
            .is_some()
    );

    let tarred = write(
        dir.path(),
        "slip.tar",
        &tar(&[Item::Link("up", "/etc"), Item::File("a", b"a")]),
    );
    let into = tempfile::tempdir().unwrap();
    assert!(matches!(
        expand(
            &tarred,
            Compressed::Tar,
            into.path(),
            Some(&Limits::DEFAULT)
        ),
        Err(Error::Unsafe(_))
    ));
}

#[test]
fn corrupt_entries_fail_their_checksum() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("broken.zip");
    let text = b"stored text that will be damaged, long enough to find";
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("notes.txt", stored).unwrap();
    std::io::Write::write_all(&mut zip, text).unwrap();
    zip.finish().unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    let at = bytes.windows(6).position(|w| w == b"stored").unwrap();
    bytes[at] = b'S';
    std::fs::write(&path, bytes).unwrap();
    let into = tempfile::tempdir().unwrap();
    let error = expand(&path, Compressed::Zip, into.path(), Some(&Limits::DEFAULT)).unwrap_err();
    assert!(error.to_string().contains("notes.txt"), "{error}");
}

#[test]
fn bombs_stop_at_the_limits_unless_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "zeros.bin.gz", &gz(&vec![0u8; 100_000]));
    let small = Limits {
        ratio: 10,
        size: 50_000,
        files: 2,
    };
    let listing = list(&path, Compressed::Gz, &small).unwrap();
    assert_eq!(
        listing.total_size(),
        50_001,
        "counting stops past the bound"
    );
    assert!(listing.too_big(&small).is_some());
    let into = tempfile::tempdir().unwrap();
    let error = expand(&path, Compressed::Gz, into.path(), Some(&small)).unwrap_err();
    assert!(matches!(error, Error::TooBig(_)), "{error}");
    let into = tempfile::tempdir().unwrap();
    let expanded = expand(&path, Compressed::Gz, into.path(), None).unwrap();
    assert_eq!(expanded.size, 100_000, "the owner said to go ahead");

    let many = dir.path().join("many.zip");
    zip(
        &many,
        &[
            Item::File("a", b"a"),
            Item::File("b", b"b"),
            Item::File("c", b"c"),
        ],
    );
    let listing = list(&many, Compressed::Zip, &small).unwrap();
    assert!(listing.truncated && listing.too_big(&small).is_some());
    let into = tempfile::tempdir().unwrap();
    assert!(matches!(
        expand(&many, Compressed::Zip, into.path(), Some(&small)),
        Err(Error::TooBig(_))
    ));
}

#[test]
fn encrypted_and_unreadable_archives_say_why() {
    let dir = tempfile::tempdir().unwrap();
    let seven = write(dir.path(), "photos.7z", b"7z\xbc\xaf\x27\x1c\0\x04");
    if carrel_core::compressed::tools::find().is_none() {
        assert!(matches!(
            list(&seven, Compressed::SevenZ, &Limits::DEFAULT),
            Err(Error::NoReader(Compressed::SevenZ))
        ));
    }
    let garbage = write(dir.path(), "fake.zip", b"PK\x03\x04 not really");
    assert!(matches!(
        list(&garbage, Compressed::Zip, &Limits::DEFAULT),
        Err(Error::Invalid(_))
    ));
}

#[cfg(unix)]
#[test]
fn seven_zip_files_are_listed_and_expanded_by_an_installed_tool() {
    use carrel_core::compressed::tools::{Tool, expand_with, list_with};
    use std::os::unix::fs::PermissionsExt as _;

    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("7z");
    // A stand-in for 7-Zip: lists one folder and one file, and expands them.
    std::fs::write(
        &program,
        r#"#!/bin/sh
case "$1" in
l) printf '7-Zip\n\n--\nPath = %s\nType = 7z\n\n----------\nPath = Holiday\nFolder = +\nAttributes = D\n\nPath = Holiday/beach.jpg\nFolder = -\nSize = 5\nModified = 2024-05-06 07:08:00\nAttributes = A\n' "$5" ;;
x) out="${2#-o}"; mkdir -p "$out/Holiday"; printf 'beach' > "$out/Holiday/beach.jpg" ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let tool = Tool::SevenZip(program);
    let path = write(dir.path(), "photos.7z", b"7z\xbc\xaf\x27\x1c\0\x04");

    let listing = list_with(&tool, &path, Compressed::SevenZ).unwrap();
    assert_eq!(listing.shape(), Shape::OneFolder("Holiday".into()));
    assert_eq!(listing.total_size(), 5);
    let base = dir.path().join("out/photos");
    expand_with(&tool, &path, &base).unwrap();
    assert_eq!(
        std::fs::read(base.join("Holiday/beach.jpg")).unwrap(),
        b"beach"
    );
}
