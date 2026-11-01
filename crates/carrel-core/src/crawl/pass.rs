//! One crawler pass: a few items in the crawler's order, each judged by the model, its verdict
//! kept, and what could be better proposed in Review or, with `crawler.changes` on and the
//! crawler sure, made as the crawler's own undoable change.

use std::collections::HashMap;

use serde::Serialize;

use super::findings::{self, CrawlEntry, Finding};
use super::judge::{self, Judgment, Place};
use super::order::{self, order_of};
use super::verdicts::{self, Verdict, Verdicts};
use super::{AGENT, CrawlError, SURE};
use crate::archive::{Archive, ItemRef};
use crate::card::{Card, Extra, Id};
use crate::card_file::CardFile;
use crate::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
use crate::providers::device::AvailableModel;
use crate::providers::{Capability, CompletionRequest, Registry};

/// Room for the model's answer, in tokens.
const ANSWER_TOKENS: u32 = 600;

/// The card fields the crawler may correct, with the name each has in a card's
/// `generated.fields` when a model wrote it (FORMAT.md §5.6). A field the owner wrote is
/// left alone.
const FIELDS: [(&str, &str); 4] = [
    ("title", "title"),
    ("summary", "summary"),
    ("created", "dates"),
    ("tags", "tags"),
];

/// The crawler, for one archive on this device.
pub struct Crawler<'a> {
    /// The archive.
    pub archive: &'a Archive,
    /// The providers; the model is one of them.
    pub registry: &'a Registry,
    /// The model that judges: the strongest the device has
    /// ([`crate::librarian::model::choose`]).
    pub model: AvailableModel,
    /// Who its changes are made for: the owner on this device. Its device keeps the verdicts.
    pub holder: Holder,
}

/// What a pass did.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Crawled {
    /// Items judged.
    pub judged: usize,
    /// Of those, the ones that don't fit where they are.
    pub misfits: usize,
    /// Findings left in Review.
    pub proposed: usize,
    /// Changes the crawler made itself, in words.
    pub made: Vec<String>,
    /// What went wrong with single items, in words; the pass carried on.
    pub failed: Vec<String>,
    /// Whether it stopped before the end, because the owner was doing something.
    pub stopped: bool,
}

impl Crawled {
    /// What it did, in a line, or `None` when it did nothing.
    #[must_use]
    pub fn summary(&self) -> Option<String> {
        if self.judged == 0 {
            return None;
        }
        let mut parts = vec![format!(
            "checked {} item{}",
            self.judged,
            if self.judged == 1 { "" } else { "s" }
        )];
        if self.proposed > 0 {
            parts.push(format!("{} to review", self.proposed));
        }
        if !self.made.is_empty() {
            parts.push(format!("{} changed", self.made.len()));
        }
        Some(parts.join(", "))
    }
}

