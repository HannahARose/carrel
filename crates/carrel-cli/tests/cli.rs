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
            .env("CARREL_TRASH_DIR", self.path("Trash"))
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
fn status_shows_what_was_changed_or_worked_on_lately() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    let recent = &sandbox.json(&root, &["status"])["recent"];
    assert_eq!(
        recent,
        &json!([{
            "kind": "work",
            "date": "2026-10-27",
            "item": "01a18701-0500-776a-9867-080924eb019c",
            "title": "oscillator-sim",
            "duration": "45m",
            "summary": "Set up the project and the basic Euler integration loop.",
        }]),
        "approved sessions only, not pending ones"
    );

    sandbox.ok(sandbox.dir.path(), &["init", root.to_str().unwrap()]);
    let recent = &sandbox.json(&root, &["status"])["recent"];
    assert_eq!(
        recent[0]["kind"], "change",
        "today's change first: {recent}"
    );
    assert_eq!(recent[1]["kind"], "work");
    let text = sandbox.ok(&root, &["status"]);
    assert!(text.contains("\nRecent:\n"), "{text}");
    assert!(
        text.contains("  2026-10-27        Worked on “oscillator-sim” for 45m: Set up the project"),
        "{text}"
    );
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
    assert_eq!(
        Path::new(by_item["root"].as_str().unwrap()),
        root.canonicalize().unwrap(),
        "paths are relative to the root it gives"
    );
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

    assert_eq!(by_item["history"], Value::Null, "no history started");

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
    assert_eq!(listed["provisional"], false, "the benchmark has chosen");
    let catalog = listed["catalog"].as_array().unwrap();
    assert!(
        catalog
            .iter()
            .all(|m| m["state"] == "available" || !m["comes_with"].is_null())
    );
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
fn show_lists_the_changes_that_touched_the_item() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    sandbox.ok(sandbox.dir.path(), &["init", root.to_str().unwrap()]);
    let desk = sandbox.path("desk");
    let item = "Projects/Hardware/parts-list.csv";
    sandbox.ok(&root, &["checkout", item, "--to", desk.to_str().unwrap()]);
    let log = sandbox.json(&root, &["log"]);
    let history = &sandbox.json(&root, &["show", item])["history"];
    assert_eq!(history.as_array().unwrap().len(), 1, "{history}");
    assert_eq!(history[0]["id"], log[0]["id"]);
    assert_eq!(history[0]["operation"], "checkout");
    let shown = sandbox.ok(&root, &["show", item]);
    assert!(
        shown.contains("History (1 change; undo one with `carrel undo CHANGE`):"),
        "{shown}"
    );
    let other = &sandbox.json(&root, &["show", "Projects/Hardware/lamp-build.md"])["history"];
    assert_eq!(other, &json!([]), "a change touches only its own items");
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

    let active = std::fs::read_to_string(root.join("Active/_index.md")).unwrap();
    assert!(
        active.contains("Desk lamp parts list"),
        "Active/ lists it: {active}"
    );
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

    returns_and_cancels(&sandbox, &root, &desk, &working);
}

/// The rest of the checkout test: returning the item, then a checkout cancelled.
fn returns_and_cancels(sandbox: &Sandbox, root: &Path, desk: &Path, working: &Path) {
    // Returning brings the working copy back, unlocked, with an approved history entry.
    let back = sandbox.json(
        root,
        &["return", "Projects/Hardware/parts-list.csv", "--yes"],
    );
    assert_eq!(back["changed"], json!(["parts-list.csv"]));
    let archived = root.join("Projects/Hardware/parts-list.csv");
    assert_eq!(
        std::fs::read_to_string(&archived).unwrap(),
        "changed,on,the,desk\n"
    );
    std::fs::write(&archived, "writable again\n").unwrap();
    let shown = sandbox.json(root, &["show", "Projects/Hardware/parts-list.csv"]);
    assert!(shown["fields"]["checked_out"].is_null());
    let card = std::fs::read_to_string(root.join("Projects/Hardware/parts-list.csv.md")).unwrap();
    assert!(card.contains("Returned: 1 changed file(s)."), "{card}");
    assert!(
        !card.contains("[!pending]"),
        "the entry is approved with the return"
    );
    assert_eq!(sandbox.json(root, &["log"])[0]["operation"], "return");
    let active = std::fs::read_to_string(root.join("Active/_index.md")).unwrap();
    assert!(
        !active.contains("Desk lamp parts list"),
        "it's off Active/: {active}"
    );
    let month = jiff::Zoned::now().strftime("%Y/%Y-%m").to_string();
    let journal = std::fs::read_to_string(root.join(format!("Journal/{month}.md"))).unwrap();
    assert!(
        journal.contains("Returned: 1 changed file(s)."),
        "the journal lists it: {journal}"
    );
    assert!(working.exists(), "the working copy is left for the owner");
    let again = sandbox.run_in(
        root,
        &["return", "Projects/Hardware/parts-list.csv", "--yes"],
    );
    assert!(String::from_utf8_lossy(&again.stderr).contains("isn't checked out"));

    // Cancelling unlocks the archive copy as it was.
    sandbox.ok(
        root,
        &[
            "checkout",
            "Projects/Hardware/parts-list.csv",
            "--to",
            desk.to_str().unwrap(),
        ],
    );
    sandbox.ok(
        root,
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

#[cfg(target_os = "macos")]
#[test]
fn a_checked_out_working_copy_is_tagged_in_finder_until_it_comes_back() {
    use carrel_core::finder::{CHECKED_OUT, add_tag, tags};

    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    sandbox.ok(sandbox.dir.path(), &["init", root.to_str().unwrap()]);
    let item = "Projects/Hardware/parts-list.csv";
    let archived = root.join(item);
    add_tag(&archived, "Red\n6").unwrap();
    let desk = sandbox.path("desk");
    let out = sandbox.json(&root, &["checkout", item, "--to", desk.to_str().unwrap()]);
    let working = PathBuf::from(out["working"].as_str().unwrap());
    assert_eq!(tags(&working).unwrap(), ["Red\n6", CHECKED_OUT]);
    assert_eq!(
        tags(&archived).unwrap(),
        ["Red\n6"],
        "only the working copy"
    );

    std::fs::write(&working, "changed,on,the,desk\n").unwrap();
    sandbox.ok(&root, &["return", item, "--yes"]);
    assert_eq!(tags(&working).unwrap(), ["Red\n6"], "the owner's tag stays");
    assert_eq!(
        tags(&archived).unwrap(),
        ["Red\n6"],
        "the tag doesn't come back into the archive"
    );

    let out = sandbox.json(&root, &["checkout", item, "--to", desk.to_str().unwrap()]);
    let working = PathBuf::from(out["working"].as_str().unwrap());
    assert!(tags(&working).unwrap().contains(&CHECKED_OUT.to_owned()));
    sandbox.ok(&root, &["return", item, "--discard", "--yes"]);
    assert_eq!(
        tags(&working).unwrap(),
        ["Red\n6"],
        "cancelling removes it too"
    );
}

#[test]
fn desk_items_get_a_card_beside_them_and_stay_where_they_are() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    sandbox.ok(sandbox.dir.path(), &["init", root.to_str().unwrap()]);
    let project = sandbox.path("Code/budget-app");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("Cargo.toml"), "[package]\nname = \"budget\"\n").unwrap();
    std::fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();

    let added = sandbox.json(
        &root,
        &[
            "desk",
            "add",
            project.to_str().unwrap(),
            "--title",
            "Budget app",
        ],
    );
    let card = sandbox.path("Code/budget-app.md");
    assert_eq!(
        PathBuf::from(added["card"].as_str().unwrap()),
        card.canonicalize().unwrap()
    );
    let text = std::fs::read_to_string(&card).unwrap();
    assert!(text.contains("title: Budget app"), "{text}");
    assert!(
        text.contains("kind: bundle"),
        "a Cargo project is a bundle: {text}"
    );
    assert!(
        !project.join("Budget app.md").exists(),
        "nothing is written inside it"
    );
    assert_eq!(
        sandbox.json(&root, &["desk"])["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let mirror = root.join("Active/On desk/Budget app.md");
    let mirrored = std::fs::read_to_string(&mirror).unwrap();
    std::fs::write(
        &mirror,
        mirrored.replace(
            "<!-- /carrel:mirror -->\n",
            "<!-- /carrel:mirror -->\nTry the new chart crate.\n",
        ),
    )
    .unwrap();
    let listed = sandbox.json(&root, &["desk"]);
    assert_eq!(listed["copied"].as_array().unwrap().len(), 1, "{listed}");
    assert!(
        std::fs::read_to_string(&card)
            .unwrap()
            .contains("Try the new chart crate."),
        "notes written in the mirror go to the card"
    );

    let again = sandbox.run_in(&root, &["desk", "add", project.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&again.stderr).contains("already on the desk"));
    let inside = sandbox.run_in(
        &root,
        &["desk", "add", root.join("Projects").to_str().unwrap()],
    );
    assert!(String::from_utf8_lossy(&inside.stderr).contains("inside the archive"));

    sandbox.ok(&root, &["desk", "remove", project.to_str().unwrap()]);
    assert!(card.exists(), "the card stays unless asked");
    assert!(!mirror.exists(), "the mirror goes with it");
    assert_eq!(sandbox.json(&root, &["desk"])["items"], json!([]));
    assert!(project.join("src/main.rs").exists());
}

#[test]
fn a_desk_items_card_is_drafted_and_archiving_it_keeps_the_card() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    sandbox.ok(sandbox.dir.path(), &["init", root.to_str().unwrap()]);
    let bin = sandbox.path("bin");
    std::fs::create_dir(&bin).unwrap();
    std::fs::copy(
        echo_provider(),
        bin.join(format!(
            "carrel-provider-echo-card{}",
            std::env::consts::EXE_SUFFIX
        )),
    )
    .unwrap();
    let with_model = |args: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_carrel"))
            .args(args)
            .arg("--json")
            .current_dir(&root)
            .env("PATH", &bin)
            .env("CARREL_CONFIG_DIR", sandbox.path("config"))
            .env("CARREL_WORKER", "off")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "carrel {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice::<Value>(&out.stdout).unwrap()
    };
    let notes = sandbox.path("Desktop/lamp notes.txt");
    std::fs::create_dir_all(notes.parent().unwrap()).unwrap();
    std::fs::write(&notes, "Wiring the desk lamp: a 5 V supply and two LEDs.\n").unwrap();

    let added = with_model(&["desk", "add", notes.to_str().unwrap()]);
    let card = PathBuf::from(added["card"].as_str().unwrap());
    let text = std::fs::read_to_string(&card).unwrap();
    assert!(text.contains("title: Echoed card"), "{text}");
    assert!(text.contains("- draft"), "{text}");
    assert!(text.contains("model: echo/1"), "{text}");
    // The owner renames it; archiving keeps their title, not a new draft's.
    std::fs::write(
        &card,
        text.replace("title: Echoed card", "title: Lamp wiring notes"),
    )
    .unwrap();

    let archived = sandbox.json(&root, &["desk", "archive", notes.to_str().unwrap()]);
    assert_eq!(archived["worker_started"], false, "{archived}");
    assert_eq!(sandbox.json(&root, &["desk"])["items"], json!([]));
    assert!(card.exists(), "the card waits beside it until it's filed");
    sandbox.ok(&root, &["worker"]);
    let proposals = sandbox.json(&root, &["review"]);
    let proposal = proposals["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["card"]["title"] == "Lamp wiring notes")
        .unwrap_or_else(|| panic!("no proposal with the desk card's title: {proposals}"));
    sandbox.ok(&root, &["review", "hold", proposal["id"].as_str().unwrap()]);
    let filed = std::fs::read_to_string(root.join("Holding/lamp notes.txt.md")).unwrap();
    assert!(filed.contains("title: Lamp wiring notes"), "{filed}");
    assert!(
        filed.contains(&format!("id: {}", added["id"].as_str().unwrap())),
        "the desk card's identifier is kept: {filed}"
    );
    assert!(!card.exists(), "the card moved into the archive with it");
}

#[test]
fn status_suggests_folders_to_put_on_the_desk() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    sandbox.ok(sandbox.dir.path(), &["init", root.to_str().unwrap()]);
    let home = sandbox.path("home");
    std::fs::create_dir_all(home.join("Code/budget-app/.git")).unwrap();
    std::fs::write(home.join("Code/budget-app/main.rs"), "fn main() {}\n").unwrap();
    std::fs::create_dir_all(home.join("Desktop")).unwrap();
    let status = |args: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_carrel"))
            .args(args)
            .current_dir(&root)
            .env("HOME", &home)
            .env("CARREL_CONFIG_DIR", sandbox.path("config"))
            .env("CARREL_WORKER", "off")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    };
    let text = status(&["status"]);
    assert!(text.contains("Looks like active work"), "{text}");
    assert!(text.contains("budget-app  (a Git repository"), "{text}");
    let json: Value = serde_json::from_str(&status(&["status", "--json"])).unwrap();
    assert_eq!(json["desk"]["to_track"][0]["git"], true, "{json}");
    assert_eq!(json["desk"]["to_archive"], json!([]));
}

