//! Discovery (`docs/design/librarian.md`, Discovery): views of the archive itself, computed
//! here as plain data that every interface draws its own way.
//!
//! - [`today::on_this_day`]: what was filed, created or worked on on a date in other years.
//! - [`wander::Walk`]: a reproducible random walk through the stacks, one item at a time.
//! - [`timeline::timeline`]: items by date, by year, month or day, with search's filters.
//! - [`map::map`]: collections by total size and number of items, as a tree.
//! - [`activity::activity`]: filing and work sessions by day, for a heatmap.
//! - [`visits::Visits`]: which collections this device opened when, kept on the device, and
//!   [`visits::Visits::resurface`], which now and then names one not opened in a long time.
//!
//! Each view reads a [`Catalog`]: every card in the archive, read once. Build one and compute
//! as many views from it as needed.
//!
//! ```
//! use carrel_core::archive::{Archive, InitOptions};
//! use carrel_core::discover::{Catalog, today};
//!
//! let dir = tempfile::tempdir()?;
//! let archive = Archive::init(&dir.path().join("Archive"), &InitOptions::new("My archive"))?;
//! let catalog = Catalog::read(&archive)?;
//! let day = today::on_this_day(&catalog, "2026-11-01".parse()?);
//! assert!(day.years.is_empty());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod activity;
pub mod map;
pub mod timeline;
pub mod today;
pub mod visits;
pub mod wander;

use std::collections::HashMap;

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::archive::{Archive, ArchiveError};
use crate::card::{Card, Checksum, Id, Kind, Sensitivity};
use crate::card_file::CardFile;

/// Which of an item's dates a view goes by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DateKind {
    /// When it was made: its `created` date, else its own date (`document`).
    Created,
    /// When it was filed: its `added` date.
    Filed,
    /// When it was worked on: its history entries' dates and its `worked` date.
    Worked,
}

impl DateKind {
    /// Every kind, in the order views list them.
    pub const ALL: [Self; 3] = [Self::Created, Self::Filed, Self::Worked];

    /// The kind's name, as `--json` and the command line write it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Filed => "filed",
            Self::Worked => "worked",
        }
    }

    /// Reads a kind's name: `created`, `filed` (or `added`) or `worked`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_lowercase().as_str() {
            "created" | "made" => Some(Self::Created),
            "filed" | "added" => Some(Self::Filed),
            "worked" | "work" => Some(Self::Worked),
            _ => None,
        }
    }
}

/// An item as the views show it: enough to list it and open it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// Its identifier.
    pub id: Id,
    /// Its card's title.
    pub title: String,
    /// Its card, relative to the archive root.
    pub card: String,
    /// Its contents, relative to the archive root; card-only items have none.
    pub item: Option<String>,
    /// The collection it's in.
    pub collection: String,
    /// Its kind.
    pub kind: Kind,
    /// How sensitive it is, so a restricted or sensitive one can be marked.
    pub sensitivity: Sensitivity,
}

/// One item in a [`Catalog`]: its card's fields that discovery uses.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The item.
    pub item: Item,
    /// Its card's summary.
    pub summary: Option<String>,
    /// Its tags.
    pub tags: Vec<String>,
    /// The items its card links to, with each link's type.
    pub links: Vec<(String, Id)>,
    /// Its size in bytes, if the card records one.
    pub size: Option<u64>,
    /// Whether it's checked out.
    pub checked_out: bool,
    /// When it was made, as `YYYY-MM-DD`.
    pub created: Option<Date>,
    /// When it was filed.
    pub filed: Option<Date>,
    /// The days it was worked on, oldest first, each once: its history entries' dates, and its
    /// `worked` date.
    pub worked: Vec<Date>,
    /// How many work sessions its history records on each day it was worked on.
    pub sessions: Vec<(Date, usize)>,
    /// The checksum its cached text is kept under, for an item that isn't restricted.
    pub text: Option<Checksum>,
}

impl Entry {
    /// Whether the item's text may be shown: never for a restricted item.
    #[must_use]
    pub fn text_allowed(&self) -> bool {
        self.item.sensitivity != Sensitivity::Restricted
    }

    /// The days the item has of `kind`.
    #[must_use]
    pub fn dates(&self, kind: DateKind) -> Vec<Date> {
        match kind {
            DateKind::Created => self.created.into_iter().collect(),
            DateKind::Filed => self.filed.into_iter().collect(),
            DateKind::Worked => self.worked.clone(),
        }
    }
}

/// A collection in a [`Catalog`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionEntry {
    /// Its folder, relative to the archive root.
    pub path: String,
    /// Its title.
    pub title: String,
}

