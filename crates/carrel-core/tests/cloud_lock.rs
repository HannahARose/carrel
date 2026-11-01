//! The `CloudKit` writer lock for archives in iCloud Drive (`docs/design/sync-and-mobile.md`,
//! "The writer lock"), with a fake `CloudKit` shared by devices that each have their own copy
//! of the archive. "iCloud Drive delivering" is copying one device's archive to another's.

use std::path::Path;
use std::time::Duration;

use carrel_core::archive::{Archive, InitOptions};
use carrel_core::card::Id;
use carrel_core::coordinator::cloud::{LOCAL_LOCK_FILE, MARKER_FILE};
use carrel_core::coordinator::{
    Coordinator, CoordinatorError, DEFAULT_LEASE, DiskLock, FakeCloudKit, Holder, LOCK_FILE,
    WriterLease,
};
use carrel_core::history::{Change, History, Operation};

const DAY: Duration = Duration::from_hours(24);

/// A device: its identity, its copy of the archive and its connection to the fake.
struct Device {
    holder: Holder,
    dir: tempfile::TempDir,
    cloud: FakeCloudKit,
}

impl Device {
    fn new(cloud: &FakeCloudKit) -> Self {
        Self {
            holder: Holder {
                person: Id::new_v7(),
                device: Id::new_v7(),
            },
            dir: tempfile::tempdir().unwrap(),
            cloud: cloud.connection(),
        }
    }

    fn archive(&self) -> Archive {
        Archive::open(self.dir.path()).unwrap()
    }

    fn lock(&self, wait: Duration) -> DiskLock {
        let archive = self.archive();
        DiskLock::with_cloud(&archive, self.cloud.lock(&archive, wait))
    }

    fn acquire(&self, wait: Duration) -> Result<WriterLease, CoordinatorError> {
        self.lock(wait).acquire(self.holder, "edit", DEFAULT_LEASE)
    }

    /// Edits the glossary and records it in history, returning the commit.
    fn commit(&self, lease: &WriterLease, text: &str) -> String {
        let archive = self.archive();
        std::fs::write(
            self.dir.path().join("_System/glossary.md"),
            format!("# Glossary\n\n{text}\n"),
        )
        .unwrap();
        let change = Change::new(
            Operation::Edit,
            text,
            self.holder.person,
            self.holder.device,
        );
        History::open(&archive)
            .unwrap()
            .commit(&archive, &change, lease)
            .unwrap()
            .expect("a commit")
    }

    fn head(&self) -> String {
        History::open(&self.archive()).unwrap().log().unwrap()[0]
            .id
            .clone()
    }

    /// iCloud Drive delivers `from`'s archive here: everything but `cache.nosync`.
    fn receive(&self, from: &Self) {
        copy_synced(from.dir.path(), self.dir.path());
    }
}

fn copy_synced(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        if entry.file_name() == "cache.nosync" {
            continue;
        }
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_synced(&source, &target);
        } else {
            // Git's objects are read-only.
            let _ = std::fs::remove_file(&target);
            std::fs::copy(&source, &target).unwrap();
        }
    }
}

/// Two devices with the same archive, created on the first with history.
fn two_devices() -> (FakeCloudKit, Device, Device) {
    let cloud = FakeCloudKit::new();
    let (mac, ipad) = (Device::new(&cloud), Device::new(&cloud));
    let archive = Archive::init(mac.dir.path(), &InitOptions::new("Shared")).unwrap();
    let lease = mac.acquire(Duration::ZERO).unwrap();
    History::init(&archive, &lease).unwrap();
    lease.release().unwrap();
    ipad.receive(&mac);
    (cloud, mac, ipad)
}

fn archive_id(device: &Device) -> Id {
    device.archive().doc().id
}

#[test]
fn two_devices_take_turns_and_each_waits_for_the_others_history() {
    let (cloud, mac, ipad) = two_devices();

    let lease = mac.acquire(Duration::ZERO).unwrap();
    let first = mac.commit(&lease, "From the Mac.");
    match ipad.acquire(Duration::ZERO) {
        Err(CoordinatorError::Held(info)) => assert_eq!(info.device, mac.holder.device),
        other => panic!("expected Held, got {other:?}"),
    }
    lease.release().unwrap();
    assert_eq!(
        cloud.record(archive_id(&mac)).unwrap().head.as_deref(),
        Some(first.as_str()),
        "releasing records the head"
    );

    // The iPad hasn't got the Mac's commit yet, so it mustn't write.
    match ipad.acquire(Duration::from_millis(50)) {
        Err(CoordinatorError::HistoryBehind { head }) => assert_eq!(head, first),
        other => panic!("expected HistoryBehind, got {other:?}"),
    }
    assert!(
        cloud
            .record(archive_id(&mac))
            .unwrap()
            .lease
            .expires
            .parse::<jiff::Timestamp>()
            .unwrap()
            <= jiff::Timestamp::now(),
        "a device that gives up waiting releases the lock again"
    );
    ipad.receive(&mac);
    let lease = ipad.acquire(Duration::ZERO).unwrap();
    let second = ipad.commit(&lease, "From the iPad.");
    lease.release().unwrap();

    assert!(matches!(
        mac.acquire(Duration::ZERO),
        Err(CoordinatorError::HistoryBehind { .. })
    ));
    mac.receive(&ipad);
    let lease = mac.acquire(Duration::ZERO).unwrap();
    assert_eq!(mac.head(), second);
    lease.release().unwrap();
}

