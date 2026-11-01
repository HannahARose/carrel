//! The background agent's loop (`docs/design/mac-app.md`, "The background agent"), as
//! `carrel agent` runs it on every platform: for each archive it follows, it claims the
//! archive so the CLI's own background worker steps aside, notices changes in the archive and
//! in the working copies of what's checked out and on the desk, and runs the jobs its
//! [`Plan`] says are due, one at a time: the worker loop, recording work sessions, and
//! scheduled snapshots.
//!
//! The claim is the agent lock ([`crate::worker::agent_lock`]) the Mac app's agent holds too,
//! so there's one agent per archive and device: while another agent (the Mac app's, or
//! another `carrel agent`) has an archive, this one steps aside, says so, and tries again
//! every minute in case it has gone. Everything else goes through the archive's files and the
//! writer lease, as between the CLI's commands: a job that finds another writer holding the
//! archive is put off for half a minute.
//!
//! What each job does is the caller's ([`Jobs`]), since drafting needs the providers it has;
//! what happens is reported as [`Event`]s, so each interface shows it in its own way.
//! [`service`] installs the agent to start at log-in.

pub mod plan;
pub mod poll;
pub mod service;
pub mod watch;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::archive::Archive;
use crate::worker::{Held, agent_lock, agent_running, file_in};

pub use plan::{Job, Plan};
pub use poll::Poller;
pub use watch::{How, Watcher};

/// How often an archive another agent has is tried again.
pub const CLAIM_RETRY: Duration = Duration::from_mins(1);
/// How long a job is put off when another writer holds the archive.
pub const BUSY_RETRY: Duration = Duration::from_secs(30);
/// How often what's checked out and on the desk, the idle time and the backups are read again,
/// besides when a change says to.
pub const REFRESH: Duration = Duration::from_mins(10);
/// The longest the loop sleeps.
const LONGEST_SLEEP: Duration = Duration::from_mins(1);
/// The shortest the loop sleeps, so a job that keeps failing at once doesn't spin.
const SHORTEST_SLEEP: Duration = Duration::from_millis(200);

/// Why a job didn't run to the end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobError {
    /// Another writer holds the archive, or another worker is drafting its queue: the job is
    /// tried again shortly.
    Busy,
    /// The job failed, in the caller's words. A failed snapshot is retried in an hour.
    Failed(String),
}

/// What the agent's jobs do, supplied by whoever runs the loop.
pub trait Jobs {
    /// Runs `job` for `archive`, freshly opened: what it did, as a line for people, or
    /// `None` when there was nothing to do.
    ///
    /// # Errors
    ///
    /// [`JobError`] if it couldn't run now, or failed.
    fn run(&mut self, archive: &Archive, job: Job) -> Result<Option<String>, JobError>;

    /// The working copies of what's checked out of `archive` and on this device's desk.
    fn working_copies(&self, archive: &Archive) -> Vec<PathBuf>;

    /// Whether backups are set up for `archive` on this device, so snapshots are scheduled.
    fn backups_set_up(&self, archive: &Archive) -> bool;
}

/// What happened, for the interface to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The agent looks after the archive now.
    Claimed {
        /// The archive's name.
        name: String,
        /// Its root folder.
        root: PathBuf,
        /// Whether it took over from another agent that had it.
        took_over: bool,
        /// How it notices changes.
        how: How,
    },
    /// Another agent looks after the archive, so this one steps aside until it goes.
    SteppedAside {
        /// The archive's name.
        name: String,
        /// Its root folder.
        root: PathBuf,
    },
    /// The archive couldn't be opened; it's tried again every minute.
    CannotOpen {
        /// The folder given.
        root: PathBuf,
        /// Why not.
        error: String,
    },
    /// A job started.
    Started {
        /// The archive's name.
        name: String,
        /// The job.
        job: Job,
    },
    /// A job finished.
    Finished {
        /// The archive's name.
        name: String,
        /// The job.
        job: Job,
        /// What it did, if anything.
        summary: Option<String>,
    },
    /// A job was put off because another writer had the archive.
    Postponed {
        /// The archive's name.
        name: String,
        /// The job.
        job: Job,
    },
    /// A job failed.
    Failed {
        /// The archive's name.
        name: String,
        /// The job.
        job: Job,
        /// Why.
        error: String,
    },
}

