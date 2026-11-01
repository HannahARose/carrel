//! Performance benchmarks for the scale target of a million files (#48): card parsing,
//! manifest parsing and diffing, building and searching the word index, searching and opening
//! the embedding index, and reading history, each over generated fixtures at a few sizes.
//!
//! `cargo bench -p carrel-core --bench scale` runs them all and writes the results to
//! `target/scale-bench/results.json`; `cargo test` runs each once at its smallest size, to
//! check it still works. `docs/dev/development-setup.md` ("Coverage and performance") lists
//! the settings, and CI's "Performance" workflow runs them on `dev` and compares each run with
//! the last.

mod fixtures;
mod harness;

use std::path::PathBuf;

use carrel_core::card_file::CardFile;
use carrel_core::index::{Index, Neighbours};
use carrel_core::manifest::Manifest;

use harness::{Measurement, Plan, Results, Rng, measure};

/// Sizes for benchmarks that are cheap per item: up to the million-file target.
const LARGE: &[usize] = &[10_000, 100_000, 1_000_000];
/// Sizes for benchmarks that are slower per item, where a million would take minutes; the
/// time per item shows how they grow.
const SMALLER: &[usize] = &[1_000, 10_000, 100_000];

fn main() {
    let plan = Plan::from_env();
    let mut measurements = Vec::new();
    cards(&plan, &mut measurements);
    manifests(&plan, &mut measurements);
    words(&plan, &mut measurements);
    embeddings(&plan, &mut measurements);
    history(&plan, &mut measurements);
    let results = Results {
        commit: std::env::var("GITHUB_SHA").unwrap_or_default(),
        measurements,
    };
    if plan.smoke && std::env::var_os("CARREL_SCALE_OUT").is_none() {
        // A smoke run checks the report can be made, and writes nothing.
        let (report, _) = harness::report(&results, Some(&results), 25.0);
        assert!(report.contains("| Benchmark |"));
        return;
    }
    finish(&results);
}

/// Writes the results file, then the report, compared with the baseline if there is one.
fn finish(results: &Results) {
    let out = std::env::var_os("CARREL_SCALE_OUT").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/scale-bench/results.json"),
        PathBuf::from,
    );
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).expect("the results folder");
    }
    let json = serde_json::to_string_pretty(results).expect("results serialize");
    std::fs::write(&out, json + "\n").expect("the results file");
    eprintln!("Results: {}", out.display());

    let baseline = std::env::var_os("CARREL_SCALE_BASELINE")
        .and_then(|path| harness::read_results(&PathBuf::from(path)));
    let threshold = std::env::var("CARREL_SCALE_THRESHOLD")
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or(25.0);
    let (report, flagged) = harness::report(results, baseline.as_ref(), threshold);
    match std::env::var_os("CARREL_SCALE_REPORT") {
        Some(path) => {
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("the report file");
            file.write_all(report.as_bytes()).expect("the report");
        }
        None => println!("{report}"),
    }
    if flagged > 0 {
        // A warning annotation in CI; the run itself still passes.
        println!(
            "::warning title=Performance::{flagged} benchmarks slower by more than {threshold}%"
        );
    }
}

fn cards(plan: &Plan, out: &mut Vec<Measurement>) {
    if !plan.wants("card_parse") {
        return;
    }
    for size in plan.sizes(SMALLER) {
        let texts = fixtures::card_texts(size);
        out.push(measure(plan, "card_parse", size, || {
            texts
                .iter()
                .map(|t| CardFile::parse(t).expect("a generated card parses"))
                .collect::<Vec<_>>()
        }));
    }
}

fn manifests(plan: &Plan, out: &mut Vec<Measurement>) {
    if !plan.wants("manifest") {
        return;
    }
    for size in plan.sizes(LARGE) {
        let older = fixtures::manifest(size);
        if plan.wants("manifest_diff") {
            let newer = fixtures::changed_manifest(&older);
            let diff = older.diff(&newer);
            assert!(!diff.changed.is_empty() && !diff.renamed.is_empty());
            out.push(measure(plan, "manifest_diff", size, || older.diff(&newer)));
        }
        if plan.wants("manifest_parse") {
            let tsv = older.to_tsv();
            out.push(measure(plan, "manifest_parse", size, || {
                Manifest::parse(&tsv).expect("a written manifest parses")
            }));
        }
    }
}

fn words(plan: &Plan, out: &mut Vec<Measurement>) {
    if !plan.wants("word_") {
        return;
    }
    for size in plan.sizes(LARGE) {
        let documents = fixtures::documents(size);
        if plan.wants("word_index_build") {
            out.push(measure(plan, "word_index_build", size, || {
                fixtures::word_index(&documents)
            }));
        }
        if plan.wants("word_search") {
            let index = fixtures::word_index(&documents);
            let query = fixtures::word_query();
            assert_ne!(index.search(&query, &|_| true), []);
            out.push(measure(plan, "word_search", size, || {
                index.search(&query, &|_| true)
            }));
        }
    }
}

fn embeddings(plan: &Plan, out: &mut Vec<Measurement>) {
    if !plan.wants("embedding_") {
        return;
    }
    for size in plan.sizes(LARGE) {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let archive = fixtures::archive(dir.path());
        let mut index = fixtures::embedding_index(&archive, size);
        if plan.wants("embedding_search") {
            let query = fixtures::vector(&mut Rng::new(6));
            assert_eq!(
                index.nearest(&query, &|_| true, 20).len(),
                20.min(index.len())
            );
            out.push(measure(plan, "embedding_search", size, || {
                index.nearest(&query, &|_| true, 20)
            }));
        }
        if plan.wants("embedding_index_open") {
            index.save().expect("the index saves");
            out.push(measure(plan, "embedding_index_open", size, || {
                Index::open(&archive, "bench-model")
                    .expect("the index reads")
                    .expect("the index is there")
            }));
        }
    }
}

fn history(plan: &Plan, out: &mut Vec<Measurement>) {
    if !plan.wants("history_log") {
        return;
    }
    for size in plan.sizes(SMALLER) {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let archive = fixtures::archive(dir.path());
        let history = fixtures::history(&archive, size);
        assert_eq!(history.log().expect("the log reads").len(), size);
        out.push(measure(plan, "history_log", size, || {
            history.log().expect("the log reads")
        }));
    }
}
