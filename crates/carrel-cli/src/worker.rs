//! `carrel worker`: runs the core's worker loop ([`carrel_core::worker`]), which drafts the
//! items waiting in an archive's intake queue on this device, one at a time, until none are
//! left (`docs/design/tui-ingest.md`), then brings the embedding index up to date
//! (`docs/design/search.md`).
//!
//! Adding to the queue starts a worker, and so does every change to filed items (filing,
//! corrections, edits, refiles and returns), so the index catches up in the background and
//! the change itself never waits for the embedding model.
//!
//! Adding to the queue starts a worker in the background, detached so it outlives the
//! terminal, unless one is already running. An operating-system lock on a file in Carrel's
//! configuration folder keeps it to one per archive and device; the lock goes with the process,
//! so a crashed worker never leaves a stale one. The worker writes its progress to a status
//! file, which `carrel ingest --status` and the interactive mode read, and a log beside it.
//!
//! While an agent looks after the archive on this device (the Mac app's or `carrel agent`;
//! it holds the agent lock, [`carrel_core::worker::agent_lock`]), the CLI starts no worker of
//! its own and `carrel worker` steps aside: the agent notices the queue and changes as they
//! happen.
//!
//! This module keeps what's the CLI's own: starting the worker detached, its lock, and its log.

use std::fmt::Write as _;
use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::backup::Backups;
use carrel_core::intake::Processed;
use carrel_core::worker::{FileLock, Progress, Worker, WorkerError, file_in, now};
use serde_json::json;

use crate::commands::{holder, locate};
use crate::config::Identity;
use crate::{Failure, Report};

pub use carrel_core::worker::Status;

/// Set to `off` to never start a worker in the background, as in tests and some scripts.
pub const WORKER_ENV: &str = "CARREL_WORKER";

fn folder() -> Result<PathBuf, Failure> {
    Ok(crate::config::dir()?.join("workers"))
}

fn file_for(archive: &Archive, extension: &str) -> Result<PathBuf, Failure> {
    Ok(file_in(&folder()?, archive, extension))
}

/// The worker's log for `archive` on this device.
///
/// # Errors
///
/// [`Failure`] if there's no configuration folder.
pub fn log_path(archive: &Archive) -> Result<PathBuf, Failure> {
    file_for(archive, "log")
}

/// Opens the worker's log for appending, creating it and its folder: before the first worker
/// on this device, the folder isn't there yet.
fn open_log(path: &Path) -> Result<File, Failure> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Failure::io(dir, &e))?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| Failure::io(path, &e))
}

/// Held while a worker runs for an archive on this device.
pub type Lock = carrel_core::worker::Held;

/// The worker lock for `archive`.
fn lock_for(archive: &Archive) -> Result<FileLock, Failure> {
    Ok(FileLock::new(&folder()?, archive))
}

/// Takes the worker lock for `archive`, or `None` if another worker holds it.
///
/// # Errors
///
/// [`Failure`] if the lock file can't be opened.
pub fn try_lock(archive: &Archive) -> Result<Option<Lock>, Failure> {
    let lock = lock_for(archive)?;
    lock.take().map_err(|e| Failure::io(&lock.path, &e))
}

/// Whether a worker is running for `archive` on this device.
///
/// # Errors
///
/// [`Failure`] if the lock file can't be opened.
pub fn running(archive: &Archive) -> Result<bool, Failure> {
    let lock = lock_for(archive)?;
    lock.is_held().map_err(|e| Failure::io(&lock.path, &e))
}

/// Whether an agent (the Mac app's or `carrel agent`) looks after `archive` on this device, so
/// the CLI's worker steps aside.
#[must_use]
pub fn agent(archive: &Archive) -> bool {
    folder().is_ok_and(|folder| carrel_core::worker::agent_running(&folder, archive))
}

/// The running worker's status, if there is a worker and it has written one.
#[must_use]
pub fn status(archive: &Archive) -> Option<Status> {
    if !running(archive).unwrap_or(false) {
        return None;
    }
    carrel_core::worker::read_status(&file_for(archive, "json").ok()?)
}

