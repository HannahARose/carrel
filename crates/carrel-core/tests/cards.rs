//! Card frontmatter against the sample archive and FORMAT.md §5.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use carrel_core::card::{Card, CardError, DateOrTime, Kind, LinkType, Tag};

fn sample_archive() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive")
}

/// Markdown files outside `_System`, recursively.
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

/// The YAML between the opening and closing `---` lines, if the file has frontmatter.
fn frontmatter(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    Some(&rest[..=end])
}

fn fixture_cards() -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    markdown_files(&sample_archive(), &mut files);
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            let yaml = frontmatter(&text)?.to_owned();
            yaml.starts_with("carrel: card\n").then_some((path, yaml))
        })
        .collect()
}

#[test]
fn every_fixture_card_parses_conforms_and_round_trips() {
    let cards = fixture_cards();
    assert_eq!(cards.len(), 12, "the sample archive has 12 cards");
    for (path, yaml) in cards {
        let card = Card::from_yaml(&yaml).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let problems = card.problems();
        assert!(problems.is_empty(), "{}: {problems:?}", path.display());

        let written = card.to_yaml().unwrap();
        let reread = Card::from_yaml(&written)
            .unwrap_or_else(|e| panic!("{} after writing: {e}\n{written}", path.display()));
        assert_eq!(
            reread,
            card,
            "{} changed on round trip:\n{written}",
            path.display()
        );
        assert_eq!(
            reread.to_yaml().unwrap(),
            written,
            "{}: writing isn't stable",
            path.display()
        );
        assert!(
            written.starts_with("carrel: card\n"),
            "{}: carrel must be first",
            path.display()
        );
    }
}

#[test]
fn the_fixture_covers_every_kind() {
    let kinds: HashSet<_> = fixture_cards()
        .iter()
        .map(|(_, yaml)| Card::from_yaml(yaml).unwrap().kind)
        .collect();
    for kind in [
        Kind::File,
        Kind::Folder,
        Kind::Bundle,
        Kind::Physical,
        Kind::Url,
        Kind::External,
        Kind::Tombstone,
    ] {
        assert!(
            kinds.contains(&kind),
            "no {kind:?} card in the sample archive"
        );
    }
}

const MINIMAL: &str = r#"carrel: card
format: "1.0"
id: 01a18701-0500-776a-9867-0802f3a80e26
kind: physical
title: "A book"
location: "Shelf"
sensitivity: public
"#;

#[test]
fn unknown_fields_are_preserved_in_order() {
    let yaml = format!(
        "{MINIMAL}links:\n  - type: cites\n    id: 01a18701-0500-776a-9867-0802f3a80e27\n    weight: 3\nzeta: 1\nalpha: [x, y]\n"
    );
    let card = Card::from_yaml(&yaml).unwrap();
    assert_eq!(card.extra.keys().collect::<Vec<_>>(), ["zeta", "alpha"]);
    assert_eq!(card.links[0].extra["weight"], 3);
    let written = card.to_yaml().unwrap();
    let (zeta, alpha) = (
        written.find("zeta").unwrap(),
        written.find("alpha").unwrap(),
    );
    assert!(
        zeta < alpha,
        "unknown keys must keep their order:\n{written}"
    );
    assert!(
        written.contains("weight: 3"),
        "nested unknown keys must survive:\n{written}"
    );
    assert_eq!(Card::from_yaml(&written).unwrap(), card);
}

#[test]
fn ambiguous_strings_stay_strings() {
    let card = Card::from_yaml(MINIMAL).unwrap();
    let written = card.to_yaml().unwrap();
    assert!(
        written.contains("format: \"1.0\"") || written.contains("format: '1.0'"),
        "{written}"
    );
}

#[test]
fn only_cards_are_cards() {
    assert!(matches!(
        Card::from_yaml("carrel: collection\ntitle: x\n"),
        Err(CardError::NotACard)
    ));
    let carrel_second = MINIMAL.replacen(
        "carrel: card\nformat: \"1.0\"\n",
        "format: \"1.0\"\ncarrel: card\n",
        1,
    );
    assert!(matches!(
        Card::from_yaml(&carrel_second),
        Err(CardError::NotACard)
    ));
    assert!(matches!(
        Card::from_yaml("just: [unclosed\n"),
        Err(CardError::Yaml(_))
    ));
    let bad_kind = MINIMAL.replace("kind: physical", "kind: spaceship");
    assert!(matches!(
        Card::from_yaml(&bad_kind),
        Err(CardError::Yaml(_))
    ));
}

