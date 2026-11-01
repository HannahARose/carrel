//! Crawlers (`docs/design/restructure.md`, "Crawlers"): a background pass that keeps
//! reassessing where items are filed, one item at a time, with the strongest text model the
//! device has.
//!
//! Each pass takes a few items in a fixed order ([`order`]): Holding first, then items filed
//! with low confidence, then the items of crowded collections, then a slow sweep of everything.
//! For each, the model judges whether it still fits its collection's design document and where
//! it would go instead, and reads its card again ([`judge`]). What it finds
//! ([`pass`]):
//!
//! - a misplaced item becomes a move, and a card that could be better a correction, through the
//!   corrections path, leaving the fields the owner wrote alone. Each waits in Review as a
//!   finding ([`findings`]), unless `crawler.changes` is on and the crawler is sure, when it
//!   makes the change itself, marked as the crawler's (`Carrel-Agent: crawler`) so undo
//!   reverses it, one at a time or together;
//! - every verdict is kept ([`verdicts`]), so a collection whose items keep failing becomes a
//!   restructure signal ([`crate::signals::Signal::Misfits`]).
//!
//! Its pace is its own (`crawler.pace`), not set by `restructure.eagerness`. It runs only when
//! the device is idle and on power, a few items at a time, and stops for anything the owner
//! does: how idle the device is and whether it's on power are the caller's to find out
//! ([`Device`]), since the core has no platform-specific code.

pub mod findings;
pub mod judge;
pub mod model;
pub mod order;
pub mod pass;
pub mod verdicts;

use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::card::Extra;

pub use order::Stage;
pub use pass::{Crawled, Crawler};

/// The name the crawler's changes carry in history (`Carrel-Agent`, FORMAT.md §8.2).
pub const AGENT: &str = "crawler";

/// How sure the crawler must be, from 0 to 1, to make a change itself when `crawler.changes`
/// is on.
pub const SURE: f64 = 0.9;

/// What the crawler may do, and how much of the device's idle time it uses
/// (`settings.json`'s `crawler`, FORMAT.md §7.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrawlerSettings {
    /// Whether it makes the refiles and card corrections it's sure of itself, each undoable.
    /// Off, every one waits in Review.
    #[serde(default)]
    pub changes: bool,
    /// How much of the idle time it uses.
    #[serde(default)]
    pub pace: Pace,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for CrawlerSettings {
    fn default() -> Self {
        Self {
            changes: false,
            pace: Pace::default(),
            extra: Extra::new(),
        }
    }
}

impl CrawlerSettings {
    /// Whether these are the defaults, so `settings.json` can leave them out.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// How much of the device's idle time the crawler uses (`crawler.pace`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pace {
    /// It doesn't run.
    Off,
    /// A couple of items an hour, after a long idle spell.
    Gentle,
    /// A few items every twenty minutes of idle time.
    #[default]
    Steady,
    /// More items, more often, whenever the device has been idle a little while.
    Brisk,
}

impl Pace {
    /// Every pace, slowest first.
    pub const ALL: [Self; 4] = [Self::Off, Self::Gentle, Self::Steady, Self::Brisk];

    /// Its name in `settings.json`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Gentle => "gentle",
            Self::Steady => "steady",
            Self::Brisk => "brisk",
        }
    }

    /// The pace named `name`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == name.trim())
    }

    /// How many items a pass judges.
    #[must_use]
    pub fn items(self) -> usize {
        match self {
            Self::Off => 0,
            Self::Gentle => 2,
            Self::Steady => 4,
            Self::Brisk => 8,
        }
    }

    /// How long it rests after a pass.
    #[must_use]
    pub fn rest(self) -> Duration {
        match self {
            Self::Off | Self::Gentle => Duration::from_hours(1),
            Self::Steady => Duration::from_mins(20),
            Self::Brisk => Duration::from_mins(5),
        }
    }

    /// How long the device must have been idle before a pass starts.
    #[must_use]
    pub fn idle(self) -> Duration {
        match self {
            Self::Off | Self::Gentle => Duration::from_mins(15),
            Self::Steady => Duration::from_mins(5),
            Self::Brisk => Duration::from_mins(2),
        }
    }

    /// Whether a pass is due at `now`, the last one having been at `last`.
    #[must_use]
    pub fn due(self, last: Option<SystemTime>, now: SystemTime) -> bool {
        self != Self::Off && last.is_none_or(|last| now >= last + self.rest())
    }

    /// Whether a pass may run on `device` now, or why not.
    ///
    /// # Errors
    ///
    /// Why not, in words: the pace is off, the device is on battery, or it's in use.
    pub fn may_run(self, device: &Device) -> Result<(), String> {
        if self == Self::Off {
            return Err("crawler.pace is off".into());
        }
        if device.on_power == Some(false) {
            return Err("the device is on battery".into());
        }
        match device.idle {
            Some(idle) if idle < self.idle() => Err(format!(
                "the device is in use (a pass waits for {} minutes of idle time)",
                self.idle().as_secs() / 60
            )),
            _ => Ok(()),
        }
    }
}

