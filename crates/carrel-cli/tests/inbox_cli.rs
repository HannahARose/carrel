//! Acting through this device's inbox while another device is the writer, and restoring from
//! the holding folder, with the `carrel` binary run as a person would.

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

/// A sandbox with its own configuration folder, so tests never touch the real one.
struct InboxSandbox {
    dir: tempfile::TempDir,
}

impl InboxSandbox {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    /// A copy of the sample archive, with its history started.
    fn sample(&self) -> PathBuf {
        let root = self.path("sample");
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
            &root,
        );
        self.ok(self.dir.path(), &["init", root.to_str().unwrap()]);
        root
    }

    fn run_in(&self, cwd: &Path, args: &[&str]) -> Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_carrel"))
            .args(args)
            .current_dir(cwd)
            .env("CARREL_CONFIG_DIR", self.path("config"))
            .env("CARREL_TRASH_DIR", self.path("Trash"))
            .env("CARREL_WORKER", "off")
            .env_remove("CARREL_SYNCED")
            .output()
            .unwrap()
    }

    fn ok(&self, cwd: &Path, args: &[&str]) -> String {
        let out = self.run_in(cwd, args);
        assert!(
            out.status.success(),
            "carrel {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn json(&self, cwd: &Path, args: &[&str]) -> Value {
        let mut args = args.to_vec();
        args.push("--json");
        serde_json::from_str(&self.ok(cwd, &args)).unwrap()
    }
}

const OTHER_WRITER: &str = "01928f3a-0000-7000-8000-00000000d002";

/// Another device takes the writer lease of the archive at `root`, until long after the test.
fn another_device_writes(root: &Path) {
    let lock = serde_json::json!({
        "token": "019a2b3c-4d5e-7f60-8a1b-2c3d4e5f6a7b",
        "person": "01928f39-eeee-7fff-8000-000000000005",
        "device": OTHER_WRITER,
        "purpose": "ingest",
        "program": "carrel 0.1.0",
        "acquired": "2026-10-29T14:02:11-07:00",
        "expires": "2999-01-01T00:00:00Z",
    });
    std::fs::write(root.join("_System/writer.lock"), lock.to_string()).unwrap();
}

/// The `action` of the inbox entry `sent` names, having checked it's an inbox entry.
fn sent_action(root: &Path, sent: &Value) -> String {
    assert_eq!(sent["applied"], false, "{sent}");
    assert_eq!(sent["writer"], OTHER_WRITER, "{sent}");
    let entry = PathBuf::from(sent["inbox"].as_str().unwrap());
    let inboxes = root.canonicalize().unwrap().join("_System/inbox");
    assert!(entry.starts_with(&inboxes), "{}", entry.display());
    let text = std::fs::read_to_string(&entry).unwrap();
    let entry: Value = serde_json::from_str(&text).unwrap();
    entry["action"].as_str().unwrap().to_owned()
}

#[test]
fn flags_rejections_and_undos_go_into_the_inbox_while_another_device_writes() {
    let sandbox = InboxSandbox::new();
    let root = sandbox.sample();
    let card = "Reading/On the Origin of Species.md";
    sandbox.ok(&root, &["flag", "Projects", "--note", "split this folder"]);
    let log = sandbox.json(&root, &["log"]);
    let flagged = log[0]["id"].as_str().unwrap().to_owned();
    let changes = log.as_array().unwrap().len();
    let before = std::fs::read_to_string(root.join(card)).unwrap();

    another_device_writes(&root);
    let sent = sandbox.json(&root, &["flag", card, "--note", "wrong title"]);
    assert_eq!(sent_action(&root, &sent), "flag");
    let sent = sandbox.json(&root, &["review", "reject", "00000001"]);
    assert_eq!(sent_action(&root, &sent), "reject");
    let out = sandbox.ok(&root, &["undo", &flagged[..8]]);
    assert!(out.contains("is the writer now"), "{out}");
    assert!(out.contains("Sent: undo a change."), "{out}");

    // Nothing else changed: the card, the proposal and the history are as they were.
    assert_eq!(std::fs::read_to_string(root.join(card)).unwrap(), before);
    let review = sandbox.json(&root, &["review"]);
    assert_eq!(review["entries"].as_array().unwrap().len(), 1, "{review}");
    assert_eq!(review["entries"][0]["state"], "proposed");
    let log = sandbox.json(&root, &["log"]);
    assert_eq!(log.as_array().unwrap().len(), changes);
    assert!(
        std::fs::read_to_string(root.join("Projects/_collection.md"))
            .unwrap()
            .contains("split this folder"),
        "the undo isn't done here"
    );

    let status = sandbox.ok(&root, &["status"]);
    assert!(status.contains("\nInbox:\n"), "{status}");
    assert!(
        status.contains("3 changes in this device's inbox, waiting for the writer"),
        "{status}"
    );
    let status = sandbox.json(&root, &["status"]);
    assert_eq!(status["inbox"]["through_inbox"], true, "{status}");
    assert_eq!(status["inbox"]["writer"], OTHER_WRITER);
    assert_eq!(status["inbox"]["waiting"], 3);
}

#[test]
fn a_return_without_backups_keeps_the_version_it_replaced_to_restore() {
    let sandbox = InboxSandbox::new();
    let root = sandbox.sample();
    let item = "Projects/Hardware/parts-list.csv";
    let original = std::fs::read(root.join(item)).unwrap();
    let desk = sandbox.path("desk");
    let out = sandbox.json(&root, &["checkout", item, "--to", desk.to_str().unwrap()]);
    let working = PathBuf::from(out["working"].as_str().unwrap());
    std::fs::write(&working, "part,count\nlamp,2\n").unwrap();

    let out = sandbox.ok(&root, &["return", item, "--yes", "--working-copy", "keep"]);
    assert!(out.contains("kept in the holding folder"), "{out}");
    assert_eq!(
        std::fs::read_to_string(root.join(item)).unwrap(),
        "part,count\nlamp,2\n"
    );
    let status = sandbox.ok(&root, &["status"]);
    assert!(status.contains("Holding folder:"), "{status}");
    assert!(status.contains("1 version replaced here"), "{status}");
    let status = sandbox.json(&root, &["status"]);
    assert_eq!(status["holding"]["count"], 1, "{status}");

    let to = sandbox.path("restored.csv");
    let restored = sandbox.json(
        &root,
        &[
            "backup",
            "restore",
            item,
            "--held",
            "--to",
            to.to_str().unwrap(),
        ],
    );
    assert_eq!(std::fs::read(&to).unwrap(), original, "{restored}");
    let held = restored["held"].as_str().unwrap().to_owned();

    // Without backups set up, it's restored from the holding folder anyway, beside the
    // archive, and an identifier picks the version.
    let beside = sandbox.json(&root, &["backup", "restore", item, "--held", &held[..13]]);
    let beside = PathBuf::from(beside["to"].as_str().unwrap());
    assert_eq!(
        beside.parent().unwrap().canonicalize().unwrap(),
        sandbox.dir.path().canonicalize().unwrap(),
        "{}",
        beside.display()
    );
    assert_eq!(std::fs::read(&beside).unwrap(), original);
    let again = sandbox.json(&root, &["backup", "restore", item]);
    assert_eq!(again["held"], held.as_str());
    let missing = sandbox.run_in(
        &root,
        &["backup", "restore", "Projects/Hardware/lamp-build.md"],
    );
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("in the holding folder"),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
}
