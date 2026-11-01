//! `carrel review show` listing a proposal's readings of what its item is, and `carrel review
//! pick` choosing one (#449), on a copy of the sample archive whose proposal is given two
//! readings. Drafting them is tested in the core (`crates/carrel-core/tests/readings.rs`).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

/// A copy of the sample archive with its own configuration and home folders, so tests never
/// touch the real ones.
struct Sandbox {
    dir: tempfile::TempDir,
}

const PROPOSAL: &str = "_System/queue/review/01a18701-0500-776a-9867-08f000000001.json";

impl Sandbox {
    /// The sample archive, its proposal given a second reading.
    fn with_readings() -> Self {
        let sandbox = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
            &sandbox.root(),
        );
        let path = sandbox.root().join(PROPOSAL);
        let mut entry: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let card = entry["card"].clone();
        entry["readings"] = json!([
            {
                "title": card["title"],
                "summary": card["summary"],
                "tags": card["tags"],
                "reason": "The file name says it's a diffuser."
            },
            {
                "title": "Lampshade print file",
                "summary": "A 3D-print file for a small lampshade.",
                "tags": [],
                "reason": "The shape is a shallow dome.",
                "against": ["the file name (“lamp-diffuser”)"]
            }
        ]);
        std::fs::write(&path, entry.to_string()).unwrap();
        sandbox
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("archive")
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_carrel"))
            .args(args)
            .current_dir(self.root())
            .env("CARREL_CONFIG_DIR", self.dir.path().join("config"))
            .env("CARREL_WORKER", "off")
            .env("HOME", self.dir.path().join("home"))
            .env("USERPROFILE", self.dir.path().join("home"))
            .env("XDG_CONFIG_HOME", self.dir.path().join("home/.config"))
            .env("APPDATA", self.dir.path().join("home/AppData"))
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
fn review_show_lists_the_readings_and_pick_rewrites_the_card() {
    let sandbox = Sandbox::with_readings();
    let shown = sandbox.ok(&["review", "show", "00000001"]);
    assert!(
        shown.contains("What it is isn't clear. Readings (pick one with `carrel review pick"),
        "{shown}"
    );
    assert!(
        shown.contains("1. Desk lamp diffuser (print file)  (the card)"),
        "{shown}"
    );
    assert!(shown.contains("2. Lampshade print file\n"), "{shown}");
    assert!(
        shown.contains("why: The shape is a shallow dome."),
        "{shown}"
    );
    assert!(
        shown.contains("doesn't fit: the file name (“lamp-diffuser”)"),
        "{shown}"
    );

    let asked = sandbox.ok(&["review", "pick", "00000001", "2"]);
    assert!(asked.contains("Lampshade print file"), "{asked}");
    assert!(
        asked.contains("Not saved"),
        "no terminal to ask on: {asked}"
    );

    let saved = sandbox.json(&["review", "pick", "00000001", "2", "--yes"]);
    assert_eq!(saved["saved"], true, "{saved}");
    let after = sandbox.json(&["review", "show", "00000001"]);
    assert_eq!(after["card"]["title"], "Lampshade print file");
    assert_eq!(
        after["card"]["summary"],
        "A 3D-print file for a small lampshade."
    );
    assert_eq!(
        after["card"]["tags"],
        json!(null),
        "the reading has no tags"
    );
    assert_eq!(
        after["card"]["generated"]["corrections"][0]["said"],
        "Picked reading 2: “Lampshade print file”"
    );
    let shown = sandbox.ok(&["review", "show", "00000001"]);
    assert!(
        shown.contains("2. Lampshade print file  (the card)"),
        "{shown}"
    );
    assert!(
        !shown.contains("isn't clear"),
        "the owner settled it: {shown}"
    );

    let wrong = sandbox.run(&["review", "pick", "00000001", "3", "--yes"]);
    assert_eq!(wrong.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&wrong.stderr).contains("there's no reading 3; pick 1 to 2"),
        "{}",
        String::from_utf8_lossy(&wrong.stderr)
    );
}
