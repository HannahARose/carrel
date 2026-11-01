//! `carrel restructure propose|list|show|approve|reject` and the `restructure.*` settings, run
//! as a person or script would, on a copy of the sample archive with six bird-walk notes added
//! to Reading. The proposals themselves are tested in the core
//! (`crates/carrel-core/tests/restructure_section.rs`), with fake embeddings and a scripted
//! model; here there's no meaning index and no model (`--no-model`), so items are grouped by
//! their tags alone.

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

/// A copy of the sample archive with history and six notes in Reading that belong together,
/// and its own configuration folder, so tests never touch the real one.
struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let sandbox = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        let root = sandbox.root();
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
            &root,
        );
        for n in 1..=6 {
            std::fs::write(
                root.join(format!("Reading/bird-walk-{n}.md")),
                format!(
                    "---\ncarrel: card\nformat: \"1.0\"\nid: 01a19600-0000-7000-8000-00000000000{n}\n\
                     kind: physical\ntitle: \"Bird walk {n}\"\nsummary: \"Notes from bird walk {n}\"\n\
                     tags: [birds, walks]\nsensitivity: personal\nlocation: \"Shelf\"\n---\n\n\
                     The owner's notes from walk {n}.\n"
                ),
            )
            .unwrap();
        }
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

    fn fails(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(!out.status.success(), "carrel {args:?} succeeded");
        String::from_utf8(out.stderr).unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let mut args = args.to_vec();
        args.push("--json");
        serde_json::from_str(&self.ok(&args)).unwrap()
    }

    /// Proposes for Reading and returns the proposal's JSON.
    fn propose(&self) -> Value {
        self.json(&["restructure", "propose", "Reading", "--no-model"])["proposal"].clone()
    }
}

#[test]
fn propose_groups_changes_by_kind_with_reasons_and_the_judgement() {
    let sandbox = Sandbox::new();
    let text = sandbox.ok(&["restructure", "propose", "Reading", "--no-model"]);
    assert!(text.contains("Restructure proposal"), "{text}");
    assert!(text.contains("for Reading"), "{text}");
    assert!(text.contains("Eagerness: balanced (the archive's setting)"), "{text}");
    assert!(text.contains("Findability: untested"), "{text}");
    assert!(text.contains("Subdividing a collection"), "{text}");
    assert!(text.contains("create the collection Reading/Birds"), "{text}");
    assert!(text.contains("Changes 6 items: Bird walk 1"), "{text}");
    assert!(text.contains("Rewriting design documents and tags"), "{text}");

    let shown = sandbox.json(&["restructure", "show"]);
    let proposal = &shown["proposal"];
    assert_eq!(proposal["kind"], "section");
    assert_eq!(proposal["section"], "Reading");
    assert_eq!(proposal["invasiveness"], "subdividing");
    assert_eq!(proposal["assessment"]["eagerness"], "balanced");
    let changes = proposal["changes"].as_array().unwrap();
    assert_eq!(changes[0]["change"]["kind"], "create-collection");
    assert_eq!(changes[1]["change"]["kind"], "move-items");
    assert_eq!(changes[1]["depends_on"][0], "1");
    let file = sandbox.root().join(format!(
        "_System/queue/proposals/{}.json",
        proposal["id"].as_str().unwrap()
    ));
    assert!(file.is_file(), "it waits in the queue as FORMAT.md says");

    let list = sandbox.json(&["restructure", "list"]);
    assert_eq!(list["proposals"].as_array().unwrap().len(), 1);
    assert_eq!(list["proposals"][0]["section"], "Reading");

    // Proposing again for the same section replaces it.
    let again = sandbox.json(&["restructure", "propose", "Reading", "--no-model"]);
    assert_eq!(again["replaced"], 1);

    let wrong = sandbox.fails(&["restructure", "propose", "Nowhere", "--no-model"]);
    assert!(wrong.contains("there's no collection Nowhere"), "{wrong}");
}

