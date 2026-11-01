//! The worker loop: drafts the items waiting in an archive's intake queue on this device, one
//! at a time, until none are left (`docs/design/tui-ingest.md`), then brings the embedding
//! index up to date (`docs/design/search.md`).
//!
//! The CLI's background worker and the Mac app's login-item agent run this same loop
//! (`docs/design/mac-app.md`). Each keeps one worker per archive and device with a lock of its
//! own, which the loop takes through [`DeviceLock`]: it lets go when the queue looks empty and
//! takes it again to look once more, so an item added meanwhile is never left waiting.
//!
//! While it works, the loop writes what it's doing to a status file in the configuration
//! folder ([`status_path`]), which `carrel ingest --status`, the interactive mode and the app
//! read with [`read_status`]. What happens is reported through [`Progress`], so each interface
//! shows it in its own way.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::archive::{Archive, ArchiveError};
use crate::backup::Backups;
use crate::coordinator::Holder;
use crate::index::{Index, IndexError, plan};
use crate::intake::{Processed, process, waiting};
use crate::providers::{Capability, Registry};
use crate::review::ReviewError;

/// What a worker is doing, as its status file says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// The worker's process.
    pub pid: u32,
    /// The item being drafted, if any.
    pub item: Option<String>,
    /// Its stage, such as "Drafting cards".
    pub stage: Option<String>,
    /// Items finished since the worker started.
    pub done: usize,
    /// Items still waiting, including the current one.
    pub waiting: usize,
    /// When the status was written.
    pub updated: String,
}

/// The folder in the configuration folder ([`crate::local::config_dir`]) where workers keep
/// their status files, logs and locks. `None` if there's no configuration folder.
#[must_use]
pub fn folder() -> Option<PathBuf> {
    crate::local::config_dir().map(|dir| dir.join("workers"))
}

/// The worker's file for `archive` with `extension` in `folder`: `json` for its status, `log`
/// for its log and `lock` for its lock.
#[must_use]
pub fn file_in(folder: &Path, archive: &Archive, extension: &str) -> PathBuf {
    folder.join(format!("{}.{extension}", archive.info().id))
}

/// Where the worker for `archive` on this device writes its status, or `None` if there's no
/// configuration folder.
#[must_use]
pub fn status_path(archive: &Archive) -> Option<PathBuf> {
    folder().map(|dir| file_in(&dir, archive, "json"))
}

/// The status at `path`, if a worker has written one there.
#[must_use]
pub fn read_status(path: &Path) -> Option<Status> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Writes `status` to `path`, replacing what was there in one step so a reader never sees
/// half of it. Best effort: a status that can't be written is skipped.
pub fn write_status(path: &Path, status: &Status) {
    if let Ok(text) = serde_json::to_string(status) {
        let temp = path.with_extension("json.tmp");
        if std::fs::write(&temp, text).is_ok() {
            let _ = std::fs::rename(&temp, path);
        }
    }
}

/// The time now, as the status file records it.
#[must_use]
pub fn now() -> String {
    jiff::Zoned::now()
        .strftime("%Y-%m-%dT%H:%M:%S%:z")
        .to_string()
}

/// Why the worker loop stopped.
#[derive(Debug)]
pub enum WorkerError {
    /// The intake queue couldn't be read.
    Queue(ReviewError),
    /// An item's results couldn't be saved.
    Action(crate::actions::ActionError),
    /// The archive couldn't be read, as when reading the text of items for the index.
    Archive(ArchiveError),
    /// The embedding index for `model` couldn't be brought up to date.
    Index {
        /// The archive's embedding model.
        model: String,
        /// What went wrong.
        source: IndexError,
    },
    /// The device lock couldn't be taken; the caller's own words.
    Lock(String),
}