#[test]
fn sessions_on_a_checked_out_item_become_pending_entries_to_approve() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    sandbox.ok(sandbox.dir.path(), &["init", root.to_str().unwrap()]);
    let desk = sandbox.path("desk");
    let item = "Projects/Hardware/parts-list.csv";
    sandbox.ok(&root, &["checkout", item, "--to", desk.to_str().unwrap()]);

    // Checked out three hours ago, edited two hours ago: one finished session.
    let card_path = root.join(format!("{item}.md"));
    let card = std::fs::read_to_string(&card_path).unwrap();
    let three_hours_ago = jiff::Timestamp::now() - jiff::SignedDuration::from_hours(3);
    let card = card
        .lines()
        .map(|l| {
            if l.trim_start().starts_with("since:") {
                format!("  since: \"{three_hours_ago}\"")
            } else {
                l.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&card_path, card).unwrap();
    let working = desk.join("parts-list.csv");
    std::fs::write(&working, "edited,at,the,desk\n").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&working)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_hours(2))
        .unwrap();

    let listed = sandbox.json(&root, &["sessions"]);
    assert!(listed["recorded"].is_string(), "{listed}");
    let ours = |listed: &Value| {
        listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["title"] == "Desk lamp parts list")
            .cloned()
    };
    let pending = &ours(&listed).expect("the item has pending entries")["pending"];
    assert_eq!(pending.as_array().unwrap().len(), 1);
    assert_eq!(pending[0]["lines"][0], "Changed 1 file(s): parts-list.csv");
    assert_eq!(
        sandbox.json(&root, &["sessions"])["recorded"],
        Value::Null,
        "a session is recorded once"
    );

    sandbox.ok(&root, &["sessions", "approve", item]);
    assert_eq!(ours(&sandbox.json(&root, &["sessions"])), None, "approved");
    let card = std::fs::read_to_string(&card_path).unwrap();
    assert!(card.contains("Changed 1 file(s): parts-list.csv"), "{card}");
    assert!(!card.contains("[!pending]"));
    assert_eq!(sandbox.json(&root, &["log"])[0]["operation"], "session");
}

#[cfg(unix)]
#[test]
fn sessions_edit_opens_an_entry_in_the_editor_and_keeps_it_pending() {
    use std::os::unix::fs::PermissionsExt as _;
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    sandbox.ok(sandbox.dir.path(), &["init", root.to_str().unwrap()]);
    let item = "Projects/Hardware/parts-list.csv";
    let card = root.join(format!("{item}.md"));
    let mut file =
        carrel_core::card_file::CardFile::parse(&std::fs::read_to_string(&card).unwrap()).unwrap();
    let mut log = file.history().unwrap();
    log.add(carrel_core::entries::HistoryEntry::pending(
        "2099-01-02",
        Some(70),
        vec!["Changed 1 file(s): parts-list.csv".into()],
    ));
    file.set_history(&log).unwrap();
    std::fs::write(&card, file.to_text().unwrap()).unwrap();
    let editor = |name: &str, text: &str| {
        let path = sandbox.path(name);
        std::fs::write(&path, format!("#!/bin/sh\nprintf '{text}' > \"$1\"\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_str().unwrap().to_owned()
    };
    let run = |editor: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_carrel"))
            .args(["sessions", "edit", item, "1"])
            .current_dir(&root)
            .env("CARREL_CONFIG_DIR", sandbox.path("config"))
            .env_remove("VISUAL")
            .env("EDITOR", editor)
            .output()
            .unwrap()
    };

    let out = run(&editor(
        "rewrite.sh",
        "Added the new switch.\\nMy notes: order spares.\\n",
    ));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "Edit a history entry of “Desk lamp parts list”\n"
    );
    let text = std::fs::read_to_string(&card).unwrap();
    assert!(
        text.contains(
            "> [!pending] 2099-01-02 · 1h 10m · auto-generated\n\
             > Added the new switch.\n\
             > My notes: order spares.\n"
        ),
        "{text}"
    );
    let log = sandbox.json(&root, &["log"]);
    assert_eq!(log[0]["operation"], "session");

    // Nothing saved, or text that would break the section, changes nothing.
    assert_eq!(
        String::from_utf8_lossy(&run("true").stdout),
        "Nothing changed\n"
    );
    let out = run(&editor(
        "breaks.sh",
        "Fine.\\n### 2099-01-03\\nNot an entry.\\n",
    ));
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("would break the history section"),
        "{stderr}"
    );
    assert!(stderr.contains("your text is in"), "{stderr}");
    assert_eq!(std::fs::read_to_string(&card).unwrap(), text);
    let out = run("false");
    assert!(!out.status.success(), "a failed editor is an error");
    let missing = sandbox.run_in(&root, &["sessions", "edit", item, "2"]);
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("there's no pending entry 2"),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );

    // Approved, it's in the Journal as edited.
    sandbox.ok(&root, &["sessions", "approve", item, "1"]);
    let journal = std::fs::read_to_string(root.join("Journal/2099/2099-01.md")).unwrap();
    assert!(journal.contains("Added the new switch."), "{journal}");
}

