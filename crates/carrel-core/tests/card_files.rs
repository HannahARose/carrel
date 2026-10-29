//! Whole card files: owner text survives every program edit (FORMAT.md §5.3).

use std::path::{Path, PathBuf};

use carrel_core::card::{Card, Tag};
use carrel_core::card_file::{CardFile, HISTORY};
use carrel_core::{frontmatter, sections};
use proptest::prelude::*;

fn sample_archive() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive")
}

fn markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "_System" && !name.starts_with('.') {
                markdown_files(&path, out);
            }
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            out.push(path);
        }
    }
}

fn fixture_documents() -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    markdown_files(&sample_archive(), &mut files);
    files.sort();
    files
        .into_iter()
        .map(|p| (p.clone(), std::fs::read_to_string(p).unwrap()))
        .collect()
}

#[test]
fn every_fixture_document_splits_and_joins_byte_for_byte() {
    let mut with_frontmatter = 0;
    for (path, text) in fixture_documents() {
        let Ok(split) = frontmatter::split(&text) else {
            continue;
        };
        with_frontmatter += 1;
        assert_eq!(
            frontmatter::join(split.yaml, split.body),
            text,
            "{}",
            path.display()
        );
        sections::find_all(split.body).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
    assert!(
        with_frontmatter >= 17,
        "cards, collections and ARCHIVE.md all have frontmatter"
    );
}

#[test]
fn unchanged_fixture_cards_are_written_back_byte_for_byte() {
    let mut cards = 0;
    for (path, text) in fixture_documents() {
        if !text.starts_with("---\ncarrel: card\n") {
            continue;
        }
        cards += 1;
        let file = CardFile::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(!file.frontmatter_changed());
        assert_eq!(file.to_text().unwrap(), text, "{}", path.display());
        assert!(
            file.section(HISTORY).unwrap().is_some(),
            "{}: no history",
            path.display()
        );
    }
    assert_eq!(cards, 12);
}

#[test]
fn editing_a_fixture_card_keeps_its_body() {
    let path = sample_archive().join("Projects/oscillator-sim.md");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut file = CardFile::parse(&text).unwrap();
    let body = file.body().to_owned();
    assert!(
        file.section(HISTORY)
            .unwrap()
            .unwrap()
            .contains("> [!pending] 2026-10-28")
    );

    file.card_mut().tags.push(Tag("phase-response".into()));
    file.card_mut().checked_out = None;
    let written = file.to_text().unwrap();
    assert_eq!(frontmatter::split(&written).unwrap().body, body);
    let reread = CardFile::parse(&written).unwrap();
    assert_eq!(reread.card(), file.card());
    assert!(reread.card().checked_out.is_none());
}

#[test]
fn approving_a_history_entry_changes_only_the_section() {
    let path = sample_archive().join("Projects/oscillator-sim.md");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut file = CardFile::parse(&text).unwrap();
    let old = file.section(HISTORY).unwrap().unwrap().to_owned();
    let approved = old.replacen(
        "> [!pending] 2026-10-28 · 2h 10m · auto-generated\n> Added",
        "### 2026-10-28 · 2h 10m\nAdded",
        1,
    );
    assert_ne!(approved, old);
    file.set_section(HISTORY, &approved).unwrap();
    let written = file.to_text().unwrap();
    assert_eq!(written, text.replacen(&old, &approved, 1));
}

#[test]
fn a_new_card_file_has_an_empty_history() {
    let card = Card::from_yaml(
        "carrel: card\nformat: \"1.0\"\nid: 01a18701-0500-776a-9867-0802f3a80e26\nkind: url\ntitle: \"x\"\nurl: \"https://example.com\"\nsensitivity: public\n",
    )
    .unwrap();
    let file = CardFile::new(card.clone(), "Notes.").unwrap();
    let text = file.to_text().unwrap();
    assert!(text.ends_with(
        "---\nNotes.\n\n<!-- carrel:history -->\n## History\n<!-- /carrel:history -->\n"
    ));
    assert_eq!(CardFile::parse(&text).unwrap().card(), &card);
}

// ---------------------------------------------------------------------------- properties

const BASE_YAML: &str = "carrel: card\nformat: \"1.0\"\nid: 01a18701-0500-776a-9867-0802f3a80e26\nkind: physical\ntitle: \"A book\"\nlocation: \"Shelf\"\nsensitivity: public\n";

/// Lines chosen to trip up a naive parser, mixed with arbitrary text.
fn tricky_line() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => "[^\n]{0,40}",
        1 => Just("---".to_owned()),
        1 => Just("```".to_owned()),
        1 => Just("~~~~".to_owned()),
        1 => Just("<!-- carrel:history -->".to_owned()),
        1 => Just("<!-- /carrel:history -->".to_owned()),
        1 => Just("| a | b |".to_owned()),
        1 => Just(String::new()),
        1 => Just("\r".to_owned()),
    ]
}

