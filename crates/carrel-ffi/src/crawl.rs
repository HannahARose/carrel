//! The crawler (`docs/design/restructure.md`, "Crawlers"): its findings waiting in Review,
//! approved or rejected, how far it has got, the changes it made on its own this week, undone
//! together, and a pass, which the app's agent runs when the Mac is idle, as `carrel crawl`
//! and the background worker do.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::crawl::findings::{self, CrawlEntry, Finding};
use carrel_core::crawl::pass::Crawler;
use carrel_core::crawl::verdicts::Verdicts;
use carrel_core::crawl::{self, Device};
use carrel_core::history::History;
use carrel_core::librarian::changes;

use crate::correct::ModelProvider;
use crate::history::Change;
use crate::librarian::LibrarianUndone;
use crate::{ArchiveHandle, CarrelError};

/// A finding of the crawler's, waiting in Review.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CrawlerFinding {
    /// Its identifier.
    pub id: String,
    /// The item it's about.
    pub item: String,
    /// The item's card when it was found.
    pub card: String,
    /// The item's title when it was found.
    pub title: String,
    /// For a move, the collection it's in.
    pub from: Option<String>,
    /// For a move, the collection it fits better.
    pub to: Option<String>,
    /// For a correction, the field: `title`, `summary`, `created` or `tags`.
    pub field: Option<String>,
    /// For a correction, the new value; for tags, `+tag` for each to add.
    pub value: Option<String>,
    /// What it would change, in words.
    pub description: String,
    /// Why, in the model's words.
    pub reason: String,
    /// How sure the model was, from 0 to 1.
    pub confidence: f64,
    /// The model, as `provider/offer`.
    pub model: String,
    /// When it was found, as RFC 3339.
    pub created: String,
}

impl From<&CrawlEntry> for CrawlerFinding {
    fn from(entry: &CrawlEntry) -> Self {
        let (from, to, field, value) = match &entry.finding {
            Finding::Move { from, to } => (Some(from.clone()), Some(to.clone()), None, None),
            Finding::Correct { field, value } => {
                (None, None, Some(field.clone()), Some(value.clone()))
            }
        };
        Self {
            id: entry.id.to_string(),
            item: entry.item.to_string(),
            card: entry.card.clone(),
            title: entry.title.clone(),
            from,
            to,
            field,
            value,
            description: entry.finding.describe(&entry.title),
            reason: entry.reason.clone(),
            confidence: entry.confidence,
            model: entry.model.clone(),
            created: entry.created.clone(),
        }
    }
}

/// How many items wait for the crawler at one stage of its order.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CrawlerStage {
    /// The stage, for people, such as "in Holding".
    pub stage: String,
    /// How many items are due.
    pub items: u64,
}

/// A collection whose items keep failing the crawler's checks.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CrawlerMisfits {
    /// The collection.
    pub collection: String,
    /// How many of its items don't fit.
    pub misfits: u64,
    /// Of how many judged.
    pub judged: u64,
}

/// How far the crawler has got through the archive.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CrawlerStatus {
    /// Its pace: `off`, `gentle`, `steady` or `brisk` (`crawler.pace`).
    pub pace: String,
    /// Whether it makes the changes it's sure of itself (`crawler.changes`).
    pub changes: bool,
    /// The items it can judge.
    pub items: u64,
    /// Those it has judged, on any device.
    pub judged: u64,
    /// Those due, by stage, in the order it takes them.
    pub waiting: Vec<CrawlerStage>,
    /// Collections whose items keep failing.
    pub misfits: Vec<CrawlerMisfits>,
    /// Its findings waiting in Review.
    pub findings: u64,
    /// When a pass last ran on this Mac, as RFC 3339.
    pub last_pass: Option<String>,
}

/// What a pass did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CrawlerPassed {
    /// What it did, in a line, or `None` when it judged nothing.
    pub summary: Option<String>,
    /// Findings left in Review.
    pub proposed: u64,
    /// Changes it made itself, in words.
    pub made: Vec<String>,
    /// What went wrong with single items, in words.
    pub failed: Vec<String>,
    /// Whether it stopped before the end, because the owner was doing something.
    pub stopped: bool,
}

