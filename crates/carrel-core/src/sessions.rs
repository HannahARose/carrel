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
use crate::card::Id;
use crate::card_file::CardFile;
use crate::coordinator::WriterLease;
use crate::detect::{Rules, is_ignored};
use crate::entries::HistoryEntry;
use crate::history::{Change, History, HistoryError, Operation};

/// How long without changes ends a session, unless the archive sets `sessions.idle_minutes`.
pub const IDLE_MINUTES: u64 = 45;

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
/// end of the last session written, or when it was checked out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scanned {
    /// Seconds since 1970, by item.
    pub items: BTreeMap<String, u64>,
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

/// What recording found: the items that got entries, with how many, and the commit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recorded {
    /// Each item with new entries: its identifier, title and how many.
    pub items: Vec<(Id, String, usize)>,
    /// The history commit, when anything was written.
    pub commit: Option<String>,
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

/// Finds the finished work sessions on every item checked out on this device (the lease's)
/// since each was last looked at, adds a pending history entry for each, and commits them as
/// one `session` change. Items whose working copy is missing are skipped.
///
/// # Errors
///
/// [`SessionError`] if the archive can't be read or the cards and history written.
pub fn record(archive: &Archive, lease: &WriterLease) -> Result<Recorded, SessionError> {
    let fail = |e: &dyn std::fmt::Display| SessionError::Archive(e.to_string());
    let device = lease.info().device;
    let rules = archive.rules().map_err(|e| fail(&e))?;
    let idle = Duration::from_mins(IDLE_MINUTES);
    let now = SystemTime::now();
    let state_path = Scanned::path(&archive.doc().id.to_string());
    let mut scanned = state_path.as_deref().map(Scanned::load).unwrap_or_default();
    let mut recorded = Recorded::default();
    for item in archive.items().map_err(|e| fail(&e))? {
        let path = archive.root().join(&item.card);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(mut file) = CardFile::parse(&text) else {
            continue;
        };
        let Some(out) = file.card().checked_out.clone() else {
            continue;
        };
        if out.device != device {
            continue;
        }
        let working = PathBuf::from(&out.to);
        let checked_out = out
            .since
            .0
            .parse::<jiff::Timestamp>()
            .map_or(now, SystemTime::from);
        let key = item.id.to_string();
        let since = scanned.since(&key, checked_out);
        let Ok(sessions) = find(&working, &rules, since, idle, now) else {
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
        let temp = path.with_extension("md.tmp-sessions");
        std::fs::write(&temp, text).map_err(|e| fail(&e))?;
        std::fs::rename(&temp, &path).map_err(|e| fail(&e))?;
        scanned.mark(&key, last);
        recorded
            .items
            .push((item.id, file.card().title.clone(), sessions.len()));
    }
    if recorded.items.is_empty() {
        return Ok(recorded);
    }
    let count: usize = recorded.items.iter().map(|(_, _, n)| n).sum();
    let mut change = Change::new(
        Operation::Session,
        format!("Record {count} work session(s)"),
        lease.info().person,
        device,
    );
    change.items = recorded.items.iter().map(|(id, _, _)| *id).collect();
    recorded.commit = History::open(archive)
        .map_err(SessionError::History)?
        .commit(archive, &change, lease)
        .map_err(SessionError::History)?;
    if let Some(path) = state_path {
        scanned.save(&path).map_err(|e| fail(&e))?;
    }
    Ok(recorded)
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

    #[test]
    fn what_was_scanned_is_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions/a.json");
        let mut scanned = Scanned::load(&path);
        assert_eq!(scanned.since("x", at(1)), at(1));
        scanned.mark("x", at(90));
        scanned.save(&path).unwrap();
        assert_eq!(Scanned::load(&path).since("x", at(1)), at(90));
        scanned.mark("y", at(90) + Duration::from_millis(400));
        assert_eq!(
            scanned.since("y", at(1)),
            at(90) + Duration::from_secs(1),
            "rounded up, so the change isn't found again"
        );
    }
}