#[test]
fn a_new_writer_waits_until_icloud_delivers_the_history_head() {
    let (_cloud, mac, ipad) = two_devices();
    let lease = mac.acquire(Duration::ZERO).unwrap();
    let head = mac.commit(&lease, "From the Mac.");
    lease.release().unwrap();

    std::thread::scope(|scope| {
        let waiting = scope.spawn(|| ipad.acquire(Duration::from_secs(20)));
        std::thread::sleep(Duration::from_millis(100));
        assert!(!waiting.is_finished(), "it waits for the head");
        ipad.receive(&mac);
        let lease = waiting.join().unwrap().unwrap();
        assert_eq!(ipad.head(), head);
        lease.release().unwrap();
    });
}

#[test]
fn an_expired_lease_is_taken_over_and_its_holder_loses_it() {
    let (cloud, mac, ipad) = two_devices();
    let mut stale = mac.acquire(Duration::ZERO).unwrap();
    assert!(matches!(
        ipad.acquire(Duration::ZERO),
        Err(CoordinatorError::Held(_))
    ));

    cloud.advance(DEFAULT_LEASE + Duration::from_secs(1));
    let fresh = ipad.acquire(Duration::ZERO).unwrap();
    assert_eq!(
        cloud.record(archive_id(&mac)).unwrap().lease.token,
        fresh.info().token
    );
    assert!(matches!(stale.check(), Err(CoordinatorError::LeaseLost)));
    assert!(matches!(
        stale.renew(DEFAULT_LEASE),
        Err(CoordinatorError::LeaseLost)
    ));
    drop(stale);
    assert_eq!(
        cloud.record(archive_id(&mac)).unwrap().lease.token,
        fresh.info().token,
        "a lost lease never releases its successor's"
    );
    assert!(
        !mac.dir.path().join(LOCAL_LOCK_FILE).exists(),
        "its disk lock is released"
    );
}

#[test]
fn a_device_offline_for_days_neither_blocks_others_nor_writes_on_return() {
    let (cloud, mac, ipad) = two_devices();
    let away = ipad.acquire(Duration::ZERO).unwrap();
    ipad.cloud.set_offline(true);

    cloud.advance(3 * DAY);
    let lease = mac.acquire(Duration::ZERO).unwrap();
    let head = mac.commit(&lease, "While the iPad was away.");
    lease.release().unwrap();

    // Back online: its old lease is gone, and it waits for the Mac's history.
    ipad.cloud.set_offline(false);
    assert!(matches!(away.check(), Err(CoordinatorError::LeaseLost)));
    drop(away);
    match ipad.acquire(Duration::ZERO) {
        Err(CoordinatorError::HistoryBehind { head: wanted }) => assert_eq!(wanted, head),
        other => panic!("expected HistoryBehind, got {other:?}"),
    }
    ipad.receive(&mac);
    ipad.acquire(Duration::ZERO).unwrap().release().unwrap();
}

#[test]
fn offline_a_device_cant_take_the_lock() {
    let (_cloud, mac, _ipad) = two_devices();
    mac.cloud.set_offline(true);
    assert!(matches!(
        mac.acquire(Duration::ZERO),
        Err(CoordinatorError::Unavailable(_))
    ));
    assert!(
        !mac.dir.path().join(LOCAL_LOCK_FILE).exists(),
        "the disk lock isn't kept"
    );
}

#[test]
fn renewing_records_the_head_and_releasing_frees_the_lock_at_once() {
    let (cloud, mac, ipad) = two_devices();
    let id = archive_id(&mac);
    let mut lease = mac.acquire(Duration::ZERO).unwrap();
    let before = cloud.record(id).unwrap();
    let head = mac.commit(&lease, "Long work, part one.");
    lease.renew(Duration::from_mins(30)).unwrap();
    let renewed = cloud.record(id).unwrap();
    assert_eq!(renewed.head.as_deref(), Some(head.as_str()));
    assert!(
        renewed.lease.expires.parse::<jiff::Timestamp>().unwrap()
            > before.lease.expires.parse::<jiff::Timestamp>().unwrap()
    );
    cloud.advance(DEFAULT_LEASE * 2);
    lease.check().unwrap();

    lease.release().unwrap();
    ipad.receive(&mac);
    let lease = ipad.acquire(Duration::ZERO).unwrap();
    assert_eq!(
        lease.info().device,
        ipad.holder.device,
        "no wait for expiry"
    );
}