#[test]
fn entries_approved_by_hand_are_noticed_and_kept_as_written() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    sandbox.ok(sandbox.dir.path(), &["init", root.to_str().unwrap()]);
    let item = "Projects/Hardware/parts-list.csv";
    let card = root.join(format!("{item}.md"));
    let mut file =
        carrel_core::card_file::CardFile::parse(&std::fs::read_to_string(&card).unwrap()).unwrap();
    let mut log = file.history().unwrap();
    for (date, text) in [
        ("2099-02-03", "Soldered the new switch."),
        ("2099-02-04", "Tidied the list."),
    ] {
        log.add(carrel_core::entries::HistoryEntry::pending(
            date,
            Some(70),
            vec![text.into()],
        ));
    }
    file.set_history(&log).unwrap();
    std::fs::write(&card, file.to_text().unwrap()).unwrap();
    // Dropping one records the other, pending, in history.
    sandbox.ok(&root, &["sessions", "drop", item, "1"]);

    // Changing a pending entry's text by hand isn't approving it.
    let text = std::fs::read_to_string(&card).unwrap();
    std::fs::write(&card, text.replace("> Soldered the", "> Soldered  the")).unwrap();
    let listed = sandbox.json(&root, &["sessions"]);
    assert_eq!(listed["approved_by_hand"], json!([]), "{listed}");
    assert_eq!(listed["recorded"], Value::Null, "nothing to record");
    std::fs::write(&card, &text).unwrap();

    // In Obsidian, the owner turns the callout into a normal entry, and adds a line.
    let pending = "> [!pending] 2099-02-03 · 1h 10m · auto-generated\n> Soldered the new switch.\n";
    let text = std::fs::read_to_string(&card).unwrap();
    assert!(text.contains(pending), "{text}");
    let approved =
        "### 2099-02-03 · 1h 10m · auto-generated\nSoldered the new switch.\nIt works!\n";
    let text = text.replace(pending, approved);
    std::fs::write(&card, &text).unwrap();

    let listed = sandbox.json(&root, &["sessions"]);
    assert_eq!(
        listed["approved_by_hand"],
        json!([{ "item": listed["approved_by_hand"][0]["item"], "title": "Desk lamp parts list", "entries": 1 }]),
        "{listed}"
    );
    assert!(listed["recorded"].is_string(), "{listed}");
    assert_eq!(
        std::fs::read_to_string(&card).unwrap(),
        text,
        "the owner's text is kept exactly"
    );
    let journal = std::fs::read_to_string(root.join("Journal/2099/2099-02.md")).unwrap();
    assert!(journal.contains("Soldered the new switch."), "{journal}");
    let log = sandbox.json(&root, &["log"]);
    assert_eq!(log[0]["operation"], "session");
    assert_eq!(log[0]["summary"], "Note 1 history entry approved by hand");

    let again = sandbox.json(&root, &["sessions"]);
    assert_eq!(again["approved_by_hand"], json!([]), "noticed once");
    assert_eq!(again["recorded"], Value::Null);
    assert!(
        again["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["title"] != "Desk lamp parts list"),
        "nothing pending on it: {again}"
    );
}

#[test]
fn a_desk_items_pending_entries_are_decided_by_its_path() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    sandbox.ok(sandbox.dir.path(), &["init", root.to_str().unwrap()]);
    let project = sandbox.path("Code/thesis");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("draft.md"), "# Draft\n").unwrap();
    sandbox.ok(&root, &["desk", "add", project.to_str().unwrap()]);
    let card = sandbox.path("Code/thesis.md");
    let mut file =
        carrel_core::card_file::CardFile::parse(&std::fs::read_to_string(&card).unwrap()).unwrap();
    let mut log = file.history().unwrap();
    log.add(carrel_core::entries::HistoryEntry::pending(
        "2026-10-30",
        Some(70),
        vec!["Changed 1 file(s): draft.md".into()],
    ));
    file.set_history(&log).unwrap();
    std::fs::write(&card, file.to_text().unwrap()).unwrap();

    let dropped = sandbox.json(&root, &["sessions", "drop", project.to_str().unwrap(), "1"]);
    assert_eq!(dropped["entries"], 1, "{dropped}");
    assert!(!std::fs::read_to_string(&card).unwrap().contains("draft.md"));
    let unknown = sandbox.run_in(&root, &["sessions", "drop", "Code/elsewhere"]);
    assert!(
        String::from_utf8_lossy(&unknown.stderr).contains("isn't an item"),
        "{}",
        String::from_utf8_lossy(&unknown.stderr)
    );
}

#[test]
fn review_correct_shows_the_change_and_saves_only_when_told() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    let lines = "created: 2019-03-31\nsensitivity: public\ncollection: Reading";

    let shown = sandbox.ok(&root, &["review", "correct", "00000001", lines]);
    assert!(
        shown.contains("created      (none)  →  2019-03-31  (yours)"),
        "{shown}"
    );
    assert!(
        shown.contains("collection   Projects/Hardware  →  Reading"),
        "{shown}"
    );
    assert!(
        shown.contains("Not saved"),
        "no terminal to ask on: {shown}"
    );
    let unchanged = sandbox.json(&root, &["review", "show", "00000001"]);
    assert_eq!(unchanged["destination"]["path"], "Projects/Hardware");

    let saved = sandbox.json(&root, &["review", "correct", "00000001", lines, "--yes"]);
    assert_eq!(saved["saved"], true);
    let after = sandbox.json(&root, &["review", "show", "00000001"]);
    assert_eq!(after["destination"]["path"], "Reading");
    assert_eq!(after["card"]["dates"]["created"], "2019-03-31");
    assert_eq!(after["card"]["sensitivity"], "public");
    assert_eq!(
        after["card"]["generated"]["corrections"][0]["said"], lines,
        "the owner's words are kept with the card"
    );

    let bad = sandbox.run_in(
        &root,
        &["review", "correct", "00000001", "created: soon", "--yes"],
    );
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("isn't a date"));
}

#[test]
fn correct_refiles_a_filed_item_in_one_change_that_undo_reverses() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    let init = std::process::Command::new(env!("CARGO_BIN_EXE_carrel"))
        .args(["init", root.to_str().unwrap()])
        .env("CARREL_CONFIG_DIR", sandbox.path("config"))
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let lines = "created: 2019-03-31\ncollection: Reading";
    let saved = sandbox.json(
        &root,
        &[
            "correct",
            "Projects/Hardware/parts-list.csv",
            lines,
            "--yes",
        ],
    );
    assert_eq!(saved["saved"], true, "{saved}");
    assert_eq!(saved["card"], "Reading/parts-list.csv.md");
    assert!(root.join("Reading/parts-list.csv").exists());
    assert!(!root.join("Projects/Hardware/parts-list.csv").exists());
    let card = std::fs::read_to_string(root.join("Reading/parts-list.csv.md")).unwrap();
    assert!(card.contains("created: 2019-03-31"), "{card}");
    let log = sandbox.json(&root, &["log"]);
    assert!(
        log[0]["summary"]
            .as_str()
            .unwrap()
            .contains("Owner: created: 2019-03-31"),
        "{log}"
    );

    let change = log[0]["id"].as_str().unwrap().to_owned();
    sandbox.ok(&root, &["undo", &change[..8]]);
    assert!(
        root.join("Projects/Hardware/parts-list.csv").exists(),
        "undo moved it back"
    );
    assert!(!root.join("Reading/parts-list.csv.md").exists());
}

#[test]
fn types_nothing_reads_are_logged_without_names_and_listed() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    let downloads = sandbox.path("Downloads");
    std::fs::create_dir(&downloads).unwrap();
    std::fs::write(
        downloads.join("Secret Plans.F3D"),
        [0u8, 159, 146, 150, 255, 1, 2],
    )
    .unwrap();
    let downloads_arg = downloads.to_str().unwrap();

    let report = sandbox.ok(&root, &["ingest", "--wait", "--each", downloads_arg]);
    assert!(
        report
            .contains("1 item of a type nothing on this device reads (.f3d); see `carrel formats`"),
        "{report}"
    );
    let log = std::fs::read_to_string(sandbox.path("config/unreadable.json")).unwrap();
    assert!(log.contains("\"extension\": \"f3d\""), "{log}");
    assert!(log.contains("\"why\": \"no-reader\""), "{log}");
    assert!(!log.contains("Secret"), "no file names: {log}");

    let formats = sandbox.json(&root, &["formats"]);
    assert_eq!(formats["unreadable"][0]["count"], 1);
    assert!(formats["reads"]["text/plain"].is_array(), "{formats}");
    let text = sandbox.ok(&root, &["formats"]);
    assert!(
        text.contains("Couldn't read, most frequent first:"),
        "{text}"
    );
    assert!(text.contains(".f3d"), "{text}");
}

/// Sets `path`'s modification time to `hours` ago.
fn backdate(path: &Path, hours: u64) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_hours(hours))
        .unwrap();
}

