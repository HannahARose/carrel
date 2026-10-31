//! The `carrel` binary, run as a person or script would.

use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::{Value, json};

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
            .env("CARREL_WORKER", "off")
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
    assert_eq!(
        again.status.code(),
        Some(1),
        "already an archive with history"
    );
    assert!(String::from_utf8_lossy(&again.stderr).contains("history has started"));
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

#[test]
fn init_starts_history_for_an_archive_made_without_it() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    let out = sandbox.run_in(&root, &["log"]);
    assert_eq!(out.status.code(), Some(1), "no history yet");

    let named = sandbox.run_in(&root, &["init", "--name", "Other"]);
    assert_eq!(named.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&named.stderr).contains("already named “Sample archive”"));

    let started = sandbox.json(&root, &["init"]);
    assert_eq!(started["history"], "started");
    assert_eq!(started["archive"]["name"], "Sample archive");
    let log = sandbox.json(&root, &["log"]);
    assert_eq!(log.as_array().unwrap().len(), 1);
    assert_eq!(log[0]["summary"], "Start history of “Sample archive”");
    assert_eq!(log[0]["operation"], "create");
    assert!(!root.join("_System/writer.lock").exists(), "lock released");

    let report = sandbox.ok(&root, &["status"]);
    assert!(report.contains("1 change recorded"), "{report}");
    assert_eq!(sandbox.run_in(&root, &["init"]).status.code(), Some(1));
}

/// The core's example provider, built if needed.
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

