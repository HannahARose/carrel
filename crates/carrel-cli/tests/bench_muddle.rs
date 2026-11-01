//! `carrel bench --crawl` and `carrel bench --restructure` on the muddled test archive, run as
//! a person would, with the example provider as the only text and embedding model. Its answers
//! aren't in the crawler's or the findability test's form, which is enough to check the runs,
//! their reports and carrying on; the scoring is tested in the core
//! (`crates/carrel-core/tests/bench_crawl.rs` and `bench_restructure.rs`) with stub models.

use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::Value;

fn echo_provider() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let status = std::process::Command::new(env!("CARGO"))
        .args([
            "build",
            "--quiet",
            "-p",
            "carrel-core",
            "--example",
            "echo_provider",
        ])
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(status.success());
    let exe = std::env::current_exe().unwrap();
    exe.parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join(format!("echo_provider{}", std::env::consts::EXE_SUFFIX))
}

fn run(dir: &Path, bin: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_carrel"))
        .args(args)
        .current_dir(dir)
        .env("PATH", bin)
        .env("CARREL_CONFIG_DIR", dir.join("config"))
        .env("CARREL_TRASH_DIR", dir.join("Trash"))
        .env("CARREL_WORKER", "off")
        .output()
        .unwrap()
}

#[test]
fn bench_crawl_judges_the_muddled_archive_and_carries_on_where_it_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    std::fs::copy(
        echo_provider(),
        bin.join(format!(
            "carrel-provider-echo{}",
            std::env::consts::EXE_SUFFIX
        )),
    )
    .unwrap();
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/bench");
    let out = dir.path().join("results");
    let args = [
        "bench",
        "--crawl",
        "--corpus",
        corpus.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--model",
        "echo/echo",
        "--limit",
        "3",
    ];

    let first = run(dir.path(), &bin, &args);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let text = String::from_utf8_lossy(&first.stdout).into_owned();
    assert!(text.contains("| Model | Found | Recall |"), "{text}");
    assert!(text.contains("no: recall 0%"), "{text}");
    let report = std::fs::read_to_string(out.join("crawl/report.md")).unwrap();
    assert!(report.contains("# Crawler benchmark report"), "{report}");
    assert!(report.contains("Unusable:"), "{report}");
    assert!(
        !out.join("crawl/archive/Recipes").exists(),
        "the missing collection"
    );
    let results: Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("crawl/results.json")).unwrap())
            .unwrap();
    let model = &results["models"][0];
    assert_eq!(model["offer"], "echo");
    assert_eq!(model["score"]["items"], 3);
    assert_eq!(model["score"]["unusable"], 3);
    assert_eq!(model["meets_bar"], false);

    // Run again, what was judged is kept: the saved run has the same three lines.
    let runs = std::fs::read_dir(out.join("crawl/runs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let before = std::fs::read_to_string(&runs).unwrap();
    assert_eq!(before.lines().count(), 3);
    let again = run(dir.path(), &bin, &args);
    assert!(again.status.success());
    assert_eq!(std::fs::read_to_string(&runs).unwrap(), before);
}

#[test]
fn bench_restructure_scores_a_proposal_at_each_eagerness_and_reuses_them() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    std::fs::copy(
        echo_provider(),
        bin.join(format!(
            "carrel-provider-echo{}",
            std::env::consts::EXE_SUFFIX
        )),
    )
    .unwrap();
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/bench");
    let out = dir.path().join("results");
    let args = [
        "bench",
        "--restructure",
        "--corpus",
        corpus.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--model",
        "echo/echo",
    ];

    let first = run(dir.path(), &bin, &args);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let text = String::from_utf8_lossy(&first.stdout).into_owned();
    assert!(text.contains("| numbers alone | settled |"), "{text}");
    assert!(text.contains("echo-embed vectors"), "{text}");
    let report = std::fs::read_to_string(out.join("restructure/report.md")).unwrap();
    assert!(
        report.contains("# Restructure benchmark report"),
        "{report}"
    );
    let results: Value = serde_json::from_str(
        &std::fs::read_to_string(out.join("restructure/results.json")).unwrap(),
    )
    .unwrap();
    let proposals = results["proposals"].as_array().unwrap();
    assert_eq!(
        proposals.len(),
        6,
        "numbers alone and echo, at three eagernesses each"
    );
    assert_eq!(proposals[0]["score"]["misplaced"], 18);
    assert_eq!(proposals[0]["label"], "numbers alone");
    for numbers in &proposals[..3] {
        let score = &numbers["score"];
        assert!(
            score["changes"] == 0
                || score["untested"]
                    .as_str()
                    .unwrap()
                    .contains("no text model"),
            "{score}"
        );
    }

    // Run again, each proposal's score is reused.
    let saved = out.join("restructure/runs/numbers-only.json");
    let before = std::fs::read_to_string(&saved).unwrap();
    let again = run(dir.path(), &bin, &args);
    assert!(again.status.success());
    assert_eq!(std::fs::read_to_string(&saved).unwrap(), before);
}