impl Crawler<'_> {
    /// Judges up to `items` items in the crawler's order, asking `stop` before each whether
    /// to stop, as when the owner starts using the device.
    ///
    /// # Errors
    ///
    /// [`CrawlError::Model`] when the model fails, which stops the pass; otherwise, if the
    /// archive can't be read or what the crawler keeps can't be written.
    pub fn pass(&self, items: usize, stop: &dyn Fn() -> bool) -> Result<Crawled, CrawlError> {
        let archive = self.archive;
        let device = self.holder.device;
        let now = jiff::Timestamp::now();
        let judged = verdicts::all(archive);
        let open = findings::open_items(archive)?;
        let entries = order::entries(archive)?;
        let crowded = order::crowded(archive)?;
        let due = order_of(&entries, &crowded, &judged, &open, now);
        let by_id: HashMap<Id, ItemRef> = archive.items()?.into_iter().map(|i| (i.id, i)).collect();
        let places = places(archive)?;
        let mut crawled = Crawled::default();
        for (id, _) in due.into_iter().take(items) {
            if stop() {
                crawled.stopped = true;
                break;
            }
            let Some(item) = by_id.get(&id) else {
                continue;
            };
            let Some(card) = read_card(archive, item) else {
                crawled
                    .failed
                    .push(format!("{}: its card can't be read", item.card));
                continue;
            };
            let collection = folder(&item.card);
            let Some(judgment) = self.judge(&card, collection, &places)? else {
                crawled
                    .failed
                    .push(format!("{}: the model's answer wasn't usable", card.title));
                continue;
            };
            crawled.judged += 1;
            if !judgment.fits {
                crawled.misfits += 1;
            }
            // Read now, not when the pass began, so a finding the owner rejected meanwhile
            // stays declined.
            let declined = verdicts::all(archive)
                .get(&id)
                .map(|v| v.declined.clone())
                .unwrap_or_default();
            for finding in findings_of(archive, &judgment, &card, collection, &places) {
                if !declined.iter().any(|d| declines(d, &finding)) {
                    self.act(item, &card, finding, &judgment, &mut crawled)?;
                }
            }
            record(
                archive,
                device,
                id,
                Verdict {
                    collection: collection.to_owned(),
                    fits: judgment.fits,
                    better: judgment.better.clone(),
                    confidence: judgment.confidence,
                    reason: judgment.reason.clone(),
                    model: self.model_name(),
                    judged: jiff::Timestamp::now().to_string(),
                    declined,
                    extra: Extra::new(),
                },
            )?;
        }
        let mut mine = Verdicts::load(archive, device);
        mine.last_pass = Some(now.to_string());
        mine.save(archive, device)?;
        Ok(crawled)
    }

    fn model_name(&self) -> String {
        format!("{}/{}", self.model.provider, self.model.offer)
    }

    /// What the model concludes about `item` where it is now, without recording anything:
    /// whether it fits, where it would go instead, and how its card could be better. `None`
    /// when its card can't be read or the model's answer wasn't usable. The crawler benchmark
    /// uses it to measure each model.
    ///
    /// # Errors
    ///
    /// [`CrawlError::Model`] when the model fails, or the archive can't be read.
    pub fn judge_item(&self, item: &ItemRef) -> Result<Option<Judgment>, CrawlError> {
        let Some(card) = read_card(self.archive, item) else {
            return Ok(None);
        };
        let places = places(self.archive)?;
        self.judge(&card, folder(&item.card), &places)
    }

    /// The model's judgment of `item`, or `None` when its answer wasn't usable.
    fn judge(
        &self,
        card: &Card,
        collection: &str,
        places: &[Place],
    ) -> Result<Option<Judgment>, CrawlError> {
        let design = design_document(self.archive, collection);
        let text = card
            .checksum
            .as_ref()
            .and_then(|c| crate::search::texts::TextCache::of(self.archive).get(c));
        let shown = judge::Item {
            card,
            collection,
            design: &design,
            text: text.as_deref(),
        };
        let paths: Vec<String> = places.iter().map(|p| p.path.clone()).collect();
        let request = CompletionRequest {
            system: Some(judge::system()),
            prompt: judge::prompt(&shown, places),
            schema: Some(judge::schema(&paths)),
            max_tokens: Some(ANSWER_TOKENS),
        };
        let ranked = self.registry.ranked(Capability::TextModel);
        let (provider, offer) = ranked
            .iter()
            .find(|(p, o)| p.name() == self.model.provider && o.id == self.model.offer)
            .ok_or_else(|| {
                CrawlError::NoModel(format!("{} isn't available any more", self.model.label))
            })?;
        let completion = provider
            .complete(&offer.id, &request)
            .map_err(CrawlError::Model)?;
        Ok(completion
            .json
            .and_then(|answer| judge::read(&answer, card, collection)))
    }

    /// Makes `finding` as the crawler's own change when `crawler.changes` is on and it's sure,
    /// else leaves it in Review. A change it can't make is left in Review too.
    fn act(
        &self,
        item: &ItemRef,
        card: &Card,
        finding: Finding,
        judgment: &Judgment,
        crawled: &mut Crawled,
    ) -> Result<(), CrawlError> {
        let entry = CrawlEntry::new(
            item,
            &card.title,
            finding,
            &judgment.reason,
            judgment.confidence,
            &self.model_name(),
        );
        let settings = &self.archive.settings().crawler;
        if settings.changes && judgment.confidence >= SURE {
            match self.make(&entry) {
                Ok(()) => {
                    crawled.made.push(entry.finding.describe(&card.title));
                    return Ok(());
                }
                Err(why) => crawled
                    .failed
                    .push(format!("{}: {why}", entry.finding.describe(&card.title))),
            }
        }
        if findings::propose(self.archive, &entry)? {
            crawled.proposed += 1;
        }
        Ok(())
    }

    /// Makes `entry`'s change as the crawler.
    fn make(&self, entry: &CrawlEntry) -> Result<(), String> {
        let mut lease = DiskLock::for_archive(self.archive)
            .acquire(self.holder, AGENT, DEFAULT_LEASE)
            .map_err(|e| e.to_string())?;
        lease.act_as(Some(AGENT));
        let made = findings::make(self.archive, self.registry, entry, &lease);
        let released = lease.release().map_err(|e| e.to_string());
        made.and(released)
    }
}

