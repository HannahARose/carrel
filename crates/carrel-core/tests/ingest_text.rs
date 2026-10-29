//! Ingest's text stage: media types, reading, back matter and fitting to a budget.

use std::fmt::Write as _;

use carrel_core::card::{Checksum, Kind};
use carrel_core::detect::Rules;
use carrel_core::ingest::Candidate;
use carrel_core::ingest::text::{CUT, drop_back_matter, fit, media_type, read};
use carrel_core::providers::{Registry, estimate_tokens};

fn candidate(path: &std::path::Path, kind: Kind) -> Candidate {
    Candidate {
        path: path.to_owned(),
        kind,
        sealed: None,
        markers: Vec::new(),
        checksum: Checksum::of_bytes(b""),
        size: 0,
        duplicate_of: None,
        same_as: None,
    }
}

#[test]
fn media_types_come_from_content_first() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, bytes: &[u8]| {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    };
    assert_eq!(
        media_type(&write("paper.txt", b"%PDF-1.7\n...")),
        "application/pdf",
        "the bytes win"
    );
    assert_eq!(
        media_type(&write("scan", b"\x89PNG\r\n\x1a\n....")),
        "image/png"
    );
    assert_eq!(
        media_type(&write("photo.HEIC", b"\0\0\0\x18ftypheic....")),
        "image/heic"
    );
    assert_eq!(
        media_type(&write("notes", b"just some words\n")),
        "text/plain"
    );
    assert_eq!(media_type(&write("data.csv", b"a,b\n1,2\n")), "text/csv");
    assert_eq!(
        media_type(&write("blob", b"\0\x01\x02\x03")),
        "application/octet-stream"
    );
    assert_eq!(
        media_type(&write("letter.docx", b"PK\x03\x04....")),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "a ZIP-based document keeps its extension's type"
    );
    assert_eq!(
        media_type(&write("archive", b"PK\x03\x04....")),
        "application/zip"
    );
}

#[test]
fn files_are_read_through_the_registry() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Registry::with_built_ins();
    let rules = Rules::default();

    let notes = dir.path().join("notes.md");
    std::fs::write(&notes, "# Lamp\nSome notes.\n").unwrap();
    let text = read(&registry, &rules, &candidate(&notes, Kind::File));
    assert_eq!(text.text, "# Lamp\nSome notes.\n");
    assert_eq!(text.media_type.as_deref(), Some("text/markdown"));
    assert_eq!(text.method, "embedded");
    assert_eq!(text.attempts.len(), 1);
    assert_eq!(text.attempts[0].provider, "built-in");
    assert_eq!(text.attempts[0].result, "ok");

    let blob = dir.path().join("firmware.bin");
    std::fs::write(&blob, b"\0\x01\x02").unwrap();
    let text = read(&registry, &rules, &candidate(&blob, Kind::File));
    assert_eq!(text.method, "none");
    assert_eq!(text.text, "");
    assert!(text.note.unwrap().contains("application/octet-stream"));
}

#[test]
fn folders_are_described_by_readme_project_files_and_listing() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("oscillator-sim");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::create_dir_all(project.join("target/debug")).unwrap();
    std::fs::create_dir_all(project.join(".git")).unwrap();
    std::fs::write(
        project.join("README.md"),
        "Simulates coupled oscillators.\n",
    )
    .unwrap();
    std::fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"oscillator-sim\"\n",
    )
    .unwrap();
    std::fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(project.join("target/debug/big"), "x").unwrap();

    let text = read(
        &Registry::with_built_ins(),
        &Rules::default(),
        &candidate(&project, Kind::Bundle),
    );
    assert_eq!(text.method, "listing");
    let t = &text.text;
    assert!(t.starts_with("Folder: oscillator-sim\n"), "{t}");
    assert!(t.contains("README:\nSimulates coupled oscillators."), "{t}");
    assert!(t.contains("Cargo.toml:\n[package]"), "{t}");
    assert!(t.contains("src/\n  main.rs"), "{t}");
    assert!(t.contains("target/ (regenerable)"), "{t}");
    assert!(
        !t.contains("big"),
        "regenerable folders aren't entered: {t}"
    );
    assert!(!t.contains(".git"), "hidden entries are left out: {t}");
}

#[test]
fn back_matter_is_dropped_but_appendices_kept() {
    let paper = "Abstract\nWe study oscillators.\n\n2. Results\nThey synchronize.\n\nReferences\n[1] Kuramoto, 1975.\n[2] Strogatz, 2000.\n\nAppendix A\nDerivations.\n\nAcknowledgments\nThanks to the lab.\n";
    let kept = drop_back_matter(paper);
    assert!(kept.contains("They synchronize."));
    assert!(!kept.contains("Kuramoto"));
    assert!(kept.contains("Appendix A\nDerivations."));
    assert!(!kept.contains("Thanks to the lab"));
    assert_eq!(
        drop_back_matter("A sentence about references in general.\n"),
        "A sentence about references in general.\n"
    );
}

#[test]
fn fit_keeps_the_start_and_end_within_the_budget() {
    let count = |t: &str| estimate_tokens(t);
    let short = "A short note.";
    assert_eq!(fit(short, 100, &count), (short.to_owned(), false));

    let mut long = String::from("Abstract. This paper is about oscillators. ");
    for i in 0..400 {
        write!(long, "Sentence {i} of the middle, with some detail. ").unwrap();
    }
    long.push_str("In conclusion, they synchronize.");
    let (fitted, cut) = fit(&long, 300, &count);
    assert!(cut);
    assert!(count(&fitted) <= 300, "{} tokens", count(&fitted));
    assert!(fitted.starts_with("Abstract. This paper is about oscillators."));
    assert!(fitted.ends_with("In conclusion, they synchronize."));
    assert!(fitted.contains(CUT));

    let with_refs = format!(
        "Body text here.\n\nReferences\n{}",
        "[1] A cited work, 1999.\n".repeat(200)
    );
    let (fitted, cut) = fit(&with_refs, 50, &count);
    assert!(cut);
    assert_eq!(
        fitted, "Body text here.\n\n",
        "dropping references was enough"
    );

    let cjk = "猫が好きです。".repeat(300);
    let (fitted, _) = fit(&cjk, 100, &count);
    assert!(count(&fitted) <= 100);
}