/// Asked before each item of a pass whether to stop, as when the owner starts using the Mac,
/// implemented in Swift.
#[uniffi::export(with_foreign)]
pub trait CrawlStop: Send + Sync {
    /// Whether to stop now.
    fn should_stop(&self) -> bool;
}

fn count(n: usize) -> u64 {
    n as u64
}

impl ArchiveHandle {
    /// A pass of `items` items with the strongest text model in `registry` that fits in
    /// `memory_gb`, asking `stop` before each.
    fn pass(
        &self,
        archive: &carrel_core::archive::Archive,
        registry: &carrel_core::providers::Registry,
        memory_gb: Option<u64>,
        items: usize,
        stop: &dyn Fn() -> bool,
    ) -> Result<CrawlerPassed, CarrelError> {
        let chosen = carrel_core::librarian::model::choose(registry, None, memory_gb)
            .map_err(CarrelError::invalid)?
            .ok_or_else(|| {
                CarrelError::invalid("the crawler needs a text model, and this Mac has none")
            })?;
        let crawler = Crawler {
            archive,
            registry,
            model: chosen,
            holder: self.holder()?,
        };
        let done = crawler.pass(items, stop).map_err(CarrelError::failed)?;
        Ok(CrawlerPassed {
            summary: done.summary(),
            proposed: count(done.proposed),
            made: done.made,
            failed: done.failed,
            stopped: done.stopped,
        })
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// The crawler's findings waiting in Review, oldest first.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the queue can't be read.
    pub fn crawler_findings(&self) -> Result<Vec<CrawlerFinding>, CarrelError> {
        let archive = self.fresh()?;
        Ok(findings::list(&archive)?.iter().map(Into::into).collect())
    }

    /// Approves the finding `id`: makes its change as the owner's, through the corrections
    /// path, with `model` or the device's for a correction. Returns the history commit.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Busy`] if another writer holds the archive, or [`CarrelError::Failed`]
    /// if it isn't found or the change can't be made, as when the item has gone; the finding
    /// is kept then.
    pub fn approve_crawler_finding(
        &self,
        id: String,
        model: Option<Arc<dyn ModelProvider>>,
    ) -> Result<Option<String>, CarrelError> {
        let archive = self.fresh()?;
        let entry = findings::find(&archive, &id)?;
        let registry = self.correction_registry(model);
        let lease =
            DiskLock::for_archive(&archive).acquire(self.holder()?, "review", DEFAULT_LEASE)?;
        let done = findings::approve(&archive, &registry, &entry, &lease);
        lease.release()?;
        done.map_err(CarrelError::failed)
    }

    /// Rejects the finding `id`: it leaves Review, and the crawler doesn't suggest it again.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if it isn't found or can't be removed.
    pub fn reject_crawler_finding(&self, id: String) -> Result<(), CarrelError> {
        let archive = self.fresh()?;
        let entry = findings::find(&archive, &id)?;
        findings::reject(&archive, &entry, self.identity()?.device)?;
        Ok(())
    }

    /// How far the crawler has got through the archive, as `carrel crawl status` says.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive, the verdicts or the findings can't be read.
    pub fn crawler_status(&self) -> Result<CrawlerStatus, CarrelError> {
        let archive = self.fresh()?;
        let status = crawl::status(&archive, self.identity()?.device, jiff::Timestamp::now())
            .map_err(CarrelError::failed)?;
        Ok(CrawlerStatus {
            pace: status.pace.as_str().to_owned(),
            changes: status.changes,
            items: count(status.items),
            judged: count(status.judged),
            waiting: status
                .waiting
                .into_iter()
                .map(|(stage, n)| CrawlerStage {
                    stage: stage.describe().to_owned(),
                    items: count(n),
                })
                .collect(),
            misfits: status
                .misfits
                .into_iter()
                .map(|(collection, misfits, judged)| CrawlerMisfits {
                    collection,
                    misfits: count(misfits),
                    judged: count(judged),
                })
                .collect(),
            findings: count(status.findings),
            last_pass: status.last_pass,
        })
    }

    /// The changes the crawler made on its own at or after `since` (seconds since the Unix
    /// epoch) that haven't been undone, newest first.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the history can't be read.
    pub fn crawler_changes(&self, since: i64) -> Result<Vec<Change>, CarrelError> {
        let archive = self.fresh()?;
        let log = History::open(&archive)?.log()?;
        Ok(changes::recent_by(&log, crawl::AGENT, since)
            .iter()
            .map(Into::into)
            .collect())
    }

    /// Undoes every change the crawler made on its own at or after `since`, newest first, as
    /// `carrel crawl undo` does. One that can't be undone is reported, and the rest are still
    /// undone.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Busy`] if another writer holds the archive, or [`CarrelError::Failed`].
    pub fn undo_crawler_changes(&self, since: i64) -> Result<LibrarianUndone, CarrelError> {
        let archive = self.fresh()?;
        let lease =
            DiskLock::for_archive(&archive).acquire(self.holder()?, "undo", DEFAULT_LEASE)?;
        let done = changes::undo_since_by(&archive, crawl::AGENT, since, &lease);
        lease.release()?;
        let done = done?;
        Ok(LibrarianUndone {
            undone: done.undone.iter().map(|(e, _)| e.summary.clone()).collect(),
            failed: done
                .failed
                .iter()
                .map(|(e, why)| format!("{}: {why}", e.summary))
                .collect(),
        })
    }

    /// A pass of the crawler now, of `items` items (the pace's share when `None`), whatever
    /// the pace and however idle the Mac, since the owner asked, as `carrel crawl run` does.
    /// It judges with `model` or the strongest model this Mac has that fits in `memory_gb`.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if there's no text model, or [`CarrelError::Failed`] if the
    /// pass failed.
    pub fn crawl_now(
        &self,
        items: Option<u32>,
        model: Option<Arc<dyn ModelProvider>>,
        memory_gb: Option<u64>,
    ) -> Result<CrawlerPassed, CarrelError> {
        let archive = self.fresh()?;
        let pace = archive.settings().crawler.pace;
        let items = items.map_or_else(|| pace.items().max(1), |n| n as usize);
        let registry = self.correction_registry(model);
        self.pass(&archive, &registry, memory_gb, items, &|| false)
    }

    /// A pass of the crawler, when its pace says one is due and this Mac (idle for
    /// `idle_seconds`, on mains power when `on_power`; `None` when unknown) allows: what it
    /// did, or `None` when it didn't run. It judges with `model` or, when it's `None`, the
    /// strongest model this Mac has that fits in `memory_gb`, and asks `stop` before each
    /// item. What the app's agent runs.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if there's no text model, or [`CarrelError::Failed`] if the
    /// pass failed.
    pub fn crawl_when_idle(
        &self,
        idle_seconds: Option<u64>,
        on_power: Option<bool>,
        model: Option<Arc<dyn ModelProvider>>,
        memory_gb: Option<u64>,
        stop: Option<Arc<dyn CrawlStop>>,
    ) -> Result<Option<CrawlerPassed>, CarrelError> {
        let archive = self.fresh()?;
        let pace = archive.settings().crawler.pace;
        let identity = self.identity()?;
        let last = Verdicts::load(&archive, identity.device)
            .last_pass
            .and_then(|t| t.parse::<jiff::Timestamp>().ok())
            .map(SystemTime::from);
        let device = Device {
            idle: idle_seconds.map(Duration::from_secs),
            on_power,
        };
        if !pace.due(last, SystemTime::now()) || pace.may_run(&device).is_err() {
            return Ok(None);
        }
        let registry = self.correction_registry(model);
        let device_id = identity.device;
        let should_stop = || {
            stop.as_ref().is_some_and(|s| s.should_stop())
                || carrel_core::intake::waiting(&archive, device_id, false)
                    .is_ok_and(|w| !w.is_empty())
        };
        self.pass(&archive, &registry, memory_gb, pace.items(), &should_stop)
            .map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::correct::{ModelAnswer, ModelRequest};
    use crate::tests::Sample;

    /// A model that finds every item belongs in Reading, half sure unless `sure`, as a Swift
    /// model would.
    #[derive(Default)]
    struct ToReading {
        sure: bool,
    }

    impl ModelProvider for ToReading {
        fn complete(&self, _: ModelRequest) -> Result<ModelAnswer, CarrelError> {
            let confidence = if self.sure { 0.95 } else { 0.5 };
            Ok(ModelAnswer {
                text: format!(
                    r#"{{"fits": false, "collection": "Reading", "confidence": {confidence},
                    "reason": "It reads like a reference.", "title": "", "summary": "",
                    "created": "", "tags": []}}"#
                ),
                model: "swift/to-reading".into(),
            })
        }
    }

