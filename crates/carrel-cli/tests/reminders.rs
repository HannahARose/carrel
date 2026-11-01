//! `carrel reminders`, the "Reminders" lines of `carrel status`, and `carrel agent` showing
//! each reminder once, run as a person or script would, on a copy of the sample archive.
//! Its `oscillator-sim` is checked out with a pending entry, and a filing proposal waits in
//! Review, so with the default `reminders` the idle and weekly reminders are due. The agent's
//! notifications go to a file (`CARREL_NOTIFY_LOG`) instead of the desktop. Which reminders
//! are due when is tested in the core (`crates/carrel-core/tests/reminders.rs`).

use std::path::{Path, PathBuf};
use std::process::Command;

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

/// A copy of the sample archive with history, and its own configuration and home folders,
/// so tests never touch the real ones.
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
        sandbox.json(&["init", "."]);
        sandbox
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("archive")
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_carrel"));
        command
            .args(args)
            .current_dir(self.root())
            .env("CARREL_CONFIG_DIR", self.path("config"))
            .env("CARREL_TRASH_DIR", self.path("Trash"))
            .env("CARREL_WORKER", "off")
            .env("CARREL_NOTIFY_LOG", self.path("notifications.jsonl"))
            .env("HOME", self.path("home"))
            .env("USERPROFILE", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("home/.config"))
            .env("APPDATA", self.path("home/AppData"));
        command
    }

    fn json(&self, args: &[&str]) -> Value {
        let mut args = args.to_vec();
        args.push("--json");
        let out = self.command(&args).output().unwrap();
        assert!(
            out.status.success(),
            "carrel {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }

    /// What the agent has shown, by kind.
    fn notified(&self) -> Vec<String> {
        std::fs::read_to_string(self.path("notifications.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| {
                let value: Value = serde_json::from_str(line).unwrap();
                assert_eq!(value["archive"], "Sample archive");
                value["reminder"]["kind"].as_str().unwrap().to_owned()
            })
            .collect()
    }
}

fn kinds(reminders: &Value) -> Vec<&str> {
    reminders
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["kind"].as_str().unwrap())
        .collect()
}

#[test]
fn reminders_are_listed_by_commands_and_shown_once_by_the_agent() {
    let sandbox = Sandbox::new();
    let listed = sandbox.json(&["reminders"]);
    assert_eq!(kinds(&listed["reminders"]), ["idle", "weekly"]);
    assert_eq!(
        listed["reminders"][0]["text"],
        "You worked on “oscillator-sim” for 2h 10m. Its entry is waiting for you."
    );
    assert_eq!(
        listed["reminders"][0]["item"],
        "01a18701-0500-776a-9867-080924eb019c"
    );
    let out = sandbox.command(&["status"]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("\nReminders:\n  You worked on “oscillator-sim”"),
        "{text}"
    );
    assert_eq!(
        sandbox.notified(),
        Vec::<String>::new(),
        "commands never notify"
    );

    let agent = |sandbox: &Sandbox| {
        let out = sandbox
            .command(&["agent", "--poll", "--stop-after", "2"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let said = agent(&sandbox);
    assert!(said.contains("2 reminders shown"), "{said}");
    assert_eq!(sandbox.notified(), ["idle", "weekly"]);
    agent(&sandbox);
    assert_eq!(sandbox.notified(), ["idle", "weekly"], "each is shown once");
    // Status still lists them: the weekly sum on the day it was first shown.
    assert_eq!(
        kinds(&sandbox.json(&["status"])["reminders"]),
        ["idle", "weekly"]
    );
    assert!(sandbox.path("config/reminders.json").is_file());

    sandbox.json(&["settings", "set", "reminders", "none"]);
    assert_eq!(
        kinds(&sandbox.json(&["reminders"])["reminders"]),
        Vec::<&str>::new()
    );
}
