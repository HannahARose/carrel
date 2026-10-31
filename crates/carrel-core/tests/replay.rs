//! Replays how search merges words and meaning on the first findability run with real
//! models (`docs/bench/2026-10-31/findability`, issue #249), and checks the merge search uses
//! by default does at least as well as meaning alone with a strong model, and as words alone
//! with a weak one. Run with `--nocapture` to see every strategy's scores.

use std::fmt::Write as _;
use std::path::Path;

use carrel_core::bench::findability::replay::{replay, saved};
use carrel_core::bench::findability::{Findability, Lookup, score};
use carrel_core::search::Merge;

fn results() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/bench/2026-10-31/findability/results.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// The lookups of the ranking `way` with `model` in `results`.
fn lookups(results: &serde_json::Value, way: &str, model: Option<&str>) -> Vec<Lookup> {
    let ranking = results["rankings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["way"] == way && r["model"].as_str() == model)
        .unwrap();
    serde_json::from_value(ranking["lookups"].clone()).unwrap()
}

/// Words counting `words` to meaning's 1.
const fn words(words: f64) -> Merge {
    Merge {
        words,
        meaning: 1.0,
    }
}

const STRATEGIES: [(&str, Merge); 9] = [
    ("equal, as before #249", Merge::EQUAL),
    ("words 0.5", words(0.5)),
    ("words 0.25", words(0.25)),
    ("words 0.15", words(0.15)),
    ("words 0.1", words(0.1)),
    ("strong: words 0.05", Merge::STRONG),
    ("words 0.02", words(0.02)),
    ("words 0: meaning and exact matches", words(0.0)),
    ("weak: meaning 0.05", Merge::WEAK),
];

fn row(text: &mut String, name: &str, s: Findability) {
    writeln!(
        text,
        "| {name} | {:.1}% | {:.3} |",
        s.top_five * 100.0,
        s.reciprocal_rank
    )
    .unwrap();
}

#[test]
fn the_default_merge_beats_meaning_alone_on_the_2026_10_31_run() {
    let results = results();
    let items = usize::try_from(results["items"].as_u64().unwrap()).unwrap();
    let words = lookups(&results, "words", None);
    let words_alone = score(&words);
    let mut table = String::new();
    for model in [
        "embeddinggemma-300m",
        "qwen3-embedding-0.6b",
        "apple-nl-sentence-en",
    ] {
        let meaning = lookups(&results, "meaning", Some(model));
        assert!(!saved(&meaning), "this run saved only the first five");
        let meaning_alone = score(&meaning);
        let recorded = score(&lookups(&results, "both", Some(model)));
        writeln!(
            table,
            "\n{model}\n\n| Ranking | Top five | MRR |\n| --- | --- | --- |"
        )
        .unwrap();
        row(&mut table, "words alone (recorded)", words_alone);
        row(&mut table, "meaning alone (recorded)", meaning_alone);
        row(&mut table, "equal (recorded)", recorded);
        for (name, weights) in STRATEGIES {
            row(
                &mut table,
                name,
                score(&replay(&words, &meaning, items, weights)),
            );
        }

        // The estimate is close to what the run's own merge gave.
        let equal = score(&replay(&words, &meaning, items, Merge::EQUAL));
        if model != "apple-nl-sentence-en" {
            assert!(
                (equal.top_five - recorded.top_five).abs() <= 0.015,
                "{model}: {equal:?} against {recorded:?}"
            );
            assert!(
                (equal.reciprocal_rank - recorded.reciprocal_rank).abs() <= 0.02,
                "{model}: {equal:?} against {recorded:?}"
            );
        }

        let chosen = score(&replay(&words, &meaning, items, Merge::for_model(model)));
        let floor = if Merge::for_model(model) == Merge::WEAK {
            words_alone
        } else {
            meaning_alone
        };
        assert!(
            chosen.top_five >= floor.top_five && chosen.reciprocal_rank >= floor.reciprocal_rank,
            "{model}: {chosen:?} against {floor:?}\n{table}"
        );
        assert!(
            chosen.top_five > recorded.top_five,
            "{model}: {chosen:?} against {recorded:?}"
        );
    }
    println!("{table}");
}