/// Makes the next command that changes the archive look for sessions, as if the last look
/// were long ago.
fn forget_last_look(sandbox: &Sandbox) {
    for entry in std::fs::read_dir(sandbox.path("config/sessions")).unwrap() {
        let path = entry.unwrap().path();
        let mut state: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        state.as_object_mut().unwrap().remove("looked");
        std::fs::write(&path, state.to_string()).unwrap();
    }
}

/// Rewrites a YAML or JSON line that starts with `key` (after spaces) to `replacement`.
fn rewrite_line(path: &Path, key: &str, replacement: &str) {
    let text = std::fs::read_to_string(path).unwrap();
    let text = text
        .lines()
        .map(|l| {
            if l.trim_start().starts_with(key) {
                replacement.to_owned()
            } else {
                l.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(path, text).unwrap();
}

#[test]
fn commands_that_change_the_archive_record_desk_sessions_every_few_minutes() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    sandbox.ok(sandbox.dir.path(), &["init", root.to_str().unwrap()]);
    let project = sandbox.path("Code/budget-app");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("notes.md"), "plans\n").unwrap();
    sandbox.ok(&root, &["desk", "add", project.to_str().unwrap()]);

    // Put on the desk three hours ago, worked on two hours ago.
    let three_hours_ago = jiff::Timestamp::now() - jiff::SignedDuration::from_hours(3);
    // This device's desk: the sample archive has another device's, and folders come in no
    // particular order.
    let desk = std::fs::read_dir(root.join("_System/devices"))
        .unwrap()
        .map(|d| d.unwrap().path().join("desk.json"))
        .find(|d| std::fs::read_to_string(d).is_ok_and(|t| t.contains("budget-app")))
        .unwrap();
    rewrite_line(
        &desk,
        "\"added\"",
        &format!("      \"added\": \"{three_hours_ago}\""),
    );
    backdate(&project.join("notes.md"), 2);

    // `desk add` just looked, so the next command doesn't look again yet.
    let quiet = sandbox.run_in(&root, &["collection", "new", "Projects/Garden"]);
    assert!(quiet.status.success());
    let card = sandbox.path("Code/budget-app.md");
    assert!(
        !std::fs::read_to_string(&card)
            .unwrap()
            .contains("[!pending]")
    );

    forget_last_look(&sandbox);
    let found = sandbox.run_in(&root, &["collection", "new", "Projects/Orchard"]);
    assert!(found.status.success());
    assert!(
        String::from_utf8_lossy(&found.stderr).contains("Found 1 new session(s) on “budget-app”"),
        "{}",
        String::from_utf8_lossy(&found.stderr)
    );
    let text = std::fs::read_to_string(&card).unwrap();
    assert!(text.contains("[!pending]"), "{text}");
    assert!(text.contains("Changed 1 file(s): notes.md"), "{text}");
    assert!(
        !String::from_utf8_lossy(&sandbox.run_in(&root, &["status"]).stderr).contains("Found"),
        "commands that only read don't look"
    );

    let listed = sandbox.json(&root, &["sessions"]);
    let ours = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["title"] == "budget-app")
        .expect("the desk item has pending entries");
    assert_eq!(ours["desk"], true, "{listed}");
    assert_eq!(ours["pending"].as_array().unwrap().len(), 1);
    sandbox.ok(&root, &["sessions", "approve", project.to_str().unwrap()]);
    assert!(
        !std::fs::read_to_string(&card)
            .unwrap()
            .contains("[!pending]")
    );
}

#[test]
fn session_snapshots_are_taken_and_can_be_forgotten_on_return() {
    if std::process::Command::new("kopia")
        .arg("--version")
        .output()
        .is_err()
    {
        assert!(
            std::env::var_os("CARREL_REQUIRE_KOPIA").is_none(),
            "Kopia is required here but isn't installed"
        );
        eprintln!("skipped: kopia isn't installed");
        return;
    }
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    sandbox.ok(sandbox.dir.path(), &["init", root.to_str().unwrap()]);
    let password = format!("test-{}", std::process::id());
    let run = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_carrel"))
            .args(args)
            .current_dir(&root)
            .env("CARREL_CONFIG_DIR", sandbox.path("config"))
            .env("CARREL_WORKER", "off")
            .env("KOPIA_PASSWORD", &password)
            .env("KOPIA_CHECK_FOR_UPDATES", "false")
            .output()
            .unwrap()
    };
    let repository = sandbox.path("backups");
    let setup = run(&["backup", "setup", repository.to_str().unwrap()]);
    assert!(
        setup.status.success(),
        "{}",
        String::from_utf8_lossy(&setup.stderr)
    );
    let desk = sandbox.path("desk");
    let item = "Projects/Hardware/parts-list.csv";
    assert!(
        run(&["checkout", item, "--to", desk.to_str().unwrap()])
            .status
            .success()
    );
    let three_hours_ago = jiff::Timestamp::now() - jiff::SignedDuration::from_hours(3);
    rewrite_line(
        &root.join(format!("{item}.md")),
        "since:",
        &format!("  since: \"{three_hours_ago}\""),
    );
    let working = desk.join("parts-list.csv");
    std::fs::write(&working, "edited,at,the,desk\n").unwrap();
    backdate(&working, 2);

    let listed = run(&["sessions", "--json"]);
    let listed: Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(listed["sessions"], 1, "{listed}");
    assert!(listed["snapshots"][0]["snapshot"].is_string(), "{listed}");

    let returned = run(&["return", item, "--yes", "--trim-snapshots", "--json"]);
    assert!(
        String::from_utf8_lossy(&returned.stderr).contains("1 session snapshot(s)"),
        "the proposal says so: {}",
        String::from_utf8_lossy(&returned.stderr)
    );
    let returned: Value = serde_json::from_slice(&returned.stdout).unwrap();
    let trimmed = &returned["session_snapshots"];
    assert!(trimmed["returned"].is_string(), "{returned}");
    assert_eq!(trimmed["forgotten"], 1, "{returned}");
    assert_eq!(trimmed["kept"], 0);
}

/// Writes a zip of `(name, contents)` files at `path`.
fn write_zip(path: &Path, files: &[(&str, &[u8])]) {
    use std::io::Write as _;
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, bytes) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn compressed_files_are_drafted_from_what_is_inside() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    let downloads = sandbox.path("Downloads");
    std::fs::create_dir(&downloads).unwrap();
    write_zip(
        &downloads.join("lamp-firmware-main.zip"),
        &[
            (
                "lamp-firmware-main/README.md",
                b"Firmware for the desk lamp's dimmer.\n",
            ),
            (
                "lamp-firmware-main/Cargo.toml",
                b"[package]\nname = \"lamp-firmware\"\n",
            ),
            ("lamp-firmware-main/src/main.rs", b"fn main() {}\n"),
            ("__MACOSX/lamp-firmware-main/._README.md", b"junk"),
        ],
    );
    std::fs::write(
        downloads.join("Holiday photos.7z"),
        b"7z\xbc\xaf\x27\x1c\0\x04",
    )
    .unwrap();
    let downloads_arg = downloads.to_str().unwrap();

    let report = sandbox.json(&root, &["ingest", "--wait", "--each", downloads_arg]);
    let items = report["items"].as_array().unwrap();
    let firmware = items
        .iter()
        .find(|i| i["path"].as_str().unwrap().ends_with("main.zip"))
        .unwrap();
    assert_eq!(firmware["kind"], "file");
    assert_eq!(firmware["compressed"]["format"], "zip");
    assert_eq!(
        firmware["compressed"]["holds"],
        "a folder, lamp-firmware-main, with 3 files"
    );
    assert_eq!(firmware["text"]["method"], "listing");
    assert!(firmware["text"]["characters"].as_u64().unwrap() > 50);
    assert!(
        downloads.join("lamp-firmware-main.zip").exists(),
        "nothing is expanded before approval"
    );
    assert!(
        !downloads.join("lamp-firmware-main").exists(),
        "nothing is expanded before approval"
    );

    let seven = items
        .iter()
        .find(|i| i["path"].as_str().unwrap().ends_with("photos.7z"))
        .unwrap();
    assert_eq!(seven["compressed"]["format"], "7z");
    if which("7z").is_none() && which("7zz").is_none() && which("unar").is_none() {
        assert_eq!(seven["text"]["method"], "none");
        let log = std::fs::read_to_string(sandbox.path("config/unreadable.json")).unwrap();
        assert!(log.contains("\"extension\": \"7z\""), "{log}");
        assert!(!log.contains("Holiday"), "no file names: {log}");
    }
}

/// Where `program` is on `PATH`, if it's there.
fn which(program: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(program))
        .find(|path| path.is_file())
}

