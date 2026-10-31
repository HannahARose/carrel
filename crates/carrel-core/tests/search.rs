//! Search over a small archive: words in cards, item text, collection documents and the
//! glossary; filters; reasons and passages; restricted items; and a second ranking merged in.

use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::card::{Checksum, Id, Kind, Sensitivity};
use carrel_core::providers::Registry;
use carrel_core::search::query::Query;
use carrel_core::search::texts::{TEXTS, TextCache, fill, tidy};
use carrel_core::search::{Found, Meaning, Merge, Ranked, Ranking, Search, Why};

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

/// Files `name` in `collection` with `contents`, and a card with `fields` beside it.
fn file(root: &Path, collection: &str, name: &str, contents: &str, fields: &str) -> Id {
    let path = root.join(collection).join(name);
    std::fs::write(&path, contents).unwrap();
    let checksum = Checksum::of_file(&path).unwrap();
    let id = Id::new_v7();
    let card = format!(
        "---\ncarrel: card\nformat: \"1.0\"\nid: {id}\nkind: file\nitem: \"{name}\"\n{fields}\
         checksum: \"{}\"\nsize: {}\n---\n\n<!-- carrel:history -->\n## History\n\
         <!-- /carrel:history -->\n",
        checksum.0,
        contents.len()
    );
    std::fs::write(root.join(collection).join(format!("{name}.md")), card).unwrap();
    id
}

struct Setup {
    _dir: tempfile::TempDir,
    archive: Archive,
    invoice: Id,
    statement: Id,
}

/// The sample archive with an invoice and a restricted bank statement filed in Holding, and
/// the text of every item read into the cache.
fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("archive");
    copy_dir(&sample_archive(), &root);
    let invoice = file(
        &root,
        "Holding",
        "invoice.txt",
        "Please pay\ninvoice 2057-4038 by Friday.\nIt covers the desk chair.",
        "title: \"Furniture shop bill\"\nsummary: \"A bill for office furniture.\"\n\
         tags: [receipts]\ndates:\n  added: \"2026-10-29\"\n  document: \"2025-03-02\"\n\
         sensitivity: personal\n",
    );
    let statement = file(
        &root,
        "Holding",
        "statement.txt",
        "Account 4111 1111 1111 1111, balance owed for invoice 2057-4038.",
        "title: \"Bank statement\"\nsummary: \"A monthly statement.\"\n\
         dates:\n  added: \"2026-10-29\"\nsensitivity: restricted\n",
    );
    let archive = Archive::open(&root).unwrap();
    let mut seen = Vec::new();
    let (wanted, removed) = tidy(&archive).unwrap();
    assert_eq!(removed, 0);
    assert!(
        !wanted.iter().any(|w| w.item.ends_with("statement.txt")),
        "restricted"
    );
    let filled = fill(
        &archive,
        &Registry::with_built_ins(),
        wanted,
        &mut |done, total, item| seen.push((done, total, item.map(str::to_owned))),
    )
    .unwrap();
    assert_eq!(filled.unread, 1, "no PDF reader among the built-ins");
    let left: Vec<String> = tidy(&archive)
        .unwrap()
        .0
        .into_iter()
        .map(|w| w.item)
        .collect();
    assert_eq!(left, Vec::<String>::new(), "tried again next week, not now");
    assert!(filled.read >= 3, "{filled:?}");
    assert_eq!(seen.last().unwrap().0, seen.last().unwrap().1);
    Setup {
        _dir: dir,
        archive,
        invoice,
        statement,
    }
}

fn search(search: &Search, text: &str) -> Vec<Found> {
    search
        .search(&Query::parse(text).unwrap(), 20)
        .hits
        .into_iter()
        .map(|h| h.found)
        .collect()
}

fn titles(found: &[Found]) -> Vec<String> {
    found
        .iter()
        .map(|f| match f {
            Found::Item(item) => item.title.clone(),
            Found::Collection { title, .. } => format!("collection {title}"),
            Found::Glossary { term } => format!("glossary {term}"),
        })
        .collect()
}

fn item_id(found: &Found) -> Option<Id> {
    match found {
        Found::Item(item) => Some(item.id),
        _ => None,
    }
}

