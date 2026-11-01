//! The writer lock for archives in iCloud Drive (`docs/design/sync-and-mobile.md`, "The writer
//! lock"): one record per archive in the owner's private `CloudKit` database, holding the
//! writing device, its lease's expiry and the history head it last committed. Never file
//! contents.
//!
//! The rules:
//!
//! - A device takes the lock only when the lease has expired or was released.
//! - Before writing, it waits until iCloud Drive has delivered the history head the record
//!   names, so it never writes on top of history it hasn't seen.
//! - It commits, then records the new head when it renews or releases the lock.
//!
//! The record is kept by a [`RemoteLock`]: `CloudKit` itself, which Swift provides (the Mac
//! agent and the iOS app), the agent reached over its socket by the app and the command line
//! on a Mac ([`super::agent_lock`]), or [`FakeCloudKit`] in tests. Within a device, the processes
//! still take turns through a disk lock, kept in `_System/cache.nosync/` so it never syncs.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::{CoordinatorError, LockInfo, timestamp};
use crate::archive::Archive;
use crate::card::Id;

/// The file saying an archive's writer is chosen through `CloudKit`, relative to the archive
/// root. Once it exists, a device that can't reach `CloudKit` doesn't write the archive.
pub const MARKER_FILE: &str = "_System/coordinator.json";

/// The disk lock that picks the process on this device, for an archive locked through
/// `CloudKit`: in `cache.nosync/`, which iCloud Drive doesn't sync.
pub const LOCAL_LOCK_FILE: &str = "_System/cache.nosync/writer.lock";

/// How long a new writer waits for iCloud Drive to deliver the recorded history head.
pub const HISTORY_WAIT: Duration = Duration::from_mins(2);

/// Whether `root` is in iCloud Drive: `~/Library/Mobile Documents` on a Mac, or the
/// `iCloudDrive` folder of iCloud for Windows.
#[must_use]
pub fn is_in_icloud_drive(root: &Path) -> bool {
    root.to_string_lossy()
        .replace('\\', "/")
        .contains("/Library/Mobile Documents/")
        || root
            .components()
            .any(|c| c.as_os_str().to_string_lossy() == "iCloudDrive")
}

/// Whether `root`'s writer is chosen through `CloudKit` ([`MARKER_FILE`]).
#[must_use]
pub fn uses_cloud_lock(root: &Path) -> bool {
    root.join(MARKER_FILE).is_file()
}

/// What the lock record holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockRecord {
    /// The lease: who holds it, for what, and until when (by the writer's clock).
    pub lease: LockInfo,
    /// The history commit the last writer recorded, which a new writer waits for.
    pub head: Option<String>,
}

/// A lock record as stored, with the store's version of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRecord {
    /// The record.
    pub record: LockRecord,
    /// The store's version of it (`CloudKit`'s record change tag): a save naming an older one
    /// fails.
    pub tag: String,
}

/// How a conditional save went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Saved {
    /// Saved, as this new version.
    Saved(String),
    /// Not saved: the record had changed. This is what it holds now, if anything.
    Changed(Option<Box<StoredRecord>>),
}

/// The record store couldn't be reached or refused the request, in its words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteError(pub String);

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RemoteError {}

/// Where lock records are kept: `CloudKit`'s private database, in Swift.
pub trait RemoteLock: Send + Sync {
    /// The record for `archive`, if there is one.
    ///
    /// # Errors
    ///
    /// [`RemoteError`] when the store can't be reached.
    fn fetch(&self, archive: Id) -> Result<Option<StoredRecord>, RemoteError>;

    /// Saves `record` for `archive`, only if the stored record's version is still `expected`
    /// (`None`: only if there's no record yet).
    ///
    /// # Errors
    ///
    /// [`RemoteError`] when the store can't be reached.
    fn save(
        &self,
        archive: Id,
        record: &LockRecord,
        expected: Option<&str>,
    ) -> Result<Saved, RemoteError>;
}

