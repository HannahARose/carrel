//! The intake queue (`docs/design/tui-ingest.md`): files and folders dropped on the window
//! or added from the menu join it as `carrel ingest` adds them ([`carrel_core::intake`]), and
//! a worker drafts them into proposals. The agent runs the worker; with it off, the app runs
//! the same loop ([`carrel_core::worker`]) on demand with [`ArchiveHandle::draft_queued`].

use std::path::PathBuf;
use std::sync::Arc;

use carrel_core::ingest::IntakeOptions;
use carrel_core::intake::{IntakeEntry, Processed};
use carrel_core::worker::{FileLock, Worker, file_in, read_status};

use crate::{ArchiveHandle, CarrelError, ProgressListener, text};

/// An item waiting in the intake queue.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Queued {
    /// The entry's identifier.
    pub id: String,
    /// Where the item is.
    pub path: String,
    /// When it was added.
    pub added: String,
    /// How many times drafting it has failed.
    pub attempts: u32,
    /// Why the last attempt failed.
    pub failure: Option<String>,
}

impl From<&IntakeEntry> for Queued {
    fn from(entry: &IntakeEntry) -> Self {
        Self {
            id: entry.id.to_string(),
            path: text(&entry.path),
            added: entry.added.0.clone(),
            attempts: entry.attempts,
            failure: entry.failure.clone(),
        }
    }
}

/// A path left out when adding to the queue, and why.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LeftOut {
    /// The path.
    pub path: String,
    /// Why: skipped by the rules, already queued, already waiting for review, and so on.
    pub reason: String,
}

/// What adding to the queue did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Added {
    /// The entries added.
    pub added: Vec<Queued>,
    /// The paths left out.
    pub left_out: Vec<LeftOut>,
}

/// The intake queue on this device.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct IntakeQueue {
    /// The entries waiting, in the order they'll be drafted.
    pub waiting: Vec<Queued>,
    /// The entries that have failed too often to be tried again without the owner.
    pub failed: Vec<Queued>,
}

/// What a worker is doing, as its status file says.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WorkerStatus {
    /// The item being drafted, if any.
    pub item: Option<String>,
    /// Its stage, such as "Drafting cards".
    pub stage: Option<String>,
    /// Items finished since the worker started.
    pub done: u64,
    /// Items still waiting, including the current one.
    pub waiting: u64,
    /// When the status was written.
    pub updated: String,
}

/// What a run of the worker did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Drafted {
    /// How many items were drafted into proposals.
    pub drafted: u64,
    /// How many items were embedded for search.
    pub embedded: u64,
    /// Items that couldn't be drafted, and why.
    pub failures: Vec<LeftOut>,
    /// Whether another worker was already running, so this one did nothing.
    pub already_running: bool,
}

/// Tells Swift how the worker is getting on, and collects its failures.
struct Reporting {
    progress: Option<Arc<dyn ProgressListener>>,
    failures: Vec<LeftOut>,
}

impl carrel_core::worker::Progress for Reporting {
    fn drafting(&mut self, done: usize, waiting: usize, name: &str) {
        if let Some(progress) = &self.progress {
            progress.update(done as u64, (done + waiting) as u64, Some(name.to_owned()));
        }
    }

    fn drafted(&mut self, processed: Processed) {
        if let Err(why) = processed.run {
            self.failures.push(LeftOut {
                path: text(&processed.entry.path),
                reason: why,
            });
        }
    }

    fn reading(&mut self, done: usize, total: usize, item: Option<&str>) {
        if let Some(progress) = &self.progress {
            progress.update(done as u64, total as u64, item.map(str::to_owned));
        }
    }

    fn embedding(&mut self, done: usize, total: usize, item: Option<&str>) {
        self.reading(done, total, item);
    }
}

