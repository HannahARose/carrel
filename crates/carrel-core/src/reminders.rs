//! Reminders (`docs/design/reminders.md`): what the archive's `reminders` setting (FORMAT.md
//! §7.2) turns on, worked out here so each interface only presents it.
//!
//! - **`idle`:** a work session has been recorded and its entry waits for approval.
//! - **`weekly`:** once a week, on the first day Carrel runs in it (weeks start on Monday), a
//!   sum of what waits: proposals, questions, pending entries, long checkouts and an old
//!   backup. Skipped when nothing waits.
//! - **`due-date`:** a checkout is due back tomorrow, today, or is overdue (once a day).
//! - **`menu-bar`:** not a reminder here, but whether the Mac agent shows its menu bar list
//!   ([`shows_menu_bar`]).
//!
//! [`due`] lists the reminders due now, which every interface's Status shows. Only the agents
//! notify: [`notify()`] shows each one this device hasn't shown yet through a [`Notifier`] and
//! records it in [`Shown`], kept per device in the configuration folder (`reminders.json`),
//! not in the archive.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jiff::civil::Date;
use jiff::{Timestamp, ToSpan as _, Zoned};
use serde::{Deserialize, Serialize};

use crate::archive::Archive;
use crate::card::Id;
use crate::card_file::CardFile;
use crate::entries::HistoryEntry;

/// The file in the configuration folder that records what this device has shown.
pub const SHOWN_FILE: &str = "reminders.json";
/// How long a checkout is out before the weekly reminder mentions it.
const LONG_CHECKOUT_DAYS: i64 = 14;
/// How old the latest snapshot is before the weekly reminder mentions it.
const OLD_BACKUP_DAYS: i64 = 7;
/// How long what was shown is remembered; older records are dropped.
const REMEMBERED_DAYS: i64 = 60;

/// Which reminder it is: the `reminders` setting's name for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReminderKind {
    /// A recorded work session's entry is waiting.
    Idle,
    /// The week's sum of what's waiting.
    Weekly,
    /// A checkout is due back.
    DueDate,
}

impl ReminderKind {
    /// Its name in the `reminders` setting.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Weekly => "weekly",
            Self::DueDate => "due-date",
        }
    }
}

/// A reminder due now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reminder {
    /// Which reminder.
    pub kind: ReminderKind,
    /// What makes it this reminder and not another, so it's shown once: the kind, the item
    /// and the day or entry it's for.
    pub key: String,
    /// A short heading, such as `Due back tomorrow`.
    pub title: String,
    /// What it says.
    pub text: String,
    /// The item it's about, to open when it's chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<Id>,
}

/// Whether the Mac agent shows its menu bar list for `archive`: the `menu-bar` reminder.
#[must_use]
pub fn shows_menu_bar(archive: &Archive) -> bool {
    on(archive, "menu-bar")
}

fn on(archive: &Archive, name: &str) -> bool {
    archive.settings().reminders.iter().any(|r| r == name)
}

/// The reminders due in `archive` at `now`, as the `reminders` setting turns them on: idle
/// entries waiting, then checkouts due back, then the weekly sum. `device` is this device, for
/// its desk; `shown` says when the weekly sum was first shown this week, since it's due only
/// on that day.
#[must_use]
pub fn due(archive: &Archive, device: Option<Id>, now: &Zoned, shown: &Shown) -> Vec<Reminder> {
    let found = Found::read(archive, device);
    let today = now.date();
    let mut due = Vec::new();
    if on(archive, "idle") {
        due.extend(found.pending.iter().map(idle_reminder));
    }
    if on(archive, "due-date") {
        due.extend(found.out.iter().filter_map(|o| due_reminder(o, today)));
    }
    if on(archive, "weekly")
        && let Some(weekly) = weekly_reminder(archive, &found, now)
    {
        let first_shown = shown
            .when(archive.doc().id, &weekly.key)
            .map(|t| t.to_zoned(now.time_zone().clone()).date());
        if first_shown.is_none_or(|day| day == today) {
            due.push(weekly);
        }
    }
    due
}

/// A checked-out item, as the reminders need it.
struct Out {
    id: Id,
    title: String,
    since: Option<Date>,
    due: Option<Date>,
}

/// A pending history entry, with its item.
struct Pending {
    id: Id,
    title: String,
    entry: HistoryEntry,
}

/// What's checked out and on the desk, read from the cards.
struct Found {
    out: Vec<Out>,
    pending: Vec<Pending>,
}