/// What time it is, for leases. Tests give one they can move forward.
pub type Clock = Arc<dyn Fn() -> jiff::Timestamp + Send + Sync>;

static REGISTERED: RwLock<Option<Arc<dyn RemoteLock>>> = RwLock::new(None);

/// Makes `remote` this process's lock record store, as the Mac agent and the iOS app do with
/// `CloudKit`; `None` stops using it.
pub fn set_remote_lock(remote: Option<Arc<dyn RemoteLock>>) {
    *REGISTERED.write().unwrap_or_else(PoisonError::into_inner) = remote;
}

/// This process's lock record store: the one set with [`set_remote_lock`], else, on a Mac,
/// the agent's while it's running.
#[must_use]
pub fn remote_lock() -> Option<Arc<dyn RemoteLock>> {
    if let Some(remote) = REGISTERED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
    {
        return Some(remote);
    }
    #[cfg(target_os = "macos")]
    if let Some(agent) = super::agent_lock::AgentLockClient::running() {
        return Some(Arc::new(agent));
    }
    None
}

/// The `CloudKit` side of an archive's writer lock.
#[derive(Clone)]
pub struct CloudLock {
    remote: Arc<dyn RemoteLock>,
    archive: Id,
    root: PathBuf,
    clock: Clock,
    wait: Duration,
    poll: Duration,
}

impl std::fmt::Debug for CloudLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudLock")
            .field("archive", &self.archive)
            .field("root", &self.root)
            .field("wait", &self.wait)
            .finish_non_exhaustive()
    }
}

impl CloudLock {
    /// The lock for `archive`, kept in `remote`.
    #[must_use]
    pub fn new(archive: &Archive, remote: Arc<dyn RemoteLock>) -> Self {
        Self {
            remote,
            archive: archive.doc().id,
            root: archive.root().to_path_buf(),
            clock: Arc::new(jiff::Timestamp::now),
            wait: HISTORY_WAIT,
            poll: Duration::from_secs(1),
        }
    }

    /// Reads leases' times from `clock` instead of the system's.
    #[must_use]
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    /// Waits up to `wait` for the recorded history head, looking every `poll`.
    #[must_use]
    pub fn with_history_wait(mut self, wait: Duration, poll: Duration) -> Self {
        self.wait = wait;
        self.poll = poll;
        self
    }

    fn now(&self) -> jiff::Timestamp {
        (self.clock)()
    }

    fn is_live(&self, lease: &LockInfo) -> bool {
        lease
            .expires
            .parse::<jiff::Timestamp>()
            .is_ok_and(|expires| expires > self.now())
    }

    fn fetch(&self) -> Result<Option<StoredRecord>, CoordinatorError> {
        self.remote.fetch(self.archive).map_err(unavailable)
    }

    /// The record's lease, if there is one (including one that has run out).
    ///
    /// # Errors
    ///
    /// [`CoordinatorError::Unavailable`] if the store can't be reached.
    pub fn current(&self) -> Result<Option<LockInfo>, CoordinatorError> {
        Ok(self.fetch()?.map(|stored| stored.record.lease))
    }

    /// Takes the lock for `local`'s holder, for `lease`, then waits for the history head.
    /// The holder's own device may take over a lease it left: the disk lock already makes
    /// sure no other process on the device is writing.
    pub(super) fn take(
        &self,
        local: &LockInfo,
        lease: Duration,
    ) -> Result<CloudHold, CoordinatorError> {
        let mut stored = self.fetch()?;
        for _ in 0..3 {
            if let Some(current) = &stored
                && current.record.lease.device != local.device
                && self.is_live(&current.record.lease)
            {
                return Err(CoordinatorError::Held(Box::new(
                    current.record.lease.clone(),
                )));
            }
            let now = self.now();
            let mut mine = local.clone();
            mine.acquired = timestamp(now);
            mine.expires = timestamp(now + lease);
            let record = LockRecord {
                lease: mine,
                head: stored.as_ref().and_then(|s| s.record.head.clone()),
            };
            let expected = stored.as_ref().map(|s| s.tag.as_str());
            match self
                .remote
                .save(self.archive, &record, expected)
                .map_err(unavailable)?
            {
                Saved::Saved(tag) => {
                    let mut hold = CloudHold {
                        lock: self.clone(),
                        record,
                        tag,
                    };
                    let ready = self.mark().and_then(|()| hold.wait_for_head());
                    if let Err(e) = ready {
                        let _ = hold.release();
                        return Err(e);
                    }
                    return Ok(hold);
                }
                Saved::Changed(now_stored) => stored = now_stored.map(|s| *s),
            }
        }
        Err(CoordinatorError::Unavailable(
            "the archive's lock record kept changing while taking it; try again".into(),
        ))
    }