#[cfg(unix)]
#[test]
fn edit_opens_a_filed_card_keeps_its_body_and_undo_reverses_it() {
    use std::os::unix::fs::PermissionsExt as _;
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    let init = std::process::Command::new(env!("CARGO_BIN_EXE_carrel"))
        .args(["init", root.to_str().unwrap()])
        .env("CARREL_CONFIG_DIR", sandbox.path("config"))
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let card = root.join("Projects/Hardware/parts-list.csv.md");
    let before = std::fs::read_to_string(&card).unwrap();
    let body = before.splitn(3, "---").nth(2).unwrap().to_owned();
    let editor = sandbox.path("editor.sh");
    std::fs::write(
        &editor,
        "#!/bin/sh\nawk '{sub(/^title: .*/, \"title: Parts for the lamp\"); sub(/^sensitivity: .*/, \"sensitivity: public\")}1' \"$1\" > \"$1.new\" && mv \"$1.new\" \"$1\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_carrel"))
        .args(["edit", "Projects/Hardware/parts-list.csv", "--json"])
        .current_dir(&root)
        .env("CARREL_CONFIG_DIR", sandbox.path("config"))
        .env("CARREL_WORKER", "off")
        .env_remove("VISUAL")
        .env("EDITOR", &editor)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        report["changed"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("title"))
    );
    let after = std::fs::read_to_string(&card).unwrap();
    assert!(after.contains("title: Parts for the lamp"), "{after}");
    assert!(
        after.contains("sensitivity: public"),
        "the owner may lower it"
    );
    assert!(after.ends_with(&body), "the body is kept exactly");

    let change = sandbox.json(&root, &["log"])[0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    sandbox.ok(&root, &["undo", &change[..8]]);
    assert_eq!(
        std::fs::read_to_string(&card).unwrap(),
        before,
        "undo restores the card"
    );
}

/// Files a text file in `collection` with a card beside it carrying `fields`.
fn file_with_card(root: &Path, collection: &str, name: &str, contents: &str, fields: &str) {
    let path = root.join(collection).join(name);
    std::fs::write(&path, contents).unwrap();
    let checksum = carrel_core::card::Checksum::of_file(&path).unwrap();
    let card = format!(
        "---\ncarrel: card\nformat: \"1.0\"\nid: {}\nkind: file\nitem: \"{name}\"\n{fields}\
         checksum: \"{}\"\nsize: {}\n---\n\n<!-- carrel:history -->\n## History\n\
         <!-- /carrel:history -->\n",
        carrel_core::card::Id::new_v7(),
        checksum.0,
        contents.len()
    );
    std::fs::write(root.join(collection).join(format!("{name}.md")), card).unwrap();
}

/// What search says when the sample archive, which has no meaning index, is searched.
const WORDS_ONLY: &str = "There's no meaning index for sample/embedding-model-v1 yet, so this \
                          was searched by words only; build it with `carrel index rebuild`.\n";

#[test]
fn search_finds_items_by_their_words_with_filters_and_reasons() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    file_with_card(
        &root,
        "Holding",
        "invoice.txt",
        "Please pay invoice 2057-4038 by Friday.",
        "title: \"Furniture shop bill\"\nsummary: \"A bill for office furniture.\"\n\
         tags: [receipts]\ndates:\n  document: \"2025-03-02\"\nsensitivity: sensitive\n",
    );
    file_with_card(
        &root,
        "Holding",
        "statement.txt",
        "Balance owed on invoice 2057-4038.",
        "title: \"Bank statement\"\nsensitivity: restricted\n",
    );

    let found = sandbox.json(&root, &["search", "invoice", "2057-4038"]);
    assert_eq!(
        found["total"], 1,
        "the restricted statement's text isn't searched: {found}"
    );
    assert_eq!(
        Path::new(found["root"].as_str().unwrap()),
        root.canonicalize().unwrap()
    );
    let first = &found["results"][0];
    assert_eq!(first["found"]["type"], "item");
    assert_eq!(first["found"]["title"], "Furniture shop bill");
    assert_eq!(first["found"]["collection"], "Holding");
    assert_eq!(first["found"]["sensitivity"], "sensitive");
    assert_eq!(first["found"]["date"], "2025-03-02");
    assert_eq!(first["why"], json!(["text"]));
    assert_eq!(first["passage_from"], "text");
    assert_eq!(found["meaning"]["state"], "off");
    let text = first["passage"]["text"].as_str().unwrap();
    let range = &first["passage"]["highlights"][1];
    let (start, end) = (
        usize::try_from(range["start"].as_u64().unwrap()).unwrap(),
        usize::try_from(range["end"].as_u64().unwrap()).unwrap(),
    );
    assert_eq!(&text[start..end], "2057-4038");
    assert!(
        root.join("_System/cache.nosync/text").is_dir(),
        "texts are kept in the cache"
    );

    let report = sandbox.ok(&root, &["search", "invoice 2057-4038"]);
    assert_eq!(
        report,
        format!(
            "1 result for “invoice 2057-4038”:\n\nFurniture shop bill  [sensitive]\n  \
         Holding · 2025-03-02 · file\n  Please pay **invoice** **2057-4038** by Friday.\n  \
         matched: text\n\n{WORDS_ONLY}"
        )
    );
    let report = sandbox.ok(&root, &["search", "bank", "statement"]);
    assert!(report.contains("Bank statement  [restricted]"), "{report}");

    let filtered = sandbox.json(&root, &["search", "tag:receipts", "before:2026"]);
    assert_eq!(
        filtered["results"][0]["found"]["title"],
        "Furniture shop bill"
    );
    assert_eq!(filtered["results"][0]["why"], json!(["filters"]));
    let checked_out = sandbox.json(&root, &["search", "checked-out"]);
    assert_eq!(checked_out["results"][0]["found"]["checked_out"], true);
    let limited = sandbox.json(&root, &["search", "oscillators", "--limit", "1"]);
    assert_eq!(limited["results"].as_array().unwrap().len(), 1);
    assert!(limited["total"].as_u64().unwrap() > 1, "{limited}");
    let glossary = sandbox.json(&root, &["search", "implements"]);
    assert_eq!(glossary["results"][0]["found"]["type"], "glossary");
    assert_eq!(
        sandbox.ok(&root, &["search", "zebra"]),
        format!("Nothing found for “zebra”.\n\n{WORDS_ONLY}")
    );

    let bad = sandbox.run_in(&root, &["search", "kind:box"]);
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("kind:box isn't a kind"));
    assert_eq!(sandbox.run_in(&root, &["search"]).status.code(), Some(2));
}

