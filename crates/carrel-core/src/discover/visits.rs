//! Which collections were opened when, kept on this device rather than in the archive, so
//! reading never writes the archive; and resurfacing, which now and then names a collection
//! not opened in a long time, with an item or two from it, for Status.
//!
//! An interface records a collection as opened when an item in it is shown, as `carrel show`
//! and a wander step do. A collection this device never opened counts as last seen when
//! anything in it was last filed or worked on.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use super::{Catalog, Item, Rng, day_of, day_seed};

/// How long a collection goes unseen before it counts as forgotten, in days.
pub const FORGOTTEN_DAYS: i32 = 180;

/// How long after one is resurfaced before another is, in days: "now and then".
pub const RESURFACE_EVERY_DAYS: i32 = 7;

/// How many of a resurfaced collection's items are named with it.
pub const RESURFACED_ITEMS: usize = 2;

/// When a collection was resurfaced, so another isn't for a while.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mention {
    /// The day, as `YYYY-MM-DD`.
    pub date: String,
    /// The collection.
    pub collection: String,
}

/// This device's record of discovery for one archive.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visits {
    /// The day each collection was last opened, as `YYYY-MM-DD`, by path.
    #[serde(default)]
    pub opened: BTreeMap<String, String>,
    /// The last collection resurfaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resurfaced: Option<Mention>,
    /// The walk through the stacks last taken, so it can be stepped on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walk: Option<super::wander::Walk>,
}

/// A collection not opened in a long time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Forgotten {
    /// Its folder.
    pub collection: String,
    /// Its title.
    pub title: String,
    /// When it was last opened on this device, or else last filed into or worked on, as
    /// `YYYY-MM-DD`.
    pub last_seen: String,
    /// How many days ago that was.
    pub days: i32,
    /// An item or two from it.
    pub items: Vec<Item>,
}

fn days_between(from: Date, to: Date) -> i32 {
    to.since(from).map_or(0, |span| span.get_days())
}

impl Visits {
    /// The file it's kept in for the archive `archive_id`, in Carrel's folder on this device.
    #[must_use]
    pub fn path(archive_id: &str) -> Option<PathBuf> {
        crate::local::config_dir().map(|d| Self::path_in(&d, archive_id))
    }

    /// The file it's kept in for the archive `archive_id`, in the configuration folder
    /// `config`.
    #[must_use]
    pub fn path_in(config: &Path, archive_id: &str) -> PathBuf {
        config.join("discover").join(format!("{archive_id}.json"))
    }

    /// The saved record, or an empty one.
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

    /// Records that `collection` was opened on `today`; whether that changed anything, so a
    /// caller saves only when it did.
    pub fn open(&mut self, collection: &str, today: Date) -> bool {
        let day = today.to_string();
        if self.opened.get(collection) == Some(&day) {
            return false;
        }
        self.opened.insert(collection.to_owned(), day);
        true
    }

    /// When `collection` was last opened on this device.
    #[must_use]
    pub fn last_opened(&self, collection: &str) -> Option<Date> {
        self.opened.get(collection).and_then(|d| day_of(d))
    }

    /// When `collection` was last seen: opened here, or else filed into or worked on.
    #[must_use]
    pub fn last_seen(&self, catalog: &Catalog, collection: &str) -> Option<Date> {
        self.last_opened(collection)
            .into_iter()
            .chain(catalog.last_touched(collection))
            .max()
    }

    /// The collections with items not seen for [`FORGOTTEN_DAYS`] as of `today`, longest
    /// forgotten first, each with up to [`RESURFACED_ITEMS`] of its items chosen by `seed`.
    /// Holding, which is for what's just arrived, never is.
    #[must_use]
    pub fn forgotten(&self, catalog: &Catalog, today: Date, seed: u64) -> Vec<Forgotten> {
        let mut found: Vec<Forgotten> = catalog
            .collections
            .iter()
            .filter(|c| c.path != "Holding")
            .filter_map(|c| {
                let last = self.last_seen(catalog, &c.path)?;
                let days = days_between(last, today);
                let items: Vec<&Item> = catalog.in_collection(&c.path).map(|e| &e.item).collect();
                (days >= FORGOTTEN_DAYS && !items.is_empty()).then(|| {
                    let mut rng = Rng::new(seed ^ hash(&c.path));
                    let mut chosen: Vec<Item> = Vec::new();
                    let mut left = items;
                    while chosen.len() < RESURFACED_ITEMS && !left.is_empty() {
                        chosen.push(left.remove(rng.below(left.len())).clone());
                    }
                    Forgotten {
                        collection: c.path.clone(),
                        title: c.title.clone(),
                        last_seen: last.to_string(),
                        days,
                        items: chosen,
                    }
                })
            })
            .collect();
        found.sort_by(|a, b| b.days.cmp(&a.days).then(a.collection.cmp(&b.collection)));
        found
    }