#[test]
fn exact_numbers_in_item_text_are_found_with_the_passage_marked() {
    let s = setup();
    let index = Search::build(&s.archive).unwrap();
    let results = index.search(&Query::parse("invoice 2057-4038").unwrap(), 20);
    let first = &results.hits[0];
    assert_eq!(item_id(&first.found), Some(s.invoice));
    assert!(first.why.contains(&Why::Text), "{:?}", first.why);
    assert_eq!(first.passage_from, Some(Why::Text));
    let passage = first.passage.as_ref().unwrap();
    assert_eq!(
        passage.marked("[", "]"),
        "Please pay [invoice] [2057-4038] by Friday. It covers the desk chair."
    );
    assert_eq!(results.meaning, Meaning::Off);
    let Found::Item(item) = &first.found else {
        panic!()
    };
    assert_eq!(item.collection, "Holding");
    assert_eq!(item.date.as_deref(), Some("2025-03-02"));
    assert_eq!(item.sensitivity, Sensitivity::Personal);

    let found = search(&index, "blue folder");
    assert_eq!(titles(&found)[0], "Untitled scan", "by its OCR text");
}

#[test]
fn restricted_items_are_found_by_their_card_never_their_text() {
    let s = setup();
    let cache = TextCache::of(&s.archive);
    assert!(s.archive.root().join(TEXTS).is_dir());
    let statement = Checksum::of_file(&s.archive.root().join("Holding/statement.txt")).unwrap();
    assert!(!cache.has(&statement), "never cached");
    cache.put(&statement, "4111 1111 1111 1111").unwrap();
    let index = Search::build(&s.archive).unwrap();
    for words in ["4111", "2057-4038 balance", "account"] {
        let found = search(&index, words);
        assert!(
            !found.iter().any(|f| item_id(f) == Some(s.statement)),
            "{words}: {:?}",
            titles(&found)
        );
    }
    let found = search(&index, "bank statement");
    assert_eq!(item_id(&found[0]), Some(s.statement));
    let (_, removed) = tidy(&s.archive).unwrap();
    assert_eq!(removed, 1);
    assert!(!cache.has(&statement));
}

#[test]
fn filters_narrow_the_items_found() {
    let s = setup();
    let index = Search::build(&s.archive).unwrap();
    let only = |q: &str| titles(&search(&index, q));
    assert_eq!(only("tag:receipts"), ["Furniture shop bill"]);
    assert_eq!(only("invoice tag:receipts"), ["Furniture shop bill"]);
    assert_eq!(only("after:2025 before:2025-04"), ["Furniture shop bill"]);
    assert_eq!(only("invoice before:2025-03-02"), Vec::<String>::new());
    assert_eq!(only("sensitivity:restricted"), ["Bank statement"]);
    assert_eq!(only("checked-out"), ["oscillator-sim"]);
    for found in search(&index, "kind:folder") {
        let Found::Item(item) = found else { panic!() };
        assert_eq!(item.kind, Kind::Folder);
    }
    for found in search(&index, "collection:hardware") {
        let Found::Item(item) = found else { panic!() };
        assert_eq!(item.collection, "Projects/Hardware");
    }
    assert_ne!(only("collection:Projects").len(), 0);
    let tombstones = only("kind:tombstone");
    assert_eq!(tombstones, ["A note on coupled oscillators (preprint)"]);
    assert!(
        !only("oscillators").contains(&tombstones[0]),
        "removed items only when asked for"
    );
    let results = index.search(&Query::parse("tag:receipts").unwrap(), 20);
    assert_eq!(results.hits[0].why, [Why::Filters]);
    assert_eq!(index.search(&Query::default(), 20).hits.len(), 0);
}

#[test]
fn collection_documents_and_glossary_facts_are_found_too() {
    let s = setup();
    let index = Search::build(&s.archive).unwrap();
    assert!(titles(&search(&index, "technical standards")).contains(&"collection Reading".into()));
    let found = titles(&search(&index, "implements the model"));
    assert_eq!(found[0], "glossary oscillator-sim");
    assert!(
        !titles(&search(&index, "implements tag:receipts")).contains(&found[0]),
        "filters are for items"
    );
}

/// A meaning ranking that knows one item, and that two aren't embedded yet.
struct Near(Id);

impl Ranking for Near {
    fn rank(&self, _query: &str, allowed: &dyn Fn(Id) -> bool, _limit: usize) -> Vec<Ranked> {
        [self.0]
            .into_iter()
            .filter(|id| allowed(*id))
            .map(|id| Ranked {
                id,
                passage: None,
                score: 0.5,
            })
            .collect()
    }

    fn behind(&self) -> usize {
        2
    }
}

