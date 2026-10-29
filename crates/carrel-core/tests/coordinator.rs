//! The writer lock for archives on local and external disks (FORMAT.md §8.4).

use std::path::Path;
use std::sync::{Arc, Barrier};
use std::time::Duration;

use carrel_core::archive::{Archive, InitOptions};
use carrel_core::card::Id;
use carrel_core::coordinator::{
    Coordinator, CoordinatorError, DEFAULT_LEASE, DiskLock, Holder, LOCK_FILE, LockInfo,
};
use carrel_core::history::{Change, History, HistoryError, Operation};

fn holder() -> Holder {
    Holder {
        person: Id::new_v7(),
        device: Id::new_v7(),
    }
}

fn new_archive(dir: &Path) -> Archive {
    Archive::init(dir, &InitOptions::new("Lock test")).unwrap()
}

#[test]
fn one_writer_at_a_time_until_released() {
    let dir = tempfile::tempdir().unwrap();
    let archive = new_archive(dir.path());
    let lock = DiskLock::for_archive(&archive);
    assert_eq!(lock.current().unwrap(), None);

    let first = holder();
    let lease = lock.acquire(first, "ingest", DEFAULT_LEASE).unwrap();
    lease.check().unwrap();
    let on_disk = lock.current().unwrap().expect("a lock file");
    assert_eq!(&on_disk, lease.info());
    assert_eq!(
        (on_disk.person, on_disk.device),
        (first.person, first.device)
    );
    assert_eq!(on_disk.purpose, "ingest");
    assert!(on_disk.program.starts_with("carrel "));
    assert!(!on_disk.is_expired());

    match lock.acquire(holder(), "edit", DEFAULT_LEASE) {
        Err(CoordinatorError::Held(info)) => assert_eq!(info.token, lease.info().token),
        other => panic!("expected Held, got {other:?}"),
    }

    lease.release().unwrap();
    assert!(!dir.path().join(LOCK_FILE).exists());
    lock.acquire(holder(), "edit", DEFAULT_LEASE).unwrap();
}

#[test]
fn dropping_a_lease_releases_it() {
    let dir = tempfile::tempdir().unwrap();
    let lock = DiskLock::for_archive(&new_archive(dir.path()));
    drop(lock.acquire(holder(), "edit", DEFAULT_LEASE).unwrap());
    assert_eq!(lock.current().unwrap(), None);
}

#[test]
fn exactly_one_of_many_simultaneous_writers_wins() {
    let dir = tempfile::tempdir().unwrap();
    let lock = DiskLock::for_archive(&new_archive(dir.path()));
    let start = Arc::new(Barrier::new(16));
    let attempts: Vec<_> = (0..16)
        .map(|_| {
            let lock = lock.clone();
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                lock.acquire(holder(), "race", DEFAULT_LEASE)
            })
        })
        .collect();
    let results: Vec<_> = attempts.into_iter().map(|t| t.join().unwrap()).collect();
    let winners: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    assert_eq!(winners.len(), 1);
    assert!(
        results
            .iter()
            .filter_map(|r| r.as_ref().err())
            .all(|e| matches!(e, CoordinatorError::Held(_)))
    );
    assert_eq!(
        lock.current().unwrap().unwrap().token,
        winners[0].info().token
    );
}

#[test]
fn an_expired_lease_is_taken_over_and_its_holder_can_no_longer_commit() {
    let dir = tempfile::tempdir().unwrap();
    let archive = new_archive(dir.path());
    let lock = DiskLock::for_archive(&archive);
    let history = {
        let lease = lock.acquire(holder(), "create", DEFAULT_LEASE).unwrap();
        History::init(&archive, &lease).unwrap()
    };

    let stale = lock.acquire(holder(), "crashed", Duration::ZERO).unwrap();
    assert!(lock.current().unwrap().unwrap().is_expired());
    let fresh = lock.acquire(holder(), "edit", DEFAULT_LEASE).unwrap();
    assert_eq!(lock.current().unwrap().unwrap().token, fresh.info().token);
    assert!(matches!(stale.check(), Err(CoordinatorError::LeaseLost)));

    std::fs::write(
        dir.path().join("_System/glossary.md"),
        "# Glossary\n\nEdited.\n",
    )
    .unwrap();
    let info = stale.info().clone();
    let change = Change::new(Operation::Edit, "Too late", info.person, info.device);
    assert!(matches!(
        history.commit(&archive, &change, &stale),
        Err(HistoryError::Writer(CoordinatorError::LeaseLost))
    ));
    assert_eq!(history.log().unwrap().len(), 1, "nothing committed");

    drop(stale);
    assert_eq!(
        lock.current().unwrap().unwrap().token,
        fresh.info().token,
        "a lost lease never removes its successor's lock"
    );
    let change = Change::new(
        Operation::Edit,
        "In time",
        fresh.info().person,
        fresh.info().device,
    );
    assert!(history.commit(&archive, &change, &fresh).unwrap().is_some());
}

