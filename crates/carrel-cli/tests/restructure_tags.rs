//! `carrel restructure tags`, run as a person or script would, on a copy of the sample
//! archive. The proposals themselves are tested in the core
//! (`crates/carrel-core/tests/restructure_tags.rs`), with fake embeddings.

use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::Value;

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

/// A copy of the sample archive with history, and its own configuration folder, so tests
/// never touch the real one.
struct Sandbox {
    dir: tempfile::TempDir,
}

const PARTS: &str = "Projects/Hardware/parts-list.csv.md";
const NOTES: &str = "Projects/Hardware/notes.card.md";

impl Sandbox {
    /// The sample archive, with two cards' tags made a model's, one of them written
    /// `Electronics` with a rare tag beside it, and one card whose `Electronics` the owner
    /// wrote.
    fn new() -> Self {
        let sandbox = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
            &sandbox.root(),
        );
        sandbox.ok(&["init", "."]);
        let generated = "generated:\n  fields: [tags]\n  by: \"carrel test\"\n  at: \"2026-11-01\"";
        sandbox.retag(
            PARTS,
            &format!("tags: [Electronics, gadgetry]\n{generated}"),
        );
        sandbox.retag(NOTES, "tags: [Electronics]");
        sandbox
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("archive")
    }

    fn retag(&self, card: &str, with: &str) {
        let path = self.root().join(card);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("tags: [electronics]\n"), "{card}");
        std::fs::write(&path, text.replacen("tags: [electronics]", with, 1)).unwrap();
    }

    fn read(&self, card: &str) -> String {
        std::fs::read_to_string(self.root().join(card)).unwrap()
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
}

#[test]
fn propose_lists_each_change_with_why_and_the_owners_own_tags_held() {
    let sandbox = Sandbox::new();
    let text = sandbox.ok(&["restructure", "tags"]);
    assert!(text.contains("Without the meaning index"), "{text}");
    assert!(
        text.contains("1. merge “Electronics” into “electronics”"),
        "{text}"
    );
    assert!(text.contains("differ only in capitals"), "{text}");
    assert!(
        text.contains("Changes 1 item: Desk lamp parts list"),
        "{text}"
    );
    assert!(text.contains("you wrote its tags yourself"), "{text}");
    assert!(text.contains("retire “gadgetry”"), "{text}");

    let shown = sandbox.json(&["restructure", "tags", "show"]);
    let changes = shown["proposal"]["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0]["change"]["kind"], "merge-tags");
    assert_eq!(changes[0]["change"]["into"], "electronics");
    assert_eq!(changes[0]["held"][0]["card"], NOTES);
    assert_eq!(changes[1]["change"]["kind"], "retire-tag");
    assert_eq!(shown["proposal"]["invasiveness"], "rewriting");

    // Proposing again replaces the waiting proposal rather than piling up.
    let again = sandbox.json(&["restructure", "tags", "propose"]);
    assert_eq!(again["replaced"], 1);
}

#[test]
fn approving_part_is_one_change_that_undo_reverses() {
    let sandbox = Sandbox::new();
    sandbox.ok(&["restructure", "tags"]);
    let before = sandbox.read(PARTS);
    let approved = sandbox.json(&["restructure", "tags", "approve", "--only", "1"]);
    assert_eq!(approved["approved"]["selection"]["approved"][0], "1");
    assert_eq!(approved["approved"]["selection"]["rejected"][0], "2");
    let commit = approved["approved"]["commit"].as_str().unwrap().to_owned();

    let after = sandbox.read(PARTS);
    assert!(after.contains("- electronics\n- gadgetry"), "{after}");
    assert!(
        sandbox.read(NOTES).contains("tags: [Electronics]"),
        "the owner's stays"
    );
    let log = sandbox.ok(&["log", "-n", "1"]);
    assert!(
        log.contains("restructure") && log.contains("Restructure tags: 1 of 2"),
        "{log}"
    );
    // The proposal has been used.
    assert!(
        !sandbox
            .run(&["restructure", "tags", "show"])
            .status
            .success()
    );

    sandbox.ok(&["undo", &commit]);
    assert_eq!(sandbox.read(PARTS), before);
}

#[test]
fn reject_changes_nothing_and_an_unknown_change_is_refused() {
    let sandbox = Sandbox::new();
    sandbox.ok(&["restructure", "tags"]);
    let out = sandbox.run(&["restructure", "tags", "approve", "--only", "7"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no change 7"));
    let before = sandbox.read(PARTS);
    let rejected = sandbox.json(&["restructure", "tags", "reject"]);
    assert!(rejected["rejected"].is_string());
    assert_eq!(sandbox.read(PARTS), before);
    assert!(
        !sandbox
            .run(&["restructure", "tags", "show"])
            .status
            .success()
    );
}