    /// Writes [`MARKER_FILE`], once.
    fn mark(&self) -> Result<(), CoordinatorError> {
        let path = self.root.join(MARKER_FILE);
        if path.is_file() {
            return Ok(());
        }
        let text = serde_json::to_string_pretty(&serde_json::json!({ "coordinator": "cloudkit" }))
            .expect("JSON always serializes")
            + "\n";
        std::fs::write(&path, text).map_err(super::io(&path))
    }
}

fn unavailable(error: RemoteError) -> CoordinatorError {
    CoordinatorError::Unavailable(error.0)
}

/// The archive's history head on this device, if it has history.
fn history_head(root: &Path) -> Option<String> {
    let repo = git2::Repository::open(root.join(crate::history::HISTORY_DIR)).ok()?;
    let head = repo.head().ok()?.target()?;
    Some(head.to_string())
}

/// Whether this device's history has `want`: it's the head, or the head descends from it.
fn history_has(root: &Path, want: &str) -> bool {
    let Ok(want) = git2::Oid::from_str(want) else {
        return false;
    };
    let Ok(repo) = git2::Repository::open(root.join(crate::history::HISTORY_DIR)) else {
        return false;
    };
    let Some(head) = repo.head().ok().and_then(|h| h.target()) else {
        return false;
    };
    head == want || repo.graph_descendant_of(head, want).unwrap_or(false)
}

/// A lease's hold on the lock record, kept by its [`super::WriterLease`].
#[derive(Debug)]
pub(super) struct CloudHold {
    lock: CloudLock,
    record: LockRecord,
    tag: String,
}

impl CloudHold {
    /// Waits until this device's history has the recorded head.
    fn wait_for_head(&self) -> Result<(), CoordinatorError> {
        let Some(want) = &self.record.head else {
            return Ok(());
        };
        let started = Instant::now();
        loop {
            if history_has(&self.lock.root, want) {
                return Ok(());
            }
            let waited = started.elapsed();
            if waited >= self.lock.wait {
                return Err(CoordinatorError::HistoryBehind { head: want.clone() });
            }
            std::thread::sleep(self.lock.poll.min(self.lock.wait.saturating_sub(waited)));
        }
    }

    /// Whether the lease is still live by the lock's clock. Another device takes the lock
    /// only once it has run out, so this needs no round trip.
    pub(super) fn check(&self) -> Result<(), CoordinatorError> {
        if self.lock.is_live(&self.record.lease) {
            Ok(())
        } else {
            Err(CoordinatorError::LeaseLost)
        }
    }

    fn save(&mut self, record: LockRecord) -> Result<bool, CoordinatorError> {
        match self
            .lock
            .remote
            .save(self.lock.archive, &record, Some(&self.tag))
            .map_err(unavailable)?
        {
            Saved::Saved(tag) => {
                self.tag = tag;
                self.record = record;
                Ok(true)
            }
            Saved::Changed(_) => Ok(false),
        }
    }

    fn with_head(&self) -> LockRecord {
        let mut record = self.record.clone();
        record.head = history_head(&self.lock.root).or(record.head);
        record
    }

