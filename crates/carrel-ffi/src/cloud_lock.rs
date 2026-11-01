//! The `CloudKit` writer lock's store, implemented in Swift (`docs/design/sync-and-mobile.md`,
//! "The writer lock"). The Mac agent serves it to the app and the command line
//! ([`serve_cloud_lock`]); the iOS app uses it directly ([`use_cloud_lock`]). Archives in
//! iCloud Drive then take their writer lock through it whenever they're written.

use std::sync::{Arc, Mutex, PoisonError};

use carrel_core::card::Id;
#[cfg(unix)]
use carrel_core::coordinator::agent_lock::{AgentLockService, socket_path};
use carrel_core::coordinator::cloud::{
    LockRecord, RemoteError, RemoteLock, Saved, StoredRecord, set_remote_lock,
};
use carrel_core::coordinator::{LockInfo, cloud};

use crate::CarrelError;

/// An archive's lock record, as `CloudKit` keeps it: who holds the lease and until when, and
/// the history head. Never file contents.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CloudLockRecord {
    /// The lease's random identifier.
    pub token: String,
    /// The person holding it.
    pub person: String,
    /// The device holding it.
    pub device: String,
    /// What the writer is doing, such as `ingest`.
    pub purpose: String,
    /// The program, such as `carrel 0.8.0`.
    pub program: String,
    /// When the lease was taken (RFC 3339).
    pub acquired: String,
    /// When it runs out (RFC 3339); released leases have run out.
    pub expires: String,
    /// The history commit the last writer recorded.
    pub head: Option<String>,
    /// The store's version of the record (its change tag); empty when saving.
    pub tag: String,
}

/// How a conditional save went.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum CloudLockSave {
    /// Saved, as this version.
    Saved {
        /// The record's new change tag.
        tag: String,
    },
    /// Not saved, because the record had changed: what it holds now, if anything.
    Changed {
        /// The record as stored now.
        current: Option<CloudLockRecord>,
    },
}

/// Where lock records are kept: one per archive in the owner's private `CloudKit` database,
/// implemented in Swift. Called from a background thread, and may block on the network.
#[uniffi::export(with_foreign)]
pub trait CloudLockStore: Send + Sync {
    /// The record for the archive `archive_id`, if there is one.
    ///
    /// # Errors
    ///
    /// When `CloudKit` can't be reached, as [`CarrelError::Callback`].
    fn fetch(&self, archive_id: String) -> Result<Option<CloudLockRecord>, CarrelError>;

    /// Saves `record` for the archive, only if the stored record's change tag is still
    /// `expected_tag` (`None`: only if there's no record yet).
    ///
    /// # Errors
    ///
    /// When `CloudKit` can't be reached, as [`CarrelError::Callback`].
    fn save(
        &self,
        archive_id: String,
        record: CloudLockRecord,
        expected_tag: Option<String>,
    ) -> Result<CloudLockSave, CarrelError>;
}

fn to_ffi(stored: StoredRecord) -> CloudLockRecord {
    let lease = stored.record.lease;
    CloudLockRecord {
        token: lease.token.to_string(),
        person: lease.person.to_string(),
        device: lease.device.to_string(),
        purpose: lease.purpose,
        program: lease.program,
        acquired: lease.acquired,
        expires: lease.expires,
        head: stored.record.head,
        tag: stored.tag,
    }
}

fn from_ffi(record: CloudLockRecord) -> Result<StoredRecord, RemoteError> {
    let id = |text: &str| {
        text.parse()
            .map(Id)
            .map_err(|_| RemoteError(format!("the lock record has a bad identifier, {text}")))
    };
    Ok(StoredRecord {
        record: LockRecord {
            lease: LockInfo {
                token: id(&record.token)?,
                person: id(&record.person)?,
                device: id(&record.device)?,
                purpose: record.purpose,
                program: record.program,
                acquired: record.acquired,
                expires: record.expires,
                extra: carrel_core::card::Extra::new(),
            },
            head: record.head,
        },
        tag: record.tag,
    })
}

fn remote(error: CarrelError) -> RemoteError {
    RemoteError(error.to_string())
}

/// A [`CloudLockStore`] as the core's [`RemoteLock`].
struct FromSwift(Arc<dyn CloudLockStore>);

impl RemoteLock for FromSwift {
    fn fetch(&self, archive: Id) -> Result<Option<StoredRecord>, RemoteError> {
        self.0
            .fetch(archive.to_string())
            .map_err(remote)?
            .map(from_ffi)
            .transpose()
    }

    fn save(
        &self,
        archive: Id,
        record: &LockRecord,
        expected: Option<&str>,
    ) -> Result<Saved, RemoteError> {
        let record = to_ffi(StoredRecord {
            record: record.clone(),
            tag: String::new(),
        });
        match self
            .0
            .save(archive.to_string(), record, expected.map(str::to_owned))
            .map_err(remote)?
        {
            CloudLockSave::Saved { tag } => Ok(Saved::Saved(tag)),
            CloudLockSave::Changed { current } => Ok(Saved::Changed(
                current.map(from_ffi).transpose()?.map(Box::new),
            )),
        }
    }
}