impl Found {
    fn read(archive: &Archive, device: Option<Id>) -> Self {
        let mut found = Self {
            out: Vec::new(),
            pending: Vec::new(),
        };
        for item in archive.items().unwrap_or_default() {
            let Some(file) = read_card(&archive.root().join(&item.card)) else {
                continue;
            };
            let Some(checkout) = file.card().checked_out.clone() else {
                continue;
            };
            let title = file.card().title.clone();
            found.add_pending(item.id, &title, &file);
            found.out.push(Out {
                id: item.id,
                title,
                since: date_of(&checkout.since.0),
                due: checkout.due.as_ref().and_then(|d| date_of(&d.0)),
            });
        }
        let desk = device
            .and_then(|d| crate::desk::Desk::load(archive, d).ok())
            .unwrap_or_default();
        for entry in desk.items {
            if let Some(file) = read_card(&entry.card) {
                let title = file.card().title.clone();
                found.add_pending(entry.id, &title, &file);
            }
        }
        found
    }

    fn add_pending(&mut self, id: Id, title: &str, file: &CardFile) {
        let Ok(log) = file.history() else {
            return;
        };
        for i in log.pending() {
            self.pending.push(Pending {
                id,
                title: title.to_owned(),
                entry: log.entries[i].clone(),
            });
        }
    }
}

fn read_card(path: &Path) -> Option<CardFile> {
    CardFile::parse(&std::fs::read_to_string(path).ok()?).ok()
}

/// The day a date or timestamp is on, from its `YYYY-MM-DD` start.
fn date_of(text: &str) -> Option<Date> {
    text.get(..10)?.parse().ok()
}

fn idle_reminder(p: &Pending) -> Reminder {
    let worked = match &p.entry.duration {
        Some(duration) => format!("You worked on “{}” for {duration}.", p.title),
        None => format!("You worked on “{}” on {}.", p.title, p.entry.date),
    };
    Reminder {
        kind: ReminderKind::Idle,
        key: format!(
            "idle:{}:{}:{}",
            p.id,
            p.entry.date,
            p.entry.duration.as_deref().unwrap_or("")
        ),
        title: "A work session's entry is waiting".into(),
        text: format!("{worked} Its entry is waiting for you."),
        item: Some(p.id),
    }
}

fn due_reminder(o: &Out, today: Date) -> Option<Reminder> {
    let due = o.due?;
    let (title, text) = if today == due.yesterday().ok()? {
        (
            "Due back tomorrow".to_owned(),
            format!("“{}” is due back tomorrow.", o.title),
        )
    } else if today == due {
        (
            "Due back today".to_owned(),
            format!("“{}” is due back today.", o.title),
        )
    } else if today > due {
        let days = (today - due).get_days();
        (
            "Overdue".to_owned(),
            format!(
                "“{}” was due back on {due}, {} ago.",
                o.title,
                days_text(i64::from(days))
            ),
        )
    } else {
        return None;
    };
    Some(Reminder {
        kind: ReminderKind::DueDate,
        key: format!("due-date:{}:{today}", o.id),
        title,
        text,
        item: Some(o.id),
    })
}

fn days_text(days: i64) -> String {
    if days == 1 {
        "1 day".into()
    } else {
        format!("{days} days")
    }
}

fn count_text(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// The week `now` is in, as `2026-W44`: weeks start on Monday.
fn week_of(now: &Zoned) -> String {
    let week = now.date().iso_week_date();
    format!("{}-W{:02}", week.year(), week.week())
}

/// The weekly sum of what's waiting in `archive`, or `None` when nothing is.
fn weekly_reminder(archive: &Archive, found: &Found, now: &Zoned) -> Option<Reminder> {
    let today = now.date();
    let mut parts = Vec::new();
    let proposals = waiting_proposals(archive);
    if proposals > 0 {
        parts.push(format!(
            "{} to review",
            count_text(proposals, "proposal", "proposals")
        ));
    }
    let questions = waiting_questions(archive);
    if questions > 0 {
        parts.push(count_text(questions, "question", "questions"));
    }
    if !found.pending.is_empty() {
        parts.push(format!(
            "{} to approve",
            count_text(found.pending.len(), "history entry", "history entries")
        ));
    }
    for o in &found.out {
        if let Some(since) = o.since {
            let days = i64::from((today - since).get_days());
            if days > LONG_CHECKOUT_DAYS {
                parts.push(format!("“{}” out for {}", o.title, days_text(days)));
            }
        }
    }
    if let Some(days) = days_since_backup(archive, now.timestamp())
        && days > OLD_BACKUP_DAYS
    {
        parts.push(format!("no backup for {}", days_text(days)));
    }
    if parts.is_empty() {
        return None;
    }
    Some(Reminder {
        kind: ReminderKind::Weekly,
        key: format!("weekly:{}", week_of(now)),
        title: "This week in your archive".into(),
        text: format!("Waiting for you: {}.", parts.join(", ")),
        item: None,
    })
}

/// Filing proposals, restructure proposals and the crawler's findings waiting in Review.
fn waiting_proposals(archive: &Archive) -> usize {
    let filing = crate::review::ReviewQueue::of(archive)
        .list()
        .map_or(0, |(entries, _)| {
            entries.iter().filter(|e| e.state.is_open()).count()
        });
    let restructure = crate::restructure::waiting(archive, None).map_or(0, |p| p.len());
    let crawler = crate::crawl::findings::list(archive).map_or(0, |f| f.len());
    filing + restructure + crawler
}

/// Questions waiting that the owner hasn't declined.
fn waiting_questions(archive: &Archive) -> usize {
    crate::review::Queue::<crate::questions::QuestionEntry>::of(archive)
        .list()
        .map_or(0, |(entries, _)| {
            entries.iter().filter(|q| q.declined.is_none()).count()
        })
}

/// Days since the latest snapshot history records, or `None` when none was ever taken.
fn days_since_backup(archive: &Archive, now: Timestamp) -> Option<i64> {
    let log = crate::history::History::open(archive).ok()?.log().ok()?;
    let latest = crate::backup::snapshot_entries(&log)
        .map(|e| e.time)
        .max()?;
    Some((now.as_second() - latest) / 86_400)
}

/// What this device has shown, by archive and reminder key, with when it was first shown
/// (`reminders.json` in the configuration folder).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shown {
    /// By archive identifier, then reminder key: when, as RFC 3339.
    #[serde(default)]
    pub archives: BTreeMap<Id, BTreeMap<String, String>>,
}