/// Every item and collection in an archive, read once, for the views to compute from.
/// Tombstones are left out.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    /// The items, sorted by card path.
    pub entries: Vec<Entry>,
    /// The collections, sorted by path.
    pub collections: Vec<CollectionEntry>,
    by_id: HashMap<Id, usize>,
}

/// The day a date or timestamp falls on, as written (FORMAT.md §2.4): its first ten characters.
#[must_use]
pub fn day_of(text: &str) -> Option<Date> {
    text.get(..10)?.parse().ok()
}

fn entry(card: &Card, card_path: &str, history: &[String]) -> Entry {
    let collection = card_path.rsplit_once('/').map_or("", |(c, _)| c).to_owned();
    let dates = card.dates.clone().unwrap_or_default();
    let date = |d: &Option<crate::card::DateOrTime>| d.as_ref().and_then(|d| day_of(&d.0));
    let mut sessions: Vec<(Date, usize)> = Vec::new();
    for day in history.iter().filter_map(|d| day_of(d)) {
        match sessions.iter_mut().find(|(d, _)| *d == day) {
            Some((_, n)) => *n += 1,
            None => sessions.push((day, 1)),
        }
    }
    let mut worked: Vec<Date> = sessions.iter().map(|(d, _)| *d).collect();
    if let Some(day) = date(&dates.worked) {
        if sessions.is_empty() {
            // A day worked on that no entry records was one session all the same.
            sessions.push((day, 1));
        }
        worked.push(day);
    }
    worked.sort_unstable();
    worked.dedup();
    sessions.sort_unstable();
    Entry {
        item: Item {
            id: card.id,
            title: card.title.clone(),
            card: card_path.to_owned(),
            item: card
                .item
                .as_ref()
                .map(|name| format!("{collection}/{name}")),
            collection,
            kind: card.kind,
            sensitivity: card.sensitivity,
        },
        summary: card.summary.clone(),
        tags: card.tags.iter().map(|t| t.0.clone()).collect(),
        links: card
            .links
            .iter()
            .map(|l| (l.link_type.0.clone(), l.id))
            .collect(),
        size: card.size,
        checked_out: card.checked_out.is_some(),
        created: date(&dates.created).or_else(|| date(&dates.document)),
        filed: date(&dates.added),
        worked,
        sessions,
        text: (card.sensitivity != Sensitivity::Restricted)
            .then(|| crate::search::texts::fingerprint(card).cloned())
            .flatten(),
    }
}

impl Catalog {
    /// Reads every card and collection in `archive`. Cards that don't parse are left out, as
    /// search leaves them out.
    ///
    /// # Errors
    ///
    /// An error listing the archive's collections or items.
    pub fn read(archive: &Archive) -> Result<Self, ArchiveError> {
        let mut entries = Vec::new();
        for item in archive.items()? {
            if item.kind == Kind::Tombstone {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(archive.root().join(&item.card)) else {
                continue;
            };
            let Ok(file) = CardFile::parse(&text) else {
                continue;
            };
            let history: Vec<String> = file
                .history()
                .map(|log| log.entries.into_iter().map(|e| e.date).collect())
                .unwrap_or_default();
            entries.push(entry(file.card(), &item.card, &history));
        }
        let collections = archive
            .collections()?
            .into_iter()
            .map(|c| CollectionEntry {
                title: c.doc.title.clone(),
                path: c.path,
            })
            .collect();
        Ok(Self::new(entries, collections))
    }

    /// A catalog of `entries` and `collections`, as [`Catalog::read`] makes from an archive.
    #[must_use]
    pub fn new(mut entries: Vec<Entry>, mut collections: Vec<CollectionEntry>) -> Self {
        entries.sort_by(|a, b| a.item.card.cmp(&b.item.card));
        collections.sort_by(|a, b| a.path.cmp(&b.path));
        let by_id = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.item.id, i))
            .collect();
        Self {
            entries,
            collections,
            by_id,
        }
    }

    /// The item `id`, if the catalog has it.
    #[must_use]
    pub fn get(&self, id: Id) -> Option<&Entry> {
        self.by_id.get(&id).map(|&i| &self.entries[i])
    }

    /// The collection at `path`, if there is one.
    #[must_use]
    pub fn collection(&self, path: &str) -> Option<&CollectionEntry> {
        self.collections.iter().find(|c| c.path == path)
    }

    /// The items directly in the collection at `path`.
    pub fn in_collection<'a>(&'a self, path: &'a str) -> impl Iterator<Item = &'a Entry> + 'a {
        self.entries
            .iter()
            .filter(move |e| e.item.collection == path)
    }

    /// The latest day anything in the collection at `path`, or below it, was filed or worked
    /// on.
    #[must_use]
    pub fn last_touched(&self, path: &str) -> Option<Date> {
        let below = format!("{path}/");
        self.entries
            .iter()
            .filter(|e| e.item.collection == path || e.item.collection.starts_with(&below))
            .flat_map(|e| e.filed.into_iter().chain(e.worked.iter().copied()))
            .max()
    }
}