impl std::fmt::Display for WorkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Queue(e) => write!(f, "{e}"),
            Self::Action(e) => write!(f, "{e}"),
            Self::Archive(e) => write!(f, "{e}"),
            Self::Index { source, .. } => write!(f, "{source}"),
            Self::Lock(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for WorkerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Queue(e) => Some(e),
            Self::Action(e) => Some(e),
            Self::Archive(e) => Some(e),
            Self::Index { source, .. } => Some(source),
            Self::Lock(_) => None,
        }
    }
}

impl From<ReviewError> for WorkerError {
    fn from(error: ReviewError) -> Self {
        Self::Queue(error)
    }
}

impl From<crate::actions::ActionError> for WorkerError {
    fn from(error: crate::actions::ActionError) -> Self {
        Self::Action(error)
    }
}

impl From<ArchiveError> for WorkerError {
    fn from(error: ArchiveError) -> Self {
        Self::Archive(error)
    }
}

/// The lock that keeps one worker per archive on a device, supplied by whoever runs the loop:
/// the CLI's is a file lock in the configuration folder, which the app's agent shares.
pub trait DeviceLock {
    /// Held while the lock is; dropping it lets go.
    type Held;

    /// Takes the lock, or `None` if another worker holds it.
    ///
    /// # Errors
    ///
    /// [`WorkerError::Lock`] if it can't be taken or checked.
    fn try_lock(&self) -> Result<Option<Self::Held>, WorkerError>;
}

/// The worker lock the CLI's worker and the app's agent share: an operating-system lock on
/// a file in the workers' folder (`<archive id>.lock`), which goes with the process, so a
/// crashed worker never leaves a stale one.
#[derive(Debug, Clone)]
pub struct FileLock {
    /// The lock file.
    pub path: PathBuf,
}

/// Held while a worker runs for an archive on this device; dropping it lets go.
#[derive(Debug)]
pub struct Held {
    file: std::fs::File,
}

impl Drop for Held {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

impl FileLock {
    /// The lock for `archive`'s worker in the workers' folder `folder`.
    #[must_use]
    pub fn new(folder: &Path, archive: &Archive) -> Self {
        Self {
            path: file_in(folder, archive, "lock"),
        }
    }

    /// Takes the lock, creating its file and folder, or `None` if another worker holds it.
    ///
    /// # Errors
    ///
    /// [`std::io::Error`] if the lock file can't be opened.
    pub fn take(&self) -> std::io::Result<Option<Held>> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&self.path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Held { file })),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(e)) => Err(e),
        }
    }

    /// Whether a worker holds the lock. Only looks: a missing lock file means no worker has
    /// ever run here, and nothing is created.
    ///
    /// # Errors
    ///
    /// [`std::io::Error`] if the lock file can't be opened.
    pub fn is_held(&self) -> std::io::Result<bool> {
        let file = match std::fs::OpenOptions::new().write(true).open(&self.path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e),
        };
        match file.try_lock() {
            Ok(()) => {
                let _ = file.unlock();
                Ok(false)
            }
            Err(std::fs::TryLockError::WouldBlock) => Ok(true),
            Err(std::fs::TryLockError::Error(e)) => Err(e),
        }
    }
}

/// The lock the Mac app's agent holds for as long as it looks after `archive` on this device
/// (`<archive id>.agent` in the workers' folder `folder`). While it's held, the CLI's
/// background worker steps aside and leaves the queue and the index to the agent, which
/// notices changes as they happen. Like the worker lock, it goes with the process.
#[must_use]
pub fn agent_lock(folder: &Path, archive: &Archive) -> FileLock {
    FileLock {
        path: file_in(folder, archive, "agent"),
    }
}

/// Whether the Mac app's agent looks after `archive` on this device, so the CLI's background
/// worker steps aside. A lock that can't be checked counts as no agent.
#[must_use]
pub fn agent_running(folder: &Path, archive: &Archive) -> bool {
    agent_lock(folder, archive).is_held().unwrap_or(false)
}

impl DeviceLock for FileLock {
    type Held = Held;