    fn pass(sample: &Sample, idle: u64) -> Option<CrawlerPassed> {
        sample
            .handle
            .crawl_when_idle(
                Some(idle),
                Some(true),
                Some(Arc::new(ToReading::default())),
                Some(16),
                None,
            )
            .unwrap()
    }

    #[test]
    fn a_pass_runs_only_when_the_mac_is_idle_and_leaves_findings_in_review() {
        let sample = Sample::new();
        assert_eq!(pass(&sample, 0), None, "in use");
        let passed = pass(&sample, 3600).expect("idle for an hour");
        assert!(passed.summary.is_some_and(|s| s.starts_with("checked")));
        assert!(passed.proposed > 0);
        assert_eq!(pass(&sample, 3600), None, "the next pass isn't due yet");

        let found = sample.handle.crawler_findings().unwrap();
        assert_eq!(found.len() as u64, passed.proposed);
        let first = &found[0];
        assert_eq!(first.to.as_deref(), Some("Reading"));
        assert!(first.description.starts_with("move “"));
        assert_eq!(first.reason, "It reads like a reference.");

        let status = sample.handle.crawler_status().unwrap();
        assert_eq!(status.pace, "steady");
        assert!(!status.changes);
        assert!(status.judged > 0 && status.judged <= status.items);
        assert_eq!(status.findings, passed.proposed);
        assert!(status.last_pass.is_some());
    }