#[test]
fn renewing_extends_the_lease() {
    let dir = tempfile::tempdir().unwrap();
    let lock = DiskLock::for_archive(&new_archive(dir.path()));
    let mut expired = lock.acquire(holder(), "ingest", Duration::ZERO).unwrap();
    assert!(matches!(
        expired.renew(DEFAULT_LEASE),
        Err(CoordinatorError::LeaseLost)
    ));
    drop(expired);

    let mut lease = lock
        .acquire(holder(), "ingest", Duration::from_secs(1))
        .unwrap();
    let before = lease.info().expires.clone();
    lease.renew(DEFAULT_LEASE).unwrap();
    let on_disk = lock.current().unwrap().unwrap();
    assert_eq!(&on_disk, lease.info());
    assert_ne!(on_disk.expires, before);
    lease.check().unwrap();
    assert!(matches!(
        lock.acquire(holder(), "other", DEFAULT_LEASE),
        Err(CoordinatorError::Held(_))
    ));

    let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("_System"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("writer.lock."))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn a_corrupt_lock_is_only_removed_by_force() {
    let dir = tempfile::tempdir().unwrap();
    let lock = DiskLock::for_archive(&new_archive(dir.path()));
    std::fs::write(dir.path().join(LOCK_FILE), "not json").unwrap();
    assert!(matches!(lock.current(), Err(CoordinatorError::Corrupt(_))));
    assert!(matches!(
        lock.acquire(holder(), "edit", DEFAULT_LEASE),
        Err(CoordinatorError::Corrupt(_))
    ));
    assert_eq!(lock.force_break().unwrap(), None);
    lock.acquire(holder(), "edit", DEFAULT_LEASE).unwrap();
}

#[test]
fn force_break_returns_what_was_held() {
    let dir = tempfile::tempdir().unwrap();
    let lock = DiskLock::for_archive(&new_archive(dir.path()));
    let lease = lock.acquire(holder(), "edit", DEFAULT_LEASE).unwrap();
    let broken: Option<LockInfo> = lock.force_break().unwrap();
    assert_eq!(broken.as_ref(), Some(lease.info()));
    assert!(matches!(lease.check(), Err(CoordinatorError::LeaseLost)));
    assert_eq!(lock.force_break().unwrap(), None);
}

#[test]
fn newer_format_archives_cannot_be_locked_for_writing() {
    let dir = tempfile::tempdir().unwrap();
    new_archive(dir.path());
    let path = dir.path().join("_System/archive.json");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace("\"format\": \"1.0\"", "\"format\": \"2.0\""),
    )
    .unwrap();
    let lock = DiskLock::for_archive(&Archive::open(dir.path()).unwrap());
    assert!(matches!(
        lock.acquire(holder(), "edit", DEFAULT_LEASE),
        Err(CoordinatorError::ReadOnly)
    ));
    assert!(!dir.path().join(LOCK_FILE).exists());
}

#[test]
fn synced_folders_are_recognized() {
    assert!(DiskLock::is_synced_folder(Path::new(
        "/Users/me/Library/Mobile Documents/com~apple~CloudDocs/Archive"
    )));
    assert!(DiskLock::is_synced_folder(Path::new(
        "/Users/me/Library/CloudStorage/Dropbox/Archive"
    )));
    assert!(!DiskLock::is_synced_folder(Path::new(
        "/Volumes/Backup/Archive"
    )));
    assert!(!DiskLock::is_synced_folder(Path::new(
        "/Users/me/Documents/Archive"
    )));
}
