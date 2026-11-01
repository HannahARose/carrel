//! When the agent does what for one archive: what a changed file means, and when each job is
//! due. It's plain values and times, so the scheduling is tested without waiting. The Mac
//! app's agent keeps the same schedule (`AgentPlan` in `CarrelKit`).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A job the agent runs for an archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Job {
    /// Draft the intake queue, then bring the embedding index up to date: the worker loop.
    Work,
    /// Record the work sessions that have finished on checked-out and desk items.
    Sessions,
    /// Take a scheduled snapshot of the archive.
    Backup,
    /// Recheck where a few items are filed, when the crawler's pace and the device allow
    /// (`docs/design/restructure.md`, "Crawlers").
    Crawl,
}

impl Job {
    /// What the job does, for people: "drafting the queue", say.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Work => "drafting the queue and keeping the index current",
            Self::Sessions => "looking for work sessions",
            Self::Backup => "taking a scheduled snapshot",
            Self::Crawl => "rechecking where items are filed",
        }
    }
}

/// What a changed path means to the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Something joined or left the intake queue.
    Queue,
    /// A filed item or card changed, so the index may be behind.
    Filed,
    /// What's checked out or on the desk changed, so the working copies to watch did too.
    Active,
    /// A working copy changed: a work session is going on.
    Working,
    /// Nothing the agent acts on, such as its own index or the history repository.
    Ignored,
}

/// How long changes settle before the worker runs, so a burst of them runs it once.
pub const SETTLE: Duration = Duration::from_secs(2);
/// How long after a session goes idle it's recorded, past `sessions.idle_minutes`.
pub const MARGIN: Duration = Duration::from_mins(1);
/// How often a scheduled snapshot is taken.
pub const BACKUP_EVERY: Duration = Duration::from_hours(24);
/// How soon a snapshot that failed, as when the drive isn't plugged in, is tried again.
pub const BACKUP_RETRY: Duration = Duration::from_hours(1);
/// How often the crawler's job looks whether a pass is due; its pace decides whether one runs.
pub const CRAWL_LOOK: Duration = Duration::from_mins(5);
/// How long after the owner changes anything the crawler waits, so it never works while they
/// do.
pub const CRAWL_QUIET: Duration = Duration::from_mins(5);

/// One archive's schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The archive's root folder.
    pub root: PathBuf,
    /// The working copies of what's checked out and on the desk.
    pub working_copies: Vec<PathBuf>,
    /// How long a pause ends a work session.
    pub idle: Duration,
    /// Whether backups are set up for the archive on this device.
    pub backups_set_up: bool,
    /// When the last scheduled snapshot was taken, or tried and failed.
    pub last_backup: Option<SystemTime>,
    /// Whether the last scheduled snapshot failed.
    pub backup_failed: bool,
    work_due: Option<SystemTime>,
    sessions_due: Option<SystemTime>,
    crawl_due: Option<SystemTime>,
}

impl Plan {
    /// The schedule for the archive at `root`, with sessions ending after `idle_minutes`
    /// without changes, and nothing due yet: [`Plan::start`] catches up.
    #[must_use]
    pub fn new(root: &Path, idle_minutes: u32) -> Self {
        Self {
            root: root.to_owned(),
            working_copies: Vec::new(),
            idle: Duration::from_mins(u64::from(idle_minutes)),
            backups_set_up: false,
            last_backup: None,
            backup_failed: false,
            work_due: None,
            sessions_due: None,
            crawl_due: None,
        }
    }

