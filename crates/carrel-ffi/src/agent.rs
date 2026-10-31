//! What the Mac app's agent does beyond the app's screens (`docs/design/mac-app.md`, "The
//! background agent"): it claims the archives it looks after, so the CLI's background worker
//! steps aside ([`carrel_core::worker::agent_lock`]); it records work sessions as they finish,
//! as the CLI does every few minutes; and it takes scheduled snapshots once backups are set
//! up. Drafting and the index run through [`ArchiveHandle::draft_queued`], the worker loop the
//! CLI runs.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use carrel_core::archive::Archive;
use carrel_core::backup::Backups;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::sessions::Scanned;
use carrel_core::worker::{Held, agent_lock, agent_running};

use crate::{ArchiveHandle, CarrelError};

/// The agent's claim on an archive on this device. While it's held, the CLI's background
/// worker steps aside. It goes when released, when dropped, or with the process.
#[derive(Debug, uniffi::Object)]
pub struct AgentClaim {
    held: Mutex<Option<Held>>,
}

#[uniffi::export]
impl AgentClaim {
    /// Lets go, so the CLI's worker drafts again, as when the agent is paused.
    pub fn release(&self) {
        if let Ok(mut held) = self.held.lock() {
            held.take();
        }
    }
}

/// What recording work sessions found.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SessionsRecorded {
    /// How many sessions were found, on all the items.
    pub sessions: u32,
    /// The titles of the items they were found on.
    pub items: Vec<String>,
    /// How many items had pending entries approved by hand, noticed and recorded.
    pub approved_by_hand: u32,
    /// The history commit, when anything in the archive was written.
    pub commit: Option<String>,
    /// Why a working copy's snapshot after its sessions wasn't taken, one line each.
    pub snapshot_failures: Vec<String>,
}

