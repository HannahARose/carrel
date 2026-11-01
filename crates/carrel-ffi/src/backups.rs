//! Backups, as `carrel backup` and the interactive mode's Backups screen do them
//! (`docs/design/backups.md`): setting up a repository, the snapshots from history, taking one
//! now, verifying, and restoring an item's contents beside the archive.
//!
//! Kopia keeps the repository's password in this Mac's keychain once setup has it. A caller
//! that keeps it itself instead, as tests do so they never write the keychain, sets up with
//! `keep_password` off and passes the password to each call, as `KOPIA_PASSWORD` does for the
//! CLI. Kopia's configuration is kept in the handle's configuration folder.

use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::backup::{
    BackupError, default_description, is_repository, record_setup, restore_beside, restore_point,
    snapshot_entries, snapshot_now,
};
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::history::History;

use crate::history::Change;
use crate::{ArchiveHandle, CarrelError, path, text};

/// Where this archive's backups stand on this device.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BackupsState {
    /// Whether the `kopia` program can be found.
    pub installed: bool,
    /// The repository's description from the archive's settings, once any device has set up
    /// backups, such as "Backup drive".
    pub description: Option<String>,
    /// Whether this device is connected to the repository.
    pub set_up_here: bool,
    /// The repository's folder on this device.
    pub repository: Option<String>,
    /// Whether that folder can be reached now: `false` when its drive isn't connected.
    pub reachable: bool,
    /// The changes that took a snapshot, newest first, from history, so they're listed without
    /// the drive.
    pub snapshots: Vec<Change>,
}

/// What setting up backups did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BackupsSetUp {
    /// Whether a new repository was created, rather than one already there connected.
    pub created: bool,
    /// The repository's description, recorded in the archive's settings.
    pub description: String,
    /// Whether the repository is on the same disk as the archive, which guards against
    /// mistakes but not against losing the disk.
    pub same_disk: bool,
    /// Whether Kopia keeps the password in this Mac's keychain.
    pub kept_in_keychain: bool,
    /// The history commit recording it.
    pub commit: Option<String>,
}

/// A snapshot taken on request.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SnapshotTaken {
    /// The snapshot's identifier in the backup system.
    pub snapshot: String,
    /// The history commit it's recorded with.
    pub commit: Option<String>,
    /// How many files it holds, when known.
    pub files: Option<u64>,
    /// How many bytes it holds, when known.
    pub size: Option<u64>,
}

/// An item's contents restored beside the archive.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Restored {
    /// The item's identifier.
    pub item: String,
    /// Its path in the archive.
    pub path: String,
    /// The folder or file it was restored to.
    pub to: String,
    /// The change whose snapshot it came from.
    pub change: Change,
}

impl From<BackupError> for CarrelError {
    fn from(error: BackupError) -> Self {
        match error {
            BackupError::NotSetUp | BackupError::Refused(_) | BackupError::NoSnapshot(_) => {
                Self::invalid(error)
            }
            error => Self::failed(error),
        }
    }
}

#[cfg(not(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
)))]
mod kopia {
    use std::path::{Path, PathBuf};

    use carrel_core::archive::Archive;
    use carrel_core::backup::{BackupError, Backups, Kopia, Setup};
    use carrel_core::card::Id;

    pub fn installed() -> bool {
        Kopia::program().is_some()
    }

    /// The repository's folder, when this device is set up with `config`.
    pub fn repository(archive: &Archive, config: &Path) -> Option<PathBuf> {
        Kopia::open(archive, config.to_owned())
            .ok()?
            .repository_path()
    }

    pub fn open(
        archive: &Archive,
        config: PathBuf,
        password: Option<String>,
    ) -> Result<Box<dyn Backups>, BackupError> {
        let kopia = Kopia::open(archive, config)?;
        Ok(Box::new(match password {
            Some(password) => kopia.with_password(password),
            None => kopia,
        }))
    }

    pub fn setup(
        archive: &Archive,
        config: PathBuf,
        repository: &Path,
        password: &str,
        keep: bool,
        device: Id,
    ) -> Result<Setup, BackupError> {
        Kopia::setup(archive, config, repository, password, keep, device).map(|(_, setup)| setup)
    }
}

/// No Kopia away from a Mac.
#[cfg(any(
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos",
    target_os = "visionos"
))]
mod kopia {
    use std::path::{Path, PathBuf};

    use carrel_core::archive::Archive;
    use carrel_core::backup::{BackupError, Backups, Setup};
    use carrel_core::card::Id;

    pub fn installed() -> bool {
        false
    }

    pub fn repository(_archive: &Archive, _config: &Path) -> Option<PathBuf> {
        None
    }

    pub fn open(
        _archive: &Archive,
        _config: PathBuf,
        _password: Option<String>,
    ) -> Result<Box<dyn Backups>, BackupError> {
        Err(BackupError::NotInstalled)
    }

    pub fn setup(
        _archive: &Archive,
        _config: PathBuf,
        _repository: &Path,
        _password: &str,
        _keep: bool,
        _device: Id,
    ) -> Result<Setup, BackupError> {
        Err(BackupError::NotInstalled)
    }
}

