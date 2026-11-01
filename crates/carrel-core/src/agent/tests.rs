use std::collections::HashMap;

use super::*;
use crate::archive::InitOptions;

/// Jobs that record what ran, with results set by the test.
#[derive(Default)]
struct Fake {
    ran: Vec<Job>,
    results: HashMap<Job, JobError>,
    backups: bool,
    working: Vec<PathBuf>,
}

impl Jobs for Fake {
    fn run(&mut self, _: &Archive, job: Job) -> Result<Option<String>, JobError> {
        self.ran.push(job);
        self.results.get(&job).map_or(Ok(None), |e| Err(e.clone()))
    }

    fn working_copies(&self, _: &Archive) -> Vec<PathBuf> {
        self.working.clone()
    }

    fn backups_set_up(&self, _: &Archive) -> bool {
        self.backups
    }
}

struct Setup {
    _dir: tempfile::TempDir,
    archive: Archive,
    workers: PathBuf,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let archive = Archive::init(&dir.path().join("Archive"), &InitOptions::new("Papers")).unwrap();
    let workers = dir.path().join("config/workers");
    Setup {
        _dir: dir,
        archive,
        workers,
    }
}

fn at(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000_000 + seconds)
}

fn tick(agent: &mut Agent, jobs: &mut Fake, now: SystemTime) -> (Vec<Event>, Duration) {
    let mut events = Vec::new();
    let wait = agent.tick(jobs, &mut |e| events.push(e), now);
    (events, wait)
}

#[test]
fn it_claims_the_archive_catches_up_and_the_clis_worker_steps_aside() {
    let s = setup();
    let root = s.archive.root().to_owned();
    let mut agent = Agent::new(&s.workers, std::slice::from_ref(&root), true);
    let mut jobs = Fake::default();
    assert!(!agent_running(&s.workers, &s.archive));
    let (events, _) = tick(&mut agent, &mut jobs, at(0));
    assert_eq!(
        events[0],
        Event::Claimed {
            name: "Papers".into(),
            root: root.clone(),
            took_over: false,
            how: How::Polling,
        }
    );
    assert_eq!(
        jobs.ran,
        [Job::Work, Job::Sessions],
        "it catches up at once"
    );
    assert!(
        agent_running(&s.workers, &s.archive),
        "the CLI's worker sees the agent's lock"
    );
    assert_eq!(
        looked_after(&s.workers, std::slice::from_ref(&root)),
        [(root, true)]
    );
    agent.release();
    assert!(!agent_running(&s.workers, &s.archive), "letting go ends it");
}

#[test]
fn it_steps_aside_while_another_agent_has_the_archive_and_takes_over_once_it_goes() {
    let s = setup();
    let root = s.archive.root().to_owned();
    // The Mac app's agent, say.
    let other = agent_lock(&s.workers, &s.archive).take().unwrap().unwrap();
    let mut agent = Agent::new(&s.workers, std::slice::from_ref(&root), true);
    let mut jobs = Fake::default();
    let (events, wait) = tick(&mut agent, &mut jobs, at(0));
    assert_eq!(
        events,
        [Event::SteppedAside {
            name: "Papers".into(),
            root: root.clone()
        }]
    );
    assert_eq!(jobs.ran, [], "it does nothing while stepping aside");
    assert_eq!(wait, CLAIM_RETRY);
    let (events, _) = tick(&mut agent, &mut jobs, at(30));
    assert_eq!(
        events,
        [],
        "it says so once, and waits a minute to try again"
    );

    drop(other);
    let (events, _) = tick(&mut agent, &mut jobs, at(60));
    assert!(matches!(
        events[0],
        Event::Claimed {
            took_over: true,
            ..
        }
    ));
    assert_eq!(jobs.ran, [Job::Work, Job::Sessions]);

    // A second agent now steps aside for this one.
    let mut second = Agent::new(&s.workers, std::slice::from_ref(&root), true);
    let (events, _) = tick(&mut second, &mut Fake::default(), at(61));
    assert!(matches!(events[0], Event::SteppedAside { .. }));
    assert!(!second.claimed(&root) && agent.claimed(&root));
}

