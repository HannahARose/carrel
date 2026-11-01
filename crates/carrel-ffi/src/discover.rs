//! Discovery (`docs/design/librarian.md`, Discovery), as `carrel discover` shows it: on this
//! day, a walk through the stacks, the timeline, the archive map and activity, computed by the
//! core ([`carrel_core::discover`]) for the app to draw its own way. Also the device's record
//! of which collections were opened when, which wander's jumps and Status's resurfacing read.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use carrel_core::archive::Archive;
use carrel_core::discover::activity;
use carrel_core::discover::map::Region;
use carrel_core::discover::timeline::{self, Options, Resolution};
use carrel_core::discover::today as on_this_day;
use carrel_core::discover::visits::Visits;
use carrel_core::discover::wander::{self, Reason, Surroundings, Walk};
use carrel_core::discover::{Catalog, DateKind, Item, map};
use carrel_core::search::query::Query;
use carrel_core::search::texts::TextCache;
use jiff::civil::Date;

use crate::records::Sensitivity;
use crate::{ArchiveHandle, CarrelError, ItemKind};

/// An item, as the views list it: enough to show it and open it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DiscoveredItem {
    /// Its identifier.
    pub id: String,
    /// Its card's title.
    pub title: String,
    /// Its card, relative to the archive root.
    pub card: String,
    /// Its contents, relative to the archive root; card-only items have none.
    pub item: Option<String>,
    /// The collection it's in.
    pub collection: String,
    /// Its kind.
    pub kind: ItemKind,
    /// How sensitive it is.
    pub sensitivity: Sensitivity,
}

impl From<&Item> for DiscoveredItem {
    fn from(item: &Item) -> Self {
        Self {
            id: item.id.to_string(),
            title: item.title.clone(),
            card: item.card.clone(),
            item: item.item.clone(),
            collection: item.collection.clone(),
            kind: item.kind.into(),
            sensitivity: item.sensitivity.into(),
        }
    }
}

/// Something that happened to an item on this day in another year.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Happened {
    /// The item.
    pub item: DiscoveredItem,
    /// What happened: `created`, `filed` or `worked`.
    pub what: Vec<String>,
    /// Its card's summary.
    pub summary: Option<String>,
}

/// One earlier year's items on this day.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DayInYear {
    /// The year.
    pub year: i16,
    /// How many years ago.
    pub years_ago: i16,
    /// What happened then.
    pub items: Vec<Happened>,
}

/// On this day: what was made, filed or worked on on this date in other years.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OnThisDay {
    /// The day, as `YYYY-MM-DD`.
    pub date: String,
    /// The earlier years with anything, latest first.
    pub years: Vec<DayInYear>,
}

/// Why a walk's step came next.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum WanderReason {
    /// The walk's first step, into a collection not opened in a while.
    Start {
        /// The collection.
        collection: String,
    },
    /// The item before links to it, or it links to the item before.
    Linked {
        /// The link's type, such as `related` or `version-of`.
        link_type: String,
        /// The item before's title.
        from: String,
        /// Whether it's this item that links to the one before.
        back: bool,
    },
    /// It's near the item before by meaning.
    Near {
        /// The item before's title.
        from: String,
        /// How near: 1 is the same direction.
        score: f32,
    },
    /// It's in the same collection as the item before.
    Neighbour {
        /// The collection.
        collection: String,
    },
    /// A jump to a collection not opened in a while.
    Jump {
        /// The collection.
        collection: String,
        /// When it was last seen, as `YYYY-MM-DD`, if ever.
        last_seen: Option<String>,
    },
}

/// One step of a walk through the stacks.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct WanderStep {
    /// Which step it is, from 1.
    pub number: u32,
    /// The item.
    pub item: DiscoveredItem,
    /// Why it came next.
    pub reason: WanderReason,
    /// Its card's summary.
    pub summary: Option<String>,
    /// Its tags.
    pub tags: Vec<String>,
    /// The start of its text; never for a restricted item.
    pub preview: Option<String>,
}

/// A walk through the stacks, one step at a time. The walk the app takes is also kept as the
/// device's last, so `carrel discover wander --next` goes on with it.
#[derive(Debug, uniffi::Object)]
pub struct Wander {
    walk: Mutex<Walk>,
}

#[uniffi::export]
impl Wander {
    /// The walk's seed, which makes it reproducible.
    #[must_use]
    pub fn seed(&self) -> u64 {
        self.walk.lock().map(|w| w.seed).unwrap_or_default()
    }

