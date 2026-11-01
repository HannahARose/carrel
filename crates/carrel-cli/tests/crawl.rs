//! `carrel crawl` and the crawler's findings in `carrel review`, run as a person or script
//! would, on a copy of the sample archive. The crawler itself, with a scripted model, is
//! tested in the core (`crates/carrel-core/tests/crawl.rs`).

use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::{Value, json};

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

/// The sample's one item in Holding, whose summary a model wrote.
const SCAN: &str = "01a18701-0500-776a-9867-0810dc3b08d1";

/// A copy of the sample archive with history, and its own configuration folder, so tests
/// never touch the real one.
struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let sandbox = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
            &sandbox.root(),
        );
        sandbox.ok(&["init", "."]);
        sandbox
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("archive")
    }

    fn run(&self, args: &[&str]) -> Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_carrel"))
            .args(args)
            .current_dir(self.root())
            .env("CARREL_CONFIG_DIR", self.dir.path().join("config"))
            .env("CARREL_TRASH_DIR", self.dir.path().join("Trash"))
            .env("CARREL_WORKER", "off")
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "carrel {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let mut args = args.to_vec();
        args.push("--json");
        serde_json::from_str(&self.ok(&args)).unwrap()
    }

    /// Leaves a finding in Review, as a crawler pass would.
    fn finding(&self, id: &str, change: &Value) {
        let dir = self.root().join("_System/queue/crawler");
        std::fs::create_dir_all(&dir).unwrap();
        let mut entry = json!({
            "carrel": "crawl",
            "format": "1.0",
            "id": id,
            "created": "2026-11-01T10:00:00Z",
            "item": SCAN,
            "card": "Holding/untitled scan.txt.md",
            "title": "Untitled scan",
            "reason": "It's a note about a meeting.",
            "confidence": 0.8,
            "model": "test/m"
        });
        for (key, value) in change.as_object().unwrap() {
            entry[key] = value.clone();
        }
        std::fs::write(dir.join(format!("{id}.json")), entry.to_string()).unwrap();
    }
}

#[test]
fn status_shows_the_pace_and_whats_due_and_the_settings_change_it() {
    let sandbox = Sandbox::new();
    let text = sandbox.ok(&["crawl"]);
    assert!(text.contains("Pace: steady"), "{text}");
    assert!(text.contains("only proposes"), "{text}");
    assert!(text.contains("1 item in Holding"), "{text}");
    assert!(text.contains("No pass has run"), "{text}");

    sandbox.ok(&["settings", "set", "crawler.pace", "gentle"]);
    sandbox.ok(&["settings", "set", "crawler.changes", "on"]);
    let status = sandbox.json(&["crawl", "status"]);
    assert_eq!(status["pace"], "gentle");
    assert_eq!(status["changes"], true);
    assert_eq!(status["judged"], 0);
    assert_eq!(status["waiting"][0][0], "holding");

    let out = sandbox.run(&["settings", "set", "crawler.pace", "frantic"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("gentle"));

    let none = sandbox.ok(&["crawl", "undo", "7d"]);
    assert!(none.contains("no changes"), "{none}");
}

#[test]
fn the_crawlers_findings_are_approved_and_rejected_in_review() {
    let sandbox = Sandbox::new();
    sandbox.finding(
        "01a195c0-0000-7000-8000-00000000c001",
        &json!({"change": "move", "from": "Holding", "to": "Reading"}),
    );
    sandbox.finding(
        "01a195c0-0000-7000-8000-00000000c002",
        &json!({"change": "correct", "field": "summary", "value": "A scanned note moving a meeting."}),
    );
    let list = sandbox.ok(&["review"]);
    assert!(list.contains("From the crawler"), "{list}");
    assert!(list.contains("move “Untitled scan” to Reading"), "{list}");
    assert!(list.contains("80% sure"), "{list}");
    assert_eq!(
        sandbox.json(&["review"])["crawler"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(sandbox.ok(&["crawl"]).contains("2 findings waiting"));

    let approved = sandbox.ok(&["review", "approve", "0000c002"]);
    assert!(approved.contains("Approved"), "{approved}");
    let card =
        std::fs::read_to_string(sandbox.root().join("Holding/untitled scan.txt.md")).unwrap();
    assert!(card.contains("A scanned note moving a meeting."), "{card}");

    let rejected = sandbox.ok(&["review", "reject", "0000c001"]);
    assert!(rejected.contains("won't suggest it again"), "{rejected}");
    assert!(!sandbox.ok(&["review"]).contains("From the crawler"));
    assert!(
        sandbox.root().join("Holding/untitled scan.txt.md").exists(),
        "rejecting moved nothing"
    );
}