#[test]
fn init_chooses_the_archives_embedding_model() {
    let sandbox = Sandbox::new();
    let settings = |root: &Path| -> Value {
        serde_json::from_str(&std::fs::read_to_string(root.join("_System/settings.json")).unwrap())
            .unwrap()
    };
    let text = sandbox.ok(sandbox.dir.path(), &["init", "Notes"]);
    assert!(
        text.contains("searches it with embeddinggemma-300m"),
        "{text}"
    );
    assert_eq!(
        settings(&sandbox.path("Notes"))["embedding"]["model"],
        "embeddinggemma-300m"
    );

    let created = sandbox.json(
        sandbox.dir.path(),
        &[
            "init",
            "Papers",
            "--embedding-model",
            "qwen3-embedding-0.6b",
        ],
    );
    assert_eq!(created["embedding_model"], "qwen3-embedding-0.6b");
    assert_eq!(
        settings(&sandbox.path("Papers"))["embedding"]["model"],
        "qwen3-embedding-0.6b"
    );

    let out = sandbox.run_in(
        sandbox.dir.path(),
        &["init", "Other", "--embedding-model", "word2vec"],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no embedding model word2vec"));
    assert!(!sandbox.path("Other").exists(), "nothing is created");
}

/// Sets the sample archive's embedding model in its settings, as another program might, or
/// removes it.
fn set_embedding_model(root: &Path, model: Option<&str>) {
    let path = root.join("_System/settings.json");
    let mut settings: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    match model {
        Some(model) => settings["embedding"] = json!({ "model": model }),
        None => {
            settings.as_object_mut().unwrap().remove("embedding");
        }
    }
    std::fs::write(&path, serde_json::to_string_pretty(&settings).unwrap()).unwrap();
}

#[test]
fn an_archive_without_an_embedding_model_asks_for_one_on_first_use() {
    let sandbox = Sandbox::new();
    let root = sandbox.dir.path().join("archive");
    sandbox.ok(sandbox.dir.path(), &["init", root.to_str().unwrap()]);
    set_embedding_model(&root, None);

    // Not in a terminal, there's no one to ask, so it says how to choose.
    let out = sandbox.run_in(&root, &["models", "embed", "a lease"]);
    assert!(!out.status.success());
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(
        error.contains("carrel settings set embedding.model NAME"),
        "{error}"
    );
    assert!(error.contains("embeddinggemma-300m"), "{error}");

    let listed = sandbox.json(&root, &["settings"]);
    assert_eq!(listed["archive"]["embedding.model"], "none");
    let set = sandbox.json(
        &root,
        &["settings", "set", "embedding.model", "qwen3-embedding-0.6b"],
    );
    assert_eq!(set["to"], "qwen3-embedding-0.6b");
    assert!(set["commit"].is_string(), "recorded in history");

    // Once chosen, it stays: switching models means re-embedding the archive.
    let out = sandbox.run_in(
        &root,
        &["settings", "set", "embedding.model", "embeddinggemma-300m"],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("re-embeds"));
}

#[test]
fn texts_are_embedded_with_the_archives_model_and_no_other() {
    let sandbox = Sandbox::new();
    let bin = sandbox.path("bin");
    std::fs::create_dir(&bin).unwrap();
    let suffix = std::env::consts::EXE_SUFFIX;
    std::fs::copy(
        echo_provider(),
        bin.join(format!("carrel-provider-echo{suffix}")),
    )
    .unwrap();
    let root = sandbox.sample();
    let embed = |args: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_carrel"))
            .args(["models", "embed"])
            .args(args)
            .current_dir(&root)
            .env("PATH", &bin)
            .env("CARREL_CONFIG_DIR", sandbox.path("config"))
            .env("CARREL_WORKER", "off")
            .output()
            .unwrap()
    };

    set_embedding_model(&root, Some("echo-embed"));
    let out = embed(&["a lease", "a receipt", "--query", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["model"], "echo-embed");
    assert_eq!(report["provider"], "echo");
    assert_eq!(report["purpose"], "query");
    assert_eq!(report["dimensions"], 3);
    let vectors = report["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 2);
    assert_eq!(vectors[0].as_array().unwrap().len(), 3);

    // The echo provider has an embedding model, but not the archive's.
    set_embedding_model(&root, Some("embeddinggemma-300m"));
    let out = embed(&["a lease"]);
    assert!(!out.status.success());
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(
        error.contains("embeddinggemma-300m") && error.contains("carrel models download"),
        "{error}"
    );
}

#[test]
fn embedding_models_are_listed_apart_and_built_in_ones_are_never_downloaded() {
    let sandbox = Sandbox::new();
    let here = sandbox.path("");
    let listed = sandbox.json(&here, &["models"]);
    let catalog = listed["catalog"].as_array().unwrap();
    let gemma = catalog
        .iter()
        .find(|m| m["name"] == "embeddinggemma-300m")
        .unwrap();
    assert_eq!(gemma["embedding"]["dimensions"], 768);
    assert!(
        catalog
            .iter()
            .any(|m| m["name"] == "qwen3-embedding-0.6b" && m["embedding"]["pooling"] == "last")
    );
    assert_eq!(gemma["pinned"], true);
    let apple = catalog
        .iter()
        .find(|m| m["name"] == "apple-nl-sentence-en")
        .unwrap();
    assert_eq!(apple["state"], "built in");
    assert_eq!(apple["comes_with"], "carrel-provider-apple");
    let text = sandbox.ok(&here, &["models"]);
    assert!(text.contains("For search by meaning"), "{text}");
    assert!(text.contains("nothing to download"), "{text}");

    let out = sandbox.run_in(
        &here,
        &["models", "download", "apple-nl-sentence-en", "--yes"],
    );
    assert!(!out.status.success());
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(error.contains("nothing to download"), "{error}");
    assert!(!sandbox.path("config/logs/network.log").exists());
}

#[test]
fn approved_zips_are_expanded_unless_kept_compressed() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    sandbox.ok(&root, &["init", root.to_str().unwrap()]);
    let downloads = sandbox.path("Downloads");
    std::fs::create_dir(&downloads).unwrap();
    let zipped = downloads.join("lamp-firmware-main.zip");
    write_zip(
        &zipped,
        &[
            ("lamp-firmware-main/README.md", b"Dimmer firmware.\n"),
            ("lamp-firmware-main/Cargo.toml", b"[package]\n"),
            ("lamp-firmware-main/src/main.rs", b"fn main() {}\n"),
        ],
    );
    let installer = downloads.join("Lamp Utility.zip");
    write_zip(&installer, &[("Lamp Utility/Setup.exe", b"MZ")]);
    let gzipped = downloads.join("notes.zip");
    write_zip(&gzipped, &[("a.txt", b"a"), ("b.txt", b"b")]);
    sandbox.ok(
        &root,
        &["ingest", "--wait", "--each", downloads.to_str().unwrap()],
    );
    let entries = sandbox.json(&root, &["review"])["entries"]
        .as_array()
        .unwrap()
        .clone();
    let id_of = |name: &str| {
        entries
            .iter()
            .find(|e| e["source"]["path"].as_str().unwrap().ends_with(name))
            .map(|e| e["id"].as_str().unwrap().to_owned())
            .unwrap()
    };
    let (firmware, utility, notes) = (id_of("main.zip"), id_of("Utility.zip"), id_of("notes.zip"));

    let shown = sandbox.ok(&root, &["review", "show", &firmware]);
    assert!(
        shown.contains("compressed   will be expanded: 3 files"),
        "{shown}"
    );
    assert!(shown.contains("/lamp-firmware-main"), "{shown}");
    let shown = sandbox.ok(&root, &["review", "show", &utility]);
    assert!(
        shown.contains("kept compressed: it holds an installer (Setup.exe)"),
        "{shown}"
    );
    let kept = sandbox.ok(&root, &["review", "keep-compressed", &notes]);
    assert!(
        kept.contains("kept compressed: the owner chose to keep it"),
        "{kept}"
    );
    let refused = sandbox.run_in(&root, &["review", "expand", &utility]);
    assert!(
        refused.status.success(),
        "software can still be expanded on request"
    );

    let approved = sandbox.json(&root, &["review", "approve", &firmware, &notes]);
    let filed = approved["filed"].as_array().unwrap();
    assert_eq!(filed.len(), 2, "{approved}");
    let firmware_path = filed
        .iter()
        .find_map(|f| {
            f["path"]
                .as_str()
                .filter(|p| p.ends_with("lamp-firmware-main"))
        })
        .unwrap();
    assert!(root.join(firmware_path).join("src/main.rs").is_file());
    assert!(!zipped.exists());
    assert!(sandbox.path("Trash/lamp-firmware-main.zip").is_file());
    let card = std::fs::read_to_string(root.join(format!("{firmware_path}.md"))).unwrap();
    assert!(card.contains("kind: bundle"), "{card}");
    assert!(card.contains("expanded_from:"), "{card}");
    assert!(card.contains("name: lamp-firmware-main.zip"), "{card}");
    assert!(
        filed
            .iter()
            .any(|f| f["path"].as_str().unwrap().ends_with("notes.zip")),
        "kept compressed: {approved}"
    );

    sandbox.ok(&root, &["settings", "set", "compressed_files", "keep"]);
    let settings = std::fs::read_to_string(root.join("_System/settings.json")).unwrap();
    assert!(
        settings.contains("\"compressed_files\": \"keep\""),
        "{settings}"
    );
}

#[test]
fn expand_opens_a_filed_zip_in_one_change_that_undo_reverses() {
    let sandbox = Sandbox::new();
    let root = sandbox.sample();
    sandbox.ok(&root, &["init", root.to_str().unwrap()]);
    sandbox.ok(&root, &["settings", "set", "compressed_files", "keep"]);
    let downloads = sandbox.path("Downloads");
    std::fs::create_dir(&downloads).unwrap();
    write_zip(
        &downloads.join("Spectra.zip"),
        &[
            ("Spectra/run1.csv", b"nm,counts\n"),
            ("Spectra/run2.csv", b"nm,counts\n"),
        ],
    );
    sandbox.ok(
        &root,
        &["ingest", "--wait", "--each", downloads.to_str().unwrap()],
    );
    let entries = sandbox.json(&root, &["review"])["entries"].clone();
    let entry = entries
        .as_array()
        .unwrap()
        .iter()
        .find(|e| {
            e["source"]["path"]
                .as_str()
                .unwrap()
                .ends_with("Spectra.zip")
        })
        .unwrap();
    assert_eq!(entry["compressed"]["action"], "keep");
    let approved = sandbox.json(&root, &["review", "approve", entry["id"].as_str().unwrap()]);
    let zip_path = approved["filed"][0]["path"].as_str().unwrap().to_owned();
    assert!(zip_path.ends_with("Spectra.zip"), "{approved}");

    let expanded = sandbox.json(&root, &["expand", &zip_path]);
    let to = expanded["to"].as_str().unwrap();
    assert!(to.ends_with("/Spectra"), "{expanded}");
    assert_eq!(expanded["kind"], "folder");
    assert!(root.join(to).join("run2.csv").is_file());
    assert!(!root.join(&zip_path).exists());
    assert!(sandbox.path("Trash/Spectra.zip").is_file());
    let card = std::fs::read_to_string(root.join(expanded["card"].as_str().unwrap())).unwrap();
    assert!(
        card.contains("kind: folder") && card.contains("name: Spectra.zip"),
        "{card}"
    );
    assert!(
        !root.join(format!("{zip_path}.md")).exists(),
        "the card follows the item"
    );

    let again = sandbox.run_in(&root, &["expand", to]);
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("isn't a compressed file"));

    let commit = expanded["commit"].as_str().unwrap();
    sandbox.ok(&root, &["undo", commit]);
    assert!(
        !root.join(to).exists(),
        "what was expanded leaves the collection"
    );
    let restored = std::fs::read_to_string(root.join(format!("{zip_path}.md"))).unwrap();
    assert!(restored.contains("kind: file"), "{restored}");
}

