//! `carrel edit ITEM --body`, run as a person would with a scripted editor, on a copy of the
//! sample archive: the card's Notes and the rest below its fields, saved as one change, with
//! Carrel's own sections kept as they are.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Output;

const PARTS: &str = "Projects/Hardware/parts-list.csv";

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

/// A copy of the sample archive with history, and its own configuration folder, so the test
/// never touches the real one.
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
        let out = sandbox.run(&["init", "."], None);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        sandbox
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("archive")
    }

    /// Runs `carrel` with `args` in the archive, with `editor` as the editor.
    fn run(&self, args: &[&str], editor: Option<&Path>) -> Output {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_carrel"));
        command
            .args(args)
            .current_dir(self.root())
            .env("CARREL_CONFIG_DIR", self.dir.path().join("config"))
            .env("CARREL_WORKER", "off")
            .env_remove("VISUAL");
        if let Some(editor) = editor {
            command.env("EDITOR", editor);
        }
        command.output().unwrap()
    }

    /// An editor script with `body` as its shell commands, `$1` being the file.
    fn editor(&self, name: &str, body: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
}

#[test]
fn the_body_is_edited_as_one_change_and_carrels_sections_stay() {
    let sandbox = Sandbox::new();
    let card = sandbox.root().join(format!("{PARTS}.md"));
    let before = std::fs::read_to_string(&card).unwrap();
    let fields = before.split("---").nth(1).unwrap().to_owned();

    // Notes added at the end: saved, the fields kept exactly.
    let add = sandbox.editor(
        "add.sh",
        "printf '\\nThe diffuser needs a second print.\\n' >> \"$1\"",
    );
    let out = sandbox.run(&["edit", PARTS, "--body", "--json"], Some(&add));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["changed"], serde_json::json!(["notes"]));
    let after = std::fs::read_to_string(&card).unwrap();
    assert!(
        after.contains("The diffuser needs a second print."),
        "{after}"
    );
    assert!(after.contains(&fields), "the fields are kept");
    assert!(after.contains("<!-- carrel:history -->"));

    // The history's markers taken out: opened again with what to fix at the top, and the
    // owner's second try, with the markers back, is saved.
    let state = sandbox.dir.path().join("tries");
    let fix = sandbox.editor(
        "fix.sh",
        &format!(
            "if [ ! -f '{state}' ]; then\n  cp \"$1\" '{state}.orig'\n  touch '{state}'\n  grep -v 'carrel:history' \"$1\" > \"$1.new\" && mv \"$1.new\" \"$1\"\nelse\n  grep -c '^<!-- To fix: ' \"$1\" > '{state}.problems'\n  cp '{state}.orig' \"$1\"\n  printf 'Ordered the LED strip.\\n' >> \"$1\"\nfi",
            state = state.display()
        ),
    );
    let out = sandbox.run(&["edit", PARTS, "--body"], Some(&fix));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let problems = std::fs::read_to_string(state.with_extension("problems")).unwrap();
    assert_eq!(problems.trim(), "1", "one thing to fix, at the top");
    let after = std::fs::read_to_string(&card).unwrap();
    assert!(after.contains("Ordered the LED strip."), "{after}");
    assert!(!after.contains("To fix"), "{after}");
    assert!(after.contains("<!-- /carrel:history -->"));

    // Each is one change, undone on its own.
    let out = sandbox.run(&["log", "--json"], None);
    let log: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        log[0]["summary"].as_str().unwrap().ends_with(": notes"),
        "{log}"
    );
}
