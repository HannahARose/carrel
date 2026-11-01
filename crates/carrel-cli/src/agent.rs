//! `carrel agent`: the background agent's work from the command line, on macOS, Linux and
//! Windows (`docs/design/mac-app.md`, "The background agent"). It runs the core's agent loop
//! ([`carrel_core::agent`]) in the foreground until stopped: the worker loop as items are
//! queued and filed items change, work sessions recorded as they finish, and scheduled
//! snapshots.
//!
//! It holds the same agent lock as the Mac app's agent, so there's one agent per archive and
//! device: it steps aside, and says so, while the Mac app's agent runs, and the CLI's own
//! detached worker steps aside for it.
//!
//! `carrel agent install|uninstall|status` start it at log-in as a launchd user agent, a
//! systemd user service or a Task Scheduler task ([`carrel_core::agent::service`]).

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use carrel_core::agent::service::{self, Definition, Manager, Places, Platform};
use carrel_core::agent::{Agent, Event, How, Job, JobError, Jobs};
use carrel_core::archive::Archive;
use carrel_core::backup::{Kopia, SnapshotNowError, snapshot_now};
use carrel_core::card::Id;
use carrel_core::coordinator::{Coordinator as _, CoordinatorError, DEFAULT_LEASE, DiskLock};
use serde_json::{Value, json};

use crate::commands::{holder, locate, plural};
use crate::config::Identity;
use crate::{Failure, Report};

/// Set to `off` to write and remove the service's file without running the service manager,
/// as in tests.
pub const SERVICE_MANAGER_ENV: &str = "CARREL_SERVICE_MANAGER";

/// The jobs as the CLI does them.
struct CliJobs {
    identity: Identity,
}

/// A lease error as a job's: another writer means try again shortly.
fn lease_error(error: CoordinatorError) -> JobError {
    match error {
        CoordinatorError::Held(_) => JobError::Busy,
        e => JobError::Failed(e.to_string()),
    }
}

impl CliJobs {
    fn work(archive: &Archive) -> Result<Option<String>, JobError> {
        let ran = crate::worker::run_for_agent(archive)
            .map_err(|Failure(e)| JobError::Failed(e))?
            .ok_or(JobError::Busy)?;
        let mut done = Vec::new();
        if ran.drafted > 0 {
            done.push(format!("drafted {}", plural(ran.drafted, "item")));
        }
        if ran.embedded > 0 {
            done.push(format!(
                "embedded {} for search",
                plural(ran.embedded, "item")
            ));
        }
        Ok((!done.is_empty()).then(|| done.join(", ")))
    }

    /// A crawler pass, when its pace says one is due and the device is idle and on power.
    fn crawl(archive: &Archive) -> Result<Option<String>, JobError> {
        let crawled = crate::crawl::when_idle(archive).map_err(|Failure(e)| JobError::Failed(e))?;
        Ok(crawled.and_then(|c| c.summary()))
    }

    fn sessions(&self, archive: &Archive) -> Result<Option<String>, JobError> {
        let lease = DiskLock::for_archive(archive)
            .acquire(holder(&self.identity), "sessions", DEFAULT_LEASE)
            .map_err(lease_error)?;
        let (text, _) = crate::sessions::record_and_snapshot(archive, lease)
            .map_err(|Failure(e)| JobError::Failed(e))?;
        let text = text.trim();
        Ok((!text.is_empty()).then(|| text.replace('\n', "; ")))
    }

    fn backup(&self, archive: &Archive) -> Result<Option<String>, JobError> {
        let kopia = Kopia::for_archive(archive).map_err(|e| JobError::Failed(e.to_string()))?;
        let lease = DiskLock::for_archive(archive)
            .acquire(holder(&self.identity), "backup", DEFAULT_LEASE)
            .map_err(lease_error)?;
        let result = snapshot_now(archive, &kopia, &lease, "Scheduled snapshot");
        let _ = lease.release();
        let (snapshot, _) = result.map_err(|e| {
            JobError::Failed(match e {
                SnapshotNowError::Backup(e) => e.to_string(),
                SnapshotNowError::History(e) => e.to_string(),
            })
        })?;
        Ok(Some(format!("snapshot {} taken", snapshot.trailer())))
    }

    fn device(&self) -> Id {
        self.identity.device
    }
}