/// A small, reproducible source of random numbers (`SplitMix64`): the same seed gives the same
/// numbers on every device and every version, so a walk can be repeated.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// A generator started from `seed`.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// The next number.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number below `n`, which must be more than 0.
    pub fn below(&mut self, n: usize) -> usize {
        // The modulo bias is far below anything a walk could notice.
        usize::try_from(self.next_u64() % (n as u64)).unwrap_or(0)
    }

    /// Whether a chance of `percent` in 100 came up.
    pub fn chance(&mut self, percent: u64) -> bool {
        self.next_u64() % 100 < percent
    }

    /// One of `items`, or none if there are none.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        (!items.is_empty()).then(|| &items[self.below(items.len())])
    }
}

/// A seed from `date`, so something chosen "for today" stays the same all day.
#[must_use]
pub fn day_seed(date: Date) -> u64 {
    let days = date
        .since(Date::constant(1970, 1, 1))
        .map_or(0, |span| i64::from(span.get_days()));
    u64::try_from(days).unwrap_or(0)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// An entry for tests: an item `title` in `collection`.
    pub fn entry(n: u128, title: &str, collection: &str) -> Entry {
        Entry {
            item: Item {
                id: Id(uuid::Uuid::from_u128(n)),
                title: title.to_owned(),
                card: format!("{collection}/{title}.md"),
                item: Some(format!("{collection}/{title}")),
                collection: collection.to_owned(),
                kind: Kind::File,
                sensitivity: Sensitivity::Personal,
            },
            summary: None,
            tags: Vec::new(),
            links: Vec::new(),
            size: None,
            checked_out: false,
            created: None,
            filed: None,
            worked: Vec::new(),
            sessions: Vec::new(),
            text: None,
        }
    }

    pub fn collection(path: &str) -> CollectionEntry {
        CollectionEntry {
            path: path.to_owned(),
            title: path.rsplit('/').next().unwrap_or(path).to_owned(),
        }
    }

    pub fn date(text: &str) -> Date {
        text.parse().unwrap()
    }

    #[test]
    fn the_same_seed_gives_the_same_numbers() {
        let a: Vec<u64> = (0..5)
            .map({
                let mut rng = Rng::new(42);
                move |_| rng.next_u64()
            })
            .collect();
        let mut rng = Rng::new(42);
        let b: Vec<u64> = (0..5).map(|_| rng.next_u64()).collect();
        assert_eq!(a, b);
        assert_ne!(Rng::new(1).next_u64(), Rng::new(2).next_u64());
    }

    #[test]
    fn days_are_read_from_dates_and_timestamps() {
        assert_eq!(day_of("2024-03-05"), Some(date("2024-03-05")));
        assert_eq!(day_of("2024-03-05T10:00:00Z"), Some(date("2024-03-05")));
        assert_eq!(day_of("March"), None);
        assert_eq!(day_seed(date("1970-01-02")), 1);
    }

    #[test]
    fn a_card_becomes_an_entry_with_its_days() {
        let text = "---\ncarrel: card\nformat: \"1.0\"\nid: 01a18701-0500-776a-9867-080924eb019c\nkind: file\ntitle: Lease\nitem: lease.pdf\nsize: 10\ndates:\n  document: \"2019-04-02\"\n  added: \"2025-01-05T10:00:00Z\"\n  worked: \"2025-03-01\"\nsensitivity: personal\n---\n";
        let file = CardFile::parse(text).unwrap();
        let history = vec!["2025-02-01".to_owned(), "2025-02-01".to_owned()];
        let e = super::entry(file.card(), "Home/Lease.md", &history);
        assert_eq!(e.item.collection, "Home");
        assert_eq!(e.item.item.as_deref(), Some("Home/lease.pdf"));
        assert_eq!(
            e.created,
            Some(date("2019-04-02")),
            "its own date stands in"
        );
        assert_eq!(e.filed, Some(date("2025-01-05")));
        assert_eq!(e.worked, [date("2025-02-01"), date("2025-03-01")]);
        assert_eq!(e.sessions, [(date("2025-02-01"), 2)]);
    }
}