    fn try_lock(&self) -> Result<Option<Held>, WorkerError> {
        self.take()
            .map_err(|e| WorkerError::Lock(format!("{}: {e}", self.path.display())))
    }
}

/// What the loop tells whoever runs it as it goes. Every method does nothing unless
/// implemented.
pub trait Progress {
    /// An item starts being drafted: `done` finished so far, `waiting` still waiting including
    /// this one, and its file name.
    fn drafting(&mut self, done: usize, waiting: usize, name: &str) {
        let _ = (done, waiting, name);
    }

    /// An item was drafted, or failed.
    fn drafted(&mut self, processed: Processed) {
        let _ = processed;
    }

    /// Before embedding, `done` of `total` items had their text read; `item` is being read.
    fn reading(&mut self, done: usize, total: usize, item: Option<&str>) {
        let _ = (done, total, item);
    }

    /// `done` of `total` items are embedded; `item` is being embedded.
    fn embedding(&mut self, done: usize, total: usize, item: Option<&str>) {
        let _ = (done, total, item);
    }

    /// Bringing the index up to date embedded `items`.
    fn embedded(&mut self, items: usize) {
        let _ = items;
    }

    /// Bringing the index up to date failed. The loop carries on: the next change or a
    /// rebuild catches up.
    fn embedding_failed(&mut self, error: WorkerError) {
        let _ = error;
    }
}

/// A [`Progress`] that ignores everything.
#[derive(Debug, Clone, Copy, Default)]
pub struct Quiet;

impl Progress for Quiet {}

/// What a whole run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ran {
    /// Items drafted.
    pub drafted: usize,
    /// Items embedded for search.
    pub embedded: usize,
}

/// A worker for one archive on this device.
pub struct Worker<'a> {
    /// The archive.
    pub archive: &'a Archive,
    /// The providers that draft cards and embed for search.
    pub registry: &'a Registry,
    /// Who the changes are made as; its device is whose queue is drafted.
    pub holder: Holder,
    /// Backups, for snapshots before an owner rule files an item.
    pub backups: Option<&'a dyn Backups>,
    /// Where to write the status, or `None` to write none.
    pub status: Option<PathBuf>,
    /// Where files other devices sent through their inboxes wait to be drafted on this one,
    /// outside the archive (FORMAT.md §7.8); `None` leaves the inboxes for another worker.
    pub received: Option<PathBuf>,
}