    /// How many items it has visited.
    #[must_use]
    pub fn visited(&self) -> u32 {
        self.walk
            .lock()
            .map(|w| u32::try_from(w.visited.len()).unwrap_or(u32::MAX))
            .unwrap_or_default()
    }
}

/// One item on the timeline.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TimelineItem {
    /// The item.
    pub item: DiscoveredItem,
    /// The date it's placed by, as `YYYY-MM-DD`.
    pub date: String,
    /// Which of its dates that is: `created`, `filed` or `worked`.
    pub what: String,
}

/// One period of the timeline.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TimelinePeriod {
    /// The period: `2025`, `2025-03` or `2025-03-14`.
    pub period: String,
    /// Its items, oldest first.
    pub items: Vec<TimelineItem>,
}

/// Items by date.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DiscoveryTimeline {
    /// The periods that have items, oldest first.
    pub periods: Vec<TimelinePeriod>,
    /// How many items have no date of the kind it goes by, and so aren't on it.
    pub undated: u32,
}

/// A collection on the archive map. The map is a flat list, each region after its parent,
/// so the app can lay out a treemap at any depth.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MapRegion {
    /// Its folder.
    pub path: String,
    /// Its title.
    pub title: String,
    /// The collection it's inside, if any.
    pub parent: Option<String>,
    /// How deep it is: 0 for a collection at the top.
    pub depth: u32,
    /// The items directly in it.
    pub items: u64,
    /// Their size in bytes, where cards record one.
    pub size: u64,
    /// The items in it and the collections inside it.
    pub total_items: u64,
    /// Their size in bytes.
    pub total_size: u64,
}

/// Collections by size and number of items.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ArchiveMap {
    /// The collections, largest first among those with one parent.
    pub regions: Vec<MapRegion>,
    /// Every item.
    pub total_items: u64,
    /// Their size in bytes.
    pub total_size: u64,
    /// How many items have no recorded size.
    pub without_size: u64,
}

/// One day's activity.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ActivityDay {
    /// The day, as `YYYY-MM-DD`.
    pub date: String,
    /// Items filed.
    pub filed: u32,
    /// Work sessions.
    pub sessions: u32,
    /// How busy, from 0 for nothing to 4 for as busy as the busiest day.
    pub level: u8,
}

/// When things were filed and worked on, by day, for a heatmap.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ArchiveActivity {
    /// The first day, as `YYYY-MM-DD`.
    pub from: String,
    /// The last day.
    pub to: String,
    /// The days with anything, oldest first; the rest had nothing.
    pub days: Vec<ActivityDay>,
    /// The most on any one day.
    pub busiest: u32,
}

/// A collection not opened in a long time, as Status mentions it now and then.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Resurfaced {
    /// Its folder.
    pub collection: String,
    /// Its title.
    pub title: String,
    /// When it was last seen, as `YYYY-MM-DD`.
    pub last_seen: String,
    /// How many days ago that was.
    pub days: i32,
    /// An item or two from it.
    pub items: Vec<DiscoveredItem>,
}

/// How many shades an activity heatmap has above nothing.
const LEVELS: usize = 4;

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `text` as a day, or today when it's `None`.
fn day(text: Option<&str>) -> Result<Date, CarrelError> {
    match text.map(str::trim).filter(|t| !t.is_empty()) {
        Some(text) => text
            .parse()
            .map_err(|_| CarrelError::invalid(format!("{text} isn't a date such as 2026-11-01"))),
        None => Ok(jiff::Zoned::now().date()),
    }
}

/// The regions under and including `region`, flattened, each after its parent.
fn flatten(region: &Region, parent: Option<&str>, depth: u32, into: &mut Vec<MapRegion>) {
    into.push(MapRegion {
        path: region.path.clone(),
        title: region.title.clone(),
        parent: parent.map(str::to_owned),
        depth,
        items: region.items as u64,
        size: region.size,
        total_items: region.total_items as u64,
        total_size: region.total_size,
    });
    for child in &region.children {
        flatten(child, Some(&region.path), depth + 1, into);
    }
}

impl ArchiveHandle {
    /// Where this device keeps its record of discovery for the archive.
    fn visits_path(&self, archive: &Archive) -> Result<PathBuf, CarrelError> {
        Ok(Visits::path_in(
            &self.config_dir()?,
            &archive.info().id.to_string(),
        ))
    }