#[test]
fn providers_lists_what_this_device_has_and_what_is_used() {
    let sandbox = Sandbox::new();
    let bin = sandbox.path("bin");
    std::fs::create_dir(&bin).unwrap();
    let suffix = std::env::consts::EXE_SUFFIX;
    let echo = echo_provider();
    std::fs::copy(&echo, bin.join(format!("carrel-provider-echo{suffix}"))).unwrap();
    std::fs::copy(&echo, bin.join(format!("carrel-provider-echo-old{suffix}"))).unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_carrel"))
        .args(["providers", "--json"])
        .env("PATH", &bin)
        .env("CARREL_CONFIG_DIR", sandbox.path("config"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    // Other providers built beside `carrel` (in target/) may be listed too.
    let providers = report["providers"].as_array().unwrap();
    let named = |name: &str| providers.iter().find(|p| p["name"] == name).unwrap();
    assert_eq!(providers[0]["name"], "built-in");
    assert_eq!(named("echo")["label"], "Echo");
    assert!(
        named("echo-old")["error"]
            .as_str()
            .unwrap()
            .contains("protocol 0")
    );
    assert_eq!(report["chosen"]["text-model"]["provider"], "echo");
    // The built-in reader, unless a provider built beside `carrel` outranks it.
    let extractor = report["chosen"]["extract-text"]["provider"]
        .as_str()
        .unwrap();
    assert!(
        ["built-in", "extract", "apple"].contains(&extractor),
        "{extractor}"
    );
}

#[test]
fn review_lists_shows_and_rejects_proposals() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    let listed = sandbox.json(&root, &["review"]);
    let entries = listed["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["destination"]["path"], "Projects/Hardware");

    let shown = sandbox.json(&root, &["review", "show", "00000001"]);
    assert_eq!(shown["card"]["title"], "Desk lamp diffuser (print file)");
    let report = sandbox.ok(&root, &["review", "show", "00000001"]);
    assert!(report.contains("new tag      lighting"), "{report}");

    let missing = sandbox.run_in(&root, &["review", "reject", "00000001", "nope"]);
    assert_eq!(missing.status.code(), Some(1), "a typo rejects nothing");
    assert_eq!(
        sandbox.json(&root, &["review"])["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    sandbox.ok(&root, &["review", "reject", "00000001"]);
    assert_eq!(
        sandbox.json(&root, &["review"])["entries"],
        serde_json::json!([])
    );
    let all = sandbox.json(&root, &["review", "--all"]);
    assert_eq!(all["entries"][0]["state"], "rejected");
    let again = sandbox.run_in(&root, &["review", "reject", "00000001"]);
    assert!(String::from_utf8_lossy(&again.stderr).contains("already rejected"));
}

#[test]
fn ingest_reports_kinds_sizes_and_duplicates_without_changing_anything() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    let downloads = sandbox.path("Downloads");
    std::fs::create_dir(&downloads).unwrap();
    std::fs::copy(
        root.join("Projects/Hardware/parts-list.csv"),
        downloads.join("parts-list.csv"),
    )
    .unwrap();
    std::fs::write(downloads.join("new.txt"), "new").unwrap();
    let downloads_arg = downloads.to_str().unwrap();

    let refused = sandbox.run_in(&root, &["ingest", downloads_arg]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--each"));

    let dry = sandbox.json(&root, &["ingest", "--each", "--dry-run", downloads_arg]);
    assert_eq!(dry["saved"], false);
    assert_eq!(
        sandbox.json(&root, &["review"])["entries"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "a dry run saves nothing"
    );

    let report = sandbox.json(&root, &["ingest", "--wait", "--each", downloads_arg]);
    let items = report["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let csv = items
        .iter()
        .find(|i| i["path"].as_str().unwrap().ends_with("parts-list.csv"))
        .unwrap();
    assert_eq!(
        csv["duplicate_of"]["card"],
        "Projects/Hardware/parts-list.csv.md"
    );
    let new = items
        .iter()
        .find(|i| i["path"].as_str().unwrap().ends_with("new.txt"))
        .unwrap();
    assert_eq!(new["kind"], "file");
    assert_eq!(new["size"], 3);
    assert_eq!(new["duplicate_of"], serde_json::Value::Null);
    assert_eq!(new["text"]["read_by"], "built-in/plain-text");
    assert_eq!(new["text"]["characters"], 3);
    assert_eq!(
        csv["text"],
        serde_json::Value::Null,
        "duplicates aren't read"
    );
    // Which model drafts depends on the providers this machine has; there is always a card.
    assert!(new["card"]["title"].is_string());
    assert_eq!(
        csv["card"],
        serde_json::Value::Null,
        "duplicates aren't drafted"
    );

    assert!(downloads.join("new.txt").exists(), "nothing is moved");
    let proposals = report["proposals"].as_array().unwrap();
    assert_eq!(
        proposals.len(),
        1,
        "one proposal for the new file, none for the duplicate"
    );
    assert!(
        proposals[0]["source"]["path"]
            .as_str()
            .unwrap()
            .ends_with("new.txt")
    );
    let listed = sandbox.json(&root, &["review"]);
    assert_eq!(
        listed["entries"].as_array().unwrap().len(),
        2,
        "the sample's one and the new one"
    );
}

#[cfg(unix)]
#[test]
fn review_edit_opens_the_editor_and_saves_a_card_that_checks_out() {
    use std::os::unix::fs::PermissionsExt as _;
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    let editor = sandbox.path("editor.sh");
    std::fs::write(
        &editor,
        "#!/bin/sh\nawk '{sub(/^title: .*/, \"title: Edited by hand\")}1' \"$1\" > \"$1.new\" && mv \"$1.new\" \"$1\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();
    let run = |editor: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_carrel"))
            .args(["review", "edit", "00000001"])
            .current_dir(&root)
            .env("CARREL_CONFIG_DIR", sandbox.path("config"))
            .env_remove("VISUAL")
            .env("EDITOR", editor)
            .output()
            .unwrap()
    };
    let out = run(editor.to_str().unwrap());
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "Saved the card for 00000001 (proposed)\n"
    );
    let shown = sandbox.json(&root, &["review", "show", "00000001"]);
    assert_eq!(shown["card"]["title"], "Edited by hand");

    // An editor that saves nothing changes nothing.
    let out = run("true");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "Nothing changed\n");
    let out = run("false");
    assert!(!out.status.success(), "a failed editor is an error");
}

#[test]
fn bare_carrel_outside_a_terminal_asks_for_a_command() {
    let sandbox = Sandbox::new();
    let out = sandbox.run_in(sandbox.dir.path(), &[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("a command is needed when not in a terminal")
    );
}

#[test]
fn undo_reverses_a_change_by_its_identifier() {
    let sandbox = Sandbox::new();
    let root = sandbox.path("new");
    sandbox.ok(
        sandbox.dir.path(),
        &["init", root.to_str().unwrap(), "--name", "Undo test"],
    );
    let first = sandbox.json(&root, &["log"])[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let blocked = sandbox.run_in(&root, &["undo", &first[..8]]);
    assert!(
        !blocked.status.success(),
        "the first change can't be undone"
    );
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("can't be undone"));

    // A change worth undoing: an item filed in Holding.
    let item = sandbox.path("note.txt");
    std::fs::write(&item, "a note\n").unwrap();
    let ingested = sandbox.json(&root, &["ingest", "--wait", item.to_str().unwrap()]);
    let proposal = ingested["proposals"][0]["id"].as_str().unwrap().to_owned();
    sandbox.ok(&root, &["review", "hold", &proposal]);
    let filed = sandbox.json(&root, &["log"])[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(root.join("Holding/note.txt.md").is_file());

    let missing = sandbox.run_in(&root, &["undo", "ffffffffff"]);
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("no change ffffffffff in the history")
    );
    let out = sandbox.ok(&root, &["undo", &filed[..8]]);
    assert!(out.starts_with("Undid “File “note”” as change "), "{out}");
    assert!(
        !root.join("Holding/note.txt.md").exists(),
        "the card is gone again"
    );
    assert_eq!(
        sandbox.json(&root, &["log"])[0]["reverts"].as_str(),
        Some(filed.as_str())
    );
}

#[test]
fn models_lists_the_catalog_and_asks_before_downloading() {
    let sandbox = Sandbox::new();
    let here = sandbox.path("");
    let models = sandbox.path("config/models");
    std::fs::create_dir_all(&models).unwrap();
    std::fs::write(models.join("mine.gguf"), b"GGUF").unwrap();

    let listed = sandbox.json(&here, &["models"]);
    assert_eq!(listed["provisional"], true);
    let catalog = listed["catalog"].as_array().unwrap();
    assert!(catalog.iter().all(|m| m["state"] == "available"));
    assert!(catalog.iter().any(|m| m["name"] == "granite-4.2-3b"));
    assert_eq!(listed["others"][0]["name"], "mine");

    // Not in a terminal, a download needs --yes, and nothing is fetched.
    let out = sandbox.run_in(&here, &["models", "download", "granite-4.2-3b"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--yes"));
    assert!(!sandbox.path("config/logs/network.log").exists());

    let out = sandbox.run_in(&here, &["models", "download", "nope", "--yes"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("no model called nope"));

    sandbox.ok(&here, &["models", "remove", "mine", "--yes"]);
    assert!(!models.join("mine.gguf").exists());
    let out = sandbox.run_in(&here, &["models", "remove", "mine", "--yes"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("isn't downloaded"));
}

#[test]
fn collection_new_creates_a_collection_to_route_into() {
    let sandbox = Sandbox::new();
    let root = sandbox.path("archive");
    sandbox.ok(sandbox.dir.path(), &["init", root.to_str().unwrap()]);
    let created = sandbox.json(
        &root,
        &[
            "collection",
            "new",
            "Finances",
            "--description",
            "Bank accounts and tax",
            "--tag",
            "tax=Income tax",
        ],
    );
    assert_eq!(created["path"], "Finances");
    assert!(root.join("Finances/_collection.md").is_file());
    let status = sandbox.json(&root, &["status"]);
    assert_eq!(status["collections"], 2, "Holding and Finances");

    let out = sandbox.run_in(&root, &["collection", "new", "Finances"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("already exists"));
    let out = sandbox.run_in(&root, &["collection", "new", "Travel", "--tag", "flights"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("name=meaning"));
}

#[test]
fn ingest_queues_items_for_a_worker_and_the_queue_can_be_changed() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    let a = sandbox.path("a note.txt");
    let b = sandbox.path("b note.txt");
    std::fs::write(&a, "first\n").unwrap();
    std::fs::write(&b, "second\n").unwrap();

    let planned = sandbox.json(&root, &["ingest", "--dry-run", a.to_str().unwrap()]);
    assert_eq!(planned["queued"].as_array().unwrap().len(), 1);
    assert_eq!(
        sandbox.json(&root, &["ingest", "--status"])["waiting"],
        serde_json::json!([]),
        "a dry run queues nothing"
    );

    let queued = sandbox.json(&root, &["ingest", a.to_str().unwrap(), b.to_str().unwrap()]);
    assert_eq!(queued["queued"].as_array().unwrap().len(), 2);
    assert_eq!(queued["worker_started"], false, "CARREL_WORKER is off here");
    let status = sandbox.json(&root, &["ingest", "--status"]);
    assert_eq!(status["running"], false);
    let waiting = status["waiting"].as_array().unwrap();
    assert_eq!(waiting.len(), 2);

    let second = waiting[1]["id"].as_str().unwrap().to_owned();
    sandbox.ok(&root, &["ingest", "--first", &second]);
    let first = &sandbox.json(&root, &["ingest", "--status"])["waiting"][0];
    assert_eq!(first["id"].as_str().unwrap(), second);
    sandbox.ok(&root, &["ingest", "--remove", &second]);
    assert_eq!(
        sandbox.json(&root, &["ingest", "--status"])["waiting"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let worked = sandbox.json(&root, &["worker"]);
    assert_eq!(worked["done"], 1);
    assert_eq!(
        sandbox.json(&root, &["ingest", "--status"])["waiting"],
        serde_json::json!([])
    );
    let proposals = sandbox.json(&root, &["review"])["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            e["source"]["path"]
                .as_str()
                .is_some_and(|p| p.ends_with("a note.txt"))
        })
        .count();
    assert_eq!(proposals, 1);

    let out = sandbox.run_in(&root, &["ingest"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("--status"));
}

#[test]
fn settings_are_listed_set_and_undone() {
    let sandbox = Sandbox::new();
    let root = sandbox.path("archive");
    sandbox.ok(sandbox.dir.path(), &["init", root.to_str().unwrap()]);
    let listed = sandbox.json(&root, &["settings"]);
    assert_eq!(listed["archive"]["ingest.originals"], "move");

    let set = sandbox.json(&root, &["settings", "set", "ingest.originals", "keep"]);
    assert_eq!(
        (set["from"].as_str(), set["to"].as_str()),
        (Some("move"), Some("keep"))
    );
    assert_eq!(
        sandbox.json(&root, &["settings"])["archive"]["ingest.originals"],
        "keep"
    );
    let wrong = sandbox.run_in(&root, &["settings", "set", "ingest.originals", "copy"]);
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("move, keep"));
    let commit = set["commit"].as_str().unwrap().to_owned();
    sandbox.ok(&root, &["undo", &commit[..10]]);
    assert_eq!(
        sandbox.json(&root, &["settings"])["archive"]["ingest.originals"],
        "move"
    );

    sandbox.ok(&root, &["settings", "set", "picker", "builtin"]);
    assert_eq!(
        sandbox.json(&root, &["settings"])["device"]["picker"],
        "builtin"
    );
    sandbox.ok(&root, &["settings", "set", "editor", "nano"]);
    assert_eq!(
        sandbox.json(&root, &["settings"])["device"]["editor"],
        "nano"
    );
    sandbox.ok(&root, &["settings", "set", "picker", ""]);
    assert_eq!(
        sandbox.json(&root, &["settings"])["device"]["picker"],
        serde_json::Value::Null
    );

    // A model must be one that's available; nothing is saved otherwise.
    let missing = sandbox.run_in(&root, &["settings", "set", "model", "llama/nope.gguf"]);
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("no text model llama/nope.gguf"),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
    let device = &sandbox.json(&root, &["settings"])["device"];
    assert_eq!(device["model"], serde_json::Value::Null);
    assert!(device["model_in_use"].is_string());
    sandbox.ok(&root, &["settings", "set", "model", "default"]);
}

#[test]
fn checkout_copies_an_item_out_and_locks_the_archive_copy() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    sandbox.ok(sandbox.dir.path(), &["init", root.to_str().unwrap()]);
    let desk = sandbox.path("desk");
    let out = sandbox.json(
        &root,
        &[
            "checkout",
            "Projects/Hardware/parts-list.csv",
            "--to",
            desk.to_str().unwrap(),
            "--due",
            "2026-12-01",
        ],
    );
    let working = PathBuf::from(out["working"].as_str().unwrap());
    assert_eq!(working, desk.join("parts-list.csv"));
    assert_eq!(
        std::fs::read(&working).unwrap(),
        std::fs::read(root.join("Projects/Hardware/parts-list.csv")).unwrap()
    );
    assert!(out["commit"].is_string());
    assert!(
        std::fs::write(root.join("Projects/Hardware/parts-list.csv"), "x").is_err(),
        "the archive copy is read-only"
    );
    let shown = sandbox.json(&root, &["show", "Projects/Hardware/parts-list.csv"]);
    assert_eq!(shown["fields"]["checked_out"]["due"], "2026-12-01");

    let listed = sandbox.json(&root, &["checkouts"]);
    assert_eq!(listed["checked_out"][0]["state"], "unchanged");
    std::fs::write(&working, "changed,on,the,desk\n").unwrap();
    assert_eq!(
        sandbox.json(&root, &["checkouts"])["checked_out"][0]["state"],
        "changed"
    );

    let again = sandbox.run_in(&root, &["checkout", "Projects/Hardware/parts-list.csv"]);
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("already checked out"));
    let log = sandbox.json(&root, &["log"]);
    assert_eq!(log[0]["operation"], "checkout");

    // Returning brings the working copy back, unlocked, with an approved history entry.
    let back = sandbox.json(
        &root,
        &["return", "Projects/Hardware/parts-list.csv", "--yes"],
    );
    assert_eq!(back["changed"], json!(["parts-list.csv"]));
    let archived = root.join("Projects/Hardware/parts-list.csv");
    assert_eq!(
        std::fs::read_to_string(&archived).unwrap(),
        "changed,on,the,desk\n"
    );
    std::fs::write(&archived, "writable again\n").unwrap();
    let shown = sandbox.json(&root, &["show", "Projects/Hardware/parts-list.csv"]);
    assert!(shown["fields"]["checked_out"].is_null());
    let card = std::fs::read_to_string(root.join("Projects/Hardware/parts-list.csv.md")).unwrap();
    assert!(card.contains("Returned: 1 changed file(s)."), "{card}");
    assert!(
        !card.contains("[!pending]"),
        "the entry is approved with the return"
    );
    assert_eq!(sandbox.json(&root, &["log"])[0]["operation"], "return");
    assert!(working.exists(), "the working copy is left for the owner");
    let again = sandbox.run_in(
        &root,
        &["return", "Projects/Hardware/parts-list.csv", "--yes"],
    );
    assert!(String::from_utf8_lossy(&again.stderr).contains("isn't checked out"));

    // Cancelling unlocks the archive copy as it was.
    sandbox.ok(
        &root,
        &[
            "checkout",
            "Projects/Hardware/parts-list.csv",
            "--to",
            desk.to_str().unwrap(),
        ],
    );
    sandbox.ok(
        &root,
        &[
            "return",
            "Projects/Hardware/parts-list.csv",
            "--discard",
            "--yes",
        ],
    );
    assert_eq!(
        std::fs::read_to_string(&archived).unwrap(),
        "writable again\n"
    );
    std::fs::write(&archived, "and again\n").unwrap();
}