impl Worker<'_> {
    fn write(&self, status: &Status) {
        if let Some(path) = &self.status {
            write_status(path, status);
        }
    }

    /// Drafts every waiting item, holding `held` (taken from `lock`), until none are left.
    /// Items added while it runs are drafted too. Returns how many were drafted.
    ///
    /// # Errors
    ///
    /// [`WorkerError`] if the queues can't be read or written, or the lock can't be taken.
    pub fn draft<L: DeviceLock>(
        &self,
        lock: &L,
        held: L::Held,
        progress: &mut dyn Progress,
    ) -> Result<usize, WorkerError> {
        let device = self.holder.device;
        let mut held = Some(held);
        let mut done = 0;
        self.apply_inbox();
        self.clear_holding();
        loop {
            let mut queue = waiting(self.archive, device, false)?;
            if queue.is_empty() {
                // Let go, then look once more: an item added while the lock was held, by someone
                // who saw a worker running and so started none, would otherwise wait for the next.
                drop(held.take());
                if waiting(self.archive, device, false)?.is_empty() {
                    return Ok(done);
                }
                match lock.try_lock()? {
                    Some(again) => held = Some(again),
                    None => return Ok(done),
                }
                continue;
            }
            let entry = queue.remove(0);
            let name = entry.path.file_name().map_or_else(
                || entry.path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            progress.drafting(done, queue.len() + 1, &name);
            let mut status = Status {
                pid: std::process::id(),
                item: Some(name),
                stage: None,
                done,
                waiting: queue.len() + 1,
                updated: now(),
            };
            self.write(&status);
            let processed = process(
                self.archive,
                self.registry,
                entry,
                self.holder,
                self.backups,
                &mut |p| {
                    status.stage = Some(p.stage.describe().to_owned());
                    status.updated = now();
                    self.write(&status);
                },
            )?;
            progress.drafted(processed);
            done += 1;
        }
    }

    /// Applies other devices' inboxes, as the writer: each entry as its sender's, what they
    /// sent to add joining this device's queue. Nothing happens while another device is the
    /// writer, or this one acts through its inbox.
    fn apply_inbox(&self) {
        let Some(received) = &self.received else {
            return;
        };
        if crate::inbox::waiting(self.archive).is_empty()
            || crate::inbox::sending(self.archive, self.holder.device).is_some()
        {
            return;
        }
        let applying = crate::inbox::Applying {
            writer: self.holder,
            received,
            registry: self.registry,
            backups: self.backups,
        };
        let _ = crate::inbox::apply(self.archive, &applying);
    }

    /// Snapshots the archive and clears its holding folder, when this device has backups and
    /// another device without them left versions there ([`crate::holding`]). Nothing happens
    /// while this device acts through its inbox.
    fn clear_holding(&self) {
        let Some(backups) = self.backups else {
            return;
        };
        if crate::holding::list(self.archive).is_empty()
            || crate::inbox::sending(self.archive, self.holder.device).is_some()
        {
            return;
        }
        let _ = crate::holding::snapshot_and_clear(self.archive, backups);
    }

    /// Embeds what's behind in the archive's index with its embedding model, if a provider
    /// here runs it, writing progress to the status file with `done` items drafted. Returns how
    /// many items were embedded.
    ///
    /// # Errors
    ///
    /// [`WorkerError::Archive`] if the text of items can't be read, and
    /// [`WorkerError::Index`] if the model fails or the index can't be written.
    pub fn embed(&self, done: usize, progress: &mut dyn Progress) -> Result<usize, WorkerError> {
        let Some(model) = self.archive.settings().embedding_model().map(str::to_owned) else {
            return Ok(0);
        };
        if !offered(self.registry, &model) {
            return Ok(0);
        }
        let failed = |source| WorkerError::Index {
            model: model.clone(),
            source,
        };
        let mut index = Index::open_or_new(self.archive, &model).map_err(failed)?;
        let mut status = Status {
            pid: std::process::id(),
            item: None,
            stage: Some("Embedding for search".into()),
            done,
            waiting: 0,
            updated: now(),
        };
        let (wanted, _) = crate::search::texts::tidy(self.archive)?;
        if !wanted.is_empty() {
            crate::search::texts::fill(
                self.archive,
                self.registry,
                wanted,
                &mut |n, total, item| {
                    progress.reading(n, total, item);
                },
            )?;
        }
        let plan = plan(self.archive, Some(&index)).map_err(failed)?;
        let embedded = plan.pending.len();
        index
            .embed(self.archive, self.registry, plan, &mut |n, total, item| {
                progress.embedding(n, total, item);
                status.item = item.map(str::to_owned);
                status.waiting = total - n;
                status.updated = now();
                self.write(&status);
            })
            .map_err(failed)?;
        // If the archive switched models meanwhile (`carrel index model`), what was saved here
        // is of the old model's index, which the switch removed; remove it again.
        if let Ok(now) = Archive::open(self.archive.root()) {
            let settings = now.settings();
            if settings.embedding_model() != Some(model.as_str())
                && settings.embedding_switch() != Some(model.as_str())
            {
                let _ = crate::index::switch::remove_index(&now, &model);
            }
        }
        Ok(embedded)
    }

    /// Drafts the queue until it's empty, then embeds what's behind in the index, and again
    /// while items arrive meanwhile; then removes the status file. Starts holding `held`, taken
    /// from `lock`; embedding takes the lock again, so two workers never embed at once. A
    /// failure to embed is reported to `progress` and doesn't stop the run.
    ///
    /// # Errors
    ///
    /// [`WorkerError`] if the queues can't be read or written, or the lock can't be taken.
    pub fn run<L: DeviceLock>(
        &self,
        lock: &L,
        held: L::Held,
        progress: &mut dyn Progress,
    ) -> Result<Ran, WorkerError> {
        let mut ran = Ran::default();
        let mut held = held;
        loop {
            ran.drafted += self.draft(lock, held, progress)?;
            let Some(again) = lock.try_lock()? else {
                break;
            };
            match self.embed(ran.drafted, progress) {
                Ok(n) => {
                    ran.embedded += n;
                    progress.embedded(n);
                }
                Err(e) => progress.embedding_failed(e),
            }
            // Items queued while it embedded saw a worker running and started none.
            if waiting(self.archive, self.holder.device, false)?.is_empty() {
                break;
            }
            held = again;
        }
        if let Some(path) = &self.status {
            let _ = std::fs::remove_file(path);
        }
        Ok(ran)
    }
}