/// Takes archives' writer locks in iCloud Drive through `store`, in this process, as the iOS
/// app does.
#[uniffi::export]
pub fn use_cloud_lock(store: Arc<dyn CloudLockStore>) {
    set_remote_lock(Some(Arc::new(FromSwift(store))));
}

/// Stops taking writer locks through the store given to [`use_cloud_lock`].
#[uniffi::export]
pub fn stop_using_cloud_lock() {
    set_remote_lock(None);
}

/// The Mac agent's lock service, serving the app and the command line until stopped.
#[cfg(unix)]
#[derive(uniffi::Object)]
pub struct CloudLockService {
    service: Mutex<Option<AgentLockService>>,
}

#[cfg(unix)]
#[uniffi::export]
impl CloudLockService {
    /// Stops serving, and stops using the store in this process.
    pub fn stop(&self) {
        self.service
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        set_remote_lock(None);
    }

    /// The socket the app and the command line reach it at.
    #[must_use]
    pub fn socket(&self) -> Option<String> {
        self.service
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|s| s.path().to_string_lossy().into_owned())
    }
}

/// Uses `store` in this process and serves it to the app and the command line on this Mac,
/// as the agent does. `socket` is where, by default the configuration folder's
/// `cloud-lock.sock`; tests give another, so they never write the real configuration.
///
/// # Errors
///
/// [`CarrelError::Failed`] if another agent is serving, or the socket can't be made.
#[cfg(unix)]
#[uniffi::export(default(socket = None))]
pub fn serve_cloud_lock(
    store: Arc<dyn CloudLockStore>,
    socket: Option<String>,
) -> Result<Arc<CloudLockService>, CarrelError> {
    let path = socket
        .map(std::path::PathBuf::from)
        .or_else(socket_path)
        .ok_or_else(|| CarrelError::failed("there's no configuration folder for the socket"))?;
    let remote: Arc<dyn RemoteLock> = Arc::new(FromSwift(store));
    let service = AgentLockService::start(&path, Arc::clone(&remote)).map_err(|e| {
        CarrelError::failed(format!(
            "couldn't serve the writer lock at {}: {e}",
            path.display()
        ))
    })?;
    set_remote_lock(Some(remote));
    Ok(Arc::new(CloudLockService {
        service: Mutex::new(Some(service)),
    }))
}

/// Whether the archive at `root` is in iCloud Drive, so its writer is chosen through
/// `CloudKit`.
#[uniffi::export]
#[must_use]
pub fn is_in_icloud_drive(root: String) -> bool {
    cloud::is_in_icloud_drive(std::path::Path::new(&root))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store in memory, as Swift's would be.
    #[derive(Default)]
    struct Memory(Mutex<Option<CloudLockRecord>>);

    impl CloudLockStore for Memory {
        fn fetch(&self, _: String) -> Result<Option<CloudLockRecord>, CarrelError> {
            Ok(self.0.lock().unwrap().clone())
        }

        fn save(
            &self,
            _: String,
            mut record: CloudLockRecord,
            expected_tag: Option<String>,
        ) -> Result<CloudLockSave, CarrelError> {
            let mut stored = self.0.lock().unwrap();
            if stored.as_ref().map(|r| r.tag.clone()) != expected_tag {
                return Ok(CloudLockSave::Changed {
                    current: stored.clone(),
                });
            }
            record.tag = format!("{}x", expected_tag.unwrap_or_default());
            let tag = record.tag.clone();
            *stored = Some(record);
            Ok(CloudLockSave::Saved { tag })
        }
    }

    #[test]
    fn a_swift_store_keeps_the_lock_record() {
        use carrel_core::archive::{Archive, InitOptions};
        use carrel_core::coordinator::{
            CloudLock, Coordinator as _, DEFAULT_LEASE, DiskLock, Holder,
        };

        let dir = tempfile::tempdir().unwrap();
        let archive = Archive::init(dir.path(), &InitOptions::new("Swift")).unwrap();
        let store = Arc::new(Memory::default());
        let lock = DiskLock::with_cloud(
            &archive,
            CloudLock::new(&archive, Arc::new(FromSwift(store.clone()))),
        );
        let holder = Holder {
            person: Id::new_v7(),
            device: Id::new_v7(),
        };
        let lease = lock.acquire(holder, "edit", DEFAULT_LEASE).unwrap();
        let record = store.0.lock().unwrap().clone().unwrap();
        assert_eq!(record.device, holder.device.to_string());
        assert_eq!(record.token, lease.info().token.to_string());
        lease.release().unwrap();
        let other = Holder {
            person: Id::new_v7(),
            device: Id::new_v7(),
        };
        lock.acquire(other, "edit", DEFAULT_LEASE).unwrap();
    }
}
