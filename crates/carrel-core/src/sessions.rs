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
use crate::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder, WriterLease};
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
        crate::local::config_dir().map(|d| Self::path_in(&d, archive_id))
    }

    /// The file it's kept in for the archive `archive_id`, in the configuration folder
    /// `config`.
    #[must_use]
    pub fn path_in(config: &Path, archive_id: &str) -> PathBuf {
        config.join("sessions").join(format!("{archive_id}.json"))
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

/// An item whose pending entries the owner approved by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedByHand {
    /// Its card's identifier.
    pub id: Id,
    /// Its title.
    pub title: String,
    /// How many entries.
    pub entries: usize,
}

/// What recording found: the items that got entries, entries approved by hand, and the
/// commit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recorded {
    /// Each item with new entries.
    pub items: Vec<RecordedItem>,
    /// The items with pending entries the owner approved by hand, noticed and recorded.
    pub approved_by_hand: Vec<ApprovedByHand>,
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
    /// The request doesn't make sense, such as an entry that isn't pending.
    Invalid(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Archive(why) | Self::Invalid(why) => f.write_str(why),
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
    record_in(archive, lease, Scanned::path(&archive.doc().id.to_string()))
}

/// [`record`], keeping what's been looked at in `state_path` rather than in Carrel's folder
/// on this device: for a caller with a configuration folder of its own, such as the Mac
/// app's. With `None`, everything is looked at as if for the first time and nothing is kept.
///
/// # Errors
///
/// [`SessionError`] if the archive can't be read or the cards and history written.
pub fn record_in(
    archive: &Archive,
    lease: &WriterLease,
    state_path: Option<PathBuf>,
) -> Result<Recorded, SessionError> {
    let fail = |e: &dyn std::fmt::Display| SessionError::Archive(e.to_string());
    let device = lease.info().device;
    let rules = archive.rules().map_err(|e| fail(&e))?;
    let idle = Duration::from_mins(u64::from(archive.settings().sessions.idle_minutes));
    let now = SystemTime::now();
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
    commit_what_was_found(archive, lease, &mut recorded)?;
    if let Some(path) = state_path {
        scanned.save(&path).map_err(|e| fail(&e))?;
    }
    Ok(recorded)
}

/// Notices entries approved by hand, brings `Active/` and the Journal up to date, and commits
/// that with the sessions `recorded` found, as one `session` change. Without history, only
/// sessions found regenerate `Active/`, and nothing is committed.
fn commit_what_was_found(
    archive: &Archive,
    lease: &WriterLease,
    recorded: &mut Recorded,
) -> Result<(), SessionError> {
    let fail = |e: &dyn std::fmt::Display| SessionError::Archive(e.to_string());
    let history = match History::open(archive) {
        Ok(history) => history,
        Err(HistoryError::NotInitialized) if recorded.items.is_empty() => return Ok(()),
        Err(e) => return Err(SessionError::History(e)),
    };
    recorded.approved_by_hand = approved_by_hand(archive, &history)?;
    // Always, so the Journal catches up even when another change took in an approval first.
    let regenerated = crate::active::regenerate(archive).map_err(|e| fail(&e))?;
    let approved: usize = recorded.approved_by_hand.iter().map(|a| a.entries).sum();
    let mut said = Vec::new();
    if !recorded.items.is_empty() {
        said.push(format!("Record {} work session(s)", recorded.sessions()));
    }
    if approved > 0 {
        let entries = if approved == 1 { "entry" } else { "entries" };
        said.push(format!(
            "{} {approved} history {entries} approved by hand",
            if said.is_empty() { "Note" } else { "note" }
        ));
    }
    if said.is_empty() {
        if regenerated.changed.is_empty() {
            return Ok(());
        }
        said.push("Bring Active/ and the Journal up to date".to_owned());
    }
    let mut change = Change::new(
        Operation::Session,
        said.join("; "),
        lease.info().person,
        lease.info().device,
    );
    change.items = recorded.items.iter().map(|i| i.id).collect();
    for a in &recorded.approved_by_hand {
        if !change.items.contains(&a.id) {
            change.items.push(a.id);
        }
    }
    recorded.commit = history
        .commit(archive, &change, lease)
        .map_err(SessionError::History)?;
    Ok(())
}