/// Runs carrel in `root` with only `bin`'s providers on the path, and the worker off.
fn run_with_providers(sandbox: &Sandbox, root: &Path, bin: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_carrel"))
        .args(args)
        .current_dir(root)
        .env("PATH", bin)
        .env("CARREL_CONFIG_DIR", sandbox.path("config"))
        .env("CARREL_TRASH_DIR", sandbox.path("Trash"))
        .env("CARREL_WORKER", "off")
        .output()
        .unwrap()
}

/// Its standard output as JSON, after checking it succeeded.
fn json_of(out: &Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn the_index_is_rebuilt_reported_and_caught_up_by_the_worker() {
    let sandbox = Sandbox::new();
    let bin = sandbox.path("bin");
    std::fs::create_dir(&bin).unwrap();
    let suffix = std::env::consts::EXE_SUFFIX;
    std::fs::copy(
        echo_provider(),
        bin.join(format!("carrel-provider-echo{suffix}")),
    )
    .unwrap();
    let root = sandbox.sample();
    file_with_card(
        &root,
        "Holding",
        "lease.txt",
        "The landlord repairs the boiler. The tenant keeps the garden tidy.",
        "title: \"Tenancy agreement\"\nsensitivity: personal\n",
    );
    let run = |args: &[&str]| run_with_providers(&sandbox, &root, &bin, args);

    // No index yet: search says it used words only.
    let status = sandbox.json(&root, &["index", "status"]);
    assert_eq!(status["model"], "sample/embedding-model-v1");
    assert_eq!(status["exists"], false);
    let text = sandbox.ok(&root, &["index"]);
    assert!(text.contains("no meaning index"), "{text}");
    let found = sandbox.json(&root, &["search", "boiler"]);
    assert_eq!(found["meaning"]["state"], "off");
    assert!(
        found["words_only"]
            .as_str()
            .unwrap()
            .contains("carrel index rebuild"),
        "{found}"
    );

    // Only the archive's model is used: the echo provider doesn't run the sample's.
    let out = run(&["index", "rebuild"]);
    assert!(!out.status.success());
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(error.contains("sample/embedding-model-v1"), "{error}");
    assert!(!root.join("_System/index").exists(), "nothing written");

    set_embedding_model(&root, Some("echo-embed"));
    let rebuilt = json_of(&run(&["index", "rebuild", "--json"]));
    let items = rebuilt["items"].as_u64().unwrap();
    assert!(items > 5, "{rebuilt}");
    assert_eq!(rebuilt["embedded"], items);
    assert_eq!(rebuilt["dimensions"], 3);
    assert_eq!(rebuilt["resumed"], false);
    assert!(root.join("_System/index/echo-embed/index.json").is_file());
    let status = json_of(&run(&["index", "status", "--json"]));
    assert_eq!(status["embedded"], items);
    assert_eq!(status["behind"], 0);

    // Search merges meaning with words.
    let found = json_of(&run(&["search", "boiler", "--json"]));
    assert_eq!(found["meaning"]["state"], "current", "{found}");
    assert_eq!(found["words_only"], Value::Null);
    let lease = found["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["found"]["title"] == "Tenancy agreement")
        .unwrap();
    assert!(
        lease["why"].as_array().unwrap().contains(&json!("text")),
        "{lease}"
    );
    assert!(
        found["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["why"].as_array().unwrap().contains(&json!("meaning"))),
        "{found}"
    );

    // A changed card puts it behind until the worker embeds it again.
    let card = root.join("Holding/lease.txt.md");
    let text = std::fs::read_to_string(&card).unwrap();
    std::fs::write(&card, text.replace("Tenancy agreement", "Flat lease")).unwrap();
    let status = json_of(&run(&["index", "status", "--json"]));
    assert_eq!(status["behind"], 1);
    let report = String::from_utf8_lossy(&run(&["index", "status"]).stdout).into_owned();
    assert!(report.contains("1 behind"), "{report}");
    let found = json_of(&run(&["search", "lease", "--json"]));
    assert_eq!(found["meaning"], json!({ "state": "behind", "items": 1 }));
    let worked = json_of(&run(&["worker", "--json"]));
    assert_eq!(worked["embedded"], 1);
    let status = json_of(&run(&["index", "status", "--json"]));
    assert_eq!(status["behind"], 0);

    // Without the model, search falls back to words and says so.
    let found = sandbox.json(&root, &["search", "boiler"]);
    assert_eq!(found["meaning"]["state"], "off");
    assert!(
        found["words_only"]
            .as_str()
            .unwrap()
            .contains("No provider on this device runs echo-embed"),
        "{found}"
    );
    assert_eq!(found["results"][0]["found"]["title"], "Flat lease");
}

#[test]
fn bench_search_scores_findability_and_reuses_each_models_index() {
    let sandbox = Sandbox::new();
    let bin = sandbox.path("bin");
    std::fs::create_dir(&bin).unwrap();
    let suffix = std::env::consts::EXE_SUFFIX;
    std::fs::copy(
        echo_provider(),
        bin.join(format!("carrel-provider-echo{suffix}")),
    )
    .unwrap();
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/bench");
    let out = sandbox.path("results");
    let corpus = corpus.to_str().unwrap();
    let out_arg = out.to_str().unwrap();
    let args = ["bench", "--search", "--corpus", corpus, "--out", out_arg];
    let cwd = sandbox.dir.path().to_owned();

    let first = run_with_providers(&sandbox, &cwd, &bin, &args);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let text = String::from_utf8_lossy(&first.stdout).into_owned();
    assert!(text.contains("| Words alone | none |"), "{text}");
    assert!(text.contains("| Meaning alone | echo-embed |"), "{text}");
    assert!(
        text.contains("| Words and meaning | echo-embed |"),
        "{text}"
    );
    assert!(text.contains("Built an index for echo-embed"), "{text}");
    let report = std::fs::read_to_string(out.join("search/report.md")).unwrap();
    assert!(report.contains("## Not in the top five"), "{report}");
    assert!(
        out.join("search/archive/_System/index/echo-embed/index.json")
            .is_file()
    );
    let results: Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("search/results.json")).unwrap())
            .unwrap();
    let rankings = results["rankings"].as_array().unwrap();
    assert_eq!(rankings.len(), 3);
    assert_eq!(rankings[0]["way"], "words");
    let described = results["descriptions"].as_u64().unwrap();
    assert_eq!(rankings[0]["score"]["descriptions"], described);
    assert!(rankings[0]["score"]["top_five"].as_f64().unwrap() > 0.3);

    // Run again, the archive and index are reused: nothing is embedded again.
    let again = json_of(&run_with_providers(
        &sandbox,
        &cwd,
        &bin,
        &[args.as_slice(), &["--json"]].concat(),
    ));
    let note = again["notes"][0].as_str().unwrap();
    assert!(
        note.starts_with("Reused the index for echo-embed") && note.contains("0 embedded again"),
        "{note}"
    );

    // Without an embedding model on offer, words alone are measured, and a missing one fails.
    let nothing = sandbox.path("no-providers");
    std::fs::create_dir(&nothing).unwrap();
    let words = json_of(&run_with_providers(
        &sandbox,
        &cwd,
        &nothing,
        &[args.as_slice(), &["--json"]].concat(),
    ));
    assert_eq!(words["rankings"].as_array().unwrap().len(), 1);
    assert!(
        words["notes"][0]
            .as_str()
            .unwrap()
            .contains("No embedding model is on offer"),
        "{words}"
    );
    let missing = run_with_providers(
        &sandbox,
        &cwd,
        &nothing,
        &[args.as_slice(), &["--model", "nope"]].concat(),
    );
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("no embedding model nope"));
}

/// The sample archive with history, searched with `echo-embed` and its index built, and a
/// folder of providers offering `echo-embed` and `echo-embed-wide`.
fn echo_archive(sandbox: &Sandbox) -> (PathBuf, PathBuf) {
    let bin = sandbox.path("bin");
    std::fs::create_dir(&bin).unwrap();
    let suffix = std::env::consts::EXE_SUFFIX;
    for name in ["carrel-provider-echo", "carrel-provider-echo-wide"] {
        std::fs::copy(echo_provider(), bin.join(format!("{name}{suffix}"))).unwrap();
    }
    let root = sandbox.sample();
    sandbox.ok(&root, &["init", root.to_str().unwrap()]);
    file_with_card(
        &root,
        "Holding",
        "lease.txt",
        "The landlord repairs the boiler. The tenant keeps the garden tidy.",
        "title: \"Tenancy agreement\"\nsensitivity: personal\n",
    );
    set_embedding_model(&root, Some("echo-embed"));
    json_of(&run_with_providers(
        sandbox,
        &root,
        &bin,
        &["index", "rebuild", "--json"],
    ));
    (root, bin)
}

