//! Ingest's last stage: filing approved and held proposals into the archive.

use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::card::{Checksum, Id, Kind, Tag};
use carrel_core::card_file::CardFile;
use carrel_core::coordinator::{Coordinator, DEFAULT_LEASE, DiskLock, Holder, WriterLease};
use carrel_core::history::{History, Operation};
use carrel_core::ingest::examine;
use carrel_core::ingest::filing::{FilingError, Placement, file_batch};
use carrel_core::review::{REVIEW_DIR, ReviewEntry, ReviewQueue, State};

const HARDWARE: &str = "01a18701-0500-776a-9867-0804857e5ab6";

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

struct Setup {
    dir: tempfile::TempDir,
    archive: Archive,
    lease: WriterLease,
    device: Id,
}

impl Setup {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("archive");
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
            &root,
        );
        for entry in std::fs::read_dir(root.join(REVIEW_DIR)).unwrap() {
            std::fs::remove_file(entry.unwrap().path()).unwrap();
        }
        let archive = Archive::open(&root).unwrap();
        let holder = Holder {
            person: Id::new_v7(),
            device: Id::new_v7(),
        };
        let lease = DiskLock::for_archive(&archive)
            .acquire(holder, "ingest", DEFAULT_LEASE)
            .unwrap();
        History::start(&archive, &lease).unwrap();
        std::fs::create_dir_all(dir.path().join("in")).unwrap();
        Self {
            dir,
            archive,
            lease,
            device: holder.device,
        }
    }

    fn incoming(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.dir.path().join("in").join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    /// A proposal for `path`, saved in the queue, going to Projects/Hardware.
    fn proposal(&self, path: &Path) -> ReviewEntry {
        let text = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/sample-archive")
                .join(REVIEW_DIR)
                .join("01a18701-0500-776a-9867-08f000000001.json"),
        )
        .unwrap();
        let mut entry: ReviewEntry = serde_json::from_str(&text).unwrap();
        let candidate = examine(&self.archive, path).unwrap();
        entry.id = Id::new_v7();
        path.clone_into(&mut entry.source.path);
        entry.source.device = self.device;
        entry.source.checksum = candidate.checksum;
        entry.source.size = candidate.size;
        entry.kind = candidate.kind;
        entry.sealed = candidate.sealed;
        entry.card.title = format!("About {}", path.file_name().unwrap().to_string_lossy());
        entry.proposed_tags.clear();
        assert_eq!(entry.destination.collection.to_string(), HARDWARE);
        ReviewQueue::of(&self.archive).save(&entry).unwrap();
        entry
    }

    fn file(
        &mut self,
        decided: Vec<(ReviewEntry, Placement)>,
    ) -> carrel_core::ingest::filing::Outcome {
        file_batch(&self.archive, decided, &mut self.lease, self.device, None).unwrap()
    }

    fn root(&self) -> &Path {
        self.archive.root()
    }
}