#[test]
fn a_queued_item_runs_the_worker_once_changes_settle() {
    let s = setup();
    let root = s.archive.root().to_owned();
    let mut agent = Agent::new(&s.workers, std::slice::from_ref(&root), true);
    let mut jobs = Fake::default();
    tick(&mut agent, &mut jobs, at(0));
    jobs.ran.clear();
    let (_, wait) = tick(&mut agent, &mut jobs, at(1));
    assert_eq!(
        wait,
        poll::POLL.checked_sub(Duration::from_secs(1)).unwrap(),
        "until it looks again"
    );

    let intake = root.join(crate::intake::INTAKE_DIR);
    std::fs::create_dir_all(&intake).unwrap();
    std::fs::write(intake.join("entry.json"), "{}").unwrap();
    tick(&mut agent, &mut jobs, at(5));
    assert_eq!(jobs.ran, [], "still settling");
    assert_eq!(
        agent.plan(&root).unwrap().work_due(),
        Some(at(5) + plan::SETTLE)
    );
    tick(&mut agent, &mut jobs, at(7));
    assert_eq!(jobs.ran, [Job::Work]);
}

#[test]
fn a_busy_archive_puts_the_job_off_and_a_failure_is_reported() {
    let s = setup();
    let root = s.archive.root().to_owned();
    let mut agent = Agent::new(&s.workers, std::slice::from_ref(&root), true);
    let mut jobs = Fake::default();
    jobs.results.insert(Job::Work, JobError::Busy);
    jobs.results
        .insert(Job::Sessions, JobError::Failed("no cards".into()));
    let (events, _) = tick(&mut agent, &mut jobs, at(0));
    assert!(events.contains(&Event::Postponed {
        name: "Papers".into(),
        job: Job::Work
    }));
    assert!(events.contains(&Event::Failed {
        name: "Papers".into(),
        job: Job::Sessions,
        error: "no cards".into()
    }));
    let plan = agent.plan(&root).unwrap();
    assert_eq!(plan.work_due(), Some(at(0) + BUSY_RETRY));
    assert_eq!(plan.sessions_due(), None);
}

#[test]
fn scheduled_snapshots_are_remembered_between_runs() {
    let s = setup();
    let root = s.archive.root().to_owned();
    let mut jobs = Fake {
        backups: true,
        ..Fake::default()
    };
    let mut agent = Agent::new(&s.workers, std::slice::from_ref(&root), true);
    tick(&mut agent, &mut jobs, at(0));
    assert_eq!(jobs.ran, [Job::Work, Job::Sessions, Job::Backup]);
    agent.release();
    let state = State::load(&State::path(&s.workers, &s.archive));
    assert!(state.last_backup.is_some() && !state.backup_failed);

    jobs.ran.clear();
    let mut again = Agent::new(&s.workers, std::slice::from_ref(&root), true);
    tick(&mut again, &mut jobs, at(3600));
    assert_eq!(jobs.ran, [Job::Work, Job::Sessions], "not due for a day");
    assert_eq!(
        again.plan(&root).unwrap().backup_due(),
        Some(at(0) + plan::BACKUP_EVERY)
    );
}

#[test]
fn a_change_in_a_working_copy_records_sessions_once_work_is_idle() {
    let s = setup();
    let root = s.archive.root().to_owned();
    let working = s.workers.parent().unwrap().join("Thesis");
    std::fs::create_dir_all(&working).unwrap();
    let working = std::fs::canonicalize(working).unwrap();
    let mut jobs = Fake {
        working: vec![working.clone()],
        ..Fake::default()
    };
    let mut agent = Agent::new(&s.workers, std::slice::from_ref(&root), true);
    tick(&mut agent, &mut jobs, at(0));
    jobs.ran.clear();
    std::fs::write(working.join("chapter.tex"), "words").unwrap();
    tick(&mut agent, &mut jobs, at(5));
    let idle = Duration::from_mins(u64::from(s.archive.settings().sessions.idle_minutes));
    assert_eq!(
        agent.plan(&root).unwrap().sessions_due(),
        Some(at(5) + idle + plan::MARGIN)
    );
    assert_eq!(jobs.ran, []);
}

#[test]
fn an_archive_that_cannot_be_opened_is_reported_once() {
    let s = setup();
    let missing = s.workers.join("nowhere");
    let mut agent = Agent::new(&s.workers, std::slice::from_ref(&missing), true);
    let mut jobs = Fake::default();
    let (events, _) = tick(&mut agent, &mut jobs, at(0));
    assert!(matches!(events[..], [Event::CannotOpen { .. }]));
    let (events, _) = tick(&mut agent, &mut jobs, at(60));
    assert_eq!(events, []);
    assert_eq!(
        looked_after(&s.workers, std::slice::from_ref(&missing)),
        [(missing, false)]
    );
}