#[test]
fn show_lists_the_items_nearest_by_meaning_under_related() {
    let sandbox = Sandbox::new();

    // No index: the links only, and why.
    let plain = sandbox.sample();
    let parts = sandbox.json(&plain, &["show", "Projects/Hardware/parts-list.csv"]);
    assert_eq!(parts["near"], json!([]));
    assert!(
        parts["links_only"]
            .as_str()
            .unwrap()
            .contains("carrel index rebuild"),
        "{parts}"
    );
    let report = sandbox.ok(&plain, &["show", "Projects/Hardware/parts-list.csv"]);
    assert!(
        report.contains("\nRelated:\n  part-of Projects/Hardware/lamp-build.md\n  Only its links"),
        "{report}"
    );

    // A restricted copy of the lease, whose card reads like the lease's, so it's nearest.
    let (root, bin) = echo_archive(&sandbox);
    file_with_card(
        &root,
        "Holding",
        "lease copy.txt",
        "Account 4111 1111 1111 1111.",
        "title: \"Tenancy agreement\"\nsensitivity: restricted\n",
    );
    json_of(&run_with_providers(
        &sandbox,
        &root,
        &bin,
        &["index", "rebuild", "--json"],
    ));

    // Nothing is embedded to show it, so it needs no provider: none is on the path here.
    let lease = sandbox.json(&root, &["show", "Holding/lease.txt"]);
    assert_eq!(lease["links_only"], Value::Null, "{lease}");
    let near = lease["near"].as_array().unwrap();
    assert_eq!(near.len(), 5, "{lease}");
    assert!(near.iter().all(|n| n["card"] != "Holding/lease.txt.md"));
    assert_eq!(near[0]["card"], "Holding/lease copy.txt.md", "{lease}");
    assert_eq!(near[0]["sensitivity"], "restricted");
    assert!(near[0]["score"].as_f64().unwrap() > near[4]["score"].as_f64().unwrap());
    let report = sandbox.ok(&root, &["show", "Holding/lease.txt"]);
    assert!(
        report.contains("  near Holding/lease copy.txt.md  Tenancy agreement  [restricted]"),
        "{report}"
    );

    // What it links to is listed as a link, not again as near it.
    let parts = sandbox.json(&root, &["show", "Projects/Hardware/parts-list.csv"]);
    let near = parts["near"].as_array().unwrap();
    assert_eq!(near.len(), 5, "{parts}");
    assert!(
        near.iter()
            .all(|n| n["card"] != "Projects/Hardware/lamp-build.md"),
        "{parts}"
    );

    // A card the index hasn't caught up with lists its links only, and says why.
    let card = root.join("Holding/lease.txt.md");
    let text = std::fs::read_to_string(&card).unwrap();
    std::fs::write(&card, text.replace("Tenancy agreement", "Flat lease")).unwrap();
    let lease = sandbox.json(&root, &["show", "Holding/lease.txt"]);
    assert_eq!(lease["near"], json!([]));
    assert!(
        lease["links_only"]
            .as_str()
            .unwrap()
            .contains("hasn't caught up"),
        "{lease}"
    );
}

#[test]
fn index_model_lists_the_models_here_and_refuses_others() {
    let sandbox = Sandbox::new();
    let (root, bin) = echo_archive(&sandbox);
    let run = |args: &[&str]| run_with_providers(&sandbox, &root, &bin, args);
    let new = root.join("_System/index/echo-embed-wide");

    // Without a name: the model in use and the ones this device has.
    let listed = json_of(&run(&["index", "model", "--json"]));
    assert_eq!(listed["model"], "echo-embed");
    assert_eq!(listed["switch"], Value::Null);
    let available: Vec<&str> = listed["available"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["model"].as_str().unwrap())
        .collect();
    assert!(available.contains(&"echo-embed-wide"), "{listed}");
    let text = String::from_utf8_lossy(&run(&["index", "model"]).stdout).into_owned();
    assert!(
        text.contains("echo-embed-wide  through echo-wide, 4 numbers"),
        "{text}"
    );
    assert!(text.contains("download it with"), "{text}");

    // A model no provider here offers is refused, saying which can be used.
    let out = run(&["index", "model", "qwen3-embedding-0.6b"]);
    assert!(!out.status.success());
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(
        error.contains("no provider on this device offers"),
        "{error}"
    );
    assert!(
        error.contains("this device can embed with echo-embed"),
        "{error}"
    );
    assert!(error.contains("echo-embed-wide"), "{error}");
    assert!(error.contains("carrel models download"), "{error}");
    assert!(!new.exists());
}

#[test]
fn index_model_switches_models_while_the_old_index_serves() {
    let sandbox = Sandbox::new();
    let (root, bin) = echo_archive(&sandbox);
    let run = |args: &[&str]| run_with_providers(&sandbox, &root, &bin, args);
    let old = root.join("_System/index/echo-embed");
    let new = root.join("_System/index/echo-embed-wide");

    // Stopped part way: the switch is recorded, and search keeps using the old index.
    let out = stopped_switch(&sandbox, &root, &bin);
    assert!(!out.status.success());
    assert_eq!(
        archive_settings(&root)["embedding"],
        json!({ "model": "echo-embed", "switching_to": "echo-embed-wide" })
    );
    let status = json_of(&run(&["index", "status", "--json"]));
    assert_eq!(status["model"], "echo-embed");
    assert_eq!(status["switch"]["to"], "echo-embed-wide");
    assert_eq!(status["switch"]["embedded"], 2);
    // Another device without the new model sees the switch too.
    let text = sandbox.ok(&root, &["index", "status"]);
    assert!(
        text.contains("Switching to echo-embed-wide: 2 of"),
        "{text}"
    );
    assert!(text.contains("search uses echo-embed until"), "{text}");
    let found = json_of(&run(&["search", "boiler", "--json"]));
    assert_eq!(found["meaning"]["state"], "current", "{found}");
    assert!(old.join("index.json").is_file());

    // Run again, it carries on, switches, and removes the old index.
    let switched = json_of(&run(&["index", "model", "echo-embed-wide", "--json"]));
    assert_eq!(switched["resumed"], true);
    assert_eq!(switched["switched"], true);
    assert_eq!(switched["from"], "echo-embed");
    assert_eq!(switched["dimensions"], 4);
    assert_eq!(
        switched["embedded"],
        switched["items"].as_u64().unwrap() - 2
    );
    assert_eq!(
        archive_settings(&root)["embedding"],
        json!({ "model": "echo-embed-wide" })
    );
    assert!(!old.exists());
    let status = json_of(&run(&["index", "status", "--json"]));
    assert_eq!(status["model"], "echo-embed-wide");
    assert_eq!(status["switch"], Value::Null);
    assert_eq!(status["behind"], 0);
    let found = json_of(&run(&["search", "boiler", "--json"]));
    assert_eq!(found["meaning"]["state"], "current", "{found}");
    let log = sandbox.ok(&root, &["log"]);
    assert!(
        log.contains("Switch the embedding model from echo-embed to echo-embed-wide"),
        "{log}"
    );

    // The worker uses the new model.
    let card = root.join("Holding/lease.txt.md");
    let text = std::fs::read_to_string(&card).unwrap();
    std::fs::write(&card, text.replace("Tenancy agreement", "Flat lease")).unwrap();
    let worked = json_of(&run(&["worker", "--json"]));
    assert_eq!(worked["embedded"], 1);
    assert!(!old.exists());

    // A device without the new model searches by words, and says why.
    let found = sandbox.json(&root, &["search", "boiler"]);
    assert_eq!(found["meaning"]["state"], "off");
    assert!(
        found["words_only"]
            .as_str()
            .unwrap()
            .contains("No provider on this device runs echo-embed-wide"),
        "{found}"
    );

    // Switching back runs to the end; a switch stopped part way is given up by choosing the
    // archive's model again.
    let back = json_of(&run(&["index", "model", "echo-embed", "--json"]));
    assert_eq!(back["from"], "echo-embed-wide");
    assert!(!new.exists());
    let interrupted = stopped_switch(&sandbox, &root, &bin);
    assert!(!interrupted.status.success());
    assert!(new.is_dir());
    let text = String::from_utf8_lossy(&run(&["index", "model", "echo-embed"]).stdout).into_owned();
    assert!(
        text.contains("Stopped switching to echo-embed-wide"),
        "{text}"
    );
    assert_eq!(
        archive_settings(&root)["embedding"],
        json!({ "model": "echo-embed" })
    );
    assert!(!new.exists());
    assert!(old.join("index.json").is_file());
    let text = String::from_utf8_lossy(&run(&["index", "model", "echo-embed"]).stdout).into_owned();
    assert!(text.contains("already searched with echo-embed"), "{text}");
}

/// The archive's shared settings.
fn archive_settings(root: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(root.join("_System/settings.json")).unwrap())
        .unwrap()
}

/// `carrel index model echo-embed-wide`, with the wide echo provider stopping after the test
/// query and two items.
fn stopped_switch(sandbox: &Sandbox, root: &Path, bin: &Path) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_carrel"))
        .args(["index", "model", "echo-embed-wide"])
        .current_dir(root)
        .env("PATH", bin)
        .env("CARREL_CONFIG_DIR", sandbox.path("config"))
        .env("CARREL_TRASH_DIR", sandbox.path("Trash"))
        .env("CARREL_WORKER", "off")
        .env("ECHO_EMBED_FAIL_AFTER", "3")
        .output()
        .unwrap()
}