    /// This device's record of discovery, changed by `change` and saved when it says it
    /// changed something. Failing to save it is no failure: it's a convenience.
    fn with_visits<T>(
        &self,
        archive: &Archive,
        change: impl FnOnce(&mut Visits) -> (T, bool),
    ) -> Result<T, CarrelError> {
        let path = self.visits_path(archive)?;
        let mut visits = Visits::load(&path);
        let (result, changed) = change(&mut visits);
        if changed {
            let _ = visits.save(&path);
        }
        Ok(result)
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// What was made, filed or worked on on `date` (`YYYY-MM-DD`, today when `None`) in
    /// other years.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for a date that can't be read, or [`CarrelError::Failed`] if
    /// the archive can't be.
    pub fn on_this_day(&self, date: Option<String>) -> Result<OnThisDay, CarrelError> {
        let date = day(date.as_deref())?;
        let found = on_this_day::on_this_day(&Catalog::read(&self.fresh()?)?, date);
        Ok(OnThisDay {
            date: found.date,
            years: found
                .years
                .iter()
                .map(|y| DayInYear {
                    year: y.year,
                    years_ago: y.years_ago,
                    items: y
                        .items
                        .iter()
                        .map(|h| Happened {
                            item: (&h.item).into(),
                            what: h.what.iter().map(|w| w.name().to_owned()).collect(),
                            summary: h.summary.clone(),
                        })
                        .collect(),
                })
                .collect(),
        })
    }

    /// A new walk through the stacks, from the item `start` or from a collection not opened
    /// in a while, reproducible with `seed` (a new one when `None`).
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if `start` names no item.
    pub fn start_wander(
        &self,
        start: Option<String>,
        seed: Option<u64>,
    ) -> Result<Arc<Wander>, CarrelError> {
        let archive = self.fresh()?;
        let start = match start {
            Some(id) => Some(Self::item(&archive, &id)?.id),
            None => None,
        };
        let seed = seed.unwrap_or_else(|| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            (now.as_secs() ^ u64::from(now.subsec_nanos())) % 1_000_000
        });
        let visits = Visits::load(&self.visits_path(&archive)?);
        Ok(Arc::new(Wander {
            walk: Mutex::new(Walk::new(seed, start).remembering(&visits)),
        }))
    }

    /// The next step of `walk`: a related item, a link, a neighbour in its collection, or a
    /// jump somewhere not visited in a while. `None` when it has visited every item it can
    /// reach. The step's collection counts as opened on this device.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive can't be read.
    pub fn wander_step(&self, walk: Arc<Wander>) -> Result<Option<WanderStep>, CarrelError> {
        let archive = self.fresh()?;
        let catalog = Catalog::read(&archive)?;
        let index = carrel_core::index::related::open(&archive).ok();
        let near = index.as_ref().map(wander::near_in);
        let texts = TextCache::of(&archive);
        let mut around = Surroundings::new().with_texts(&texts);
        if let Some(near) = &near {
            around = around.with_near(near);
        }
        let mut walk = walk
            .walk
            .lock()
            .map_err(|_| CarrelError::failed("the walk was poisoned"))?;
        let Some(step) = walk.step(&catalog, &around) else {
            return Ok(None);
        };
        let today = jiff::Zoned::now().date();
        self.with_visits(&archive, |visits| {
            visits.open(&step.item.collection, today);
            visits.walk = Some(walk.clone());
            ((), true)
        })?;
        let title = |id| {
            catalog
                .get(id)
                .map_or_else(|| id.to_string(), |e| e.item.title.clone())
        };
        let reason = match step.reason {
            Reason::Start { collection } => WanderReason::Start { collection },
            Reason::Linked {
                link_type,
                from,
                back,
            } => WanderReason::Linked {
                link_type,
                from: title(from),
                back,
            },
            Reason::Near { from, score } => WanderReason::Near {
                from: title(from),
                score,
            },
            Reason::Neighbour { collection } => WanderReason::Neighbour { collection },
            Reason::Jump {
                collection,
                last_seen,
            } => WanderReason::Jump {
                collection,
                last_seen,
            },
        };
        Ok(Some(WanderStep {
            number: count(step.number),
            item: (&step.item).into(),
            reason,
            summary: step.summary,
            tags: step.tags,
            preview: step.preview,
        }))
    }