impl Shown {
    /// The record's path in the configuration folder `config`.
    #[must_use]
    pub fn path(config: &Path) -> PathBuf {
        config.join(SHOWN_FILE)
    }

    /// The record in `config`; empty when there's none or it can't be read.
    #[must_use]
    pub fn load(config: &Path) -> Self {
        std::fs::read_to_string(Self::path(config))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Writes the record to `config`.
    ///
    /// # Errors
    ///
    /// The error writing it.
    pub fn save(&self, config: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(config)?;
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)? + "\n";
        // Written beside it and renamed, so an agent and a command never read half of it.
        let path = Self::path(config);
        let partial = path.with_extension("json.tmp");
        std::fs::write(&partial, text)?;
        std::fs::rename(&partial, &path)
    }

    /// When the reminder `key` of `archive` was first shown here.
    #[must_use]
    pub fn when(&self, archive: Id, key: &str) -> Option<Timestamp> {
        self.archives.get(&archive)?.get(key)?.parse().ok()
    }

    /// Records the reminder `key` of `archive` as shown at `now`, unless it already was, and
    /// forgets records older than two months.
    pub fn mark(&mut self, archive: Id, key: &str, now: Timestamp) {
        let keys = self.archives.entry(archive).or_default();
        keys.entry(key.to_owned())
            .or_insert_with(|| now.to_string());
        let forget = now - (REMEMBERED_DAYS * 24).hours();
        keys.retain(|_, at| at.parse::<Timestamp>().is_ok_and(|t| t >= forget));
    }

    /// The reminders in `due` not yet shown here.
    #[must_use]
    pub fn fresh(&self, archive: Id, due: Vec<Reminder>) -> Vec<Reminder> {
        due.into_iter()
            .filter(|r| self.when(archive, &r.key).is_none())
            .collect()
    }
}

/// Shows reminders to the owner: a desktop notification, or a test's record of them.
pub trait Notifier {
    /// Shows `reminder` from the archive named `archive`.
    ///
    /// # Errors
    ///
    /// Why it couldn't, in words; the reminder is tried again later.
    fn show(&mut self, archive: &str, reminder: &Reminder) -> Result<(), String>;
}

/// What [`notify()`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notified {
    /// The reminders shown.
    pub shown: Vec<Reminder>,
    /// Those that couldn't be, with why.
    pub failed: Vec<(Reminder, String)>,
}

/// Shows each reminder due in `archive` at `now` that this device hasn't shown yet, through
/// `notifier`, and records it in `config`'s [`Shown`]. A reminder that couldn't be shown is
/// tried again next time.
///
/// # Errors
///
/// The error writing the record.
pub fn notify(
    archive: &Archive,
    device: Option<Id>,
    config: &Path,
    now: &Zoned,
    notifier: &mut dyn Notifier,
) -> std::io::Result<Notified> {
    let id = archive.doc().id;
    let mut shown = Shown::load(config);
    let fresh = shown.fresh(id, due(archive, device, now, &shown));
    let mut done = Notified::default();
    for reminder in fresh {
        match notifier.show(&archive.doc().name, &reminder) {
            Ok(()) => {
                shown.mark(id, &reminder.key, now.timestamp());
                done.shown.push(reminder);
            }
            Err(why) => done.failed.push((reminder, why)),
        }
    }
    if !done.shown.is_empty() {
        shown.save(config)?;
    }
    Ok(done)
}

/// Records the reminders `keys` of `archive` as shown at `now` in `config`, for an interface
/// that shows them itself, as the Mac agent does.
///
/// # Errors
///
/// The error writing the record.
pub fn mark_shown(
    config: &Path,
    archive: Id,
    keys: &[String],
    now: Timestamp,
) -> std::io::Result<()> {
    let mut shown = Shown::load(config);
    for key in keys {
        shown.mark(archive, key, now);
    }
    shown.save(config)
}