/// Keeps `verdict` on `id` in this device's verdicts, after each item, so a pass that's stopped
/// loses nothing. The file is read again first, so what changed in it meanwhile, such as a
/// finding the owner rejected, is kept.
fn record(archive: &Archive, device: Id, id: Id, mut verdict: Verdict) -> Result<(), CrawlError> {
    let mut mine = Verdicts::load(archive, device);
    if let Some(was) = mine.items.get(&id) {
        for declined in &was.declined {
            if !verdict.declined.contains(declined) {
                verdict.declined.push(declined.clone());
            }
        }
    }
    mine.items.insert(id, verdict);
    mine.save(archive, device)?;
    Ok(())
}

/// Whether the owner's rejection of `declined` covers `finding`: the same move or tags, or
/// any new title, summary or date, however the model words it next time.
fn declines(declined: &Finding, finding: &Finding) -> bool {
    match (declined, finding) {
        (Finding::Correct { field: a, .. }, Finding::Correct { field: b, .. })
            if a == b && matches!(a.as_str(), "title" | "summary" | "created") =>
        {
            true
        }
        _ => declined == finding,
    }
}

/// What could be better about the item with `card` in `collection`, as `judgment` says: a move
/// to a collection that exists, then corrections of the fields a model wrote or the card lacks,
/// with tags the archive defines. The corrections come first, since a move changes where the card is.
fn findings_of(
    archive: &Archive,
    judgment: &Judgment,
    card: &Card,
    collection: &str,
    places: &[Place],
) -> Vec<Finding> {
    let model_wrote = |field: &str| {
        let written = FIELDS
            .iter()
            .find(|(name, _)| *name == field)
            .map_or(field, |(_, w)| *w);
        card.generated
            .as_ref()
            .is_some_and(|g| g.fields.iter().any(|f| f == written))
    };
    // A field the card doesn't have yet is no one's, so it can be filled in.
    let missing = |field: &str| match field {
        "summary" => card.summary.as_deref().is_none_or(|s| s.trim().is_empty()),
        "created" => card.dates.as_ref().is_none_or(|d| d.created.is_none()),
        "tags" => card.tags.is_empty(),
        _ => false,
    };
    let mut found = Vec::new();
    let mut correct = |field: &str, value: Option<String>| {
        if let Some(value) = value
            && (model_wrote(field) || missing(field))
        {
            found.push(Finding::Correct {
                field: field.to_owned(),
                value,
            });
        }
    };
    correct("title", judgment.title.clone());
    correct("summary", judgment.summary.clone());
    correct("created", judgment.created.clone());
    let collections = archive.collections().unwrap_or_default();
    let defined = crate::ingest::card::Vocabulary::of(archive, &collections);
    let tags: Vec<String> = judgment
        .tags
        .iter()
        .filter(|t| defined.tags.iter().any(|(d, _)| d.0 == **t))
        .map(|t| format!("+{t}"))
        .collect();
    correct("tags", (!tags.is_empty()).then(|| tags.join(" ")));
    if let Some(better) = &judgment.better
        && better != collection
        && places.iter().any(|p| p.path == *better)
    {
        found.push(Finding::Move {
            from: collection.to_owned(),
            to: better.clone(),
        });
    }
    found
}

/// The collections an item could go to, Holding left out.
fn places(archive: &Archive) -> Result<Vec<Place>, CrawlError> {
    Ok(archive
        .collections()?
        .into_iter()
        .filter(|c| c.path != "Holding")
        .map(|c| Place {
            purpose: c
                .doc
                .description
                .clone()
                .unwrap_or_else(|| c.doc.title.clone()),
            path: c.path,
        })
        .collect())
}

/// The design document of the collection at `path`: its description and body, without the
/// frontmatter. Empty when there's none.
fn design_document(archive: &Archive, path: &str) -> String {
    let file = archive.root().join(path).join("_collection.md");
    let Ok(text) = std::fs::read_to_string(file) else {
        return String::new();
    };
    match crate::frontmatter::split(&text) {
        Ok(split) => split.body.trim().to_owned(),
        Err(_) => text,
    }
}

fn read_card(archive: &Archive, item: &ItemRef) -> Option<Card> {
    let text = std::fs::read_to_string(archive.root().join(&item.card)).ok()?;
    CardFile::parse(&text).ok().map(|f| f.card().clone())
}

/// The folder of the card at `path`: its collection.
fn folder(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(f, _)| f)
}