    /// What a change at `path` means.
    #[must_use]
    pub fn classify(&self, path: &Path) -> Change {
        // A working copy inside the archive is still the owner's work.
        if self.working_copies.iter().any(|w| path.starts_with(w)) {
            return Change::Working;
        }
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return Change::Ignored;
        };
        let text = relative.to_string_lossy().replace('\\', "/");
        let name = relative
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        if text.is_empty() || name.ends_with(".tmp") || name.contains(".tmp-") {
            return Change::Ignored;
        }
        if !relative.starts_with("_System") {
            return Change::Filed;
        }
        if relative.starts_with(crate::intake::INTAKE_DIR) {
            return Change::Queue;
        }
        if (relative.starts_with("_System/devices") && name == "desk.json")
            || (relative.starts_with("_System/manifests") && name.ends_with(".checkout.tsv"))
        {
            return Change::Active;
        }
        Change::Ignored
    }

    /// Notes the changes at `paths`, noticed at `now`, and returns whether what's checked out
    /// or on the desk changed, so the caller reads the working copies again.
    pub fn noticed<'a>(
        &mut self,
        paths: impl IntoIterator<Item = &'a Path>,
        now: SystemTime,
    ) -> bool {
        let mut active = false;
        for path in paths {
            let change = self.classify(path);
            if change != Change::Ignored {
                // The owner is doing something: the crawler waits until they've stopped.
                let quiet = now + CRAWL_QUIET;
                self.crawl_due = Some(self.crawl_due.map_or(quiet, |due| due.max(quiet)));
            }
            match change {
                Change::Queue | Change::Filed => self.work_due = Some(now + SETTLE),
                Change::Active => {
                    active = true;
                    self.work_due = Some(now + SETTLE);
                    self.sessions_due = Some(self.sessions_due.map_or(now, |due| due.min(now)));
                }
                // Recorded once the session has been idle long enough; each change pushes
                // that back.
                Change::Working => self.sessions_due = Some(now + self.idle + MARGIN),
                Change::Ignored => {}
            }
        }
        active
    }

    /// On starting, or on taking over from another agent: catch up on everything at once.
    pub fn start(&mut self, now: SystemTime) {
        self.work_due = Some(now);
        self.sessions_due = Some(now);
        self.crawl_due = Some(now + CRAWL_LOOK);
    }

    /// When the next scheduled snapshot is due, if backups are set up.
    #[must_use]
    pub fn backup_due(&self) -> Option<SystemTime> {
        if !self.backups_set_up {
            return None;
        }
        let Some(last) = self.last_backup else {
            return Some(SystemTime::UNIX_EPOCH);
        };
        Some(
            last + if self.backup_failed {
                BACKUP_RETRY
            } else {
                BACKUP_EVERY
            },
        )
    }

    /// When the worker is next due, if it is.
    #[must_use]
    pub fn work_due(&self) -> Option<SystemTime> {
        self.work_due
    }

    /// When sessions are next due to be recorded, if they are.
    #[must_use]
    pub fn sessions_due(&self) -> Option<SystemTime> {
        self.sessions_due
    }

    /// When the crawler's job next looks whether a pass is due.
    #[must_use]
    pub fn crawl_due(&self) -> Option<SystemTime> {
        self.crawl_due
    }

    /// The jobs due at `now`, in the order to run them. The crawler only runs when nothing
    /// else is due.
    #[must_use]
    pub fn due(&self, now: SystemTime) -> Vec<Job> {
        let due: Vec<Job> = [
            (self.work_due, Job::Work),
            (self.sessions_due, Job::Sessions),
            (self.backup_due(), Job::Backup),
        ]
        .into_iter()
        .filter(|(due, _)| due.is_some_and(|due| due <= now))
        .map(|(_, job)| job)
        .collect();
        if due.is_empty() && self.crawl_due.is_some_and(|due| due <= now) {
            return vec![Job::Crawl];
        }
        due
    }

    /// Notes that `job`, started at `started`, has run. A change noticed while it ran stays
    /// due.
    pub fn ran(&mut self, job: Job, started: SystemTime, succeeded: bool) {
        match job {
            Job::Work => {
                if self.work_due.is_some_and(|due| due <= started) {
                    self.work_due = None;
                }
            }
            Job::Sessions => {
                if self.sessions_due.is_some_and(|due| due <= started) {
                    self.sessions_due = None;
                }
            }
            Job::Backup => {
                self.last_backup = Some(started);
                self.backup_failed = !succeeded;
            }
            Job::Crawl => {
                if self.crawl_due.is_some_and(|due| due <= started) {
                    self.crawl_due = Some(started + CRAWL_LOOK);
                }
            }
        }
    }

    /// Puts `job` off until `later`, as when another writer holds the archive.
    pub fn postpone(&mut self, job: Job, later: SystemTime) {
        match job {
            Job::Work => self.work_due = Some(later),
            Job::Sessions => self.sessions_due = Some(later),
            Job::Crawl => self.crawl_due = Some(later),
            Job::Backup => {}
        }
    }

    /// When something is next due, if anything is.
    #[must_use]
    pub fn next_due(&self) -> Option<SystemTime> {
        [
            self.work_due,
            self.sessions_due,
            self.backup_due(),
            self.crawl_due,
        ]
        .into_iter()
        .flatten()
        .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 + seconds)
    }

    fn plan() -> Plan {
        let mut plan = Plan::new(Path::new("/a/Archive"), 30);
        plan.working_copies = vec![
            PathBuf::from("/home/me/Desktop/Thesis"),
            PathBuf::from("/a/Archive/Projects/Desk item"),
        ];
        plan
    }

    #[test]
    fn the_crawler_runs_only_when_nothing_else_is_due_and_waits_after_changes() {
        let mut plan = plan();
        plan.start(at(0));
        assert_eq!(plan.due(at(0)), [Job::Work, Job::Sessions]);
        plan.ran(Job::Work, at(0), true);
        plan.ran(Job::Sessions, at(0), true);
        let look = CRAWL_LOOK.as_secs();
        assert_eq!(plan.due(at(look - 1)), []);
        assert_eq!(plan.due(at(look)), [Job::Crawl]);
        // The owner changes something: the crawler waits until they've stopped.
        plan.noticed([Path::new("/a/Archive/Reading/Paper.pdf")], at(look));
        assert_eq!(plan.due(at(look + 2)), [Job::Work], "the worker first");
        plan.ran(Job::Work, at(look + 2), true);
        assert_eq!(plan.due(at(look + 60)), []);
        let quiet = look + CRAWL_QUIET.as_secs();
        assert_eq!(plan.due(at(quiet)), [Job::Crawl]);
        plan.ran(Job::Crawl, at(quiet), true);
        assert_eq!(plan.crawl_due(), Some(at(quiet + look)));
    }

    #[test]
    fn changes_are_classified_by_where_they_are() {
        let plan = plan();
        let cases = [
            ("/a/Archive/_System/queue/intake/x.json", Change::Queue),
            ("/a/Archive/Reading/Paper.pdf", Change::Filed),
            ("/a/Archive/Reading/Paper.pdf.md", Change::Filed),
            ("/a/Archive/_System/devices/d/desk.json", Change::Active),
            (
                "/a/Archive/_System/manifests/i.checkout.tsv",
                Change::Active,
            ),
            ("/a/Archive/_System/manifests/i.tsv", Change::Ignored),
            ("/a/Archive/_System/index/vectors.bin", Change::Ignored),
            ("/a/Archive/_System/history.git/HEAD", Change::Ignored),
            ("/a/Archive/_System/queue/review/p.json", Change::Ignored),
            ("/a/Archive/Reading/card.md.tmp", Change::Ignored),
            ("/a/Archive", Change::Ignored),
            ("/home/me/Desktop/Thesis/chapter.tex", Change::Working),
            ("/home/me/Desktop/Thesis", Change::Working),
            ("/home/me/Desktop/Thesis2/x", Change::Ignored),
            ("/a/Archive/Projects/Desk item/notes.md", Change::Working),
            ("/elsewhere/file", Change::Ignored),
        ];
        for (path, change) in cases {
            assert_eq!(plan.classify(Path::new(path)), change, "{path}");
        }
    }

    #[test]
    fn starting_catches_up_and_a_burst_of_changes_runs_the_worker_once_it_settles() {
        let mut plan = plan();
        assert_eq!(plan.due(at(0)), [], "nothing is due before starting");
        plan.start(at(0));
        assert_eq!(plan.due(at(0)), [Job::Work, Job::Sessions]);
        plan.ran(Job::Work, at(0), true);
        plan.ran(Job::Sessions, at(0), true);
        assert_eq!(plan.next_due(), plan.crawl_due(), "only the crawler's look");

        let queued = Path::new("/a/Archive/_System/queue/intake/x.json");
        assert!(!plan.noticed([queued], at(10)));
        assert!(!plan.noticed([queued], at(11)));
        assert_eq!(plan.due(at(12)), [], "still settling");
        assert_eq!(plan.due(at(13)), [Job::Work]);
        assert_eq!(plan.next_due(), Some(at(13)));
    }

    #[test]
    fn a_change_noticed_while_a_job_runs_stays_due() {
        let mut plan = plan();
        plan.start(at(0));
        plan.noticed([Path::new("/a/Archive/Reading/new.pdf")], at(5));
        plan.ran(Job::Work, at(4), true);
        assert_eq!(
            plan.work_due(),
            Some(at(7)),
            "noticed after the run started"
        );
        plan.ran(Job::Work, at(8), true);
        assert_eq!(plan.work_due(), None);
    }

    #[test]
    fn sessions_are_recorded_once_work_has_been_idle_long_enough() {
        let mut plan = plan();
        let working = Path::new("/home/me/Desktop/Thesis/chapter.tex");
        plan.noticed([working], at(0));
        let idle = 30 * 60 + 60;
        assert_eq!(plan.sessions_due(), Some(at(idle)));
        plan.noticed([working], at(600));
        assert!(
            !plan.due(at(idle)).contains(&Job::Sessions),
            "each change pushes it back"
        );
        assert_eq!(plan.due(at(600 + idle)), [Job::Sessions]);
    }

    #[test]
    fn checking_out_reads_the_working_copies_again_and_looks_for_sessions_now() {
        let mut plan = plan();
        let working = Path::new("/home/me/Desktop/Thesis/chapter.tex");
        plan.noticed([working], at(0));
        let desk = Path::new("/a/Archive/_System/devices/d/desk.json");
        assert!(plan.noticed([desk], at(100)));
        assert_eq!(plan.sessions_due(), Some(at(100)));
        assert_eq!(plan.work_due(), Some(at(102)));
    }

    #[test]
    fn snapshots_are_daily_once_set_up_and_retried_hourly_after_a_failure() {
        let mut plan = plan();
        assert_eq!(plan.backup_due(), None, "not set up");
        plan.backups_set_up = true;
        assert_eq!(plan.due(at(0)), [Job::Backup], "never taken: due now");
        plan.ran(Job::Backup, at(0), false);
        assert_eq!(plan.backup_due(), Some(at(3600)));
        plan.ran(Job::Backup, at(3600), true);
        assert_eq!(plan.backup_due(), Some(at(3600 + 24 * 3600)));
        assert_eq!(plan.due(at(3600 + 24 * 3600 - 1)), []);
    }

    #[test]
    fn a_job_put_off_runs_later_but_backups_wait_for_their_own_time() {
        let mut plan = plan();
        plan.start(at(0));
        plan.postpone(Job::Work, at(30));
        plan.postpone(Job::Sessions, at(30));
        assert_eq!(plan.due(at(10)), []);
        assert_eq!(plan.due(at(30)), [Job::Work, Job::Sessions]);
        plan.backups_set_up = true;
        plan.last_backup = Some(at(0));
        plan.postpone(Job::Backup, at(30));
        assert_eq!(plan.backup_due(), Some(at(24 * 3600)));
    }
}