/// What the agent remembers about an archive between runs, beside its lock in the workers'
/// folder (`<archive id>.agent.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// When the last scheduled snapshot was taken or tried, in seconds since 1970.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_backup: Option<u64>,
    /// Whether it failed.
    #[serde(default)]
    pub backup_failed: bool,
}

impl State {
    /// Where the agent keeps its state for `archive` in the workers' folder `folder`.
    #[must_use]
    pub fn path(folder: &Path, archive: &Archive) -> PathBuf {
        file_in(folder, archive, "agent.json")
    }

    /// The state at `path`, or none yet.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Saves the state at `path`. Best effort: the worst is a snapshot taken early.
    pub fn save(&self, path: &Path) {
        if let Ok(text) = serde_json::to_string(self) {
            let temp = path.with_extension("json.tmp");
            if std::fs::write(&temp, text).is_ok() {
                let _ = std::fs::rename(&temp, path);
            }
        }
    }

    fn last_backup(&self) -> Option<SystemTime> {
        self.last_backup
            .map(|s| SystemTime::UNIX_EPOCH + Duration::from_secs(s))
    }
}

/// Whether an agent looks after each of the archives at `roots` on this device, with the
/// workers' folder `folder`: what `carrel agent status` shows. An archive that can't be
/// opened counts as not looked after.
#[must_use]
pub fn looked_after(folder: &Path, roots: &[PathBuf]) -> Vec<(PathBuf, bool)> {
    roots
        .iter()
        .map(|root| {
            let running = Archive::open(root).is_ok_and(|a| agent_running(folder, &a));
            (root.clone(), running)
        })
        .collect()
}

/// One archive the agent follows.
#[derive(Debug)]
struct Followed {
    root: PathBuf,
    name: String,
    claim: Option<Held>,
    stepped_aside: bool,
    cannot_open: bool,
    tried: Option<SystemTime>,
    plan: Option<Plan>,
    watcher: Option<Watcher>,
    next_poll: SystemTime,
    refreshed: SystemTime,
}

impl Followed {
    fn new(root: PathBuf) -> Self {
        let name = root.file_name().map_or_else(
            || root.display().to_string(),
            |n| n.to_string_lossy().into(),
        );
        Self {
            root,
            name,
            claim: None,
            stepped_aside: false,
            cannot_open: false,
            tried: None,
            plan: None,
            watcher: None,
            next_poll: SystemTime::UNIX_EPOCH,
            refreshed: SystemTime::UNIX_EPOCH,
        }
    }
}

/// The agent: the archives it follows, and what it's claimed.
#[derive(Debug)]
pub struct Agent {
    folder: PathBuf,
    archives: Vec<Followed>,
    polling: bool,
}

impl Agent {
    /// An agent following the archives at `roots`, keeping its locks and state in the
    /// workers' folder `folder` ([`crate::worker::folder`]). It notices changes through file
    /// events, or by looking every few seconds when `polling`. It claims nothing until it
    /// runs.
    #[must_use]
    pub fn new(folder: &Path, roots: &[PathBuf], polling: bool) -> Self {
        Self {
            folder: folder.to_owned(),
            archives: roots.iter().cloned().map(Followed::new).collect(),
            polling,
        }
    }

    /// Whether the agent looks after the archive at `root` now.
    #[must_use]
    pub fn claimed(&self, root: &Path) -> bool {
        self.archives
            .iter()
            .any(|f| f.root == root && f.claim.is_some())
    }

    /// The archive at `root`'s schedule, once it's claimed.
    #[must_use]
    pub fn plan(&self, root: &Path) -> Option<&Plan> {
        self.archives
            .iter()
            .find(|f| f.root == root)
            .and_then(|f| f.plan.as_ref())
    }