    #[test]
    fn a_finding_is_approved_as_the_owners_change_or_rejected_for_good() {
        let sample = Sample::new();
        pass(&sample, 3600).unwrap();
        let found = sample.handle.crawler_findings().unwrap();
        let (first, rest) = found.split_first().unwrap();
        let commit = sample
            .handle
            .approve_crawler_finding(first.id.clone(), Some(Arc::new(ToReading::default())))
            .unwrap();
        assert!(commit.is_some());
        let card = sample
            .handle
            .items()
            .unwrap()
            .into_iter()
            .find(|i| i.id == first.item)
            .unwrap()
            .card;
        assert!(card.starts_with("Reading/"), "{card}");
        if let Some(second) = rest.first() {
            sample
                .handle
                .reject_crawler_finding(second.id.clone())
                .unwrap();
            assert!(
                sample
                    .handle
                    .crawler_findings()
                    .unwrap()
                    .iter()
                    .all(|f| f.id != second.id)
            );
        }
        // The owner's approval isn't one of the crawler's own changes.
        assert_eq!(sample.handle.crawler_changes(0).unwrap(), Vec::new());
        let undone = sample.handle.undo_crawler_changes(0).unwrap();
        assert!(undone.undone.is_empty() && undone.failed.is_empty());
    }

    #[test]
    fn with_changes_on_its_own_changes_are_listed_and_undone_together() {
        let sample = Sample::new();
        sample
            .handle
            .set_setting("crawler.changes".into(), "on".into())
            .unwrap();
        let passed = sample
            .handle
            .crawl_when_idle(
                Some(3600),
                Some(true),
                Some(Arc::new(ToReading { sure: true })),
                Some(16),
                None,
            )
            .unwrap()
            .unwrap();
        assert!(!passed.made.is_empty(), "{passed:?}");
        let made = sample.handle.crawler_changes(0).unwrap();
        assert_eq!(made.len(), passed.made.len());
        assert_eq!(made[0].agent.as_deref(), Some("crawler"));
        let undone = sample.handle.undo_crawler_changes(0).unwrap();
        assert_eq!(undone.undone.len(), made.len());
        assert_eq!(sample.handle.crawler_changes(0).unwrap(), Vec::new());
    }

    #[test]
    fn a_pass_now_runs_whatever_the_pace_and_idle_time() {
        let sample = Sample::new();
        sample
            .handle
            .set_setting("crawler.pace".into(), "off".into())
            .unwrap();
        assert_eq!(pass(&sample, 3600), None, "off doesn't run by itself");
        let passed = sample
            .handle
            .crawl_now(Some(2), Some(Arc::new(ToReading::default())), Some(16))
            .unwrap();
        assert!(
            passed
                .summary
                .is_some_and(|s| s.starts_with("checked 2 items"))
        );
    }
}
