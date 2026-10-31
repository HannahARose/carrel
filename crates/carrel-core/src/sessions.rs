//! Work sessions, worked out afterwards (`docs/design/checkout.md`): until the Mac app's
//! background agent watches items live (v0.5), Carrel finds sessions from what's on disk when
//! it next runs. Each file's modification time, and for a Git repository each commit's time,
//! marks a moment of work; moments closer together than `sessions.idle_minutes` belong to one
//! session. A session still going (work in the last `idle_minutes`) isn't written yet.
//!
//! What's been recorded is kept per device, in Carrel's own folder: when each item was last
//! looked at, so a session is written once.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::archive::Archive;
use crate::backup::{BackupError, Backups, Snapshot};
use crate::card::{Id, Snapshots};
use crate::card_file::CardFile;
use crate::coordinator::WriterLease;
use crate::desk::Desk;
use crate::detect::{Rules, is_ignored};
use crate::entries::HistoryEntry;
use crate::history::{Change, History, HistoryError, Operation};

/// A stretch of work on one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The first change.
    pub start: SystemTime,
    /// The last change.
    pub end: SystemTime,
    /// The files changed, by path inside the item.
    pub files: Vec<String>,
    /// The subjects of commits made, oldest first.
    pub commits: Vec<String>,
}

impl Session {
    /// How long it ran, in whole minutes, from its first change to its last. A session of one
    /// change counts as five minutes, the least worth recording.
    #[must_use]
    pub fn minutes(&self) -> u64 {
        let seconds = self
            .end
            .duration_since(self.start)
            .unwrap_or_default()
            .as_secs();
        (seconds / 60).max(5)
    }

    /// The pending history entry for it.
    #[must_use]
    pub fn entry(&self) -> HistoryEntry {
        let date = jiff::Timestamp::try_from(self.start)
            .map(|t| t.to_zoned(jiff::tz::TimeZone::system()).date().to_string())
            .unwrap_or_default();
        let mut lines = Vec::new();
        if !self.files.is_empty() {
            let shown: Vec<&str> = self.files.iter().take(8).map(String::as_str).collect();
            let more = self.files.len().saturating_sub(shown.len());
            lines.push(format!(
                "Changed {} file(s): {}{}",
                self.files.len(),
                shown.join(", "),
                if more > 0 {
                    format!(", and {more} more")
                } else {
                    String::new()
                }
            ));
        }
        if !self.commits.is_empty() {
            let quoted: Vec<String> = self.commits.iter().map(|c| format!("\"{c}\"")).collect();
            lines.push(format!("Commits: {}", quoted.join(", ")));
        }
        HistoryEntry::pending(&date, Some(self.minutes()), lines)
    }
}

/// One moment of work: when, and what (a file's path, or a commit's subject).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Moment {
    File(String),
    Commit(String),
}

/// Groups `moments` into sessions: a gap longer than `idle` starts a new one. Sessions whose
/// last moment is within `idle` of `now` are still going, and left out.
fn group(mut moments: Vec<(SystemTime, Moment)>, idle: Duration, now: SystemTime) -> Vec<Session> {
    moments.sort_by_key(|(t, _)| *t);
    let mut sessions: Vec<Session> = Vec::new();
    for (time, moment) in moments {
        let starts_new = sessions
            .last()
            .is_none_or(|s| time.duration_since(s.end).unwrap_or_default() > idle);
        if starts_new {
            sessions.push(Session {
                start: time,
                end: time,
                files: Vec::new(),
                commits: Vec::new(),
            });
        }
        let session = sessions.last_mut().expect("a session was just pushed");
        session.end = time;
        match moment {
            Moment::File(path) => {
                if !session.files.contains(&path) {
                    session.files.push(path);
                }
            }
            Moment::Commit(subject) => session.commits.push(subject),
        }
    }
    sessions.retain(|s| now.duration_since(s.end).unwrap_or_default() > idle);
    for session in &mut sessions {
        session.files.sort();
    }
    sessions
}