/// Starts a worker for `archive` in the background unless one is running, the agent looks
/// after the archive, or `CARREL_WORKER` is `off`. Returns whether one was started.
///
/// # Errors
///
/// [`Failure`] if the worker can't be started.
pub fn start(archive: &Archive) -> Result<bool, Failure> {
    if std::env::var(WORKER_ENV).is_ok_and(|v| v.eq_ignore_ascii_case("off"))
        || agent(archive)
        || running(archive)?
    {
        return Ok(false);
    }
    let exe =
        std::env::current_exe().map_err(|e| Failure::new(format!("can't find carrel: {e}")))?;
    let log_file = log_path(archive)?;
    let log = open_log(&log_file)?;
    let mut command = std::process::Command::new(exe);
    command
        .arg("--archive")
        .arg(archive.root())
        .arg("worker")
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().map_err(|e| Failure::io(&log_file, &e))?)
        .stderr(log);
    detach(&mut command);
    command
        .spawn()
        .map_err(|e| Failure::new(format!("can't start the worker: {e}")))?;
    Ok(true)
}

/// Lets the worker outlive the terminal: its own process group, so the terminal's signals
/// don't reach it.
#[cfg(unix)]
fn detach(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt as _;
    command.process_group(0);
}

#[cfg(windows)]
fn detach(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt as _;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn detach(_: &mut std::process::Command) {}

/// The worker for `archive` on this device, with the providers and backups here, writing its
/// status file.
fn worker<'a>(
    archive: &'a Archive,
    registry: &'a carrel_core::providers::Registry,
    backups: Option<&'a dyn Backups>,
) -> Result<Worker<'a>, Failure> {
    let identity = Identity::load_or_create()?;
    Ok(Worker {
        archive,
        registry,
        holder: holder(&identity),
        backups,
        status: file_for(archive, "json").ok(),
        received: carrel_core::local::config_dir()
            .map(|dir| dir.join("received").join(archive.info().id.to_string())),
    })
}

/// Progress from closures, for [`work`]. Each drafted item's run is recorded in the log of
/// formats nothing here could read.
struct Calls<I, D> {
    on_item: I,
    on_done: D,
}

impl<I: FnMut(usize, usize, &str), D: FnMut(Processed)> Progress for Calls<I, D> {
    fn drafting(&mut self, done: usize, waiting: usize, name: &str) {
        (self.on_item)(done, waiting, name);
    }

    fn drafted(&mut self, processed: Processed) {
        if let Ok(run) = &processed.run {
            crate::formats::record(run);
        }
        (self.on_done)(processed);
    }
}

/// Drafts every waiting item for `archive` on this device, holding `lock`, until none are
/// left, calling `on_item` with each item as it starts (finished so far, waiting, name) and
/// `on_done` with each result. Items added while it runs are drafted too.
///
/// # Errors
///
/// [`Failure`] if the queues can't be read or written.
pub fn work(
    archive: &Archive,
    lock: Lock,
    on_item: impl FnMut(usize, usize, &str),
    on_done: impl FnMut(Processed),
) -> Result<usize, Failure> {
    let (registry, _) = crate::providers::registry();
    let (backups, _) = crate::backup::for_batch(archive);
    let backups = backups.as_ref().map(|b| b as &dyn Backups);
    let worker = worker(archive, &registry, backups)?;
    let mut calls = Calls { on_item, on_done };
    Ok(worker.draft(&lock_for(archive)?, lock, &mut calls)?)
}

/// Starts a worker for the archive at `archive` (or the current one) after a change to filed
/// items, so it embeds them for search, if the archive has an embedding model. Never fails:
/// the change is made, and the next change or `carrel index rebuild` catches up.
pub fn after_change(archive: Option<&Path>) {
    let Ok(archive) = locate(archive) else {
        return;
    };
    if archive.settings().embedding_model().is_some() {
        let _ = start(&archive);
    }
}

/// The worker loop as `carrel agent` runs it, holding the agent lock: drafts the queue for
/// this device, then embeds what's behind in the index, logging to standard error. `None` if
/// another worker (one started before the agent claimed the archive) is still drafting.
///
/// # Errors
///
/// [`Failure`] if the archive or its queues can't be read or written.
pub fn run_for_agent(archive: &Archive) -> Result<Option<carrel_core::worker::Ran>, Failure> {
    let Some(lock) = try_lock(archive)? else {
        return Ok(None);
    };
    let (registry, _) = crate::providers::registry();
    let (backups, _) = crate::backup::for_batch(archive);
    let backups = backups.as_ref().map(|b| b as &dyn Backups);
    let worker = worker(archive, &registry, backups)?;
    Ok(Some(worker.run(
        &lock_for(archive)?,
        lock,
        &mut Log::default(),
    )?))
}