    /// Does what's due at `now`: claims archives, looks for changes, and runs due jobs,
    /// reporting to `events`. Returns how long to wait before calling again.
    pub fn tick(
        &mut self,
        jobs: &mut dyn Jobs,
        events: &mut dyn FnMut(Event),
        now: SystemTime,
    ) -> Duration {
        let folder = self.folder.clone();
        let polling = self.polling;
        for followed in &mut self.archives {
            if followed.claim.is_none() {
                claim(&folder, followed, jobs, events, now, polling);
            }
            if followed.claim.is_some() {
                look(followed, jobs, now, polling);
                run_due(&folder, followed, jobs, events, now);
            }
        }
        self.wait(now)
    }

    /// How long until anything is next due, from `now`.
    fn wait(&self, now: SystemTime) -> Duration {
        let next = self
            .archives
            .iter()
            .flat_map(|f| {
                if f.claim.is_some() {
                    let due = f.plan.as_ref().and_then(Plan::next_due);
                    [due, Some(f.next_poll)]
                } else {
                    [f.tried.map(|t| t + CLAIM_RETRY), None]
                }
            })
            .flatten()
            .min();
        next.map_or(LONGEST_SLEEP, |next| {
            next.duration_since(now).unwrap_or_default()
        })
        .clamp(SHORTEST_SLEEP, LONGEST_SLEEP)
    }

    /// Runs until `stop` says to, then lets go of every archive.
    pub fn run(
        &mut self,
        jobs: &mut dyn Jobs,
        events: &mut dyn FnMut(Event),
        stop: &dyn Fn() -> bool,
    ) {
        while !stop() {
            let mut wait = self.tick(jobs, events, SystemTime::now());
            while !wait.is_zero() && !stop() {
                let slice = wait.min(Duration::from_millis(250));
                std::thread::sleep(slice);
                wait -= slice;
            }
        }
        self.release();
    }

    /// Lets go of every archive, so the CLI's worker drafts again.
    pub fn release(&mut self) {
        for followed in &mut self.archives {
            followed.claim = None;
            followed.watcher = None;
        }
    }
}

/// Tries to claim `followed`, once a minute.
fn claim(
    folder: &Path,
    followed: &mut Followed,
    jobs: &dyn Jobs,
    events: &mut dyn FnMut(Event),
    now: SystemTime,
    polling: bool,
) {
    if followed
        .tried
        .is_some_and(|tried| now < tried + CLAIM_RETRY)
    {
        return;
    }
    followed.tried = Some(now);
    let archive = match Archive::open(&followed.root) {
        Ok(archive) => archive,
        Err(e) => {
            if !followed.cannot_open {
                followed.cannot_open = true;
                events(Event::CannotOpen {
                    root: followed.root.clone(),
                    error: e.to_string(),
                });
            }
            return;
        }
    };
    followed.cannot_open = false;
    followed.name.clone_from(&archive.doc().name);
    let held = match agent_lock(folder, &archive).take() {
        Ok(Some(held)) => held,
        Ok(None) => {
            if !followed.stepped_aside {
                followed.stepped_aside = true;
                events(Event::SteppedAside {
                    name: followed.name.clone(),
                    root: followed.root.clone(),
                });
            }
            return;
        }
        Err(e) => {
            events(Event::CannotOpen {
                root: followed.root.clone(),
                error: e.to_string(),
            });
            return;
        }
    };
    followed.claim = Some(held);
    let state = State::load(&State::path(folder, &archive));
    let mut plan = Plan::new(
        &real(archive.root()),
        archive.settings().sessions.idle_minutes,
    );
    plan.last_backup = state.last_backup();
    plan.backup_failed = state.backup_failed;
    plan.start(now);
    followed.plan = Some(plan);
    refresh(followed, &archive, jobs, now, polling);
    events(Event::Claimed {
        name: followed.name.clone(),
        root: followed.root.clone(),
        took_over: followed.stepped_aside,
        how: followed.watcher.as_ref().map_or(How::Polling, Watcher::how),
    });
    followed.stepped_aside = false;
}

