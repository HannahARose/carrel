//! iCloud Drive's on-demand files, conflicted copies, `sync.keep_downloaded`, sync status and
//! moving an archive into iCloud Drive (`docs/design/sync-and-mobile.md`).
//!
//! iCloud itself can't be run in a test, so evicted files are faked two ways: `.name.icloud`
//! placeholder files on disk, as iCloud leaves them on older systems, and a provider standing
//! in for the `NSURLUbiquitousItem` states the Mac app reports through the core's trait.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::cloud_files::{
    self, DownloadState, Keep, OnDemandFiles, ensure_downloaded, keep_downloaded_pass,
    with_provider,
};
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder, WriterLease};
use carrel_core::detect::Rules;
use carrel_core::history::History;
use carrel_core::icloud_move::{self, MoveError};
use carrel_core::manifest::Manifest;

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

/// A stand-in for iCloud Drive: the files it has evicted, by path, and what downloading each
/// writes back. Downloads are recorded.
#[derive(Default)]
struct FakeICloud {
    evicted: Mutex<BTreeMap<PathBuf, Vec<u8>>>,
    downloading: Mutex<BTreeSet<PathBuf>>,
    downloaded: Mutex<Vec<PathBuf>>,
    offline: bool,
}

impl FakeICloud {
    /// Evicts the file at `path` as current macOS does: the name stays, the contents go (the
    /// placeholder here holds different bytes, so reading it would be noticed).
    fn evict(&self, path: &Path) {
        let contents = std::fs::read(path).unwrap();
        std::fs::write(path, b"").unwrap();
        self.evicted
            .lock()
            .unwrap()
            .insert(path.to_owned(), contents);
    }

    /// Evicts the file at `path` as older systems do, leaving `.name.icloud` in its place.
    fn evict_to_placeholder(&self, path: &Path) {
        let contents = std::fs::read(path).unwrap();
        std::fs::remove_file(path).unwrap();
        std::fs::write(cloud_files::placeholder_path(path).unwrap(), b"plist").unwrap();
        self.evicted
            .lock()
            .unwrap()
            .insert(path.to_owned(), contents);
    }

    fn downloads(&self) -> Vec<PathBuf> {
        self.downloaded.lock().unwrap().clone()
    }
}

impl OnDemandFiles for FakeICloud {
    fn state(&self, path: &Path) -> DownloadState {
        if self.downloading.lock().unwrap().contains(path) {
            DownloadState::Downloading
        } else if self.evicted.lock().unwrap().contains_key(path) {
            DownloadState::NotDownloaded
        } else if path.exists() {
            DownloadState::Downloaded
        } else {
            DownloadState::Missing
        }
    }

    fn download(&self, path: &Path) -> std::io::Result<()> {
        if self.offline {
            return Err(std::io::Error::other("offline"));
        }
        let Some(contents) = self.evicted.lock().unwrap().remove(path) else {
            return Ok(());
        };
        if let Some(placeholder) = cloud_files::placeholder_path(path) {
            let _ = std::fs::remove_file(placeholder);
        }
        std::fs::write(path, contents)?;
        self.downloaded.lock().unwrap().push(path.to_owned());
        Ok(())
    }
}

struct Setup {
    _dir: tempfile::TempDir,
    root: PathBuf,
    holder: Holder,
}