    /// Items by date, grouped `per` year, month or day, going `by` when they were `created`,
    /// `filed` or `worked` on (`None` for each item's own date), narrowed by `query` as
    /// search narrows (words and filters such as `tag:receipts`; empty for every item).
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for a query, `by` or `per` that can't be read, or
    /// [`CarrelError::Failed`] if the archive can't be.
    pub fn timeline(
        &self,
        query: String,
        by: Option<String>,
        per: String,
    ) -> Result<DiscoveryTimeline, CarrelError> {
        let archive = self.fresh()?;
        let query = Query::parse(&query).map_err(CarrelError::invalid)?;
        let by = match by {
            Some(by) => Some(DateKind::parse(&by).ok_or_else(|| {
                CarrelError::invalid(format!("by is created, filed or worked, not “{by}”"))
            })?),
            None => None,
        };
        let resolution = Resolution::parse(&per).ok_or_else(|| {
            CarrelError::invalid(format!("per is year, month or day, not “{per}”"))
        })?;
        let options = Options {
            by,
            resolution,
            filters: query.filters.clone(),
            only: timeline::matching(&archive, &query)?,
        };
        let found = timeline::timeline(&Catalog::read(&archive)?, &options);
        Ok(DiscoveryTimeline {
            periods: found
                .periods
                .iter()
                .map(|p| TimelinePeriod {
                    period: p.period.clone(),
                    items: p
                        .items
                        .iter()
                        .map(|placed| TimelineItem {
                            item: (&placed.item).into(),
                            date: placed.date.clone(),
                            what: placed.what.name().to_owned(),
                        })
                        .collect(),
                })
                .collect(),
            undated: count(found.undated),
        })
    }

    /// The archive map: collections by total size and number of items.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive can't be read.
    pub fn archive_map(&self) -> Result<ArchiveMap, CarrelError> {
        let found = map::map(&Catalog::read(&self.fresh()?)?);
        let mut regions = Vec::new();
        for region in &found.regions {
            flatten(region, None, 0, &mut regions);
        }
        Ok(ArchiveMap {
            regions,
            total_items: found.total_items as u64,
            total_size: found.total_size,
            without_size: found.without_size as u64,
        })
    }

    /// Filing and work sessions by day, from `from` to `to` (`YYYY-MM-DD`; the last year to
    /// today when `None`).
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for dates that can't be read or are the wrong way round, or
    /// [`CarrelError::Failed`] if the archive can't be read.
    pub fn activity(
        &self,
        from: Option<String>,
        to: Option<String>,
    ) -> Result<ArchiveActivity, CarrelError> {
        let to = day(to.as_deref())?;
        let from = match from {
            Some(from) => day(Some(&from))?,
            None => activity::last_year(to).0,
        };
        if from > to {
            return Err(CarrelError::invalid(
                "the first day must come before the last",
            ));
        }
        let found = activity::activity(&Catalog::read(&self.fresh()?)?, from, to);
        Ok(ArchiveActivity {
            from: found.from.to_string(),
            to: found.to.to_string(),
            days: found
                .days
                .iter()
                .map(|d| ActivityDay {
                    date: d.date.to_string(),
                    filed: count(d.filed),
                    sessions: count(d.sessions),
                    level: u8::try_from(found.level(d, LEVELS)).unwrap_or(u8::MAX),
                })
                .collect(),
            busiest: count(found.busiest),
        })
    }

    /// Now and then, a collection not opened in a long time, with an item or two from it, as
    /// `carrel status` mentions one; the same one all day once it's chosen.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive can't be read.
    pub fn resurfaced(&self) -> Result<Option<Resurfaced>, CarrelError> {
        let archive = self.fresh()?;
        let catalog = Catalog::read(&archive)?;
        let today = jiff::Zoned::now().date();
        let found = self.with_visits(&archive, |visits| {
            let before = visits.resurfaced.clone();
            let found = visits.resurface(&catalog, today);
            let changed = visits.resurfaced != before;
            (found, changed)
        })?;
        Ok(found.map(|f| Resurfaced {
            collection: f.collection,
            title: f.title,
            last_seen: f.last_seen,
            days: f.days,
            items: f.items.iter().map(Into::into).collect(),
        }))
    }

    /// Records on this device that the collection `collection` was opened today, as showing
    /// an item does, so wander and resurfacing know it's been seen.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the configuration folder can't be found.
    pub fn record_opened(&self, collection: String) -> Result<(), CarrelError> {
        let today = jiff::Zoned::now().date();
        self.with_visits(&self.archive, |visits| {
            ((), visits.open(&collection, today))
        })
    }
}