impl Jobs for CliJobs {
    fn run(&mut self, archive: &Archive, job: Job) -> Result<Option<String>, JobError> {
        match job {
            Job::Work => Self::work(archive),
            Job::Sessions => self.sessions(archive),
            Job::Backup => self.backup(archive),
            Job::Crawl => Self::crawl(archive),
            Job::Remind => {
                crate::reminders::remind_for_agent(archive, self.device()).map_err(JobError::Failed)
            }
        }
    }

    fn working_copies(&self, archive: &Archive) -> Vec<PathBuf> {
        carrel_core::active::items(archive, Some(self.device()))
            .into_iter()
            .map(|item| item.working)
            .collect()
    }

    fn backups_set_up(&self, archive: &Archive) -> bool {
        Kopia::for_archive(archive).is_ok()
    }
}

/// An event as a line for people, and as JSON.
fn describe(event: &Event) -> (String, Value) {
    match event {
        Event::Claimed {
            name,
            root,
            took_over,
            how,
        } => {
            let how_text = match how {
                How::Events => "file events",
                How::Polling => "looking every few seconds",
            };
            let start = if *took_over {
                "The other agent has stopped; looking after"
            } else {
                "Looking after"
            };
            (
                format!(
                    "{start} “{name}” ({}), noticing changes through {how_text}.",
                    root.display()
                ),
                json!({"event": "claimed", "archive": root, "took_over": took_over,
                       "how": if *how == How::Events { "events" } else { "polling" }}),
            )
        }
        Event::SteppedAside { name, root } => (
            format!(
                "Another Carrel agent, such as the Mac app's, looks after “{name}” on this device. \
                 Stepping aside until it stops."
            ),
            json!({"event": "stepped_aside", "archive": root}),
        ),
        Event::CannotOpen { root, error } => (
            format!(
                "Can't open {}: {error}. Trying again every minute.",
                root.display()
            ),
            json!({"event": "cannot_open", "archive": root, "error": error}),
        ),
        Event::Started { name, job } => (
            format!("“{name}”: {}…", job.describe()),
            json!({"event": "started", "archive": name, "job": job_name(*job)}),
        ),
        Event::Finished { name, job, summary } => (
            format!(
                "“{name}”: {}",
                summary.as_deref().unwrap_or("nothing to do")
            ),
            json!({"event": "finished", "archive": name, "job": job_name(*job), "summary": summary}),
        ),
        Event::Postponed { name, job } => (
            format!(
                "“{name}”: another writer has the archive; {} later.",
                job.describe()
            ),
            json!({"event": "postponed", "archive": name, "job": job_name(*job)}),
        ),
        Event::Failed { name, job, error } => (
            format!("“{name}”: {} failed: {error}", job.describe()),
            json!({"event": "failed", "archive": name, "job": job_name(*job), "error": error}),
        ),
    }
}

fn job_name(job: Job) -> &'static str {
    match job {
        Job::Work => "work",
        Job::Sessions => "sessions",
        Job::Backup => "backup",
        Job::Crawl => "crawl",
        Job::Remind => "remind",
    }
}

/// The archives to follow: those given, else the one found as for any command.
fn roots(archives: &[PathBuf], archive: Option<&Path>) -> Result<Vec<PathBuf>, Failure> {
    if archives.is_empty() {
        return Ok(vec![locate(archive)?.root().to_owned()]);
    }
    archives
        .iter()
        .map(|path| {
            let found = Archive::find(path)
                .map_err(|e| Failure::new(format!("{}: {e}", path.display())))?;
            Ok(std::fs::canonicalize(found.root()).unwrap_or_else(|_| found.root().to_owned()))
        })
        .collect()
}