#[test]
fn approving_part_says_what_was_dropped_and_undo_reverses_it_all() {
    let sandbox = Sandbox::new();
    let proposal = sandbox.propose();
    let id = proposal["id"].as_str().unwrap();

    // The move without its collection drops, and so does the rewrite naming it.
    let nothing = sandbox.fails(&["restructure", "approve", id, "--only", "2,3"]);
    assert!(nothing.contains("2 needs 1"), "{nothing}");

    let text = sandbox.ok(&["restructure", "approve", id, "--only", "1,3"]);
    assert!(text.contains("Restructure Reading: 2 of 3 changes"), "{text}");
    assert!(text.contains("Left out: 2."), "{text}");
    assert!(text.contains("rewrote Reading/_collection.md"), "{text}");
    let root = sandbox.root();
    assert!(root.join("Reading/Birds/_collection.md").is_file());
    assert!(root.join("Reading/bird-walk-1.md").is_file(), "the move was left out");
    assert!(sandbox.json(&["restructure", "list"])["proposals"]
        .as_array()
        .unwrap()
        .is_empty());

    // All of it, then undone as one change.
    sandbox.ok(&["undo", &latest(&sandbox)]);
    assert!(!root.join("Reading/Birds/_collection.md").exists());
    // Reading was just restructured, so at Balanced the stability penalty holds it back.
    let recent = sandbox.json(&["restructure", "propose", "Reading", "--no-model"]);
    assert!(recent["proposal"].is_null(), "{recent}");
    assert_eq!(recent["assessment"]["recently_restructured"], true);
    sandbox.ok(&["settings", "set", "restructure.eagerness", "eager"]);
    let proposal = sandbox.propose();
    let approved = sandbox.json(&[
        "restructure",
        "approve",
        proposal["id"].as_str().unwrap(),
    ]);
    assert_eq!(approved["approved"]["outcomes"][1]["items"].as_array().unwrap().len(), 6);
    assert!(root.join("Reading/Birds/bird-walk-1.md").is_file());
    let log = sandbox.json(&["log"]);
    assert_eq!(log[0]["operation"], "restructure");
    sandbox.ok(&["undo", &latest(&sandbox)]);
    assert!(root.join("Reading/bird-walk-1.md").is_file());
    assert!(!root.join("Reading/Birds").exists());
}

fn latest(sandbox: &Sandbox) -> String {
    sandbox.json(&["log"])[0]["id"].as_str().unwrap().to_owned()
}

#[test]
fn reject_drops_a_proposal_and_changes_nothing() {
    let sandbox = Sandbox::new();
    let proposal = sandbox.propose();
    let text = sandbox.ok(&["restructure", "reject", proposal["id"].as_str().unwrap()]);
    assert!(text.contains("nothing changed"), "{text}");
    assert!(!sandbox.root().join("Reading/Birds").exists());
    let none = sandbox.fails(&["restructure", "show"]);
    assert!(none.contains("no restructure proposal is waiting"), "{none}");
}

#[test]
fn eagerness_and_its_numbers_are_settings() {
    let sandbox = Sandbox::new();
    let shown = sandbox.ok(&["settings", "list"]);
    assert!(shown.contains("0.15 (from balanced)"), "{shown}");
    sandbox.ok(&["settings", "set", "restructure.eagerness", "settled"]);
    let shown = sandbox.ok(&["settings", "list"]);
    assert!(shown.contains("0.3 (from settled)"), "{shown}");
    let wrong = sandbox.fails(&["settings", "set", "restructure.eagerness", "keen"]);
    assert!(wrong.contains("settled, balanced, eager"), "{wrong}");

    // A margin so high nothing clears it: the proposal says what it held back.
    sandbox.ok(&["settings", "set", "restructure.margin", "1"]);
    let held = sandbox.json(&["restructure", "propose", "Reading", "--no-model"]);
    assert!(held["proposal"].is_null());
    assert!(
        held["assessment"]["held_back"][0]["why"]
            .as_str()
            .unwrap()
            .contains("at this eagerness")
    );
    sandbox.ok(&["settings", "set", "restructure.margin", "default"]);
    sandbox.ok(&["settings", "set", "restructure.move_budget", "none"]);
    let settings: Value = serde_json::from_str(
        &std::fs::read_to_string(sandbox.root().join("_System/settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["restructure"]["eagerness"], "settled");
    assert_eq!(settings["restructure"]["move_budget"], "none");
    assert!(settings["restructure"].get("margin").is_none());
}
