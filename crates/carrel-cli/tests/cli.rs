//! The `carrel` binary, run as a person or script would.

use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::Value;

fn sample_archive() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive")
}

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
struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    fn sample(&self) -> PathBuf {
        let root = self.path("sample");
        copy_dir(&sample_archive(), &root);
        root
    }

    fn run_in(&self, cwd: &Path, args: &[&str]) -> Output {
        std::process::Command::new(env!("CARGO_BIN_EXE_carrel"))
            .args(args)
            .current_dir(cwd)
            .env("CARREL_CONFIG_DIR", self.path("config"))
            .output()
            .unwrap()
    }

    /// Runs a command that must succeed and returns its standard output.
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

#[test]
fn init_creates_an_archive_with_history_recorded_by_this_device() {
    let sandbox = Sandbox::new();
    let root = sandbox.path("Notes");
    let text = sandbox.ok(sandbox.dir.path(), &["init", "Notes"]);
    assert!(text.starts_with("Created archive “Notes” in "), "{text}");
    assert!(root.join("_System/archive.json").is_file());
    assert!(!root.join("_System/writer.lock").exists(), "lock released");

    let identity: Value = serde_json::from_str(
        &std::fs::read_to_string(sandbox.path("config/identity.json")).unwrap(),
    )
    .unwrap();
    let log = sandbox.json(&root, &["log"]);
    let entries = log.as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["operation"], "create");
    assert_eq!(entries[0]["summary"], "Create archive “Notes”");
    assert_eq!(entries[0]["person"], identity["person"]);
    assert_eq!(entries[0]["device"], identity["device"]);

    let status = sandbox.json(&root, &["status"]);
    assert_eq!(status["archive"]["name"], "Notes");
    assert_eq!(status["archive"]["access"], "read-write");
    assert_eq!(status["history"]["changes"], 1);
    assert_eq!(status["writer"], Value::Null);

    let again = sandbox.run_in(sandbox.dir.path(), &["init", "Notes"]);
    assert_eq!(again.status.code(), Some(1), "already an archive");
}

#[test]
fn status_counts_items_and_reports_card_problems() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    let status = sandbox.json(&root, &["status"]);
    assert_eq!(status["archive"]["name"], "Sample archive");
    assert_eq!(status["collections"], 4);
    assert_eq!(status["items"]["total"], 12);
    assert_eq!(status["items"]["by_kind"]["file"], 4);
    assert_eq!(status["problems"], Value::Array(vec![]));
    assert_eq!(status["history"], Value::Null, "the fixture has no history");

    let card = root.join("Reading/On the Origin of Species.md");
    let text = std::fs::read_to_string(&card).unwrap();
    std::fs::write(&card, text.replacen("[biology,", "[Not-A-Tag,", 1)).unwrap();
    let status = sandbox.json(&root, &["status"]);
    let problems = status["problems"].as_array().unwrap();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0]["card"], "Reading/On the Origin of Species.md");
    let report = sandbox.ok(&root, &["status"]);
    assert!(report.contains("1 card with problems:"), "{report}");
}

#[test]
fn status_shows_who_is_writing() {
    use carrel_core::archive::Archive;
    use carrel_core::card::Id;
    use carrel_core::coordinator::{Coordinator, DEFAULT_LEASE, DiskLock, Holder};

    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    let archive = Archive::open(&root).unwrap();
    let holder = Holder {
        person: Id::new_v7(),
        device: Id::new_v7(),
    };
    let _lease = DiskLock::for_archive(&archive)
        .acquire(holder, "ingest", DEFAULT_LEASE)
        .unwrap();
    let status = sandbox.json(&root, &["status"]);
    assert_eq!(status["writer"]["purpose"], "ingest");
    assert_eq!(status["writer"]["device"], holder.device.to_string());
    assert_eq!(status["writer"]["expired"], false);
    let report = sandbox.ok(&root, &["status"]);
    assert!(
        report.contains(&format!(
            "Being changed by device {} (ingest)",
            holder.device
        )),
        "{report}"
    );
}

#[test]
fn show_finds_items_by_id_card_or_item_path() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    let by_item = sandbox.json(&root, &["show", "Projects/Hardware/parts-list.csv"]);
    assert_eq!(by_item["card"], "Projects/Hardware/parts-list.csv.md");
    assert_eq!(by_item["item"], "Projects/Hardware/parts-list.csv");
    assert_eq!(by_item["collection"], "Projects/Hardware");
    assert_eq!(by_item["fields"]["title"], "Desk lamp parts list");

    let id = by_item["fields"]["id"].as_str().unwrap();
    assert_eq!(sandbox.json(&root, &["show", id]), by_item);
    let hardware = root.join("Projects/Hardware");
    assert_eq!(
        sandbox.json(&hardware, &["show", "parts-list.csv.md"]),
        by_item,
        "paths are relative to the current folder"
    );
    assert_eq!(
        sandbox.json(&hardware, &["show", "Projects/Hardware/parts-list.csv"]),
        by_item,
        "or to the archive root"
    );

    let report = sandbox.ok(&root, &["show", id]);
    assert!(report.starts_with("Desk lamp parts list\n"), "{report}");
    assert!(
        report.contains("part-of Projects/Hardware/lamp-build.md"),
        "{report}"
    );

    let missing = sandbox.run_in(&root, &["show", "nothing-here", "--json"]);
    assert_eq!(missing.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&missing.stderr).unwrap();
    assert!(error["error"].as_str().unwrap().contains("nothing-here"));
}

#[test]
fn open_chooses_the_archive_used_elsewhere() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    let outside = sandbox.path("elsewhere");
    std::fs::create_dir(&outside).unwrap();

    let out = sandbox.run_in(&outside, &["status"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("carrel open"));
    assert_eq!(sandbox.json(&outside, &["open"])["archive"], Value::Null);

    sandbox.ok(&outside, &["open", root.join("Reading").to_str().unwrap()]);
    let status = sandbox.json(&outside, &["status"]);
    assert_eq!(status["archive"]["name"], "Sample archive");
    assert_eq!(
        sandbox.json(&outside, &["open"])["archive"],
        std::fs::canonicalize(&root).unwrap().to_str().unwrap()
    );

    let new = sandbox.path("Other");
    sandbox.ok(
        &outside,
        &["init", new.to_str().unwrap(), "--name", "Other"],
    );
    assert_eq!(
        sandbox.json(&outside, &["--archive", new.to_str().unwrap(), "status"])["archive"]["name"],
        "Other",
        "--archive wins"
    );
}

#[test]
fn usage_mistakes_exit_with_status_2() {
    let sandbox = Sandbox::new();
    let out = sandbox.run_in(sandbox.dir.path(), &["frobnicate"]);
    assert_eq!(out.status.code(), Some(2));
    let version = sandbox.ok(sandbox.dir.path(), &["--version"]);
    assert!(version.starts_with("carrel 0."), "{version}");
}