/// The finished sessions of work on the item at `path` since `since`: changed files (leaving
/// out what `rules` call regenerable, and clutter), and for a Git repository, commits.
///
/// # Errors
///
/// An error reading the item.
pub fn find(
    path: &Path,
    rules: &Rules,
    since: SystemTime,
    idle: Duration,
    now: SystemTime,
) -> std::io::Result<Vec<Session>> {
    let mut moments = Vec::new();
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        let mut stack = vec![(path.to_path_buf(), String::new())];
        while let Some((dir, prefix)) = stack.pop() {
            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if is_ignored(&name) || name == ".git" {
                    continue;
                }
                let relative = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                let kind = entry.file_type()?;
                if kind.is_dir() {
                    if !rules.is_regenerable(&name) {
                        stack.push((entry.path(), relative));
                    }
                } else if kind.is_file() {
                    let modified = entry.metadata()?.modified()?;
                    if modified > since {
                        moments.push((modified, Moment::File(relative)));
                    }
                }
            }
        }
        moments.extend(commits(path, since));
    } else {
        let modified = meta.modified()?;
        if modified > since {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            moments.push((modified, Moment::File(name)));
        }
    }
    Ok(group(moments, idle, now))
}

/// A Git repository's commits since `since`, with their times.
fn commits(path: &Path, since: SystemTime) -> Vec<(SystemTime, Moment)> {
    let Ok(repo) = git2::Repository::open(path) else {
        return Vec::new();
    };
    let Ok(mut walk) = repo.revwalk() else {
        return Vec::new();
    };
    if walk.push_head().is_err() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for oid in walk.flatten() {
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        let seconds = u64::try_from(commit.time().seconds()).unwrap_or(0);
        let time = SystemTime::UNIX_EPOCH + Duration::from_secs(seconds);
        if time <= since {
            break;
        }
        found.push((
            time,
            Moment::Commit(commit.summary().ok().flatten().unwrap_or("").to_owned()),
        ));
    }
    found
}

/// When each item on this device was last looked at for sessions, by item identifier: the
/// end of the last session written, or when it was checked out or put on the desk.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scanned {
    /// Seconds since 1970, by item.
    pub items: BTreeMap<String, u64>,
    /// When sessions were last looked for at all, in seconds since 1970, so running Carrel
    /// often looks only every few minutes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub looked: Option<u64>,
}

impl Scanned {
    /// The file it's kept in for the archive `archive_id`, in Carrel's folder on this device.
    #[must_use]
    pub fn path(archive_id: &str) -> Option<PathBuf> {
        crate::local::config_dir().map(|d| d.join("sessions").join(format!("{archive_id}.json")))
    }

    /// The saved state, or none yet.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Saves it.
    ///
    /// # Errors
    ///
    /// An error writing the file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            path,
            serde_json::to_string_pretty(self).map_err(std::io::Error::other)?,
        )
    }

    /// Whether it's been at least `every` since sessions were last looked for, as of `now`.
    #[must_use]
    pub fn due(&self, every: Duration, now: SystemTime) -> bool {
        self.looked.is_none_or(|looked| {
            now.duration_since(SystemTime::UNIX_EPOCH + Duration::from_secs(looked))
                .is_ok_and(|since| since >= every)
        })
    }

    /// When `item` was last looked at, or `otherwise` the first time.
    #[must_use]
    pub fn since(&self, item: &str, otherwise: SystemTime) -> SystemTime {
        self.items.get(item).map_or(otherwise, |s| {
            SystemTime::UNIX_EPOCH + Duration::from_secs(*s)
        })
    }

    /// Records that `item` has been looked at up to `until`. It's kept in whole seconds,
    /// rounded up, so a change at `until` itself isn't found again.
    pub fn mark(&mut self, item: &str, until: SystemTime) {
        let since_epoch = until
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        let seconds = since_epoch.as_secs() + u64::from(since_epoch.subsec_nanos() > 0);
        self.items.insert(item.to_owned(), seconds);
    }
}

/// One item that got new entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedItem {
    /// Its card's identifier.
    pub id: Id,
    /// Its title.
    pub title: String,
    /// How many sessions were found.
    pub sessions: usize,
    /// The working copy: where it's checked out to, or where the desk item is.
    pub working: PathBuf,
    /// Whether its working copy is snapshotted after sessions: not with `snapshots:
    /// return-only` on the card.
    pub snapshot: bool,
}

/// What recording found: the items that got entries, and the commit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recorded {
    /// Each item with new entries.
    pub items: Vec<RecordedItem>,
    /// The history commit, when anything in the archive was written. Desk items' cards are
    /// beside them, outside the history, so recording only those commits nothing.
    pub commit: Option<String>,
}

impl Recorded {
    /// How many sessions were found, on all the items.
    #[must_use]
    pub fn sessions(&self) -> usize {
        self.items.iter().map(|i| i.sessions).sum()
    }
}