/// A snapshot taken on schedule.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BackedUp {
    /// The snapshot's identifier in the backup system.
    pub snapshot: String,
    /// The history commit, when there was a change to record it with.
    pub commit: Option<String>,
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl ArchiveHandle {
    /// Kopia's configuration for this archive in this handle's configuration folder, once
    /// backups are set up there.
    #[cfg(not(any(
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos"
    )))]
    fn backups_here(&self, archive: &Archive) -> Option<Box<dyn Backups>> {
        let config = self
            .config_dir()
            .ok()?
            .join("backups")
            .join(format!("{}.config", archive.info().id));
        carrel_core::backup::Kopia::open(archive, config)
            .ok()
            .map(|kopia| Box::new(kopia) as Box<dyn Backups>)
    }

    /// No backups away from a Mac.
    #[cfg(any(
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos"
    ))]
    fn backups_here(&self, _archive: &Archive) -> Option<Box<dyn Backups>> {
        None
    }

    fn workers_folder(&self) -> Result<PathBuf, CarrelError> {
        Ok(self.config_dir()?.join("workers"))
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// Claims this archive for the agent on this device, or `None` if another agent has it.
    /// While the claim is held, the CLI starts no worker of its own for the archive.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the claim's file can't be opened.
    pub fn claim_for_agent(&self) -> Result<Option<Arc<AgentClaim>>, CarrelError> {
        let archive = self.fresh()?;
        let held = agent_lock(&self.workers_folder()?, &archive)
            .take()
            .map_err(CarrelError::failed)?;
        Ok(held.map(|held| {
            Arc::new(AgentClaim {
                held: Mutex::new(Some(held)),
            })
        }))
    }

    /// Whether an agent looks after this archive on this device. With none, the app runs the
    /// worker on demand with [`ArchiveHandle::draft_queued`], as the CLI does.
    #[must_use]
    pub fn agent_running(&self) -> bool {
        let (Ok(archive), Ok(folder)) = (self.fresh(), self.workers_folder()) else {
            return false;
        };
        agent_running(&folder, &archive)
    }

    /// How long a pause in work ends a work session, in minutes (`sessions.idle_minutes`):
    /// a session is recorded once it has been idle this long.
    #[must_use]
    pub fn idle_minutes(&self) -> u32 {
        self.fresh()
            .map_or(self.archive.settings().sessions.idle_minutes, |a| {
                a.settings().sessions.idle_minutes
            })
    }

    /// Records the finished work sessions on the items checked out to this device and on its
    /// desk, as pending history entries, then snapshots their working copies when backups are
    /// set up: what the CLI does before a command every few minutes.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Busy`] if another writer holds the archive, or [`CarrelError::Failed`]
    /// if the cards or history can't be written.
    pub fn record_sessions(&self) -> Result<SessionsRecorded, CarrelError> {
        let archive = self.fresh()?;
        let state = Scanned::path_in(&self.config_dir()?, &archive.info().id.to_string());
        let lease =
            DiskLock::for_archive(&archive).acquire(self.holder()?, "sessions", DEFAULT_LEASE)?;
        let recorded = carrel_core::sessions::record_in(&archive, &lease, Some(state));
        lease.release()?;
        let recorded = recorded?;
        let mut snapshot_failures = Vec::new();
        if !recorded.items.is_empty()
            && let Some(backups) = self.backups_here(&archive)
        {
            for (_, result) in carrel_core::sessions::snapshot(&recorded, backups.as_ref()) {
                if let Err(e) = result {
                    snapshot_failures.push(e.to_string());
                }
            }
        }
        Ok(SessionsRecorded {
            sessions: count(recorded.sessions()),
            items: recorded.items.iter().map(|i| i.title.clone()).collect(),
            approved_by_hand: count(recorded.approved_by_hand.len()),
            commit: recorded.commit,
            snapshot_failures,
        })
    }

    /// Whether backups are set up for this archive on this device, so the agent takes
    /// scheduled snapshots.
    #[must_use]
    pub fn backups_set_up(&self) -> bool {
        self.fresh()
            .is_ok_and(|archive| self.backups_here(&archive).is_some())
    }

    /// Snapshots the whole archive, as `carrel backup now` does, described as `description`.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if backups aren't set up, [`CarrelError::Busy`] if another
    /// writer holds the archive, or [`CarrelError::Failed`] if the snapshot fails, as when
    /// the backup drive isn't plugged in.
    pub fn back_up(&self, description: String) -> Result<BackedUp, CarrelError> {
        let archive = self.fresh()?;
        let Some(backups) = self.backups_here(&archive) else {
            return Err(CarrelError::invalid(
                "backups aren't set up for this archive on this Mac",
            ));
        };
        let lease =
            DiskLock::for_archive(&archive).acquire(self.holder()?, "backup", DEFAULT_LEASE)?;
        let result =
            carrel_core::backup::snapshot_now(&archive, backups.as_ref(), &lease, &description);
        lease.release()?;
        let (snapshot, commit) = result.map_err(CarrelError::failed)?;
        Ok(BackedUp {
            snapshot: snapshot.id,
            commit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::Sample;

    #[test]
    fn the_agents_claim_is_one_per_archive_and_goes_when_released() {
        let sample = Sample::new();
        let handle = &sample.handle;
        assert!(!handle.agent_running());
        let claim = handle.claim_for_agent().unwrap().unwrap();
        assert!(handle.agent_running());
        assert!(handle.claim_for_agent().unwrap().is_none());
        claim.release();
        assert!(!handle.agent_running());
        let again = handle.claim_for_agent().unwrap().unwrap();
        drop(again);
        assert!(!handle.agent_running(), "dropping lets go too");
    }

    #[test]
    fn sessions_are_recorded_and_backups_need_setting_up() {
        let sample = Sample::new();
        let handle = &sample.handle;
        let recorded = handle.record_sessions().unwrap();
        assert_eq!(recorded.sessions, 0);
        assert!(recorded.snapshot_failures.is_empty());
        assert!(
            sample.dir.path().join("config/sessions").is_dir(),
            "what's been looked at is kept in the handle's own folder"
        );
        assert!(handle.idle_minutes() > 0);
        assert!(!handle.backups_set_up());
        assert!(matches!(
            handle.back_up("Scheduled snapshot".into()),
            Err(CarrelError::Invalid { .. })
        ));
    }
}