#[test]
fn a_meaning_ranking_is_merged_with_the_words() {
    let s = setup();
    let index = Search::build(&s.archive)
        .unwrap()
        .with_meaning(Box::new(Near(s.invoice)));
    let results = index.search(&Query::parse("somewhere to sit").unwrap(), 20);
    assert_eq!(results.meaning, Meaning::Behind { items: 2 });
    let hit = results
        .hits
        .iter()
        .find(|h| item_id(&h.found) == Some(s.invoice))
        .unwrap();
    assert!(hit.why.contains(&Why::Meaning));
    assert_eq!(hit.passage_from, Some(Why::Summary));

    let results = index.search(&Query::parse("invoice 2057-4038").unwrap(), 20);
    assert_eq!(
        item_id(&results.hits[0].found),
        Some(s.invoice),
        "both agree"
    );
    assert_eq!(results.hits[0].why.last(), Some(&Why::Meaning));
    let filtered = index.search(&Query::parse("sit kind:folder").unwrap(), 20);
    assert!(
        !filtered
            .hits
            .iter()
            .any(|h| item_id(&h.found) == Some(s.invoice))
    );
}

/// A meaning ranking that has every item, in a fixed order, as a strong model's would.
struct Order(Vec<Id>);

impl Ranking for Order {
    fn rank(&self, _query: &str, allowed: &dyn Fn(Id) -> bool, limit: usize) -> Vec<Ranked> {
        self.0
            .iter()
            .copied()
            .filter(|id| allowed(*id))
            .take(limit)
            .enumerate()
            .map(|(place, id)| Ranked {
                id,
                passage: None,
                score: 1.0 / f64::from(u32::try_from(place).unwrap() + 1),
            })
            .collect()
    }

    fn behind(&self) -> usize {
        0
    }
}

/// The setup's archive with a datasheet filed too, and a meaning ranking that puts `last`
/// below every other item.
fn with_meaning_against(s: &Setup, last: &[Id], weights: Merge) -> Search {
    let mut ids: Vec<Id> = s
        .archive
        .items()
        .unwrap()
        .into_iter()
        .map(|i| i.id)
        .filter(|id| !last.contains(id))
        .collect();
    ids.extend(last);
    Search::build(&s.archive)
        .unwrap()
        .with_meaning(Box::new(Order(ids)))
        .with_merge(weights)
}

#[test]
fn exact_names_and_numbers_come_first_whatever_meaning_says() {
    let s = setup();
    let datasheet = file(
        s.archive.root(),
        "Holding",
        "datasheet.pdf",
        "%PDF",
        "title: \"LED driver AS1115 datasheet\"\nsensitivity: personal\n",
    );
    let archive = Archive::open(s.archive.root()).unwrap();
    let s = Setup { archive, ..s };
    for weights in [Merge::STRONG, Merge::WEAK, Merge::EQUAL] {
        let index = with_meaning_against(&s, &[s.invoice, datasheet], weights);
        let first = |q: &str| item_id(&search(&index, q)[0]);
        assert_eq!(first("invoice 2057-4038"), Some(s.invoice), "{weights:?}");
        assert_eq!(first("2057-4038"), Some(s.invoice), "{weights:?}");
        assert_eq!(first("AS1115"), Some(datasheet), "{weights:?}");
        assert_eq!(first("the as1115 led chip"), Some(datasheet), "{weights:?}");
    }
    // Without exact terms a strong model's ranking leads: the invoice is its last choice, so
    // something it put first beats it even though the words name the invoice.
    let index = with_meaning_against(&s, &[s.invoice, datasheet], Merge::STRONG);
    assert_ne!(
        item_id(&search(&index, "furniture shop bill")[0]),
        Some(s.invoice)
    );
    // A weak one doesn't.
    let index = with_meaning_against(&s, &[s.invoice, datasheet], Merge::WEAK);
    assert_eq!(
        item_id(&search(&index, "furniture shop bill")[0]),
        Some(s.invoice)
    );
}

#[test]
fn without_meaning_or_while_it_is_behind_the_words_still_find_things() {
    let s = setup();
    let words = Search::build(&s.archive).unwrap();
    let results = words.search(&Query::parse("desk chair").unwrap(), 20);
    assert_eq!(results.meaning, Meaning::Off);
    assert_eq!(item_id(&results.hits[0].found), Some(s.invoice));

    // A meaning ranking that knows only another item, with two not embedded: the invoice it
    // doesn't know is judged by its words, so it isn't buried below the one it knows.
    let other = s
        .archive
        .items()
        .unwrap()
        .into_iter()
        .map(|i| i.id)
        .find(|id| *id != s.invoice && *id != s.statement)
        .unwrap();
    let index = Search::build(&s.archive)
        .unwrap()
        .with_meaning(Box::new(Near(other)));
    let results = index.search(&Query::parse("desk chair").unwrap(), 20);
    assert_eq!(results.meaning, Meaning::Behind { items: 2 });
    assert_eq!(item_id(&results.hits[0].found), Some(s.invoice));
}