/// `carrel agent`: runs the agent in the foreground until stopped (or for `stop_after`, in
/// tests), writing what it does as it happens: a line each, or with `json` a JSON value each.
///
/// # Errors
///
/// [`Failure`] if an archive given can't be found or there's no configuration folder.
pub fn run(
    archives: &[PathBuf],
    archive: Option<&Path>,
    poll: bool,
    stop_after: Option<Duration>,
    json: bool,
) -> Result<Report, Failure> {
    let roots = roots(archives, archive)?;
    let folder = crate::config::dir()?.join("workers");
    let mut jobs = CliJobs {
        identity: Identity::load_or_create()?,
    };
    let mut agent = Agent::new(&folder, &roots, poll);
    if !json {
        println!(
            "The Carrel agent is running for {}. Stop it with Ctrl-C.",
            plural(roots.len(), "archive")
        );
    }
    let mut events = |event: Event| {
        let (text, value) = describe(&event);
        let mut out = std::io::stdout().lock();
        let _ = if json {
            writeln!(out, "{value}")
        } else {
            writeln!(out, "{} {text}", carrel_core::worker::now())
        };
        let _ = out.flush();
    };
    let started = Instant::now();
    let stop = || stop_after.is_some_and(|after| started.elapsed() >= after);
    agent.run(&mut jobs, &mut events, &stop);
    Ok(Report::new(
        "The Carrel agent has stopped.\n",
        json!({ "event": "stopped" }),
    ))
}

/// The service manager, or one that only says what it would have run when
/// `CARREL_SERVICE_MANAGER` is `off`.
pub(crate) fn manager() -> (Box<dyn Manager>, bool) {
    struct Off;
    impl Manager for Off {
        fn run(&mut self, _: &str, _: &[String]) -> Result<(), String> {
            Ok(())
        }
    }
    if std::env::var(SERVICE_MANAGER_ENV).is_ok_and(|v| v.eq_ignore_ascii_case("off")) {
        (Box::new(Off), false)
    } else {
        (Box::new(service::System), true)
    }
}

fn platform() -> Result<Platform, Failure> {
    Platform::current().ok_or_else(|| {
        Failure::new("Carrel doesn't know how to start the agent at log-in on this platform; run `carrel agent` instead")
    })
}

fn places() -> Result<Places, Failure> {
    Places::here().ok_or_else(|| Failure::new("there's no home folder to install the agent in"))
}