/// The archive's cards whose pending entries were approved by hand since history last
/// recorded them (FORMAT.md §5.5): the callout turned into a normal entry, in Obsidian say.
/// The cards are left exactly as the owner wrote them.
///
/// # Errors
///
/// [`SessionError`] if the archive can't be listed or history read.
pub fn approved_by_hand(
    archive: &Archive,
    history: &History,
) -> Result<Vec<ApprovedByHand>, SessionError> {
    let fail = |e: &dyn std::fmt::Display| SessionError::Archive(e.to_string());
    let counts = |text: &str| {
        let log = CardFile::parse(text).ok()?.history().ok()?;
        let pending = log.pending().len();
        Some((pending, log.entries.len() - pending))
    };
    let mut found = Vec::new();
    for item in archive.items().map_err(|e| fail(&e))? {
        let Some(before) = history
            .committed(&item.card)
            .map_err(SessionError::History)?
        else {
            continue;
        };
        let Ok(now) = std::fs::read_to_string(archive.root().join(&item.card)) else {
            continue;
        };
        if now == before {
            continue;
        }
        let (Some((pending_before, approved_before)), Some((pending_now, approved_now))) =
            (counts(&before), counts(&now))
        else {
            continue;
        };
        // Each approval takes a pending entry away and adds an approved one; editing or
        // removing entries by hand changes one count without the other.
        let entries = pending_before
            .saturating_sub(pending_now)
            .min(approved_now.saturating_sub(approved_before));
        if entries > 0 {
            let title = CardFile::parse(&now)
                .map(|f| f.card().title.clone())
                .unwrap_or_default();
            found.push(ApprovedByHand {
                id: item.id,
                title,
                entries,
            });
        }
    }
    Ok(found)
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

/// The session snapshots of the checked-out `item`'s working copy taken since it went out,
/// oldest first: what returning it can offer to forget. None when it isn't checked out, or
/// when they can't be listed.
#[must_use]
pub fn return_snapshots(
    archive: &Archive,
    item: &crate::archive::ItemRef,
    backups: &dyn Backups,
) -> Vec<Snapshot> {
    let Some(out) = std::fs::read_to_string(archive.root().join(&item.card))
        .ok()
        .and_then(|t| CardFile::parse(&t).ok())
        .and_then(|f| f.card().checked_out.clone())
    else {
        return Vec::new();
    };
    let since = out
        .since
        .0
        .parse::<jiff::Timestamp>()
        .map_or(SystemTime::UNIX_EPOCH, SystemTime::from);
    snapshots_since(backups, Path::new(&out.to), item.id, since).unwrap_or_default()
}

/// What became of a checkout's session snapshots once it was returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnSnapshots {
    /// The snapshot holding the returned version, unless it couldn't be taken.
    pub returned: Result<Snapshot, String>,
    /// How many session snapshots were forgotten.
    pub forgotten: usize,
    /// How many are kept.
    pub kept: usize,
    /// The snapshots that couldn't be forgotten, with why.
    pub failures: Vec<(String, String)>,
}

/// After returning the item `id`, titled `title`, from `working`: snapshots the working copy,
/// which is now the returned version, then forgets its session `snapshots` when `forget`
/// says so. They're all kept if the returned version can't be snapshotted; deleting backups
/// is the owner's call, so `forget` is only ever their answer.
pub fn keep_returned_version(
    backups: &dyn Backups,
    working: &Path,
    title: &str,
    id: Id,
    snapshots: &[Snapshot],
    forget: bool,
) -> ReturnSnapshots {
    let returned = backups
        .snapshot(working, &format!("Returned “{title}” ({id})"))
        .map_err(|e| e.to_string());
    let mut failures = Vec::new();
    let mut forgotten = 0;
    if returned.is_ok() && forget {
        for snapshot in snapshots {
            match backups.forget(&snapshot.id) {
                Ok(()) => forgotten += 1,
                Err(e) => failures.push((snapshot.id.clone(), e.to_string())),
            }
        }
    }
    ReturnSnapshots {
        returned,
        forgotten,
        kept: snapshots.len() - forgotten,
        failures,
    }
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
            ..Recorded::default()
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

/// What to do with an item's pending history entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Approve them.
    Approve,
    /// Drop them.
    Drop,
    /// Combine them into one, still pending.
    Combine,
}

/// A decision on pending entries, made: what the history says, and its commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decided {
    /// The change's summary.
    pub summary: String,
    /// How many entries it concerned.
    pub entries: usize,
    /// The history commit.
    pub commit: Option<String>,
}

/// `1 history entry`, `2 history entries`.
#[must_use]
pub fn entries(n: usize) -> String {
    if n == 1 {
        "1 history entry".to_owned()
    } else {
        format!("{n} history entries")
    }
}

fn read_card(card: &Path) -> Result<CardFile, SessionError> {
    let fail =
        |e: &dyn std::fmt::Display| SessionError::Archive(format!("{}: {e}", card.display()));
    let text = std::fs::read_to_string(card).map_err(|e| fail(&e))?;
    CardFile::parse(&text).map_err(|e| fail(&e))
}

/// Where pending entry `n` (numbered from 1, as `carrel sessions` and the Active screen show
/// them) is among `pending`.
fn position(pending: &[usize], n: usize) -> Result<usize, SessionError> {
    pending.get(n.wrapping_sub(1)).copied().ok_or_else(|| {
        SessionError::Invalid(format!(
            "there's no pending entry {n}; `carrel sessions` lists them"
        ))
    })
}