/// Why sessions couldn't be recorded.
#[derive(Debug)]
pub enum SessionError {
    /// The archive or a card couldn't be read or written.
    Archive(String),
    /// The history couldn't be written.
    History(HistoryError),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Archive(why) => f.write_str(why),
            Self::History(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SessionError {}

/// An item worked on away from the archive copy: checked out to this device, or on its desk.
struct Tracked {
    id: Id,
    card: PathBuf,
    working: PathBuf,
    /// When it was checked out or put on the desk: where looking starts the first time.
    from: SystemTime,
}

/// The items checked out to `device` and on its desk.
fn tracked(archive: &Archive, device: Id, now: SystemTime) -> Result<Vec<Tracked>, SessionError> {
    let fail = |e: &dyn std::fmt::Display| SessionError::Archive(e.to_string());
    let parse_time = |t: &str| t.parse::<jiff::Timestamp>().map_or(now, SystemTime::from);
    let mut found = Vec::new();
    for item in archive.items().map_err(|e| fail(&e))? {
        let card = archive.root().join(&item.card);
        let Some(out) = std::fs::read_to_string(&card)
            .ok()
            .and_then(|t| CardFile::parse(&t).ok())
            .and_then(|f| f.card().checked_out.clone())
        else {
            continue;
        };
        if out.device == device {
            found.push(Tracked {
                id: item.id,
                card,
                working: PathBuf::from(&out.to),
                from: parse_time(&out.since.0),
            });
        }
    }
    for entry in Desk::load(archive, device).map_err(|e| fail(&e))?.items {
        found.push(Tracked {
            id: entry.id,
            card: entry.card,
            working: entry.path,
            from: parse_time(&entry.added.0),
        });
    }
    Ok(found)
}

/// Finds the finished work sessions on every item checked out to this device (the lease's)
/// or on its desk, since each was last looked at, adds a pending history entry for each, and
/// commits them as one `session` change. Items whose working copy or card is missing are
/// skipped. Snapshots are left to [`snapshot`], after the lease is released.
///
/// # Errors
///
/// [`SessionError`] if the archive can't be read or the cards and history written.
pub fn record(archive: &Archive, lease: &WriterLease) -> Result<Recorded, SessionError> {
    let fail = |e: &dyn std::fmt::Display| SessionError::Archive(e.to_string());
    let device = lease.info().device;
    let rules = archive.rules().map_err(|e| fail(&e))?;
    let idle = Duration::from_mins(u64::from(archive.settings().sessions.idle_minutes));
    let now = SystemTime::now();
    let state_path = Scanned::path(&archive.doc().id.to_string());
    let mut scanned = state_path.as_deref().map(Scanned::load).unwrap_or_default();
    let mut recorded = Recorded::default();
    for item in tracked(archive, device, now)? {
        let Ok(text) = std::fs::read_to_string(&item.card) else {
            continue;
        };
        let Ok(mut file) = CardFile::parse(&text) else {
            continue;
        };
        let key = item.id.to_string();
        let since = scanned.since(&key, item.from);
        let Ok(sessions) = find(&item.working, &rules, since, idle, now) else {
            continue;
        };
        let Some(last) = sessions.last().map(|s| s.end) else {
            continue;
        };
        let mut log = file.history().map_err(|e| fail(&e))?;
        for session in &sessions {
            log.add(session.entry());
        }
        file.set_history(&log).map_err(|e| fail(&e))?;
        let text = file.to_text().map_err(|e| fail(&e))?;
        let temp = item.card.with_extension("md.tmp-sessions");
        std::fs::write(&temp, text).map_err(|e| fail(&e))?;
        std::fs::rename(&temp, &item.card).map_err(|e| fail(&e))?;
        scanned.mark(&key, last);
        recorded.items.push(RecordedItem {
            id: item.id,
            title: file.card().title.clone(),
            sessions: sessions.len(),
            working: item.working,
            snapshot: file.card().snapshots != Some(Snapshots::ReturnOnly),
        });
    }
    scanned.looked = Some(
        now.duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    );
    if recorded.items.is_empty() {
        if let Some(path) = state_path {
            scanned.save(&path).map_err(|e| fail(&e))?;
        }
        return Ok(recorded);
    }
    crate::active::regenerate(archive).map_err(|e| fail(&e))?;
    let mut change = Change::new(
        Operation::Session,
        format!("Record {} work session(s)", recorded.sessions()),
        lease.info().person,
        device,
    );
    change.items = recorded.items.iter().map(|i| i.id).collect();
    recorded.commit = History::open(archive)
        .map_err(SessionError::History)?
        .commit(archive, &change, lease)
        .map_err(SessionError::History)?;
    if let Some(path) = state_path {
        scanned.save(&path).map_err(|e| fail(&e))?;
    }
    Ok(recorded)
}

/// The description of a session snapshot of item `id`'s working copy, which
/// [`is_session_snapshot`] recognizes.
#[must_use]
pub fn snapshot_description(title: &str, id: Id) -> String {
    format!("Work session on “{title}” ({id})")
}

/// Whether `snapshot` is a session snapshot of item `id`'s working copy.
#[must_use]
pub fn is_session_snapshot(snapshot: &Snapshot, id: Id) -> bool {
    snapshot
        .description
        .as_deref()
        .is_some_and(|d| d.starts_with("Work session on ") && d.ends_with(&format!("({id})")))
}

/// The session snapshots of item `id`'s working copy at `working` taken since `since`, as
/// when it was checked out: what a return can offer to forget. Oldest first.
///
/// # Errors
///
/// [`BackupError`] listing them.
pub fn snapshots_since(
    backups: &dyn Backups,
    working: &Path,
    id: Id,
    since: SystemTime,
) -> Result<Vec<Snapshot>, BackupError> {
    let since = jiff::Timestamp::try_from(since).unwrap_or(jiff::Timestamp::MIN);
    Ok(backups
        .list(working)?
        .into_iter()
        .filter(|s| {
            is_session_snapshot(s, id)
                && s.time
                    .parse::<jiff::Timestamp>()
                    .is_ok_and(|time| time >= since)
        })
        .collect())
}

/// Takes a snapshot of the working copy of each item [`record`] found sessions on, except
/// those whose cards say `snapshots: return-only`: the state when Carrel noticed, which for
/// most work is the state when the session ended. Each item's outcome, in order.
pub fn snapshot(
    recorded: &Recorded,
    backups: &dyn Backups,
) -> Vec<(Id, Result<Snapshot, BackupError>)> {
    recorded
        .items
        .iter()
        .filter(|i| i.snapshot && i.working.exists())
        .map(|i| {
            (
                i.id,
                backups.snapshot(&i.working, &snapshot_description(&i.title, i.id)),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minutes: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + minutes * 60)
    }

    fn file(minutes: u64, path: &str) -> (SystemTime, Moment) {
        (at(minutes), Moment::File(path.into()))
    }

    #[test]
    fn moments_far_apart_are_separate_sessions_and_a_current_one_waits() {
        let idle = Duration::from_mins(45);
        let moments = vec![
            file(0, "a.txt"),
            file(20, "b.txt"),
            file(40, "a.txt"),
            (at(50), Moment::Commit("fix units".into())),
            file(200, "c.txt"),
            file(500, "d.txt"),
        ];
        let sessions = group(moments, idle, at(520));
        assert_eq!(sessions.len(), 2, "the one at 500 is still going");
        assert_eq!(sessions[0].files, ["a.txt", "b.txt"]);
        assert_eq!(sessions[0].commits, ["fix units"]);
        assert_eq!(sessions[0].minutes(), 50);
        assert_eq!(sessions[1].files, ["c.txt"]);
        assert_eq!(
            sessions[1].minutes(),
            5,
            "one change counts as five minutes"
        );
    }

    #[test]
    fn a_session_becomes_a_pending_entry() {
        let session = Session {
            start: at(0),
            end: at(130),
            files: vec!["fig4.py".into(), "notes.md".into()],
            commits: vec!["add PRC analysis".into()],
        };
        let entry = session.entry();
        assert!(entry.pending);
        assert_eq!(entry.duration.as_deref(), Some("2h 10m"));
        assert_eq!(
            entry.lines,
            [
                "Changed 2 file(s): fig4.py, notes.md",
                "Commits: \"add PRC analysis\""
            ]
        );
    }

    #[test]
    fn changed_files_are_found_and_regenerable_ones_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let item = dir.path().join("project");
        std::fs::create_dir_all(item.join("src")).unwrap();
        std::fs::create_dir_all(item.join("target")).unwrap();
        std::fs::write(item.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(item.join("target/out"), "built").unwrap();
        std::fs::write(item.join(".DS_Store"), "").unwrap();
        let rules = Rules::default();
        let before = SystemTime::now() - Duration::from_hours(1);
        let later = SystemTime::now() + Duration::from_hours(3);
        let idle = Duration::from_mins(45);
        let sessions = find(&item, &rules, before, idle, later).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0].files,
            ["src/main.rs"],
            "not target/ or .DS_Store"
        );
        assert!(
            find(&item, &rules, before, idle, SystemTime::now())
                .unwrap()
                .is_empty(),
            "a session that just happened is still going"
        );
    }

    /// Backups that remember what they snapshotted, and list it back.
    #[derive(Default)]
    struct Remembered {
        taken: std::sync::Mutex<Vec<(PathBuf, Snapshot)>>,
    }

    impl Backups for Remembered {
        fn describe(&self) -> String {
            "remembered".into()
        }
        fn snapshot(&self, root: &Path, description: &str) -> Result<Snapshot, BackupError> {
            let mut taken = self.taken.lock().unwrap();
            let snapshot = Snapshot {
                id: format!("s{}", taken.len()),
                time: jiff::Timestamp::now().to_string(),
                description: Some(description.to_owned()),
                pinned: true,
                size: None,
                files: None,
            };
            taken.push((root.to_owned(), snapshot.clone()));
            Ok(snapshot)
        }
        fn list(&self, root: &Path) -> Result<Vec<Snapshot>, BackupError> {
            Ok(self
                .taken
                .lock()
                .unwrap()
                .iter()
                .filter(|(r, _)| r == root)
                .map(|(_, s)| s.clone())
                .collect())
        }
        fn verify(&self, _percent: u8) -> Result<String, BackupError> {
            Ok(String::new())
        }
        fn restore(&self, _snapshot: &str, _path: &str, _to: &Path) -> Result<(), BackupError> {
            Ok(())
        }
        fn forget(&self, _snapshot: &str) -> Result<(), BackupError> {
            Ok(())
        }
    }

    #[test]
    fn working_copies_are_snapshotted_unless_return_only() {
        let dir = tempfile::tempdir().unwrap();
        let (project, dataset) = (dir.path().join("project"), dir.path().join("dataset"));
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&dataset).unwrap();
        let item = |working: &Path, snapshot| RecordedItem {
            id: Id::new_v7(),
            title: "Lamp".into(),
            sessions: 1,
            working: working.to_owned(),
            snapshot,
        };
        let recorded = Recorded {
            items: vec![item(&project, true), item(&dataset, false)],
            commit: None,
        };
        let backups = Remembered::default();
        let taken = snapshot(&recorded, &backups);
        assert_eq!(taken.len(), 1, "not the return-only dataset");
        assert_eq!(taken[0].0, recorded.items[0].id);

        let id = recorded.items[0].id;
        let before = SystemTime::now() - Duration::from_hours(1);
        backups.snapshot(&project, "Returned “Lamp”").unwrap();
        let found = snapshots_since(&backups, &project, id, before).unwrap();
        assert_eq!(found.len(), 1, "only the session snapshot: {found:?}");
        assert!(is_session_snapshot(&found[0], id));
        assert!(
            !is_session_snapshot(&found[0], Id::new_v7()),
            "another item's"
        );
        let later = SystemTime::now() + Duration::from_hours(1);
        assert!(
            snapshots_since(&backups, &project, id, later)
                .unwrap()
                .is_empty(),
            "not from before the checkout"
        );
    }

    #[test]
    fn what_was_scanned_is_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions/a.json");
        let mut scanned = Scanned::load(&path);
        assert_eq!(scanned.since("x", at(1)), at(1));
        scanned.mark("x", at(90));
        scanned.save(&path).unwrap();
        assert_eq!(Scanned::load(&path).since("x", at(1)), at(90));
        assert!(scanned.due(Duration::from_mins(5), at(90)), "never looked");
        scanned.looked = Some(1_800_000_000 + 90 * 60);
        assert!(!scanned.due(Duration::from_mins(5), at(94)));
        assert!(scanned.due(Duration::from_mins(5), at(95)));
        scanned.mark("y", at(90) + Duration::from_millis(400));
        assert_eq!(
            scanned.since("y", at(1)),
            at(90) + Duration::from_secs(1),
            "rounded up, so the change isn't found again"
        );
    }
}