/// What the caller knows about the device right now. `None` is unknown, and doesn't stop the
/// crawler: a platform that can't tell relies on the caller stopping it for anything the owner
/// does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Device {
    /// How long since the owner last used the keyboard or pointer.
    pub idle: Option<Duration>,
    /// Whether it's on mains power rather than battery.
    pub on_power: Option<bool>,
}

/// Why the crawler couldn't run.
#[derive(Debug)]
pub enum CrawlError {
    /// The archive couldn't be read.
    Archive(crate::archive::ArchiveError),
    /// A finding or a verdict couldn't be read or written.
    Queue(crate::review::ReviewError),
    /// What it keeps couldn't be written.
    Io(std::io::Error),
    /// There's no text model to judge with.
    NoModel(String),
    /// The model failed, so the pass stopped.
    Model(crate::providers::ProviderError),
}

impl std::fmt::Display for CrawlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Archive(e) => write!(f, "{e}"),
            Self::Queue(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::NoModel(why) => f.write_str(why),
            Self::Model(e) => write!(f, "the crawler's model failed: {e}"),
        }
    }
}

impl std::error::Error for CrawlError {}

impl From<crate::archive::ArchiveError> for CrawlError {
    fn from(e: crate::archive::ArchiveError) -> Self {
        Self::Archive(e)
    }
}

impl From<crate::review::ReviewError> for CrawlError {
    fn from(e: crate::review::ReviewError) -> Self {
        Self::Queue(e)
    }
}

impl From<std::io::Error> for CrawlError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// How far the crawler has got through an archive, for `carrel crawl status`, Status and the
/// apps.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrawlStatus {
    /// Its pace.
    pub pace: Pace,
    /// Whether it makes the changes it's sure of itself.
    pub changes: bool,
    /// The items it can judge: every item but restricted, checked-out and removed ones.
    pub items: usize,
    /// Those it has judged, on any device.
    pub judged: usize,
    /// Those due to be judged, by stage, in the order it takes them.
    pub waiting: Vec<(Stage, usize)>,
    /// Collections whose items keep failing: each with how many failed and how many were
    /// judged.
    pub misfits: Vec<(String, usize, usize)>,
    /// Its findings waiting in Review.
    pub findings: usize,
    /// When a pass last ran on this device, as RFC 3339.
    pub last_pass: Option<String>,
}

/// How far the crawler has got through `archive`, as this device (`device`) and every other
/// left it, at `now`.
///
/// # Errors
///
/// [`CrawlError`] if the archive, the verdicts or the findings can't be read.
pub fn status(
    archive: &crate::archive::Archive,
    device: crate::card::Id,
    now: jiff::Timestamp,
) -> Result<CrawlStatus, CrawlError> {
    let settings = archive.settings().crawler.clone();
    let judged = verdicts::all(archive);
    let open = findings::open_items(archive)?;
    let entries = order::entries(archive)?;
    let crowded = order::crowded(archive)?;
    let due = order::order_of(&entries, &crowded, &judged, &open, now);
    let waiting = [
        Stage::Holding,
        Stage::Uncertain,
        Stage::Crowded,
        Stage::Sweep,
    ]
    .into_iter()
    .map(|stage| (stage, due.iter().filter(|(_, s)| *s == stage).count()))
    .filter(|(_, n)| *n > 0)
    .collect();
    let judged_here = entries
        .iter()
        .filter(|e| judged.contains_key(&e.id))
        .count();
    Ok(CrawlStatus {
        pace: settings.pace,
        changes: settings.changes,
        items: entries.len(),
        judged: judged_here,
        waiting,
        misfits: verdicts::misfits(&judged),
        findings: findings::list(archive)?.len(),
        last_pass: verdicts::Verdicts::load(archive, device).last_pass,
    })
}
