//! This device's decision log, for `carrel report` (#330): what the owner decided in Review,
//! how cards were corrected, how questions ended, how searches went and how long ingest took,
//! so it's clear how well Carrel is doing and what to tweak.
//!
//! It's kept in Carrel's own folder on each device, never in the archive, as one JSON line
//! per decision in a file per month (`decisions/2026-11.jsonl`). Like `unreadable.json`
//! ([`crate::formats`]), it records identifiers, kinds, file extensions, model names, counts
//! and timings, **never titles, text, names, paths or search queries**, so the log and the
//! report made from it can be shared.
//!
//! Nothing is written until an interface calls [`start`] (the command line, the interactive
//! mode and the apps do as they open), so library code and tests never write the owner's log.
//! It's on unless the device's `report.log` preference is `off`, and lines older than
//! `report.keep_days` ([`KEEP_DAYS`] unless set) are trimmed when it starts.

pub mod advice;
pub mod hooks;
pub mod record;
pub mod report;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::archive::Archive;
use crate::card::Id;
use crate::local::Prefs;

pub use hooks::{
    Corrected, Deciding, compressed, corrected, deciding, drafted, duplicate, moved, question,
    rejected, search, snapshot,
};
pub use record::{Decision, Event, QuestionOutcome, Record, SearchMode};

/// How many days the log keeps, unless `report.keep_days` says otherwise.
pub const KEEP_DAYS: u32 = 90;

/// The folder in the configuration folder that holds the log.
pub const FOLDER: &str = "decisions";

/// Where the log is written, once [`start`] has found it's on.
static LOG: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The log's folder in the configuration folder `config`.
#[must_use]
pub fn folder_in(config: &Path) -> PathBuf {
    config.join(FOLDER)
}

/// Starts logging to this device's configuration folder, unless `report.log` is off there,
/// trimming what's older than `report.keep_days`. Interfaces call it once as they start.
pub fn start() {
    let Some(config) = crate::local::config_dir() else {
        return;
    };
    let prefs = Prefs::load_from(&config).unwrap_or_default();
    start_in(&config, &prefs);
}

/// Starts logging to the configuration folder `config` as `prefs` say: nothing when
/// `report.log` is off. For apps that keep their own folder, and for tests.
pub fn start_in(config: &Path, prefs: &Prefs) {
    let dir = folder_in(config);
    let on = prefs.report_log != Some(false);
    if on {
        trim(&dir, keep_days(prefs), jiff::Timestamp::now());
    }
    if let Ok(mut log) = LOG.lock() {
        *log = on.then_some(dir);
    }
}

/// Stops logging until [`start`] is called again.
pub fn stop() {
    if let Ok(mut log) = LOG.lock() {
        *log = None;
    }
}

/// The folder being logged to, if logging has started.
#[must_use]
pub fn logging_to() -> Option<PathBuf> {
    LOG.lock().ok().and_then(|l| l.clone())
}

/// How many days `prefs` keep the log for.
#[must_use]
pub fn keep_days(prefs: &Prefs) -> u32 {
    prefs.report_keep_days.unwrap_or(KEEP_DAYS).max(1)
}

/// The report on `archive` from the log in the configuration folder `config`, from `since`
/// until now, with the archive's history for the changes programs made on their own, and the
/// device's `unreadable.json`.
#[must_use]
pub fn report_for(archive: &Archive, config: &Path, since: jiff::Timestamp) -> report::UsageReport {
    let records = read(&folder_in(config), since);
    let history = crate::history::History::open(archive)
        .and_then(|h| h.log())
        .unwrap_or_default();
    let unreadable = crate::formats::Unreadable::load(&config.join("unreadable.json"));
    report::UsageReport::of(&report::Sources {
        archive: archive.info().id,
        records: &records,
        history: &history,
        unreadable: &unreadable.entries,
        since,
        until: jiff::Timestamp::now(),
    })
}

/// The file `at`'s month is logged in.
fn file_for(dir: &Path, at: jiff::Timestamp) -> PathBuf {
    dir.join(format!("{}.jsonl", at.strftime("%Y-%m")))
}

/// Logs `event` about `archive`, now. Never fails: a log that can't be written is skipped,
/// as the owner's work matters more than the report.
pub fn note(archive: &Archive, event: Event) {
    let Some(dir) = logging_to() else {
        return;
    };
    let record = Record {
        at: jiff::Timestamp::now().to_string(),
        archive: archive.info().id,
        event,
    };
    let _ = append(&dir, &record);
}

/// Appends `record` to its month's file in `dir`.
fn append(dir: &Path, record: &Record) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let at = record.at.parse().unwrap_or_else(|_| jiff::Timestamp::now());
    let line = serde_json::to_string(record).map_err(std::io::Error::other)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_for(dir, at))?;
    writeln!(file, "{line}")
}

/// Every record in the log folder `dir` at or after `since`, oldest first. Lines this version
/// can't read are skipped.
#[must_use]
pub fn read(dir: &Path, since: jiff::Timestamp) -> Vec<Record> {
    let mut records = Vec::new();
    for path in month_files(dir) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        records.extend(
            text.lines()
                .filter_map(|l| serde_json::from_str::<Record>(l).ok())
                .filter(|r| r.time().is_some_and(|t| t >= since)),
        );
    }
    records.sort_by(|a, b| a.at.cmp(&b.at));
    records
}

/// The month files in `dir`, oldest first.
fn month_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .collect();
    files.sort();
    files
}