impl ArchiveHandle {
    /// Where Kopia's configuration for `archive` is kept in this handle's configuration folder.
    fn kopia_config(&self, archive: &Archive) -> Result<PathBuf, CarrelError> {
        Ok(self
            .config_dir()?
            .join("backups")
            .join(format!("{}.config", archive.info().id)))
    }

    /// This device's backups for `archive`, checked to be reachable, so that a missing drive
    /// is said plainly before Kopia is run.
    fn kopia_here(
        &self,
        archive: &Archive,
        password: Option<String>,
    ) -> Result<Box<dyn carrel_core::backup::Backups>, CarrelError> {
        let config = self.kopia_config(archive)?;
        if !config.is_file() {
            return Err(CarrelError::invalid(
                "backups aren't set up for this archive on this Mac",
            ));
        }
        if let Some(folder) = kopia::repository(archive, &config)
            && !is_repository(&folder)
        {
            return Err(BackupError::Unreachable(folder).into());
        }
        Ok(kopia::open(archive, config, password)?)
    }
}

/// Whether `folder` holds a backup repository already, so setting up connects to it, asking
/// for its password once, rather than creating one and asking twice.
#[must_use]
#[uniffi::export]
pub fn is_backup_repository(folder: String) -> bool {
    is_repository(path(&folder))
}

#[uniffi::export]
impl ArchiveHandle {
    /// Where backups stand: whether they're set up here and reachable, and the snapshots from
    /// history, newest first, which need no drive.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive or its history can't be read.
    pub fn backups(&self) -> Result<BackupsState, CarrelError> {
        let archive = self.fresh()?;
        let config = self.kopia_config(&archive)?;
        let repository = kopia::repository(&archive, &config);
        let log = match History::open(&archive) {
            Ok(history) => history.log()?,
            Err(carrel_core::history::HistoryError::NotInitialized) => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(BackupsState {
            installed: kopia::installed(),
            description: archive
                .settings()
                .backups
                .as_ref()
                .and_then(|b| b["description"].as_str().map(str::to_owned)),
            set_up_here: config.is_file(),
            reachable: repository.as_deref().is_some_and(is_repository),
            repository: repository.as_deref().map(text),
            snapshots: snapshot_entries(&log).map(Change::from).collect(),
        })
    }

    /// Creates a backup repository in `folder`, or connects to the one there, for this archive
    /// on this device, and records it in the archive's settings and history, as `carrel backup
    /// setup` does. With `keep_password`, Kopia keeps the password in this Mac's keychain;
    /// without, it's kept nowhere and each call needs it.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for a folder inside the archive or in a synced folder,
    /// [`CarrelError::Busy`] if another writer holds the archive, or [`CarrelError::Failed`]
    /// if Kopia isn't installed, the password is wrong or the repository can't be written.
    pub fn set_up_backups(
        &self,
        folder: String,
        password: String,
        description: Option<String>,
        keep_password: bool,
    ) -> Result<BackupsSetUp, CarrelError> {
        if password.is_empty() {
            return Err(CarrelError::invalid("the password can't be empty"));
        }
        let archive = self.fresh()?;
        let identity = self.identity()?;
        let folder = Path::new(&folder);
        let setup = kopia::setup(
            &archive,
            self.kopia_config(&archive)?,
            folder,
            &password,
            keep_password,
            identity.device,
        )?;
        let description = description
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| default_description(folder));
        let lease =
            DiskLock::for_archive(&archive).acquire(self.holder()?, "backup", DEFAULT_LEASE)?;
        let commit = record_setup(&archive, &setup, &description, &lease);
        lease.release()?;
        Ok(BackupsSetUp {
            created: setup.created,
            description,
            same_disk: setup.same_disk,
            kept_in_keychain: keep_password,
            commit: commit?,
        })
    }

    /// Snapshots the whole archive now and records it in history, as `carrel backup now` does.
    /// `password` is for backups set up without keeping it; `None` uses the keychain's.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if backups aren't set up here, [`CarrelError::Busy`] if
    /// another writer holds the archive, or [`CarrelError::Failed`] if the snapshot fails, as
    /// when the backup drive isn't connected.
    pub fn snapshot_now(&self, password: Option<String>) -> Result<SnapshotTaken, CarrelError> {
        let archive = self.fresh()?;
        let backups = self.kopia_here(&archive, password)?;
        let lease =
            DiskLock::for_archive(&archive).acquire(self.holder()?, "backup", DEFAULT_LEASE)?;
        let result = snapshot_now(&archive, backups.as_ref(), &lease, "Snapshot on request");
        lease.release()?;
        let (snapshot, commit) = result.map_err(CarrelError::failed)?;
        Ok(SnapshotTaken {
            snapshot: snapshot.id,
            commit,
            files: snapshot.files,
            size: snapshot.size,
        })
    }