    /// The collection to resurface on Status `today`, now and then: the same one all day once
    /// chosen, then none for [`RESURFACE_EVERY_DAYS`], then another. Choosing one records it,
    /// so save the visits afterwards. None once it's opened.
    pub fn resurface(&mut self, catalog: &Catalog, today: Date) -> Option<Forgotten> {
        let seed = day_seed(today);
        let forgotten = self.forgotten(catalog, today, seed);
        if let Some(last) = &self.resurfaced
            && let Some(when) = day_of(&last.date)
        {
            if when == today {
                return forgotten
                    .into_iter()
                    .find(|f| f.collection == last.collection);
            }
            if days_between(when, today) < RESURFACE_EVERY_DAYS && when < today {
                return None;
            }
        }
        // One of the longest forgotten, so it's not always the very oldest.
        let few = &forgotten[..forgotten.len().min(3)];
        let chosen = Rng::new(seed).pick(few)?.clone();
        self.resurfaced = Some(Mention {
            date: today.to_string(),
            collection: chosen.collection.clone(),
        });
        Some(chosen)
    }
}

/// A stable hash of `text` (FNV-1a), the same on every device and version.
fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::tests::{collection, date, entry};

    fn catalog() -> Catalog {
        let mut old = entry(1, "Old letter", "Letters");
        old.filed = Some(date("2024-01-01"));
        let mut older = entry(2, "Older letter", "Letters");
        older.filed = Some(date("2023-01-01"));
        let mut recent = entry(3, "Notes", "Work");
        recent.filed = Some(date("2026-10-01"));
        let mut held = entry(4, "Scan", "Holding");
        held.filed = Some(date("2020-01-01"));
        Catalog::new(
            vec![old, older, recent, held],
            ["Letters", "Work", "Holding", "Empty"]
                .map(collection)
                .to_vec(),
        )
    }

    #[test]
    fn a_collection_unseen_for_long_is_forgotten_until_opened() {
        let catalog = catalog();
        let mut visits = Visits::default();
        let today = date("2026-11-01");
        let forgotten = visits.forgotten(&catalog, today, 1);
        assert_eq!(
            forgotten.len(),
            1,
            "Work is recent, Holding and Empty never count"
        );
        assert_eq!(forgotten[0].collection, "Letters");
        assert_eq!(forgotten[0].last_seen, "2024-01-01");
        assert_eq!(forgotten[0].items.len(), 2);

        assert!(visits.open("Letters", date("2026-06-01")));
        assert!(!visits.open("Letters", date("2026-06-01")), "nothing new");
        assert_eq!(visits.forgotten(&catalog, today, 1), []);
        assert_eq!(
            visits.forgotten(&catalog, date("2027-06-01"), 1)[0].last_seen,
            "2026-06-01"
        );
    }

    #[test]
    fn resurfacing_is_now_and_then() {
        let catalog = catalog();
        let mut visits = Visits::default();
        let day = date("2026-11-01");
        let first = visits.resurface(&catalog, day).unwrap();
        assert_eq!(first.collection, "Letters");
        assert_eq!(
            visits.resurface(&catalog, day),
            Some(first),
            "the same one all day"
        );
        assert_eq!(
            visits.resurface(&catalog, date("2026-11-03")),
            None,
            "then a rest"
        );
        assert!(visits.resurface(&catalog, date("2026-11-08")).is_some());

        visits.open("Letters", date("2026-11-08"));
        assert_eq!(
            visits.resurface(&catalog, date("2026-11-08")),
            None,
            "opened"
        );
    }

    #[test]
    fn visits_are_saved_on_the_device() {
        let dir = tempfile::tempdir().unwrap();
        let path = Visits::path_in(dir.path(), "abc");
        assert_eq!(Visits::load(&path), Visits::default());
        let mut visits = Visits::default();
        visits.open("Letters", date("2026-11-01"));
        visits.save(&path).unwrap();
        assert_eq!(Visits::load(&path), visits);
        assert!(path.ends_with("discover/abc.json"));
    }
}