fn card_at(path: &Path) -> CardFile {
    CardFile::parse(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn approved_items_move_in_with_cards_tags_changelog_and_one_commit() {
    let mut setup = Setup::new();
    let a = setup.incoming("strip.txt", "LED strip wiring\n");
    let b = setup.incoming("base.txt", "Lamp base sketch\n");
    let mut first = setup.proposal(&a);
    first.proposed_tags = vec![Tag("lighting".into())];
    let second = setup.proposal(&b);
    let before = History::open(&setup.archive).unwrap().log().unwrap().len();

    let outcome = setup.file(vec![
        (first, Placement::Destination),
        (second, Placement::Destination),
    ]);
    assert_eq!(outcome.filed.len(), 2, "{:?}", outcome.failed);
    assert!(!a.exists() && !b.exists(), "the originals moved");

    let hardware = setup.root().join("Projects/Hardware");
    assert_eq!(
        std::fs::read_to_string(hardware.join("strip.txt")).unwrap(),
        "LED strip wiring\n"
    );
    let card = card_at(&hardware.join("strip.txt.md"));
    assert_eq!(card.card().item.as_deref(), Some("strip.txt"));
    assert_eq!(card.card().kind, Kind::File);
    assert_eq!(
        card.card().checksum,
        Some(Checksum::of_bytes(b"LED strip wiring\n"))
    );
    assert!(card.card().tags.contains(&Tag("lighting".into())));
    assert!(card.card().dates.as_ref().unwrap().added.is_some());
    assert!(
        card.card().problems().is_empty(),
        "{:?}",
        card.card().problems()
    );

    let collections = setup.archive.collections().unwrap();
    let doc = &collections
        .iter()
        .find(|c| c.path == "Projects/Hardware")
        .unwrap()
        .doc;
    assert!(
        doc.tags.iter().any(|t| t.name.0 == "lighting"),
        "the approved tag joins the vocabulary"
    );
    let changelog = std::fs::read_to_string(hardware.join("CHANGELOG.md")).unwrap();
    assert!(changelog.contains("added [About strip.txt](strip.txt.md) from ingest"));
    assert!(changelog.contains("added [About base.txt](base.txt.md) from ingest"));
    assert!(
        changelog.find("About").unwrap() < changelog.find("2026-10-26").unwrap(),
        "newest first"
    );

    let log = History::open(&setup.archive).unwrap().log().unwrap();
    assert_eq!(log.len(), before + 1, "one commit for the batch");
    assert_eq!(log[0].operation, Some(Operation::Ingest));
    assert_eq!(log[0].items.len(), 2);
    let (queued, _) = ReviewQueue::of(&setup.archive).list().unwrap();
    assert!(queued.is_empty(), "filed proposals leave the queue");
    let listing =
        std::fs::read_to_string(setup.root().join("Projects/Hardware/_collection.md")).unwrap();
    assert!(listing.contains("| [About strip.txt](strip.txt.md) | file |"));
    assert!(
        std::fs::read_to_string(setup.root().join("ARCHIVE.md"))
            .unwrap()
            .contains("(+6 in subcollections)"),
        "the counts above it change too"
    );
    assert_eq!(outcome.listing_problems, Vec::<String>::new());
}

#[test]
fn a_taken_name_gets_a_number_and_the_card_says_why() {
    let mut setup = Setup::new();
    let path = setup.incoming("parts-list.csv", "part,count\nled,4\n");
    let entry = setup.proposal(&path);
    let outcome = setup.file(vec![(entry, Placement::Destination)]);
    assert_eq!(
        outcome.filed[0].path,
        "Projects/Hardware/parts-list (2).csv"
    );
    let card = card_at(&setup.root().join("Projects/Hardware/parts-list (2).csv.md"));
    assert!(card.body().contains("“parts-list.csv”"));
    let changelog =
        std::fs::read_to_string(setup.root().join("Projects/Hardware/CHANGELOG.md")).unwrap();
    assert!(changelog.contains("(parts-list%20%282%29.csv.md)"));
}

#[test]
fn names_that_arent_portable_are_filed_under_portable_ones() {
    let mut setup = Setup::new();
    let path = setup.incoming("Q3: plan?.txt", "plan\n");
    let entry = setup.proposal(&path);
    let outcome = setup.file(vec![(entry, Placement::Destination)]);
    assert_eq!(outcome.filed[0].path, "Projects/Hardware/Q3- plan-.txt");
    let card = card_at(&setup.root().join("Projects/Hardware/Q3- plan-.txt.md"));
    assert_eq!(card.card().item.as_deref(), Some("Q3- plan-.txt"));
    assert!(
        card.body().contains("“Q3: plan?.txt” contains : ?"),
        "{}",
        card.body()
    );
}

#[test]
fn held_folders_go_to_holding_with_a_manifest() {
    let mut setup = Setup::new();
    let kit = setup.dir.path().join("in/kit");
    std::fs::create_dir_all(kit.join("src")).unwrap();
    std::fs::write(kit.join("README.md"), "kit\n").unwrap();
    std::fs::write(kit.join("src/main.rs"), "fn main() {}\n").unwrap();
    let entry = setup.proposal(&kit);
    let checksum = entry.source.checksum.clone();
    let outcome = setup.file(vec![(entry, Placement::Holding)]);
    assert_eq!(outcome.filed[0].path, "Holding/kit");

    let card = card_at(&setup.root().join("Holding/kit.md"));
    let manifest = card.card().manifest.as_ref().unwrap();
    assert_eq!(manifest.checksum, checksum, "the same contents as proposed");
    assert!(setup.root().join(&manifest.path).is_file());
    assert_eq!(card.card().size, Some(17));
    assert!(
        card.card().tags.is_empty(),
        "tags outside Holding's vocabulary are left off"
    );
    assert!(setup.root().join("Holding/kit/src/main.rs").is_file());
}

#[test]
fn changed_items_wait_for_the_owner_and_other_devices_items_wait_for_them() {
    let mut setup = Setup::new();
    let changed = setup.incoming("changed.txt", "first\n");
    let elsewhere = setup.incoming("elsewhere.txt", "here\n");
    let first = setup.proposal(&changed);
    std::fs::write(&changed, "second\n").unwrap();
    let mut second = setup.proposal(&elsewhere);
    second.source.device = Id::new_v7();
    let before = History::open(&setup.archive).unwrap().log().unwrap().len();

    let outcome = setup.file(vec![
        (first.clone(), Placement::Destination),
        (second.clone(), Placement::Destination),
    ]);
    assert_eq!(outcome.filed, []);
    assert!(outcome.commit.is_none());
    assert!(matches!(outcome.failed[0].1, FilingError::Changed(_)));
    assert!(matches!(outcome.failed[1].1, FilingError::OtherDevice(_)));
    assert!(changed.exists() && elsewhere.exists(), "nothing moved");

    let queue = ReviewQueue::of(&setup.archive);
    let waiting = queue.find(&first.id.to_string()).unwrap();
    assert_eq!(waiting.state, State::NeedsOwner);
    assert!(waiting.problems.iter().any(|p| p.stage == "archive"));
    assert_eq!(
        queue.find(&second.id.to_string()).unwrap().state,
        State::Approved,
        "it waits for its own device"
    );
    assert_eq!(
        History::open(&setup.archive).unwrap().log().unwrap().len(),
        before
    );
}

#[test]
fn keep_copies_and_leaves_the_original() {
    let mut setup = Setup::new();
    let settings = setup.root().join("_System/settings.json");
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    value["ingest"] = serde_json::json!({"originals": "keep"});
    std::fs::write(&settings, value.to_string()).unwrap();
    setup.archive = Archive::open(setup.root()).unwrap();

    let path = setup.incoming("keep.txt", "keep me\n");
    let entry = setup.proposal(&path);
    let outcome = setup.file(vec![(entry, Placement::Destination)]);
    assert_eq!(
        outcome.filed[0].kept_original.as_deref(),
        Some(path.as_path())
    );
    assert!(path.exists());
    assert!(setup.root().join("Projects/Hardware/keep.txt").is_file());
}

#[test]
fn only_owner_rules_file_without_review_and_never_sensitive_items() {
    use carrel_core::card::Sensitivity;
    use carrel_core::ingest::filing::files_itself;
    use carrel_core::ingest::route::Route;
    use carrel_core::review::Confidence;

    let setup = Setup::new();
    let path = setup.incoming("rule.txt", "by rule\n");
    let mut entry = setup.proposal(&path);
    let mut route = Route {
        collection: Some(entry.destination.collection),
        path: "Projects/Hardware".into(),
        confidence: Confidence::High,
        reasons: Vec::new(),
        rule: Some("prints".into()),
        auto_file: true,
        attempts: Vec::new(),
    };
    assert!(files_itself(&entry, &route));
    entry.card.sensitivity = Sensitivity::Sensitive;
    assert!(!files_itself(&entry, &route), "sensitive items always wait");
    entry.card.sensitivity = Sensitivity::Personal;
    entry.state = State::NeedsOwner;
    assert!(!files_itself(&entry, &route));
    entry.state = State::Proposed;
    route.rule = None;
    assert!(
        !files_itself(&entry, &route),
        "a model's choice never files itself"
    );
}

/// Backups that record what they were asked, and succeed or fail as told.
struct FakeBackups {
    fail: bool,
    taken: std::sync::Mutex<Vec<String>>,
}

impl carrel_core::backup::Backups for FakeBackups {
    fn describe(&self) -> String {
        "fake".into()
    }
    fn snapshot(
        &self,
        _root: &Path,
        description: &str,
    ) -> Result<carrel_core::backup::Snapshot, carrel_core::backup::BackupError> {
        if self.fail {
            return Err(carrel_core::backup::BackupError::Unreachable(
                "/Volumes/Backup".into(),
            ));
        }
        self.taken.lock().unwrap().push(description.to_owned());
        Ok(carrel_core::backup::Snapshot {
            id: "abc123".into(),
            time: "2026-10-29T12:00:00Z".into(),
            description: Some(description.to_owned()),
            pinned: true,
            size: None,
            files: None,
        })
    }
    fn list(
        &self,
        _root: &Path,
    ) -> Result<Vec<carrel_core::backup::Snapshot>, carrel_core::backup::BackupError> {
        Ok(Vec::new())
    }
    fn verify(&self, _percent: u8) -> Result<String, carrel_core::backup::BackupError> {
        Ok(String::new())
    }
    fn restore(
        &self,
        _snapshot: &str,
        _path: &str,
        _to: &Path,
    ) -> Result<(), carrel_core::backup::BackupError> {
        Ok(())
    }
}

#[test]
fn a_batch_is_snapshotted_and_its_commit_names_the_snapshot() {
    let mut setup = Setup::new();
    let path = setup.incoming("snap.txt", "snap\n");
    let entry = setup.proposal(&path);
    let backups = FakeBackups {
        fail: false,
        taken: std::sync::Mutex::new(Vec::new()),
    };
    let outcome = file_batch(
        &setup.archive,
        vec![(entry, Placement::Destination)],
        &mut setup.lease,
        setup.device,
        Some(&backups),
    )
    .unwrap();
    assert_eq!(outcome.snapshot.unwrap().id, "abc123");
    assert_eq!(*backups.taken.lock().unwrap(), ["File “About snap.txt”"]);
    let log = History::open(&setup.archive).unwrap().log().unwrap();
    assert_eq!(log[0].snapshot.as_deref(), Some("kopia:abc123"));
}

#[test]
fn an_unreachable_backup_never_stops_filing() {
    let mut setup = Setup::new();
    let path = setup.incoming("away.txt", "away\n");
    let entry = setup.proposal(&path);
    let backups = FakeBackups {
        fail: true,
        taken: std::sync::Mutex::new(Vec::new()),
    };
    let outcome = file_batch(
        &setup.archive,
        vec![(entry, Placement::Destination)],
        &mut setup.lease,
        setup.device,
        Some(&backups),
    )
    .unwrap();
    assert_eq!(outcome.filed.len(), 1);
    assert!(outcome.commit.is_some(), "filed and committed anyway");
    assert!(matches!(
        outcome.snapshot_error,
        Some(carrel_core::backup::BackupError::Unreachable(_))
    ));
    let log = History::open(&setup.archive).unwrap().log().unwrap();
    assert_eq!(log[0].snapshot, None);
}