/// The archives' names, for people.
fn names(roots: &[PathBuf]) -> String {
    roots
        .iter()
        .map(|root| {
            Archive::open(root).map_or_else(
                |_| root.display().to_string(),
                |a| format!("“{}”", a.doc().name),
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Lines for the service manager's commands: what ran, or would have.
fn commands_text(done: &service::Done, ran: bool, text: &mut String) {
    let verb = if ran {
        "Ran"
    } else {
        "Didn't run (CARREL_SERVICE_MANAGER=off)"
    };
    for line in &done.ran {
        writeln!(text, "{verb}: {line}").unwrap();
    }
    for line in &done.failed {
        writeln!(text, "Failed: {line}").unwrap();
    }
}

/// `carrel agent install`: starts the agent at log-in for `archives` (or the archive found as
/// for any command), and now.
///
/// # Errors
///
/// [`Failure`] if an archive can't be found, the file can't be written, or the service manager
/// won't start it.
pub fn install(
    archives: &[PathBuf],
    archive: Option<&Path>,
    poll: bool,
) -> Result<Report, Failure> {
    let platform = platform()?;
    let places = places()?;
    let roots = roots(archives, archive)?;
    let (mut manager, ran) = manager();
    install_in(platform, &places, &roots, poll, manager.as_mut(), ran)
}

/// Installs the agent as `platform`'s service in `places` for `roots`, through `manager`
/// (which `ran` says runs commands for real), saying exactly what it wrote and ran.
pub(crate) fn install_in(
    platform: Platform,
    places: &Places,
    roots: &[PathBuf],
    poll: bool,
    manager: &mut dyn Manager,
    ran: bool,
) -> Result<Report, Failure> {
    let program =
        std::env::current_exe().map_err(|e| Failure::new(format!("can't find carrel: {e}")))?;
    // The agent finds Kopia and the providers, and uses this configuration folder, as here.
    let environment = ["PATH", "CARREL_CONFIG_DIR", "CARREL_MODELS_DIR"]
        .into_iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .filter(|v| !v.is_empty())
                .map(|v| (key.to_owned(), v))
        })
        .collect();
    let definition = Definition {
        program,
        archives: roots.to_vec(),
        poll,
        environment,
        log: places.log(),
    };
    let done = service::install(platform, places, &definition, manager)?;
    let mut text = format!(
        "{} the Carrel agent as {} for {}, starting at log-in.\n",
        if done.was_installed {
            "Reinstalled"
        } else {
            "Installed"
        },
        platform.describe(),
        names(roots)
    );
    writeln!(text, "Wrote {}", done.file.display()).unwrap();
    commands_text(&done, ran, &mut text);
    match platform {
        Platform::Launchd | Platform::TaskScheduler => {
            writeln!(text, "Its log: {}", places.log().display()).unwrap();
        }
        Platform::Systemd => {
            text.push_str("systemd links it into default.target.wants to start it at log-in. Its log: journalctl --user -u carrel-agent\n");
        }
    }
    Ok(Report::new(
        text,
        json!({
            "installed": true,
            "replaced": done.was_installed,
            "as": platform.describe(),
            "file": done.file,
            "archives": roots,
            "ran": if ran { done.ran.clone() } else { Vec::new() },
            "failed": done.failed,
        }),
    ))
}

/// `carrel agent uninstall`: stops the agent started at log-in and removes exactly what
/// `install` wrote.
///
/// # Errors
///
/// [`Failure`] if the file can't be removed.
pub fn uninstall() -> Result<Report, Failure> {
    let platform = platform()?;
    let places = places()?;
    let (mut manager, ran) = manager();
    uninstall_in(platform, &places, manager.as_mut(), ran)
}

/// Uninstalls `platform`'s service from `places`, through `manager` (which `ran` says runs
/// commands for real), saying exactly what it removed and ran.
pub(crate) fn uninstall_in(
    platform: Platform,
    places: &Places,
    manager: &mut dyn Manager,
    ran: bool,
) -> Result<Report, Failure> {
    let done = service::uninstall(platform, places, manager)?;
    if !done.was_installed {
        return Ok(Report::new(
            format!(
                "The Carrel agent isn't installed here: there's no {}. Nothing was changed.\n",
                done.file.display()
            ),
            json!({"installed": false, "removed": Value::Null}),
        ));
    }
    let mut text = format!(
        "Uninstalled the Carrel agent ({}).\nRemoved {}\n",
        platform.describe(),
        done.file.display()
    );
    commands_text(&done, ran, &mut text);
    Ok(Report::new(
        text,
        json!({
            "installed": false,
            "removed": done.file,
            "ran": if ran { done.ran.clone() } else { Vec::new() },
            "failed": done.failed,
        }),
    ))
}

/// `carrel agent status`: whether the agent is installed and for which archives, and whether
/// an agent looks after each of them, and the current archive, now.
///
/// # Errors
///
/// [`Failure`] if there's no home or configuration folder.
pub fn status(archive: Option<&Path>) -> Result<Report, Failure> {
    let platform = platform()?;
    let places = places()?;
    let folder = crate::config::dir()?.join("workers");
    let current = locate(archive).ok().map(|current| {
        std::fs::canonicalize(current.root()).unwrap_or_else(|_| current.root().to_owned())
    });
    Ok(status_in(platform, &places, &folder, current))
}

/// Whether the agent is installed as `platform`'s service in `places` and for which archives,
/// and whether an agent, whose files are in `folder`, looks after each and `current` now.
pub(crate) fn status_in(
    platform: Platform,
    places: &Places,
    folder: &Path,
    current: Option<PathBuf>,
) -> Report {
    let installed = service::installed(platform, places);
    let mut roots: Vec<PathBuf> = installed
        .as_ref()
        .map(|(_, archives)| archives.clone())
        .unwrap_or_default();
    if let Some(current) = current
        && !roots.contains(&current)
    {
        roots.push(current);
    }
    let looked_after = carrel_core::agent::looked_after(folder, &roots);
    let mut text = match &installed {
        Some((file, archives)) => format!(
            "Installed as {}, for {}: {}\n",
            platform.describe(),
            plural(archives.len(), "archive"),
            file.display()
        ),
        None => "Not installed. `carrel agent install` starts it at log-in.\n".to_owned(),
    };
    for (root, running) in &looked_after {
        writeln!(
            text,
            "{}: {}",
            names(std::slice::from_ref(root)),
            if *running {
                "an agent looks after it now"
            } else {
                "no agent is running for it"
            }
        )
        .unwrap();
    }
    Report::new(
        text,
        json!({
            "installed": installed.is_some(),
            "as": platform.describe(),
            "file": installed.as_ref().map(|(file, _)| file),
            "archives": looked_after
                .iter()
                .map(|(root, running)| json!({
                    "path": root,
                    "installed": installed.as_ref().is_some_and(|(_, a)| a.contains(root)),
                    "running": running,
                }))
                .collect::<Vec<_>>(),
        }),
    )
}
