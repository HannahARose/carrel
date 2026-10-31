//! The benchmark's test set (`fixtures/bench`) is consistent: every item has an answer, every
//! destination and tag exists, and no expected level is below what the rules alone give; and
//! every item has hand-written descriptions for the findability test, which scores search by
//! words, by meaning and by both.

mod common;

use std::collections::BTreeMap;
use std::path::PathBuf;

use carrel_core::archive::Archive;
use carrel_core::bench::findability::{
    self, Descriptions, Drafted, by_meaning, by_search, look_up, names, score,
};
use carrel_core::bench::{Answers, check};
use carrel_core::index::{ByMeaning, Index, embed_query, plan};
use carrel_core::providers::{Registry, priority};
use carrel_core::search::Search;
use carrel_core::search::texts::{fill, tidy};
use common::ScriptedEmbed;

fn bench() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/bench")
}

#[test]
fn the_test_set_is_consistent() {
    let root = bench();
    let answers = Answers::load(&root.join("answers.yaml")).unwrap();
    let archive = Archive::open(&root.join("archive")).unwrap();
    let problems = check(&answers, &archive, &root.join("items")).unwrap();
    assert_eq!(problems, Vec::<String>::new());
    assert!(answers.items.len() >= 60, "{} items", answers.items.len());
    let collections = archive.collections().unwrap();
    assert!(collections.len() >= 12, "{} collections", collections.len());
}

#[test]
fn every_item_is_described_two_to_four_ways() {
    let root = bench();
    let answers = Answers::load(&root.join("answers.yaml")).unwrap();
    let descriptions = Descriptions::load(&root.join("descriptions.yaml")).unwrap();
    assert_eq!(
        findability::check(&descriptions, &answers),
        Vec::<String>::new()
    );
    assert_eq!(descriptions.items.len(), answers.items.len());
}

#[test]
fn problems_with_descriptions_are_found() {
    let answers: Answers = serde_saphyr::from_str(
        "items:\n  receipt desk chair.txt:\n    destinations: [Receipts]\n    \
         sensitivity: personal\n    date: null\n",
    )
    .unwrap();
    let descriptions: Descriptions = serde_saphyr::from_str(
        "items:\n  receipt desk chair.txt: [\"the receipt desk chair\", \"tag:receipts\", \
         \"tag:receipts\", \"x\", \"y\"]\n  other.txt: [a, b]\n",
    )
    .unwrap();
    let problems = findability::check(&descriptions, &answers);
    let has = |s: &str| problems.iter().any(|p| p.contains(s));
    assert!(has("has 5 descriptions"), "{problems:?}");
    assert!(has("other.txt is described"), "{problems:?}");
    assert!(has("copies the file name"), "{problems:?}");
    assert!(has("would be read as a filter"), "{problems:?}");
    assert!(has("is there twice"), "{problems:?}");
}

/// The test set filed into a copy of its archive, with each item's text read.
fn filed(work: &std::path::Path, drafts: &BTreeMap<String, Drafted>) -> Archive {
    let root = bench();
    let answers = Answers::load(&root.join("answers.yaml")).unwrap();
    let archive = findability::build(
        &root.join("archive"),
        &root.join("items"),
        &answers,
        drafts,
        work,
    )
    .unwrap();
    let (wanted, _) = tidy(&archive).unwrap();
    fill(
        &archive,
        &Registry::with_built_ins(),
        wanted,
        &mut |_, _, _| {},
    )
    .unwrap();
    archive
}

#[test]
fn findability_compares_words_meaning_and_both() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("archive");
    let root = bench();
    let descriptions = Descriptions::load(&root.join("descriptions.yaml")).unwrap();
    let drafts = BTreeMap::from([(
        "receipt desk chair.txt".to_owned(),
        Drafted {
            title: "Office chair receipt".into(),
            summary: Some("What the grey task chair cost at Furnish & Co.".into()),
        },
    )]);
    let archive = filed(&work, &drafts);
    let names = names(&archive).unwrap();
    assert_eq!(names.len(), descriptions.items.len(), "every item is filed");
    let chair = std::fs::read_to_string(work.join("Receipts/receipt desk chair.txt.md")).unwrap();
    assert!(chair.contains("title: \"Office chair receipt\""), "{chair}");

    let words = Search::build(&archive).unwrap();
    let by_words = look_up(&descriptions, &mut |d| by_search(&words, d));
    let words_score = score(&by_words);
    assert_eq!(words_score.descriptions, descriptions.len());
    assert!(words_score.top_five > 0.3, "{words_score:?}");
    let ledger = by_words
        .iter()
        .find(|l| l.description == "booking reference K7Q2PX")
        .unwrap();
    assert_eq!(ledger.rank, Some(1), "an exact code is found by words");

    // A fake model whose vectors count words: what a real model does for meaning, it does for
    // shared words, which is enough to check the plumbing.
    let model = "fake-embed";
    let registry = registry_with(model);
    let mut index = Index::open_or_new(&archive, model).unwrap();
    let todo = plan(&archive, Some(&index)).unwrap();
    assert_eq!(todo.pending.len(), names.len());
    index
        .embed(&archive, &registry, todo, &mut |_, _, _| {})
        .unwrap();

    let meaning = |archive: &Archive| {
        let index = Index::open(archive, model).unwrap().unwrap();
        let registry = std::sync::Arc::new(registry_with(model));
        Box::new(ByMeaning::new(
            archive,
            Box::new(index),
            0,
            Box::new(move |q: &str| embed_query(&registry, model, q).ok()),
        ))
    };
    let ranking = meaning(&archive);
    let by_meaning = look_up(&descriptions, &mut |d| by_meaning(&*ranking, &names, d));
    let meaning_score = score(&by_meaning);
    assert!(meaning_score.reciprocal_rank > 0.0, "{meaning_score:?}");
    let both = Search::build(&archive)
        .unwrap()
        .with_meaning(meaning(&archive));
    let by_both = look_up(&descriptions, &mut |d| by_search(&both, d));
    let both_score = score(&by_both);
    assert!(both_score.top_five > 0.0, "{both_score:?}");

    // Built again, the cards keep their identifiers, so the index is still current.
    let again = filed(&work, &drafts);
    let index = Index::open(&again, model).unwrap();
    assert_eq!(plan(&again, index.as_ref()).unwrap().pending.len(), 0);
}

/// A registry whose only embedding model is `model`, a fake one.
fn registry_with(model: &str) -> Registry {
    let mut fake = ScriptedEmbed::new("fake", model, priority::PORTABLE, 64);
    fake.by_words = true;
    let mut registry = Registry::new();
    registry.add(Box::new(fake));
    registry
}