/// Removes what's older than `days` before `now` from the log folder `dir`: whole months
/// that ended before then, and the older lines of the month it falls in.
pub fn trim(dir: &Path, days: u32, now: jiff::Timestamp) {
    let cutoff = now - jiff::SignedDuration::from_hours(24 * i64::from(days));
    let keep_from = cutoff.strftime("%Y-%m").to_string();
    for path in month_files(dir) {
        let month = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if month < keep_from {
            let _ = std::fs::remove_file(&path);
        } else if month == keep_from {
            trim_file(&path, cutoff);
        }
    }
}

/// Keeps the lines of `path` at or after `cutoff`, rewriting it only when some are older.
fn trim_file(path: &Path, cutoff: jiff::Timestamp) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let kept: Vec<&str> = text
        .lines()
        .filter(|l| {
            serde_json::from_str::<Record>(l)
                .ok()
                .and_then(|r| r.time())
                .is_some_and(|t| t >= cutoff)
        })
        .collect();
    if kept.len() == text.lines().count() {
        return;
    }
    if kept.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    let _ = std::fs::write(path, kept.join("\n") + "\n");
}

/// The file that remembers the last search's results, by identifier only, so opening one of
/// them, from any interface, can say where it ranked.
const LAST_SEARCH: &str = "last-search.json";

/// How long after a search opening one of its results counts as found by it, in seconds.
const SEARCH_WINDOW: i64 = 30 * 60;

/// The last search's results, by identifier, best first.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LastSearch {
    search: Id,
    archive: Id,
    at: i64,
    items: Vec<Id>,
}

/// Logs a search of `archive` that found `total` results, of which the items are `items`,
/// best first, searched as `mode`, and remembers them so opening one says where it ranked
/// among the items found ([`opened`]).
pub fn searched(archive: &Archive, mode: SearchMode, total: usize, items: Vec<Id>) {
    let Some(dir) = logging_to() else {
        return;
    };
    let search = Id::new_v7();
    let last = LastSearch {
        search,
        archive: archive.info().id,
        at: jiff::Timestamp::now().as_second(),
        items,
    };
    if let Ok(text) = serde_json::to_string(&last) {
        let _ = std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(dir.join(LAST_SEARCH), text));
    }
    note(
        archive,
        Event::Searched {
            search,
            mode,
            results: total,
        },
    );
}

/// Notes that the item `item` of `archive` was opened: when it was among the last search's
/// results, within half an hour, the search is logged as having found it, at its rank.
pub fn opened(archive: &Archive, item: Id) {
    let Some(dir) = logging_to() else {
        return;
    };
    let Some(last) = std::fs::read_to_string(dir.join(LAST_SEARCH))
        .ok()
        .and_then(|t| serde_json::from_str::<LastSearch>(&t).ok())
    else {
        return;
    };
    let fresh = jiff::Timestamp::now().as_second() - last.at <= SEARCH_WINDOW;
    if !fresh || last.archive != archive.info().id {
        return;
    }
    if let Some(rank) = last.items.iter().position(|i| *i == item) {
        note(
            archive,
            Event::Opened {
                search: last.search,
                rank: rank + 1,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_at(at: &str) -> Record {
        Record {
            at: at.into(),
            archive: Id::new_v7(),
            event: Event::Snapshot {
                items: 1,
                holding: 0,
                low: 0,
                medium: 0,
                crowded: 0,
                single: 0,
                spread_tags: 0,
            },
        }
    }

    #[test]
    fn records_go_in_their_months_file_and_old_ones_are_trimmed() {
        let dir = tempfile::tempdir().unwrap();
        for at in [
            "2026-07-20T10:00:00Z",
            "2026-08-05T10:00:00Z",
            "2026-08-20T10:00:00Z",
            "2026-11-01T10:00:00Z",
        ] {
            append(dir.path(), &snapshot_at(at)).unwrap();
        }
        assert!(dir.path().join("2026-07.jsonl").is_file());
        assert_eq!(
            read(dir.path(), "2026-01-01T00:00:00Z".parse().unwrap()).len(),
            4
        );

        // 90 days before 2026-11-01 is 2026-08-03: July goes, August keeps both.
        trim(dir.path(), 90, "2026-11-01T12:00:00Z".parse().unwrap());
        assert!(!dir.path().join("2026-07.jsonl").exists());
        assert_eq!(
            read(dir.path(), "2026-01-01T00:00:00Z".parse().unwrap()).len(),
            3
        );

        // 80 days before is 2026-08-13: the first August line goes.
        trim(dir.path(), 80, "2026-11-01T12:00:00Z".parse().unwrap());
        let kept = read(dir.path(), "2026-01-01T00:00:00Z".parse().unwrap());
        let times: Vec<&str> = kept.iter().map(|r| r.at.as_str()).collect();
        assert_eq!(times, ["2026-08-20T10:00:00Z", "2026-11-01T10:00:00Z"]);
        assert_eq!(
            read(dir.path(), "2026-09-01T00:00:00Z".parse().unwrap()).len(),
            1
        );
    }

    #[test]
    fn the_log_keeps_ninety_days_unless_told() {
        assert_eq!(keep_days(&Prefs::default()), KEEP_DAYS);
        let prefs = Prefs {
            report_keep_days: Some(0),
            ..Prefs::default()
        };
        assert_eq!(keep_days(&prefs), 1, "at least a day");
    }
}
