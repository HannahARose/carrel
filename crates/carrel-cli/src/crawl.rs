//! `carrel crawl`: the crawler that rechecks where items are filed
//! (`docs/design/restructure.md`, "Crawlers", [`carrel_core::crawl`]). It runs in the agent
//! and at the end of the background worker when the device is idle and on power; `carrel crawl
//! status` shows how far it has got, its findings wait in `carrel review`, and `carrel crawl
//! undo` takes back the changes it made itself.
//!
//! How idle the device is and whether it's on power are found out here, since the core has no
//! platform-specific code: on a Mac from `ioreg` and `pmset`, on Linux whether it's on mains
//! power from `/sys/class/power_supply`. What can't be told doesn't stop the crawler.

use std::fmt::Write as _;
use std::path::Path;
use std::time::{Duration, SystemTime};

use carrel_core::archive::Archive;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::crawl::verdicts::Verdicts;
use carrel_core::crawl::{AGENT, CrawlError, Crawled, Crawler, Device};
use carrel_core::history::{Entry, History};
use carrel_core::librarian::changes;
use carrel_core::providers::Registry;
use serde_json::{Value, json};

use crate::commands::{holder, locate, plural};
use crate::config::Identity;
use crate::{Failure, Report};

/// How many days of the crawler's changes Status lists.
const RECENT_DAYS: i64 = 7;

/// What's known about this device right now.
#[must_use]
pub fn device() -> Device {
    Device {
        idle: idle(),
        on_power: on_power(),
    }
}