    /// Extends the lease to `lease` from now, recording the history head.
    pub(super) fn renew(&mut self, lease: Duration) -> Result<(), CoordinatorError> {
        self.check()?;
        let mut record = self.with_head();
        record.lease.expires = timestamp(self.lock.now() + lease);
        if self.save(record)? {
            Ok(())
        } else {
            Err(CoordinatorError::LeaseLost)
        }
    }

    /// Records the history head and ends the lease now. A lease another device has taken
    /// over is left alone.
    pub(super) fn release(&mut self) -> Result<(), CoordinatorError> {
        let mut record = self.with_head();
        record.lease.expires = timestamp(self.lock.now());
        self.save(record).map(|_| ())
    }
}

/// A lock record store in memory, standing for `CloudKit` in tests: several devices share
/// one, each through its own connection ([`FakeCloudKit::connection`]) over its own copy of
/// the archive. A connection can go offline, and the store has a clock that tests move
/// forward.
#[derive(Clone, Default)]
pub struct FakeCloudKit {
    state: Arc<Mutex<FakeState>>,
    offline: Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Default)]
struct FakeState {
    records: std::collections::HashMap<Id, StoredRecord>,
    versions: u64,
    ahead: jiff::SignedDuration,
}

impl std::fmt::Debug for FakeCloudKit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeCloudKit").finish_non_exhaustive()
    }
}

impl FakeCloudKit {
    /// An empty store, online, with the clock at the system's time.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The store's clock, for [`CloudLock::with_clock`].
    #[must_use]
    pub fn clock(&self) -> Clock {
        let state = Arc::clone(&self.state);
        Arc::new(move || {
            let ahead = state.lock().unwrap_or_else(PoisonError::into_inner).ahead;
            jiff::Timestamp::now() + ahead
        })
    }

    /// Moves the clock forward by `by`.
    ///
    /// # Panics
    ///
    /// If the clock would pass the year 9999.
    pub fn advance(&self, by: Duration) {
        let by = jiff::SignedDuration::try_from(by).expect("a test's duration fits");
        let mut state = self.state();
        state.ahead = state.ahead.checked_add(by).expect("a test's duration fits");
    }

    /// Another device's connection to the same store, online.
    #[must_use]
    pub fn connection(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            offline: Arc::default(),
        }
    }

    /// Takes this connection offline, or brings it back.
    pub fn set_offline(&self, offline: bool) {
        self.offline
            .store(offline, std::sync::atomic::Ordering::SeqCst);
    }

    fn check_online(&self) -> Result<(), RemoteError> {
        if self.offline.load(std::sync::atomic::Ordering::SeqCst) {
            Err(RemoteError("the network is offline".into()))
        } else {
            Ok(())
        }
    }

    /// The record for `archive`, as stored.
    #[must_use]
    pub fn record(&self, archive: Id) -> Option<LockRecord> {
        self.state().records.get(&archive).map(|s| s.record.clone())
    }

    /// A lock for `archive` through this connection, on the store's clock, waiting `wait`
    /// for history.
    #[must_use]
    pub fn lock(&self, archive: &Archive, wait: Duration) -> CloudLock {
        CloudLock::new(archive, Arc::new(self.clone()))
            .with_clock(self.clock())
            .with_history_wait(wait, Duration::from_millis(10))
    }
}

impl RemoteLock for FakeCloudKit {
    fn fetch(&self, archive: Id) -> Result<Option<StoredRecord>, RemoteError> {
        self.check_online()?;
        Ok(self.state().records.get(&archive).cloned())
    }

    fn save(
        &self,
        archive: Id,
        record: &LockRecord,
        expected: Option<&str>,
    ) -> Result<Saved, RemoteError> {
        self.check_online()?;
        let mut state = self.state();
        let current = state.records.get(&archive).cloned();
        if current.as_ref().map(|s| s.tag.as_str()) != expected {
            return Ok(Saved::Changed(current.map(Box::new)));
        }
        state.versions += 1;
        let tag = format!("v{}", state.versions);
        state.records.insert(
            archive,
            StoredRecord {
                record: record.clone(),
                tag: tag.clone(),
            },
        );
        Ok(Saved::Saved(tag))
    }
}
