//! `carrel worker`: drafts the items waiting in an archive's intake queue on this device, one
//! at a time, until none are left (`docs/design/tui-ingest.md`).
//!
//! Adding to the queue starts a worker in the background, detached so it outlives the
//! terminal, unless one is already running. An operating-system lock on a file in Carrel's
//! configuration folder keeps it to one per archive and device; the lock goes with the process,
//! so a crashed worker never leaves a stale one. The worker writes its progress to a status
//! file, which `carrel ingest --status` and the interactive mode read, and a log beside it.

use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::backup::Backups;
use carrel_core::intake::{Processed, process, waiting};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::commands::{holder, locate};
use crate::config::Identity;
use crate::{Failure, Report};

/// Set to `off` to never start a worker in the background, as in tests and some scripts.
pub const WORKER_ENV: &str = "CARREL_WORKER";

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

fn folder() -> Result<PathBuf, Failure> {
    Ok(crate::config::dir()?.join("workers"))
}

fn file_for(archive: &Archive, extension: &str) -> Result<PathBuf, Failure> {
    Ok(folder()?.join(format!("{}.{extension}", archive.info().id)))
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
pub struct Lock {
    file: File,
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// Takes the worker lock for `archive`, or `None` if another worker holds it.
///
/// # Errors
///
/// [`Failure`] if the lock file can't be opened.
pub fn try_lock(archive: &Archive) -> Result<Option<Lock>, Failure> {
    let path = file_for(archive, "lock")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Failure::io(dir, &e))?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| Failure::io(&path, &e))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(Lock { file })),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(Failure::io(&path, &e)),
    }
}

/// Whether a worker is running for `archive` on this device.
///
/// # Errors
///
/// [`Failure`] if the lock file can't be opened.
pub fn running(archive: &Archive) -> Result<bool, Failure> {
    // Only looks: a missing lock file means no worker has ever run here, and nothing is
    // created, so asking never writes to the configuration folder.
    let path = file_for(archive, "lock")?;
    let file = match std::fs::OpenOptions::new().write(true).open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(Failure::io(&path, &e)),
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = file.unlock();
            Ok(false)
        }
        Err(std::fs::TryLockError::WouldBlock) => Ok(true),
        Err(std::fs::TryLockError::Error(e)) => Err(Failure::io(&path, &e)),
    }
}

/// The running worker's status, if there is a worker and it has written one.
#[must_use]
pub fn status(archive: &Archive) -> Option<Status> {
    if !running(archive).unwrap_or(false) {
        return None;
    }
    let path = file_for(archive, "json").ok()?;
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn write_status(archive: &Archive, status: &Status) {
    if let Ok(path) = file_for(archive, "json")
        && let Ok(text) = serde_json::to_string(status)
    {
        let temp = path.with_extension("json.tmp");
        if std::fs::write(&temp, text).is_ok() {
            let _ = std::fs::rename(&temp, &path);
        }
    }
}

/// Starts a worker for `archive` in the background unless one is running or `CARREL_WORKER`
/// is `off`. Returns whether one was started.
///
/// # Errors
///
/// [`Failure`] if the worker can't be started.
pub fn start(archive: &Archive) -> Result<bool, Failure> {
    if std::env::var(WORKER_ENV).is_ok_and(|v| v.eq_ignore_ascii_case("off")) || running(archive)? {
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
    mut on_item: impl FnMut(usize, usize, &str),
    mut on_done: impl FnMut(Processed),
) -> Result<usize, Failure> {
    let identity = Identity::load_or_create()?;
    let holder = holder(&identity);
    let (registry, _) = crate::providers::registry();
    let (backups, _) = crate::backup::for_batch(archive);
    let backups = backups.as_ref().map(|b| b as &dyn Backups);
    let mut lock = Some(lock);
    let mut done = 0;
    loop {
        let mut queue = waiting(archive, identity.device, false)?;
        if queue.is_empty() {
            // Let go, then look once more: an item added while the lock was held, by someone
            // who saw a worker running and so started none, would otherwise wait for the next.
            drop(lock.take());
            if waiting(archive, identity.device, false)?.is_empty() {
                return Ok(done);
            }
            match try_lock(archive)? {
                Some(again) => lock = Some(again),
                None => return Ok(done),
            }
            continue;
        }
        let entry = queue.remove(0);
        let name = entry.path.file_name().map_or_else(
            || entry.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        on_item(done, queue.len() + 1, &name);
        let mut status = Status {
            pid: std::process::id(),
            item: Some(name.clone()),
            stage: None,
            done,
            waiting: queue.len() + 1,
            updated: now(),
        };
        write_status(archive, &status);
        let processed = process(archive, &registry, entry, holder, backups, &mut |p| {
            status.stage = Some(p.stage.describe().to_owned());
            status.updated = now();
            write_status(archive, &status);
        })
        .map_err(Failure::from)?;
        on_done(processed);
        done += 1;
    }
}

/// `carrel worker`: drafts the queue for this device until it's empty. It's what adding to
/// the queue starts in the background; run by hand, it says if another is already running.
///
/// # Errors
///
/// [`Failure`] if the archive or its queues can't be read or written.
pub fn run(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let Some(lock) = try_lock(&archive)? else {
        return Ok(Report::new(
            "A worker is already drafting this archive's queue on this device.\n",
            json!({ "running": true, "done": 0 }),
        ));
    };
    let mut log = std::io::stderr();
    let done = work(
        &archive,
        lock,
        |_, waiting, name| {
            let _ = writeln!(log, "{} drafting {name} ({waiting} waiting)", now());
        },
        |processed| {
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
        },
    )?;
    if let Ok(path) = file_for(&archive, "json") {
        let _ = std::fs::remove_file(path);
    }
    Ok(Report::new(
        format!("Drafted {done} queued items.\n"),
        json!({ "running": false, "done": done }),
    ))
}

fn now() -> String {
    jiff::Zoned::now()
        .strftime("%Y-%m-%dT%H:%M:%S%:z")
        .to_string()
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