/// Whether a provider in `registry` offers the embedding model `model`.
#[must_use]
pub fn offered(registry: &Registry, model: &str) -> bool {
    registry
        .ranked(Capability::Embed)
        .iter()
        .any(|(_, offer)| offer.id == model)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::archive::InitOptions;
    use crate::card::Id;
    use crate::ingest::IntakeOptions;

    fn holder() -> Holder {
        Holder {
            person: Id::new_v7(),
            device: Id::new_v7(),
        }
    }

    /// A lock that's always free, counting how often it's taken.
    #[derive(Default)]
    struct Free(Cell<usize>);

    impl DeviceLock for Free {
        type Held = ();

        fn try_lock(&self) -> Result<Option<()>, WorkerError> {
            self.0.set(self.0.get() + 1);
            Ok(Some(()))
        }
    }

    /// A lock another worker always holds.
    struct Taken;

    impl DeviceLock for Taken {
        type Held = ();

        fn try_lock(&self) -> Result<Option<()>, WorkerError> {
            Ok(None)
        }
    }

    #[derive(Default)]
    struct Recorded {
        drafting: Vec<(usize, usize, String)>,
        drafted: Vec<PathBuf>,
        embedded: Vec<usize>,
    }

    impl Progress for Recorded {
        fn drafting(&mut self, done: usize, waiting: usize, name: &str) {
            self.drafting.push((done, waiting, name.to_owned()));
        }

        fn drafted(&mut self, processed: Processed) {
            self.drafted.push(processed.entry.path);
        }

        fn embedded(&mut self, items: usize) {
            self.embedded.push(items);
        }
    }

    fn sample() -> (tempfile::TempDir, Archive) {
        let dir = tempfile::tempdir().unwrap();
        let archive =
            Archive::init(&dir.path().join("Archive"), &InitOptions::new("Test")).unwrap();
        (dir, archive)
    }

    fn queue(dir: &Path, archive: &Archive, device: Id, names: &[&str]) {
        let paths: Vec<PathBuf> = names
            .iter()
            .map(|n| {
                let path = dir.join(n);
                std::fs::write(&path, format!("{n}\n")).unwrap();
                path
            })
            .collect();
        crate::intake::add(archive, &paths, IntakeOptions::default(), device).unwrap();
    }

    #[test]
    fn the_status_file_round_trips_and_is_replaced_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("archive.json");
        assert_eq!(read_status(&path), None);
        let status = Status {
            pid: 7,
            item: Some("notes.txt".into()),
            stage: Some("Drafting cards".into()),
            done: 2,
            waiting: 3,
            updated: now(),
        };
        write_status(&path, &status);
        assert_eq!(read_status(&path), Some(status.clone()));
        assert!(!path.with_extension("json.tmp").exists());
        let text = std::fs::read_to_string(&path).unwrap();
        for key in ["pid", "item", "stage", "done", "waiting", "updated"] {
            assert!(text.contains(&format!("\"{key}\"")), "{text}");
        }
    }

    #[test]
    fn worker_files_are_named_for_the_archive() {
        let (_dir, archive) = sample();
        let folder = Path::new("/config/workers");
        assert_eq!(
            file_in(folder, &archive, "log"),
            folder.join(format!("{}.log", archive.info().id))
        );
    }

    #[test]
    fn the_agent_is_running_while_it_holds_its_lock_and_apart_from_the_workers() {
        let (dir, archive) = sample();
        let folder = dir.path().join("workers");
        assert!(!agent_running(&folder, &archive), "no lock file yet");
        assert!(!folder.exists(), "looking creates nothing");

        let held = agent_lock(&folder, &archive).take().unwrap().unwrap();
        assert!(agent_running(&folder, &archive));
        assert!(
            agent_lock(&folder, &archive).take().unwrap().is_none(),
            "one agent per archive and device"
        );
        let worker = FileLock::new(&folder, &archive);
        assert!(
            !worker.is_held().unwrap(),
            "the worker lock is a lock of its own"
        );
        assert!(
            worker.take().unwrap().is_some(),
            "the agent's own worker takes it"
        );

        drop(held);
        assert!(!agent_running(&folder, &archive), "it goes with the agent");
    }

    #[test]
    fn drafting_takes_this_devices_queue_in_order_and_reports_each() {
        let (dir, archive) = sample();
        let holder = holder();
        queue(dir.path(), &archive, holder.device, &["a.txt", "b.txt"]);
        queue(dir.path(), &archive, Id::new_v7(), &["elsewhere.txt"]);
        let registry = Registry::with_built_ins();
        let status = dir.path().join("status.json");
        let worker = Worker {
            archive: &archive,
            registry: &registry,
            holder,
            backups: None,
            status: Some(status.clone()),
            received: None,
        };
        let mut progress = Recorded::default();
        let lock = Free::default();
        let ran = worker.run(&lock, (), &mut progress).unwrap();
        assert_eq!(
            ran,
            Ran {
                drafted: 2,
                embedded: 0
            }
        );
        assert_eq!(
            progress.drafting,
            vec![(0, 2, "a.txt".into()), (1, 1, "b.txt".into())]
        );
        assert_eq!(progress.drafted.len(), 2);
        assert_eq!(
            progress.embedded,
            vec![0],
            "no embedding model, nothing embedded"
        );
        assert!(!status.exists(), "the status goes when the run ends");
        assert_eq!(waiting(&archive, holder.device, true).unwrap(), []);
        let (all, _) = crate::intake::IntakeQueue::of(&archive).list().unwrap();
        assert_eq!(all.len(), 1, "another device's item is left");
    }

    #[test]
    fn with_the_lock_taken_elsewhere_it_drafts_then_steps_aside() {
        let (dir, archive) = sample();
        let holder = holder();
        queue(dir.path(), &archive, holder.device, &["a.txt"]);
        let registry = Registry::with_built_ins();
        let worker = Worker {
            archive: &archive,
            registry: &registry,
            holder,
            backups: None,
            status: None,
            received: None,
        };
        let mut progress = Recorded::default();
        let ran = worker.run(&Taken, (), &mut progress).unwrap();
        assert_eq!(ran.drafted, 1);
        assert!(progress.embedded.is_empty(), "another worker embeds");
    }

    #[test]
    fn an_empty_queue_drafts_nothing_and_looks_once_more() {
        let (_dir, archive) = sample();
        let registry = Registry::with_built_ins();
        let worker = Worker {
            archive: &archive,
            registry: &registry,
            holder: holder(),
            backups: None,
            status: None,
            received: None,
        };
        let lock = Free::default();
        assert_eq!(worker.draft(&lock, (), &mut Quiet).unwrap(), 0);
        assert_eq!(
            lock.0.get(),
            0,
            "nothing arrived, so the lock isn't taken again"
        );
    }
}