#[test]
fn one_process_per_device_through_a_lock_file_that_doesnt_sync() {
    let (_cloud, mac, _ipad) = two_devices();
    let lease = mac.acquire(Duration::ZERO).unwrap();
    assert!(mac.dir.path().join(LOCAL_LOCK_FILE).is_file());
    assert!(!mac.dir.path().join(LOCK_FILE).exists());
    let other_process = Holder {
        person: mac.holder.person,
        device: mac.holder.device,
    };
    assert!(matches!(
        mac.lock(Duration::ZERO)
            .acquire(other_process, "index", DEFAULT_LEASE),
        Err(CoordinatorError::Held(_))
    ));
    lease.release().unwrap();
}

#[test]
fn the_current_writer_is_the_records() {
    let (_cloud, mac, ipad) = two_devices();
    let lease = mac.acquire(Duration::ZERO).unwrap();
    let current = ipad.lock(Duration::ZERO).current().unwrap().unwrap();
    assert_eq!(current.token, lease.info().token);
}

/// An archive in a folder named like iCloud Drive's.
fn in_icloud_drive() -> (tempfile::TempDir, Archive) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir
        .path()
        .join("Library/Mobile Documents/com~apple~CloudDocs/Archive");
    std::fs::create_dir_all(&root).unwrap();
    let archive = Archive::init(&root, &InitOptions::new("In iCloud")).unwrap();
    (dir, archive)
}

#[test]
fn without_cloudkit_an_archive_that_never_used_it_keeps_its_lock_file() {
    let (_dir, archive) = in_icloud_drive();
    let lock = DiskLock::for_archive(&archive);
    assert!(!lock.uses_cloud());
    let holder = Holder {
        person: Id::new_v7(),
        device: Id::new_v7(),
    };
    let lease = lock.acquire(holder, "edit", DEFAULT_LEASE).unwrap();
    assert!(archive.root().join(LOCK_FILE).is_file());
    lease.release().unwrap();
}

#[test]
fn without_cloudkit_an_archive_that_uses_it_isnt_written() {
    let (_dir, archive) = in_icloud_drive();
    let holder = Holder {
        person: Id::new_v7(),
        device: Id::new_v7(),
    };
    let cloud = FakeCloudKit::new();
    DiskLock::with_cloud(&archive, cloud.lock(&archive, Duration::ZERO))
        .acquire(holder, "edit", DEFAULT_LEASE)
        .unwrap()
        .release()
        .unwrap();
    assert!(archive.root().join(MARKER_FILE).is_file());

    match DiskLock::for_archive(&archive).acquire(holder, "edit", DEFAULT_LEASE) {
        Err(e @ CoordinatorError::Unavailable(_)) => {
            assert!(e.to_string().contains("inbox"), "{e}");
        }
        other => panic!("expected Unavailable, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn the_app_and_command_line_go_through_the_agents_socket() {
    use std::sync::Arc;

    use carrel_core::coordinator::CloudLock;
    use carrel_core::coordinator::agent_lock::{AgentLockClient, AgentLockService};

    let (cloud, mac, _ipad) = two_devices();
    let sockets = tempfile::tempdir().unwrap();
    let path = sockets.path().join("lock.sock");
    let service = AgentLockService::start(&path, Arc::new(cloud.clone())).unwrap();
    assert!(
        AgentLockService::start(&path, Arc::new(cloud.clone())).is_err(),
        "one agent serves it"
    );

    let archive = mac.archive();
    let through_agent = CloudLock::new(&archive, Arc::new(AgentLockClient::new(path.clone())))
        .with_clock(cloud.clock())
        .with_history_wait(Duration::ZERO, Duration::ZERO);
    let lease = DiskLock::with_cloud(&archive, through_agent)
        .acquire(mac.holder, "edit", DEFAULT_LEASE)
        .unwrap();
    assert_eq!(
        cloud.record(archive.doc().id).unwrap().lease.token,
        lease.info().token
    );
    let head = mac.commit(&lease, "Through the agent.");
    lease.release().unwrap();
    assert_eq!(
        cloud.record(archive.doc().id).unwrap().head.as_deref(),
        Some(head.as_str())
    );

    drop(service);
    assert!(!path.exists());
    let gone = CloudLock::new(&archive, Arc::new(AgentLockClient::new(path)));
    assert!(matches!(
        DiskLock::with_cloud(&archive, gone).acquire(mac.holder, "edit", DEFAULT_LEASE),
        Err(CoordinatorError::Unavailable(_))
    ));
}