/// Reads what's checked out and on the desk, the idle time and the backups again, and
/// watches the working copies.
fn refresh(
    followed: &mut Followed,
    archive: &Archive,
    jobs: &dyn Jobs,
    now: SystemTime,
    polling: bool,
) {
    let working: Vec<PathBuf> = jobs
        .working_copies(archive)
        .iter()
        .map(|w| real(w))
        .collect();
    let backups = jobs.backups_set_up(archive);
    let Some(plan) = followed.plan.as_mut() else {
        return;
    };
    plan.idle = Duration::from_mins(u64::from(archive.settings().sessions.idle_minutes));
    plan.backups_set_up = backups;
    let moved = plan.working_copies != working;
    plan.working_copies.clone_from(&working);
    match followed.watcher.as_mut() {
        // Watched again even when unchanged, so a working copy that wasn't there yet is.
        Some(watcher) if moved || watcher.how() == How::Events => watcher.watch(&working),
        Some(_) => {}
        None => {
            let watcher = Watcher::new(&plan.root, &working, polling);
            followed.next_poll = now + watcher.interval();
            followed.watcher = Some(watcher);
        }
    }
    followed.refreshed = now;
}

/// Looks for changes, when it's time to.
fn look(followed: &mut Followed, jobs: &dyn Jobs, now: SystemTime, polling: bool) {
    let refresh_due = now >= followed.refreshed + REFRESH;
    let mut active = false;
    if now >= followed.next_poll
        && let (Some(watcher), Some(plan)) = (followed.watcher.as_mut(), followed.plan.as_mut())
    {
        let changes = watcher.changes();
        active = plan.noticed(changes.iter().map(PathBuf::as_path), now);
        followed.next_poll = now + watcher.interval();
    }
    if (active || refresh_due)
        && let Ok(archive) = Archive::open(&followed.root)
    {
        refresh(followed, &archive, jobs, now, polling);
    }
}

/// `path` with links resolved, as file events report it (`/private/var` for `/var` on a
/// Mac), or as it is if it isn't there.
fn real(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

/// Runs the jobs due for `followed`, one at a time.
fn run_due(
    folder: &Path,
    followed: &mut Followed,
    jobs: &mut dyn Jobs,
    events: &mut dyn FnMut(Event),
    now: SystemTime,
) {
    let due = followed
        .plan
        .as_ref()
        .map(|p| p.due(now))
        .unwrap_or_default();
    for job in due {
        let name = followed.name.clone();
        let result = match Archive::open(&followed.root) {
            Ok(archive) => {
                // The crawler looks every few minutes and usually finds nothing due, so it's
                // reported only when it did something.
                if job != Job::Crawl {
                    events(Event::Started {
                        name: name.clone(),
                        job,
                    });
                }
                let result = jobs.run(&archive, job);
                if job == Job::Backup {
                    let state = State {
                        last_backup: now
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .ok()
                            .map(|d| d.as_secs()),
                        backup_failed: result.is_err(),
                    };
                    state.save(&State::path(folder, &archive));
                }
                result
            }
            Err(e) => Err(JobError::Failed(e.to_string())),
        };
        let Some(plan) = followed.plan.as_mut() else {
            return;
        };
        match result {
            Ok(summary) => {
                plan.ran(job, now, true);
                if job != Job::Crawl || summary.is_some() {
                    events(Event::Finished { name, job, summary });
                }
            }
            Err(JobError::Busy) if job != Job::Backup => {
                plan.postpone(job, now + BUSY_RETRY);
                events(Event::Postponed { name, job });
            }
            Err(JobError::Busy) => {
                // Tried again within the hour, like a snapshot that failed.
                plan.ran(job, now, false);
                events(Event::Postponed { name, job });
            }
            Err(JobError::Failed(error)) => {
                plan.ran(job, now, false);
                events(Event::Failed { name, job, error });
            }
        }
    }
}

#[cfg(test)]
mod tests;