fn problem_fields(yaml: &str) -> Vec<String> {
    Card::from_yaml(yaml)
        .unwrap()
        .problems()
        .into_iter()
        .map(|p| p.field)
        .collect()
}

#[test]
fn problems_follow_the_rules_for_each_kind() {
    assert_eq!(problem_fields(MINIMAL), Vec::<String>::new());

    let bundle = MINIMAL
        .replace("kind: physical", "kind: bundle")
        .replace("location: \"Shelf\"\n", "");
    assert_eq!(
        problem_fields(&bundle),
        ["item", "sealed", "size", "manifest"]
    );

    let file = MINIMAL
        .replace("kind: physical", "kind: file")
        .replace("location: \"Shelf\"\n", "item: \"a.pdf\"\nsize: 3\n");
    assert_eq!(problem_fields(&file), ["checksum"]);

    let sealed_file = format!(
        "{file}sealed: hard\nchecksum: \"sha256:{}\"\n",
        "a".repeat(64)
    );
    assert_eq!(problem_fields(&sealed_file), ["sealed"]);

    let tombstone = MINIMAL
        .replace("kind: physical", "kind: tombstone")
        .replace("location: \"Shelf\"\n", "");
    assert_eq!(problem_fields(&tombstone), ["removed"]);

    let url = MINIMAL
        .replace("kind: physical", "kind: url")
        .replace("location: \"Shelf\"\n", "");
    assert_eq!(problem_fields(&url), ["url"]);
}

#[test]
fn problems_catch_malformed_values() {
    let yaml = format!(
        "{}tags: [good-tag, Bad_Tag, -dash]\ndates:\n  added: \"2026-02-30\"\n  worked: \"2026-10-29T15:42:07-07:00\"\nlinks:\n  - type: cites\n    id: 6ba7b810-9dad-11d1-80b4-00c04fd430c8\n",
        MINIMAL.replace(
            "01a18701-0500-776a-9867-0802f3a80e26",
            "6ba7b810-9dad-11d1-80b4-00c04fd430c8"
        )
    );
    assert_eq!(
        problem_fields(&yaml),
        ["id", "tags[1]", "tags[2]", "links[0].id", "dates.added"]
    );
}

#[test]
fn dates_and_timestamps() {
    let ok = [
        "2026-10-29",
        "2024-02-29",
        "2000-02-29",
        "2026-10-29T15:42:07-07:00",
        "2026-10-29T15:42:07Z",
        "2026-10-29T15:42:07.123+05:30",
    ];
    let bad = [
        "2026-13-01",
        "2023-02-29",
        "1900-02-29",
        "2026-10-29T15:42:07",
        "2026-10-29 15:42:07Z",
        "26-10-29",
        "2026-10-29T25:00:00Z",
        "2026-10-29T15:42:07.Z",
        "yesterday",
    ];
    for s in ok {
        assert!(DateOrTime(s.into()).is_valid(), "{s} should be valid");
    }
    for s in bad {
        assert!(!DateOrTime(s.into()).is_valid(), "{s} should be invalid");
    }
}

#[test]
fn tags_and_link_types() {
    assert!(Tag("c-elegans".into()).is_well_formed());
    for bad in [
        "",
        "Upper",
        "two--dashes",
        "-lead",
        "trail-",
        "under_score",
        "spa ce",
    ] {
        assert!(!Tag(bad.into()).is_well_formed(), "{bad:?}");
    }
    assert!(LinkType("derived-from".into()).is_built_in());
    assert!(!LinkType("supersedes".into()).is_built_in());
}

#[test]
fn json_schema_is_up_to_date() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/card.schema.json");
    let generated = carrel_core::card::json_schema();
    if std::env::var_os("UPDATE_SCHEMAS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &generated).unwrap();
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == generated,
        "schemas/card.schema.json is out of date; run `UPDATE_SCHEMAS=1 cargo test -p carrel-core --test cards`"
    );
}