impl ArchiveHandle {
    /// The folder where workers keep their status files and locks for this handle.
    fn workers(&self) -> Result<PathBuf, CarrelError> {
        Ok(self.config_dir()?.join("workers"))
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// Adds the files and folders at `paths` to the intake queue on this device. A folder
    /// is one item, or each thing in it is when `each` says so; one whose name suggests a
    /// pile, such as Downloads, is split unless `whole` says not to.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if a path is missing or the queue can't be written.
    pub fn queue(&self, paths: Vec<String>, each: bool, whole: bool) -> Result<Added, CarrelError> {
        let archive = self.fresh()?;
        let device = self.identity()?.device;
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let queued =
            carrel_core::intake::add(&archive, &paths, IntakeOptions { each, whole }, device)?;
        Ok(Added {
            added: queued.added.iter().map(Into::into).collect(),
            left_out: queued
                .skipped
                .into_iter()
                .map(|s| LeftOut {
                    path: text(&s.path),
                    reason: s.reason,
                })
                .collect(),
        })
    }

    /// The intake queue on this device.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if it can't be read.
    pub fn intake(&self) -> Result<IntakeQueue, CarrelError> {
        let archive = self.fresh()?;
        let listing = carrel_core::intake::listing(&archive, self.identity()?.device)?;
        Ok(IntakeQueue {
            waiting: listing.waiting.iter().map(Into::into).collect(),
            failed: listing.failed.iter().map(Into::into).collect(),
        })
    }

    /// Takes the entry `id` out of the intake queue.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if it isn't queued.
    pub fn unqueue(&self, id: String) -> Result<(), CarrelError> {
        let archive = self.fresh()?;
        carrel_core::intake::remove(&archive, &id)?;
        Ok(())
    }

    /// Moves the entry `id` to the front of the intake queue.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if it isn't queued.
    pub fn queue_first(&self, id: String) -> Result<(), CarrelError> {
        let archive = self.fresh()?;
        carrel_core::intake::move_to_front(&archive, &id, self.identity()?.device)?;
        Ok(())
    }

    /// Lets the failed entry `id` be tried again.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if it isn't queued.
    pub fn retry_queued(&self, id: String) -> Result<(), CarrelError> {
        let archive = self.fresh()?;
        carrel_core::intake::retry(&archive, &id)?;
        Ok(())
    }

    /// What the worker for this archive on this device is doing, if one is running.
    #[must_use]
    pub fn worker_status(&self) -> Option<WorkerStatus> {
        let archive = self.fresh().ok()?;
        let folder = self.workers().ok()?;
        if !FileLock::new(&folder, &archive).is_held().unwrap_or(false) {
            return None;
        }
        read_status(&file_in(&folder, &archive, "json")).map(|s| WorkerStatus {
            item: s.item,
            stage: s.stage,
            done: s.done as u64,
            waiting: s.waiting as u64,
            updated: s.updated,
        })
    }

    /// Runs the worker here, as `carrel worker` does: drafts every item waiting in the
    /// intake queue on this device, then embeds what changed for search, reporting to
    /// `progress`. Does nothing when another worker, such as the agent's, is running. It
    /// takes as long as the models do, so call it off the main thread.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the queues can't be read or written.
    pub fn draft_queued(
        &self,
        progress: Option<Arc<dyn ProgressListener>>,
    ) -> Result<Drafted, CarrelError> {
        let archive = self.fresh()?;
        let folder = self.workers()?;
        let lock = FileLock::new(&folder, &archive);
        let Some(held) = lock.take()? else {
            return Ok(Drafted {
                drafted: 0,
                embedded: 0,
                failures: Vec::new(),
                already_running: true,
            });
        };
        let registry = self.registry();
        let backups = crate::backups(&archive);
        let worker = Worker {
            archive: &archive,
            registry: &registry,
            holder: self.holder()?,
            backups: backups.as_deref(),
            status: Some(file_in(&folder, &archive, "json")),
        };
        let mut reporting = Reporting {
            progress,
            failures: Vec::new(),
        };
        let ran = worker
            .run(&lock, held, &mut reporting)
            .map_err(CarrelError::failed)?;
        Ok(Drafted {
            drafted: ran.drafted as u64,
            embedded: ran.embedded as u64,
            failures: reporting.failures,
            already_running: false,
        })
    }
}