impl Setup {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Archive");
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
            &root,
        );
        let setup = Self {
            _dir: dir,
            root,
            holder: Holder {
                person: Id::new_v7(),
                device: Id::new_v7(),
            },
        };
        let lease = setup.lease();
        History::init(&setup.archive(), &lease).unwrap();
        lease.release().unwrap();
        setup
    }

    fn archive(&self) -> Archive {
        Archive::open(&self.root).unwrap()
    }

    fn lease(&self) -> WriterLease {
        DiskLock::for_archive(&self.archive())
            .acquire(self.holder, "test", DEFAULT_LEASE)
            .unwrap()
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

const PDF: &str = "Reading/Coupled oscillators note.pdf";
const FOLDER: &str = "Projects/field-notes-2025";

fn first_file_in(folder: &Path) -> PathBuf {
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    files.remove(0)
}

#[test]
fn an_evicted_file_is_neither_missing_nor_changed_in_a_manifest() {
    let s = Setup::new();
    let folder = s.path(FOLDER);
    let (rules, id) = (Rules::default(), Id::new_v7());
    std::thread::sleep(std::time::Duration::from_millis(20));
    let before = Manifest::build(&folder, id, &rules, None).unwrap();
    let file = first_file_in(&folder);
    let fake = Arc::new(FakeICloud::default());
    fake.evict(&file);
    let after = with_provider(fake.clone(), || {
        Manifest::build(&folder, id, &rules, Some(&before)).unwrap()
    });
    assert!(before.diff(&after).is_empty(), "{:?}", before.diff(&after));
    assert_eq!(after.entries, before.entries);
    assert!(
        fake.downloads().is_empty(),
        "building a manifest downloads nothing"
    );
}

#[test]
fn a_placeholder_file_stands_for_its_file_in_a_manifest() {
    let s = Setup::new();
    let folder = s.path(FOLDER);
    let (rules, id) = (Rules::default(), Id::new_v7());
    let before = Manifest::build(&folder, id, &rules, None).unwrap();
    let file = first_file_in(&folder);
    let name = file.file_name().unwrap().to_string_lossy().into_owned();
    let fake = Arc::new(FakeICloud::default());
    fake.evict_to_placeholder(&file);

    // The core's own provider recognizes the placeholder, with no help from the platform.
    let after = Manifest::build(&folder, id, &rules, Some(&before)).unwrap();
    assert!(before.diff(&after).is_empty(), "{:?}", before.diff(&after));
    assert!(after.entries.iter().all(|e| !e.path.ends_with(".icloud")));

    // Without a previous manifest it has no checksum, so it's listed, never called removed.
    let fresh = Manifest::build(&folder, id, &rules, None).unwrap();
    assert_eq!(fresh.not_downloaded, std::slice::from_ref(&name));
    assert!(before.diff(&fresh).is_empty(), "{:?}", before.diff(&fresh));
    assert!(!fresh.to_tsv().contains(&name));
}

#[test]
fn contents_are_downloaded_when_needed_through_the_provider() {
    let s = Setup::new();
    let folder = s.path(FOLDER);
    let file = first_file_in(&folder);
    let fake = Arc::new(FakeICloud::default());
    let original = std::fs::read(&file).unwrap();
    fake.evict_to_placeholder(&file);
    with_provider(fake.clone(), || ensure_downloaded(&folder).unwrap());
    assert_eq!(fake.downloads(), std::slice::from_ref(&file));
    assert_eq!(std::fs::read(&file).unwrap(), original);
    assert!(!cloud_files::placeholder_path(&file).unwrap().exists());

    // Offline, it says so rather than reading a placeholder.
    let offline = Arc::new(FakeICloud {
        offline: true,
        ..FakeICloud::default()
    });
    offline.evict(&file);
    assert!(with_provider(offline, || ensure_downloaded(&file)).is_err());
}

#[test]
fn checking_out_an_evicted_item_downloads_it_first() {
    let s = Setup::new();
    let archive = s.archive();
    let fake = Arc::new(FakeICloud::default());
    let pdf = s.path(PDF);
    let original = std::fs::read(&pdf).unwrap();
    fake.evict_to_placeholder(&pdf);
    let item = archive
        .items()
        .unwrap()
        .into_iter()
        .find(|i| i.item.as_deref() == Some(PDF))
        .unwrap();
    let desk = tempfile::tempdir().unwrap();
    let lease = s.lease();
    let out = with_provider(fake.clone(), || {
        carrel_core::checkout::check_out(&archive, &item, desk.path(), None, &lease).unwrap()
    });
    assert_eq!(fake.downloads(), [pdf]);
    assert_eq!(std::fs::read(out.working).unwrap(), original);
}

#[test]
fn an_evicted_card_is_downloaded_and_listed() {
    let s = Setup::new();
    let card = s.path("Reading/On the Origin of Species.md");
    let before = s.archive().items().unwrap().len();
    let fake = Arc::new(FakeICloud::default());
    fake.evict_to_placeholder(&card);
    let after = with_provider(fake.clone(), || s.archive().items().unwrap().len());
    assert_eq!(after, before);
    assert_eq!(fake.downloads(), [card]);
}

#[test]
fn files_not_downloaded_are_counted_and_local_only_folders_skipped() {
    let s = Setup::new();
    let fake = Arc::new(FakeICloud::default());
    fake.evict(&s.path(PDF));
    std::fs::create_dir_all(s.path("cache.nosync")).unwrap();
    std::fs::write(s.path("cache.nosync/.thumb.png.icloud"), "").unwrap();
    let status = with_provider(fake, || carrel_core::sync_status::sync_status(&s.archive()));
    assert_eq!(status.not_downloaded.files, [PDF]);
}

#[test]
fn conflicted_copies_are_reported_and_resolved_only_as_the_owner_chooses() {
    let s = Setup::new();
    let trash = tempfile::tempdir().unwrap();
    carrel_core::trash::redirect(Some(trash.path().to_owned()));
    let card = "Reading/On the Origin of Species.md";
    let copy = "Reading/On the Origin of Species 2.md";
    std::fs::write(s.path(copy), "the other device's version").unwrap();
    std::fs::write(s.path("Reading/photo 2.jpg"), "a photo").unwrap();
    std::fs::write(s.path("Reading/photo.jpg"), "another photo").unwrap();
    std::fs::write(
        s.path("_System/settings (Mac's conflicted copy 2026-11-01).json"),
        "{}",
    )
    .unwrap();

    let found = cloud_files::conflicts(&s.root);
    let pairs: Vec<(&str, &str)> = found
        .iter()
        .map(|c| (c.copy.as_str(), c.original.as_str()))
        .collect();
    assert_eq!(
        pairs,
        [
            (copy, card),
            (
                "_System/settings (Mac's conflicted copy 2026-11-01).json",
                "_System/settings.json"
            ),
        ],
        "an item's own numbered files aren't conflicts"
    );
    // Reporting changes nothing.
    assert!(s.path(copy).exists() && s.path(card).exists());

    let lease = s.lease();
    cloud_files::resolve(&s.archive(), copy, Keep::Copy, &lease).unwrap();
    assert_eq!(
        std::fs::read_to_string(s.path(card)).unwrap(),
        "the other device's version"
    );
    assert!(!s.path(copy).exists());
    assert_eq!(std::fs::read_dir(trash.path()).unwrap().count(), 1);
    assert!(
        cloud_files::resolve(&s.archive(), copy, Keep::Original, &lease).is_err(),
        "it's no longer a conflict"
    );
    carrel_core::trash::redirect(None);
}

#[test]
fn keep_downloaded_is_off_by_default_follows_the_collection_above_and_downloads() {
    let s = Setup::new();
    let archive = s.archive();
    let collections = archive.collections().unwrap();
    assert!(!cloud_files::keeps_downloaded(
        &collections,
        "Projects/Hardware"
    ));

    let lease = s.lease();
    assert!(
        cloud_files::set_keep_downloaded(&archive, "Projects", Some(true), &lease)
            .unwrap()
            .is_some()
    );
    let archive = s.archive();
    let collections = archive.collections().unwrap();
    assert!(cloud_files::keeps_downloaded(&collections, "Projects"));
    assert!(cloud_files::keeps_downloaded(
        &collections,
        "Projects/Hardware"
    ));
    assert!(!cloud_files::keeps_downloaded(&collections, "Reading"));
    let doc = std::fs::read_to_string(s.path("Projects/_collection.md")).unwrap();
    assert!(doc.contains("keep_downloaded: true"), "{doc}");
    // The same again changes nothing.
    assert!(
        cloud_files::set_keep_downloaded(&archive, "Projects", Some(true), &lease)
            .unwrap()
            .is_none()
    );

    let fake = Arc::new(FakeICloud::default());
    let kept = first_file_in(&s.path(FOLDER));
    fake.evict(&kept);
    fake.evict(&s.path(PDF));
    let pass = with_provider(fake.clone(), || keep_downloaded_pass(&archive).unwrap());
    assert_eq!(fake.downloads(), [kept], "Reading isn't kept downloaded");
    assert!(pass.collections.contains(&"Projects".to_owned()));
    assert_eq!(pass.failed.len(), 0);

    cloud_files::set_keep_downloaded(&archive, "Projects", None, &lease).unwrap();
    let collections = s.archive().collections().unwrap();
    assert!(!cloud_files::keeps_downloaded(&collections, "Projects"));
}

#[test]
fn sync_status_counts_inboxes_holding_and_rechecks() {
    let s = Setup::new();
    let archive = s.archive();
    let device = Id::new_v7();
    let inbox = carrel_core::inbox::folder(&archive, device);
    std::fs::create_dir_all(&inbox).unwrap();
    let sample = std::fs::read_dir(s.path("_System/inbox"))
        .unwrap()
        .flatten()
        .flat_map(|d| std::fs::read_dir(d.path()).unwrap().flatten())
        .map(|f| f.path())
        .next()
        .unwrap();
    for n in 0..2 {
        std::fs::copy(&sample, inbox.join(format!("2026-11-01T10-00-0{n}-x.json"))).unwrap();
    }
    std::fs::create_dir_all(s.path("_System/holding/a")).unwrap();
    std::fs::write(s.path("_System/holding/a/old.pdf"), vec![0u8; 2048]).unwrap();
    std::fs::create_dir_all(s.path("_System/tasks")).unwrap();
    std::fs::write(s.path("_System/tasks/recheck-1.json"), "{}").unwrap();
    let lease = s.lease();

    let status = carrel_core::sync_status::sync_status(&archive);
    assert_eq!(status.writer.unwrap().device, s.holder.device);
    assert_eq!(status.holding.files, 1);
    assert_eq!(status.holding.bytes, 2048);
    assert!(!status.holding.warn);
    assert_eq!(status.rechecks, 1);
    assert_eq!(status.conflicts.len(), 0);
    assert!(!status.synced);
    let mine = status
        .inboxes
        .iter()
        .find(|i| i.device == device.to_string())
        .unwrap();
    assert_eq!(mine.waiting, 2);
    let waiting: usize = status.inboxes.iter().map(|i| i.waiting).sum();
    assert_eq!(
        waiting, 3,
        "the sample's own entry counts, though this version can't read it"
    );
    drop(lease);
}

#[test]
fn an_archive_moves_into_icloud_drive_verified_and_the_old_copy_kept() {
    let s = Setup::new();
    let archive = s.archive();
    let target = tempfile::tempdir().unwrap();
    let to = target
        .path()
        .join("Library/Mobile Documents/com~apple~CloudDocs/Archive");
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    let config = tempfile::tempdir().unwrap();

    assert!(matches!(
        icloud_move::plan_move(&archive, &s.path("Reading/inside")),
        Err(MoveError::Inside(_))
    ));
    assert!(matches!(
        icloud_move::plan_move(&archive, target.path()),
        Err(MoveError::Exists(_))
    ));

    let plan = icloud_move::plan_move(&archive, &to).unwrap();
    assert!(plan.synced && plan.files > 10 && plan.bytes > 0);
    let lease = s.lease();
    let mut last = (0, 0);
    icloud_move::copy_archive(&plan, &lease, &mut |done, total| last = (done, total)).unwrap();
    assert_eq!(last, (plan.bytes, plan.bytes));
    assert!(!to.join(carrel_core::coordinator::LOCK_FILE).exists());
    drop(lease);

    icloud_move::switch_over(&plan, config.path()).unwrap();
    let prefs = carrel_core::local::Prefs::load_from(config.path()).unwrap();
    assert_eq!(prefs.archive.as_deref(), Some(plan.to.as_path()));
    assert_eq!(icloud_move::moved_to(&s.root), Some(plan.to.clone()));
    assert!(s.path(PDF).exists(), "the old copy stays");
    let moved = Archive::open(&plan.to).unwrap();
    assert_eq!(moved.items().unwrap().len(), archive.items().unwrap().len());
    assert_ne!(History::open(&moved).unwrap().log().unwrap().len(), 0);
}

#[test]
fn a_copy_that_differs_from_the_original_isnt_switched_to() {
    let s = Setup::new();
    let archive = s.archive();
    let target = tempfile::tempdir().unwrap();
    let to = target.path().join("Archive");
    let plan = icloud_move::plan_move(&archive, &to).unwrap();
    let lease = s.lease();
    let copied_pdf = to.join(PDF);
    let result = icloud_move::copy_archive(&plan, &lease, &mut |_, _| {
        if copied_pdf.exists() {
            std::fs::write(&copied_pdf, "damaged").unwrap();
        }
    });
    match result {
        Err(MoveError::Mismatch(files)) => assert_eq!(files, [PDF]),
        other => panic!("{other:?}"),
    }
}