/// Approves, drops or combines the pending entries numbered `numbers` (from 1; none means all
/// of them) on the card at `card`, of the item `item`, as one `session` change made as
/// `holder`.
///
/// # Errors
///
/// [`SessionError::Invalid`] for an entry that isn't there or no pending entries, or an error
/// reading or writing the card or the history.
pub fn decide(
    archive: &Archive,
    card: &Path,
    item: Id,
    numbers: &[usize],
    decision: Decision,
    holder: Holder,
) -> Result<Decided, SessionError> {
    let mut file = read_card(card)?;
    let invalid = |e: &dyn std::fmt::Display| SessionError::Invalid(e.to_string());
    let mut log = file.history().map_err(|e| invalid(&e))?;
    let pending = log.pending();
    let chosen: Vec<usize> = if numbers.is_empty() {
        pending.clone()
    } else {
        numbers
            .iter()
            .map(|&n| position(&pending, n))
            .collect::<Result<_, _>>()?
    };
    if chosen.is_empty() {
        return Err(SessionError::Invalid("no pending entries to decide".into()));
    }
    let title = file.card().title.clone();
    let summary = match decision {
        Decision::Approve => {
            for &i in &chosen {
                log.approve(i).map_err(|e| invalid(&e))?;
            }
            format!("Approve {} of “{title}”", entries(chosen.len()))
        }
        Decision::Drop => {
            // From the last, so earlier positions stay right.
            let mut sorted = chosen.clone();
            sorted.sort_unstable();
            for &i in sorted.iter().rev() {
                log.drop_entry(i).map_err(|e| invalid(&e))?;
            }
            format!("Drop {} of “{title}”", entries(chosen.len()))
        }
        Decision::Combine => {
            log.combine(&chosen).map_err(|e| invalid(&e))?;
            format!("Combine {} of “{title}”", entries(chosen.len()))
        }
    };
    file.set_history(&log).map_err(|e| invalid(&e))?;
    let commit = write_and_commit(archive, card, &file, item, &summary, holder)?;
    Ok(Decided {
        summary,
        entries: chosen.len(),
        commit,
    })
}

/// Replaces the text of the pending entry `was` on the card at `card`, of the item `item`,
/// with `text`, still pending, as one change made as `holder`. The card is read again first,
/// so changes made meanwhile are kept. `None` when the text is the same.
///
/// # Errors
///
/// [`SessionError::Invalid`] if the entry is no longer pending as it was, or the text would
/// break the history section; or an error reading or writing.
pub fn edit_entry(
    archive: &Archive,
    card: &Path,
    item: Id,
    was: &HistoryEntry,
    text: &str,
    holder: Holder,
) -> Result<Option<Decided>, SessionError> {
    let mut file = read_card(card)?;
    let invalid = |e: &dyn std::fmt::Display| SessionError::Invalid(e.to_string());
    let mut log = file.history().map_err(|e| invalid(&e))?;
    let Some(i) = log.pending().into_iter().find(|&i| log.entries[i] == *was) else {
        return Err(SessionError::Invalid(
            "the entry changed while it was being edited, so nothing was saved".into(),
        ));
    };
    if !log.edit(i, text).map_err(|e| invalid(&e))? {
        return Ok(None);
    }
    file.set_history(&log).map_err(|e| invalid(&e))?;
    let summary = format!("Edit a history entry of “{}”", file.card().title);
    let commit = write_and_commit(archive, card, &file, item, &summary, holder)?;
    Ok(Some(Decided {
        summary,
        entries: 1,
        commit,
    }))
}

/// Writes `file` to `path` and commits it as a `session` change, holding the writer lease.
fn write_and_commit(
    archive: &Archive,
    path: &Path,
    file: &CardFile,
    item: Id,
    summary: &str,
    holder: Holder,
) -> Result<Option<String>, SessionError> {
    let fail = |e: &dyn std::fmt::Display| SessionError::Archive(e.to_string());
    let text = file.to_text().map_err(|e| fail(&e))?;
    let lease = DiskLock::for_archive(archive)
        .acquire(holder, "sessions", DEFAULT_LEASE)
        .map_err(|e| fail(&e))?;
    let result = (|| -> Result<Option<String>, SessionError> {
        let temp = path.with_extension("md.tmp-sessions");
        std::fs::write(&temp, text).map_err(|e| fail(&e))?;
        std::fs::rename(&temp, path).map_err(|e| fail(&e))?;
        let mut change = Change::new(
            Operation::Session,
            summary,
            lease.info().person,
            lease.info().device,
        );
        change.items = vec![item];
        // The Journal lists approved entries, so it follows the cards.
        crate::active::regenerate(archive).map_err(|e| fail(&e))?;
        History::open(archive)
            .and_then(|h| h.commit(archive, &change, &lease))
            .map_err(SessionError::History)
    })();
    lease.release().map_err(|e| fail(&e))?;
    result
}