fn body() -> impl Strategy<Value = String> {
    (prop::collection::vec(tricky_line(), 0..24), any::<bool>()).prop_map(|(lines, trailing)| {
        let mut b = lines.join("\n");
        if trailing && !b.is_empty() {
            b.push('\n');
        }
        b
    })
}

/// Lines that can't open a code block or contain a marker, for text around a section.
fn plain_line() -> impl Strategy<Value = String> {
    "[^\n`~]{0,40}".prop_filter("no markers", |l| !l.contains("carrel:"))
}

fn plain_text() -> impl Strategy<Value = String> {
    prop::collection::vec(plain_line(), 0..10)
        .prop_map(|lines| lines.into_iter().map(|l| l + "\n").collect::<String>())
}

proptest! {
    // 512 cases by default; set PROPTEST_CASES for a longer run.
    #![proptest_config(ProptestConfig::with_cases(
        std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(512)
    ))]

    #[test]
    fn any_body_is_kept_byte_for_byte(body in body()) {
        let text = frontmatter::join(BASE_YAML, &body);
        let file = CardFile::parse(&text).unwrap();
        prop_assert_eq!(file.body(), body.as_str());
        prop_assert_eq!(file.to_text().unwrap(), text);
    }

    #[test]
    fn editing_fields_never_touches_the_body(
        body in body(),
        title in "\\PC{1,40}|[:#'\"\\-\\[\\]{}&*!|>%@` ]{1,10}|\\PC{1,10}: \\PC{1,10}",
        tags in prop::collection::vec("[a-z0-9]{1,8}(-[a-z0-9]{1,8}){0,2}", 0..5),
        summary in prop::option::of("\\PC{0,80}"),
    ) {
        prop_assume!(!title.trim().is_empty());
        let text = frontmatter::join(BASE_YAML, &body);
        let mut file = CardFile::parse(&text).unwrap();
        file.card_mut().title = title.clone();
        file.card_mut().tags = tags.into_iter().map(Tag).collect();
        file.card_mut().summary = summary;

        let written = file.to_text().unwrap();
        let split = frontmatter::split(&written).unwrap();
        prop_assert_eq!(split.body, body.as_str());
        let reread = CardFile::parse(&written).unwrap();
        prop_assert_eq!(reread.card(), file.card());
        prop_assert_eq!(&reread.card().title, &title);
    }

    #[test]
    fn replacing_a_section_changes_only_its_content(
        before in plain_text(),
        after in plain_text(),
        old in plain_text(),
        new in plain_text(),
    ) {
        let body = format!("{before}<!-- carrel:history -->\n{old}<!-- /carrel:history -->\n{after}");
        let text = frontmatter::join(BASE_YAML, &body);
        let mut file = CardFile::parse(&text).unwrap();
        prop_assert_eq!(file.section(HISTORY).unwrap(), Some(old.as_str()));
        file.set_section(HISTORY, &new).unwrap();
        let expected = frontmatter::join(
            BASE_YAML,
            &format!("{before}<!-- carrel:history -->\n{new}<!-- /carrel:history -->\n{after}"),
        );
        prop_assert_eq!(file.to_text().unwrap(), expected);
    }

    #[test]
    fn table_normalization_is_idempotent(text in body()) {
        let once = sections::normalize_tables(&text);
        prop_assert_eq!(sections::normalize_tables(&once), once.clone());
        prop_assert!(sections::equivalent(&text, &once));
    }
}
