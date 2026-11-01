//! `carrel bench --crawl`, run as a person would, with the example provider as the only text
//! model. Its answers aren't in the crawler's form, so every judgment is unusable, which is
//! enough to check the run, its report and carrying on; the scoring is tested in the core
//! (`crates/carrel-core/tests/bench_crawl.rs`) with stub models.

use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::Value;

fn echo_provider() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // Built beside this test, in the target folder it runs from, which `cargo llvm-cov`
    // moves.
    let exe = std::env::current_exe().unwrap();
    let profile = exe.parent().unwrap().parent().unwrap();
    let status = std::process::Command::new(env!("CARGO"))
        .args([
            "build",
            "--quiet",
            "-p",
            "carrel-core",
            "--example",
            "echo_provider",
        ])
        .arg("--target-dir")
        .arg(profile.parent().unwrap())
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(status.success());
    profile
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