/// `carrel worker`: drafts the queue for this device until it's empty, then embeds what's
/// behind in the index. It's what adding to the queue and changing filed items start in the
/// background; run by hand, it says if another is already running. It steps aside while the
/// Mac app's agent looks after the archive, which a worker started before the agent was may
/// find.
///
/// # Errors
///
/// [`Failure`] if the archive or its queues can't be read or written.
pub fn run(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    if agent(&archive) {
        return Ok(Report::new(
            "The Carrel agent looks after this archive's queue and index on this device.\n",
            json!({ "running": true, "agent": true, "done": 0 }),
        ));
    }
    let Some(lock) = try_lock(&archive)? else {
        return Ok(Report::new(
            "A worker is already drafting this archive's queue on this device.\n",
            json!({ "running": true, "done": 0 }),
        ));
    };
    let (registry, _) = crate::providers::registry();
    let (backups, _) = crate::backup::for_batch(&archive);
    let backups = backups.as_ref().map(|b| b as &dyn Backups);
    let worker = worker(&archive, &registry, backups)?;
    let ran = worker.run(&lock_for(&archive)?, lock, &mut Log::default())?;
    let (done, embedded) = (ran.drafted, ran.embedded);
    // With the queue drafted, the crawler may use the idle time, as its pace allows.
    let crawled = crate::crawl::when_idle(&archive).unwrap_or_else(|Failure(e)| {
        eprintln!("The crawler didn't run: {e}");
        None
    });
    let mut text = format!("Drafted {done} queued items.\n");
    if let Some(summary) = crawled
        .as_ref()
        .and_then(carrel_core::crawl::Crawled::summary)
    {
        writeln!(text, "The crawler {summary}.").unwrap();
    }
    if embedded > 0 {
        writeln!(
            text,
            "Embedded {} for search.",
            crate::commands::plural(embedded, "item")
        )
        .unwrap();
    }
    Ok(Report::new(
        text,
        json!({ "running": false, "done": done, "embedded": embedded }),
    ))
}

/// The worker's log, on standard error: a line for each item drafted and each round of
/// embedding, and progress reading text for the index.
#[derive(Default)]
struct Log {
    reading: Option<crate::progress::Display>,
}

impl Log {
    fn finish_reading(&mut self) {
        if let Some(mut display) = self.reading.take() {
            display.finish();
        }
    }
}

impl Progress for Log {
    fn drafting(&mut self, _: usize, waiting: usize, name: &str) {
        let _ = writeln!(
            std::io::stderr(),
            "{} drafting {name} ({waiting} waiting)",
            now()
        );
    }

    fn drafted(&mut self, processed: Processed) {
        if let Ok(run) = &processed.run {
            crate::formats::record(run);
        }
        let line = match &processed.run {
            Ok(run) => {
                let proposed = run.entries.iter().flatten().count();
                format!("proposed {proposed} for {}", processed.entry.path.display())
            }
            Err(failure) => format!(
                "failed ({} of {}): {failure}",
                processed.entry.attempts,
                carrel_core::intake::MAX_ATTEMPTS
            ),
        };
        let _ = writeln!(std::io::stderr(), "{} {line}", now());
    }

    fn reading(&mut self, done: usize, total: usize, item: Option<&str>) {
        self.reading
            .get_or_insert_with(|| crate::progress::Display::new("Reading text", false))
            .count("items", done, total, item);
    }

    fn embedding(&mut self, _: usize, _: usize, _: Option<&str>) {
        self.finish_reading();
    }

    fn embedded(&mut self, items: usize) {
        self.finish_reading();
        if items > 0 {
            let _ = writeln!(
                std::io::stderr(),
                "{} embedded {} for search",
                now(),
                crate::commands::plural(items, "item")
            );
        }
    }

    fn embedding_failed(&mut self, error: WorkerError) {
        self.finish_reading();
        let message = match error {
            WorkerError::Index { model, source } => crate::index::failure(source, &model).0,
            e => e.to_string(),
        };
        let _ = writeln!(std::io::stderr(), "{} embedding failed: {message}", now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_and_its_folder_are_made_before_the_first_worker() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("workers").join("archive.log");
        let mut file = open_log(&log).unwrap();
        writeln!(file, "started").unwrap();
        drop(file);
        let mut again = open_log(&log).unwrap();
        writeln!(again, "again").unwrap();
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "started\nagain\n",
            "it appends"
        );
    }
}