/// How long since the keyboard or pointer was last used.
#[cfg(target_os = "macos")]
fn idle() -> Option<Duration> {
    let out = std::process::Command::new("ioreg")
        .args(["-c", "IOHIDSystem", "-d", "4"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.contains("\"HIDIdleTime\""))?;
    let nanoseconds: u64 = line.rsplit('=').next()?.trim().parse().ok()?;
    Some(Duration::from_nanos(nanoseconds))
}

#[cfg(not(target_os = "macos"))]
fn idle() -> Option<Duration> {
    None
}

/// Whether the device is on mains power rather than battery.
#[cfg(target_os = "macos")]
fn on_power() -> Option<bool> {
    let out = std::process::Command::new("pmset")
        .args(["-g", "batt"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().next()?;
    Some(first.contains("AC Power"))
}

#[cfg(target_os = "linux")]
fn on_power() -> Option<bool> {
    let supplies = std::fs::read_dir("/sys/class/power_supply").ok()?;
    let mut battery = false;
    for supply in supplies.filter_map(Result::ok) {
        let read = |name: &str| {
            std::fs::read_to_string(supply.path().join(name))
                .map(|s| s.trim().to_owned())
                .unwrap_or_default()
        };
        match read("type").as_str() {
            "Mains" if read("online") == "1" => return Some(true),
            "Battery" => battery = true,
            _ => {}
        }
    }
    // A desktop has no battery, so it's always on power.
    Some(!battery)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn on_power() -> Option<bool> {
    None
}

/// The crawler for `archive` here, on the strongest model the device has, or why there's
/// none.
fn crawler<'a>(archive: &'a Archive, registry: &'a Registry) -> Result<Crawler<'a>, Failure> {
    let model =
        carrel_core::crawl::model::choose(registry, crate::ask::memory_gb()).ok_or_else(|| {
            Failure::new(
                "the crawler needs a text model, and this device has none (see `carrel models`)",
            )
        })?;
    let identity = Identity::load_or_create()?;
    Ok(Crawler {
        archive,
        registry,
        model,
        holder: holder(&identity),
    })
}

fn crawl_failure(error: &CrawlError) -> Failure {
    Failure::new(error.to_string())
}

/// A pass, when the crawler's pace says one is due and the device is idle and on power: what
/// it did, or `None` when it didn't run. It stops for anything the owner does: the device
/// being used, or items arriving in the intake queue. It's what the agent's job and the end of
/// the background worker run. Like the background worker, it doesn't run when `CARREL_WORKER`
/// is `off`.
///
/// # Errors
///
/// [`Failure`] if there's no model, or the pass failed.
pub fn when_idle(archive: &Archive) -> Result<Option<Crawled>, Failure> {
    if std::env::var(crate::worker::WORKER_ENV).is_ok_and(|v| v.eq_ignore_ascii_case("off")) {
        return Ok(None);
    }
    let pace = archive.settings().crawler.pace;
    let identity = Identity::load_or_create()?;
    let last = Verdicts::load(archive, identity.device)
        .last_pass
        .and_then(|t| t.parse::<jiff::Timestamp>().ok())
        .map(SystemTime::from);
    if !pace.due(last, SystemTime::now()) || pace.may_run(&device()).is_err() {
        return Ok(None);
    }
    let (registry, _) = crate::providers::registry();
    let crawler = crawler(archive, &registry)?;
    let device_id = identity.device;
    let stop = || {
        pace.may_run(&device()).is_err()
            || carrel_core::intake::waiting(archive, device_id, false).is_ok_and(|w| !w.is_empty())
    };
    crawler
        .pass(pace.items(), &stop)
        .map(Some)
        .map_err(|e| crawl_failure(&e))
}

/// `carrel crawl run`: a pass now, of `items` items (the pace's share when not given), whatever
/// the pace and however idle the device, since the owner asked.
///
/// # Errors
///
/// [`Failure`] if there's no model, or the pass failed.
pub fn run(archive: Option<&Path>, items: Option<usize>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let (registry, _) = crate::providers::registry();
    let crawler = crawler(&archive, &registry)?;
    let items = items.unwrap_or_else(|| archive.settings().crawler.pace.items().max(1));
    let did = crawler
        .pass(items, &|| false)
        .map_err(|e| crawl_failure(&e))?;
    let mut text = match did.summary() {
        Some(summary) => format!("The crawler {summary}.\n"),
        None => "Nothing is due for the crawler to check.\n".to_owned(),
    };
    for made in &did.made {
        writeln!(text, "  made: {made}").unwrap();
    }
    if did.proposed > 0 {
        text.push_str("See its findings with `carrel review`.\n");
    }
    for failed in &did.failed {
        writeln!(text, "warning: {failed}").unwrap();
    }
    Ok(Report::new(text, json!(did)))
}

/// `carrel crawl status`: the crawler's pace, how far it has got and what it found.
///
/// # Errors
///
/// [`Failure`] if the archive, the verdicts or the findings can't be read.
pub fn status(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let identity = Identity::load_or_create()?;
    let status = carrel_core::crawl::status(&archive, identity.device, jiff::Timestamp::now())
        .map_err(|e| crawl_failure(&e))?;
    let mut text = format!(
        "Pace: {}; it {} the changes it's sure of itself (crawler.changes {}).\n",
        status.pace.as_str(),
        if status.changes {
            "makes"
        } else {
            "only proposes"
        },
        if status.changes { "on" } else { "off" }
    );
    writeln!(
        text,
        "Checked {} of {}.",
        status.judged,
        plural(status.items, "item")
    )
    .unwrap();
    if !status.waiting.is_empty() {
        text.push_str("Due to be checked, in this order:\n");
        for (stage, count) in &status.waiting {
            writeln!(text, "  {} {}", plural(*count, "item"), stage.describe()).unwrap();
        }
    }
    if !status.drafts.is_empty() {
        writeln!(
            text,
            "{} could get a better draft, from a stronger model or a newer Carrel:",
            plural(status.drafts.len(), "card")
        )
        .unwrap();
        for card in status.drafts.iter().take(DRAFTS_LISTED) {
            writeln!(text, "  {card}").unwrap();
        }
        if status.drafts.len() > DRAFTS_LISTED {
            writeln!(text, "  and {} more", status.drafts.len() - DRAFTS_LISTED).unwrap();
        }
    }
    if !status.misfits.is_empty() {
        text.push_str("Collections whose items keep not fitting:\n");
        for (collection, failed, judged) in &status.misfits {
            writeln!(text, "  {collection}: {failed} of {judged}").unwrap();
        }
    }
    if status.findings > 0 {
        writeln!(
            text,
            "{} waiting in `carrel review`.",
            plural(status.findings, "finding")
        )
        .unwrap();
    }
    match &status.last_pass {
        Some(at) => writeln!(text, "Last pass on this device: {at}.").unwrap(),
        None => text.push_str("No pass has run on this device yet.\n"),
    }
    let history = History::open(&archive).ok().and_then(|h| h.log().ok());
    let (recent_text, recent) = status_section(&archive, None, history.as_deref());
    text.push_str(&recent_text);
    let mut value = json!(status);
    value["recent"] = recent;
    Ok(Report::new(text, value))
}

/// How many outdated drafts `carrel crawl status` lists.
const DRAFTS_LISTED: usize = 20;

/// The part of `carrel status` about the crawler: how many cards could get a better draft, as
/// this device (`device`) sees it, and its changes this week, for people and for programs.
/// Empty when there's neither.
pub fn status_section(
    archive: &carrel_core::archive::Archive,
    device: Option<carrel_core::card::Id>,
    history: Option<&[Entry]>,
) -> (String, Value) {
    let since = jiff::Timestamp::now().as_second() - RECENT_DAYS * 86_400;
    let recent = history
        .map(|log| changes::recent_by(log, AGENT, since))
        .unwrap_or_default();
    let drafts = device
        .and_then(|d| carrel_core::crawl::status(archive, d, jiff::Timestamp::now()).ok())
        .map_or(0, |s| s.drafts.len());
    let value = json!({
        "since_days": RECENT_DAYS,
        "changes": recent.iter().map(crate::commands::entry_json).collect::<Vec<_>>(),
        "better_drafts": drafts,
    });
    let mut text = String::new();
    if drafts > 0 {
        writeln!(
            text,
            "\n{} could get a better draft: `carrel crawl status` lists them",
            plural(drafts, "card")
        )
        .unwrap();
    }
    if recent.is_empty() {
        return (text, value);
    }
    write!(
        text,
        "\nThe crawler's changes this week ({}; undo them all with `carrel crawl undo {RECENT_DAYS}d`):\n",
        plural(recent.len(), "change")
    )
    .unwrap();
    for entry in recent.iter().take(5) {
        writeln!(text, "  {}", crate::commands::log_line(entry)).unwrap();
    }
    if recent.len() > 5 {
        writeln!(text, "  and {} more: `carrel log`", recent.len() - 5).unwrap();
    }
    (text, value)
}

/// `carrel crawl undo SINCE`: undoes every change the crawler made on its own since then,
/// newest first.
///
/// # Errors
///
/// [`Failure`] if `since` can't be read, or the history can't be.
pub fn undo(archive: Option<&Path>, since: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let at = changes::parse_since(since, &jiff::Zoned::now()).ok_or_else(|| {
        Failure::new(format!(
            "{since} isn't a time; give a span such as 24h or 7d, `today`, or a date such as 2026-11-01"
        ))
    })?;
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "undo", DEFAULT_LEASE)?;
    let result = changes::undo_since_by(&archive, AGENT, at, &lease);
    lease.release()?;
    let done = result?;
    let mut text = if done.undone.is_empty() && done.failed.is_empty() {
        "The crawler made no changes since then that are still there.\n".to_owned()
    } else {
        format!(
            "Undid {} the crawler made:\n",
            plural(done.undone.len(), "change")
        )
    };
    for (entry, _) in &done.undone {
        writeln!(text, "  {}", crate::commands::log_line(entry)).unwrap();
    }
    for (entry, why) in &done.failed {
        writeln!(
            text,
            "couldn't undo {}: {why}",
            crate::commands::log_line(entry)
        )
        .unwrap();
    }
    let value = json!({
        "undone": done.undone.iter().map(|(e, by)| json!({"change": e.id, "undone_by": by})).collect::<Vec<_>>(),
        "failed": done.failed.iter().map(|(e, why)| json!({"change": e.id, "why": why})).collect::<Vec<_>>(),
    });
    Ok(Report::new(text, value))
}

/// What `carrel crawl` does.
#[derive(Debug, clap::Subcommand)]
pub enum CrawlAction {
    /// The crawler's pace, how far it has got through the archive, the collections whose
    /// items keep not fitting, and the changes it made this week.
    Status,
    /// Check a few items now, whatever the pace and however busy the device. What it finds
    /// waits in `carrel review`, unless crawler.changes is on and it's sure.
    Run {
        /// How many items; the pace's share by default.
        #[arg(long)]
        items: Option<usize>,
    },
    /// Undo every change the crawler made on its own since then: a span such as 24h or 7d,
    /// `today`, or a date such as 2026-11-01.
    Undo {
        /// Since when.
        since: String,
    },
}

/// `carrel crawl`.
///
/// # Errors
///
/// [`Failure`] as each action says.
pub fn command(archive: Option<&Path>, action: Option<CrawlAction>) -> Result<Report, Failure> {
    match action.unwrap_or(CrawlAction::Status) {
        CrawlAction::Status => status(archive),
        CrawlAction::Run { items } => run(archive, items),
        CrawlAction::Undo { since } => undo(archive, &since),
    }
}