    /// Checks every snapshot's structure and reads back `percent`% of the files' contents, as
    /// `carrel backup verify` does. Returns what Kopia found, in words.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if backups aren't set up here, or [`CarrelError::Failed`] if
    /// the check fails or the drive isn't connected.
    pub fn verify_backups(
        &self,
        percent: u8,
        password: Option<String>,
    ) -> Result<String, CarrelError> {
        let archive = self.fresh()?;
        Ok(self.kopia_here(&archive, password)?.verify(percent)?)
    }

    /// Restores the item `item`'s contents as they were at the change `at` (the snapshot taken
    /// with it, or the latest before it; without `at`, the latest) into `to`, or a new folder
    /// beside the archive, as `carrel backup restore` does. Nothing in the archive changes.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if there's no such item, it has no contents, there's no
    /// snapshot to restore from or `to` exists, or [`CarrelError::Failed`] if the restore
    /// fails, as when the drive isn't connected.
    pub fn restore_item(
        &self,
        item: String,
        at: Option<String>,
        to: Option<String>,
        password: Option<String>,
    ) -> Result<Restored, CarrelError> {
        let archive = self.fresh()?;
        let found = Self::item(&archive, &item)?;
        let Some(item_path) = found.item.clone() else {
            return Err(CarrelError::invalid(format!(
                "{} has no contents to restore",
                found.card
            )));
        };
        let backups = self.kopia_here(&archive, password)?;
        let log = History::open(&archive)?.log()?;
        let (entry, snapshot) = restore_point(&log, at.as_deref())?;
        let target = to.map_or_else(
            || restore_beside(&archive, &item_path, entry),
            PathBuf::from,
        );
        backups.restore(snapshot, &item_path, &target)?;
        Ok(Restored {
            item: found.id.to_string(),
            path: item_path,
            to: text(&target),
            change: entry.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Sample;

    #[test]
    fn backups_not_set_up_say_so_and_list_nothing() {
        let sample = Sample::new();
        let state = sample.handle.backups().unwrap();
        assert!(!state.set_up_here);
        assert!(!state.reachable);
        assert_eq!(state.description, None);
        assert_eq!(state.snapshots, Vec::new());
        assert!(matches!(
            sample.handle.snapshot_now(None),
            Err(CarrelError::Invalid { .. })
        ));
        assert!(matches!(
            sample.handle.verify_backups(1, None),
            Err(CarrelError::Invalid { .. })
        ));
        assert!(!is_backup_repository(text(sample.dir.path())));
    }

    #[test]
    fn an_empty_password_is_refused_before_kopia_runs() {
        let sample = Sample::new();
        let folder = text(&sample.dir.path().join("backups"));
        assert!(matches!(
            sample
                .handle
                .set_up_backups(folder, String::new(), None, false),
            Err(CarrelError::Invalid { .. })
        ));
    }

    #[test]
    fn backups_are_set_up_taken_listed_verified_and_restored_with_kopia() {
        if !kopia::installed() {
            assert!(
                std::env::var_os("CARREL_REQUIRE_KOPIA").is_none(),
                "Kopia is required here but isn't installed"
            );
            eprintln!("skipped: kopia isn't installed");
            return;
        }
        let sample = Sample::new();
        let handle = &sample.handle;
        let folder = sample.dir.path().join("Backup drive");
        let password = Some(format!("test-{}", std::process::id()));
        let setup = handle
            .set_up_backups(text(&folder), password.clone().unwrap(), None, false)
            .unwrap();
        assert!(setup.created);
        assert_eq!(setup.description, "Backup drive");
        assert!(!setup.kept_in_keychain);
        assert!(is_backup_repository(text(&folder)));

        // History lists a snapshot with a change, so there's one to record it with.
        let card = sample.root().join("Projects/Hardware/lamp-build.md");
        let mut text = std::fs::read_to_string(&card).unwrap();
        text.push_str("\nSanded the diffuser.\n");
        std::fs::write(&card, text).unwrap();
        let taken = handle.snapshot_now(password.clone()).unwrap();
        assert!(taken.commit.is_some());
        let state = handle.backups().unwrap();
        assert!(state.set_up_here && state.reachable, "{state:?}");
        assert_eq!(state.description.as_deref(), Some("Backup drive"));
        assert_eq!(state.snapshots.len(), 1);
        assert_eq!(
            state.snapshots[0].snapshot.as_deref(),
            Some(format!("kopia:{}", taken.snapshot).as_str())
        );
        assert!(handle.verify_backups(1, password.clone()).is_ok());

        let item = sample.item_titled("Desk lamp parts list");
        let restored = handle
            .restore_item(item, None, None, password.clone())
            .unwrap();
        assert!(Path::new(&restored.to).exists(), "{restored:?}");
        assert!(
            Path::new(&restored.to).parent() == sample.root().canonicalize().unwrap().parent(),
            "beside the archive: {restored:?}"
        );

        std::fs::rename(&folder, sample.dir.path().join("unplugged")).unwrap();
        let state = handle.backups().unwrap();
        assert!(!state.reachable);
        assert_eq!(state.snapshots.len(), 1, "listed from history alone");
        let Err(CarrelError::Failed { message }) = handle.snapshot_now(password) else {
            panic!("an unplugged drive fails");
        };
        assert!(message.contains("drive connected"), "{message}");
    }
}
