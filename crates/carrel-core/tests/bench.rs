//! The benchmark's test set (`fixtures/bench`) is consistent: every item has an answer, every
//! destination and tag exists, and no expected level is below what the rules alone give.

use std::path::PathBuf;

use carrel_core::archive::Archive;
use carrel_core::bench::{Answers, check};

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
