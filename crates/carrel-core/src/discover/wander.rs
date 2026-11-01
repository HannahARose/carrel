//! Wander the stacks: a random walk from item to item, one step at a time, each with why it
//! came next: an item it links to or that links to it, one near it by meaning, a neighbour in
//! its collection, or now and then a jump to a collection not opened in a long time.
//!
//! A walk is reproducible: the same seed from the same item, over the same archive, takes the
//! same steps. It never visits an item twice. A restricted item can be visited, by its card,
//! but its text is never shown.
//!
//! ```
//! use carrel_core::discover::{Catalog, wander::{Surroundings, Walk}};
//!
//! let catalog = Catalog::default();
//! let mut walk = Walk::new(42, None);
//! let around = Surroundings::new();
//! assert!(walk.step(&catalog, &around).is_none(), "an empty archive has nowhere to go");
//! ```

use std::collections::HashSet;

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use super::visits::Visits;
use super::{Catalog, Entry, Item, Rng};
use crate::card::Id;
use crate::index::Index;
use crate::search::texts::TextCache;

/// The chance, in 100, that a step jumps to another collection instead of following the item.
pub const JUMP_PERCENT: u64 = 15;

/// How many of the nearest items by meaning a step chooses among.
pub const NEAR_CHOICES: usize = 6;

/// How many characters of an item's text its preview shows, at most.
pub const PREVIEW_CHARS: usize = 400;

/// Why a step came next.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "why", rename_all = "kebab-case")]
pub enum Reason {
    /// The walk's first step, into a collection not opened in a while.
    Start {
        /// The collection.
        collection: String,
    },
    /// The item before links to it, or it links to the item before.
    Linked {
        /// The link's type, such as `related` or `version-of`.
        link_type: String,
        /// The item before.
        from: Id,
        /// Whether it's this item that links to the one before.
        back: bool,
    },
    /// It's near the item before by meaning.
    Near {
        /// The item before.
        from: Id,
        /// The cosine similarity of the two: 1 is the same direction.
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

/// One step of a walk: the item, and why it came next.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// Which step it is, from 1.
    pub number: usize,
    /// The item.
    pub item: Item,
    /// Why it came next.
    pub reason: Reason,
    /// Its card's summary.
    pub summary: Option<String>,
    /// Its tags.
    pub tags: Vec<String>,
    /// The start of its text, from the text cache; never for a restricted item.
    pub preview: Option<String>,
}

/// What a walk looks around with, besides the catalog. Each is optional: without the
/// embedding index a walk follows links and collections only.
#[derive(Clone, Copy)]
pub struct Surroundings<'a> {
    /// The items nearest an item by meaning, nearest first, with their similarity.
    pub near: Option<&'a dyn Fn(Id) -> Vec<(Id, f32)>>,
    /// The text cache, for previews.
    pub texts: Option<&'a TextCache>,
    /// Which collections this device opened when, for jumps.
    pub visits: Option<&'a Visits>,
}

impl std::fmt::Debug for Surroundings<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Surroundings")
            .field("near", &self.near.is_some())
            .field("texts", &self.texts.is_some())
            .field("visits", &self.visits)
            .finish()
    }
}

impl Default for Surroundings<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Surroundings<'a> {
    /// Nothing but the catalog.
    #[must_use]
    pub fn new() -> Self {
        Self {
            near: None,
            texts: None,
            visits: None,
        }
    }

    /// With `near` for items near by meaning.
    #[must_use]
    pub fn with_near(mut self, near: &'a dyn Fn(Id) -> Vec<(Id, f32)>) -> Self {
        self.near = Some(near);
        self
    }

    /// With `texts` for previews.
    #[must_use]
    pub fn with_texts(mut self, texts: &'a TextCache) -> Self {
        self.texts = Some(texts);
        self
    }

    /// With `visits` for which collections were opened when.
    #[must_use]
    pub fn with_visits(mut self, visits: &'a Visits) -> Self {
        self.visits = Some(visits);
        self
    }
}

/// The items nearest an item in `index`, for [`Surroundings::with_near`].
pub fn near_in(index: &Index) -> impl Fn(Id) -> Vec<(Id, f32)> + '_ {
    move |id| {
        index
            .near_item(id, &|_| true, NEAR_CHOICES)
            .unwrap_or_default()
            .into_iter()
            .map(|near| (near.id, near.score))
            .collect()
    }
}

/// A walk through the stacks: where it started, and the items it has visited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Walk {
    /// The seed that makes it reproducible.
    pub seed: u64,
    /// The item it started from, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<Id>,
    /// The items it has visited, in order.
    #[serde(default)]
    pub visited: Vec<Id>,
}

/// The start of `text`, cut at a word, with an ellipsis when there's more.
fn preview(text: &str) -> Option<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return None;
    }
    if text.chars().count() <= PREVIEW_CHARS {
        return Some(text);
    }
    let cut: String = text.chars().take(PREVIEW_CHARS).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(a, _)| a);
    Some(format!("{cut}…"))
}

impl Walk {
    /// A new walk with `seed`, from the item `start` or from nowhere.
    #[must_use]
    pub fn new(seed: u64, start: Option<Id>) -> Self {
        Self {
            seed,
            start,
            visited: Vec::new(),
        }
    }

    /// The item the walk is at: the last visited, else where it started.
    #[must_use]
    pub fn current(&self) -> Option<Id> {
        self.visited.last().copied().or(self.start)
    }

    fn seen(&self) -> HashSet<Id> {
        self.visited.iter().copied().chain(self.start).collect()
    }

    /// Takes the next step, or none when there's nowhere left to go.
    pub fn step(&mut self, catalog: &Catalog, around: &Surroundings) -> Option<Step> {
        let number = self.visited.len() + 1;
        let mut rng = Rng::new(self.seed ^ (number as u64).wrapping_mul(0xA24B_AED4_963E_E407));
        let seen = self.seen();
        let current = self.current().and_then(|id| catalog.get(id));
        let followed = match current {
            Some(from) if !rng.chance(JUMP_PERCENT) => {
                follow(catalog, around, from, &seen, &mut rng)
            }
            _ => None,
        };
        let (entry, reason) = followed.or_else(|| {
            let (entry, last_seen) = jump(catalog, around, current, &seen, &mut rng)?;
            let collection = entry.item.collection.clone();
            Some(match current {
                None => (entry, Reason::Start { collection }),
                Some(_) => (
                    entry,
                    Reason::Jump {
                        collection,
                        last_seen,
                    },
                ),
            })
        })?;
        self.visited.push(entry.item.id);
        let preview = entry
            .text
            .as_ref()
            .filter(|_| entry.text_allowed())
            .and_then(|checksum| around.texts?.get(checksum))
            .and_then(|text| preview(&text));
        Some(Step {
            number,
            item: entry.item.clone(),
            reason,
            summary: entry.summary.clone(),
            tags: entry.tags.clone(),
            preview,
        })
    }
}

/// A step that follows `from`: by a link, by meaning or within its collection, chosen at
/// random among those there are.
fn follow<'a>(
    catalog: &'a Catalog,
    around: &Surroundings,
    from: &'a Entry,
    seen: &HashSet<Id>,
    rng: &mut Rng,
) -> Option<(&'a Entry, Reason)> {
    let fresh = |id: &Id| !seen.contains(id) && catalog.get(*id).is_some();
    let mut linked: Vec<(Id, String, bool)> = from
        .links
        .iter()
        .filter(|(_, id)| fresh(id))
        .map(|(t, id)| (*id, t.clone(), false))
        .collect();
    for other in &catalog.entries {
        if let Some((t, _)) = other.links.iter().find(|(_, id)| *id == from.item.id)
            && fresh(&other.item.id)
            && !linked.iter().any(|(id, ..)| *id == other.item.id)
        {
            linked.push((other.item.id, t.clone(), true));
        }
    }
    let near: Vec<(Id, f32)> = around
        .near
        .map(|near| near(from.item.id))
        .unwrap_or_default()
        .into_iter()
        .filter(|(id, _)| fresh(id))
        .collect();
    let neighbours: Vec<&Entry> = catalog
        .in_collection(&from.item.collection)
        .filter(|e| fresh(&e.item.id))
        .collect();
    let mut ways: Vec<u8> = Vec::new();
    for (way, has) in [
        (0, !linked.is_empty()),
        (1, !near.is_empty()),
        (2, !neighbours.is_empty()),
    ] {
        if has {
            ways.push(way);
        }
    }
    let from_id = from.item.id;
    match rng.pick(&ways)? {
        0 => {
            let (id, link_type, back) = rng.pick(&linked)?.clone();
            Some((
                catalog.get(id)?,
                Reason::Linked {
                    link_type,
                    from: from_id,
                    back,
                },
            ))
        }
        1 => {
            let (id, score) = *rng.pick(&near)?;
            Some((
                catalog.get(id)?,
                Reason::Near {
                    from: from_id,
                    score,
                },
            ))
        }
        _ => {
            let entry = *rng.pick(&neighbours)?;
            let collection = entry.item.collection.clone();
            Some((entry, Reason::Neighbour { collection }))
        }
    }
}

/// A jump to one of the collections seen longest ago that still has items the walk hasn't
/// visited, away from the one it's in when there's another; and when it was last seen.
fn jump<'a>(
    catalog: &'a Catalog,
    around: &Surroundings,
    current: Option<&Entry>,
    seen: &HashSet<Id>,
    rng: &mut Rng,
) -> Option<(&'a Entry, Option<String>)> {
    let empty = Visits::default();
    let visits = around.visits.unwrap_or(&empty);
    let mut places: Vec<(Option<Date>, &str)> = catalog
        .collections
        .iter()
        .map(|c| c.path.as_str())
        .filter(|path| {
            catalog
                .in_collection(path)
                .any(|e| !seen.contains(&e.item.id))
        })
        .map(|path| (visits.last_seen(catalog, path), path))
        .collect();
    if places.len() > 1
        && let Some(here) = current
    {
        places.retain(|(_, path)| *path != here.item.collection);
    }
    // Never seen first, then longest ago.
    places.sort();
    // Among the oldest third, at most five, so it isn't always the very oldest.
    let few = places.len().div_ceil(3).min(5);
    let (last_seen, path) = *rng.pick(&places[..few])?;
    let items: Vec<&Entry> = catalog
        .in_collection(path)
        .filter(|e| !seen.contains(&e.item.id))
        .collect();
    let entry = *rng.pick(&items)?;
    Some((entry, last_seen.map(|d| d.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::Sensitivity;
    use crate::discover::tests::{collection, date, entry};

    fn catalog() -> Catalog {
        let mut entries = Vec::new();
        for n in 0..6u128 {
            let collection = if n < 3 { "Letters" } else { "Work" };
            let mut e = entry(n + 1, &format!("item {n}"), collection);
            e.filed = Some(date("2025-01-01"));
            entries.push(e);
        }
        entries[0].links = vec![("related".into(), Id(uuid::Uuid::from_u128(5)))];
        entries[4].item.sensitivity = Sensitivity::Restricted;
        Catalog::new(entries, ["Letters", "Work"].map(collection).to_vec())
    }

    fn walk(seed: u64, start: Option<Id>, around: &Surroundings) -> Vec<Step> {
        let catalog = catalog();
        let mut walk = Walk::new(seed, start);
        std::iter::from_fn(|| walk.step(&catalog, around)).collect()
    }

    #[test]
    fn a_walk_visits_every_item_once_and_repeats_with_its_seed() {
        let around = Surroundings::new();
        let steps = walk(7, None, &around);
        assert_eq!(steps.len(), 6, "every item, then nowhere left");
        let ids: HashSet<Id> = steps.iter().map(|s| s.item.id).collect();
        assert_eq!(ids.len(), 6, "never twice");
        assert!(matches!(steps[0].reason, Reason::Start { .. }));
        assert_eq!(steps[0].number, 1);
        assert_eq!(
            walk(7, None, &around),
            steps,
            "the same seed, the same walk"
        );
        let others: Vec<Vec<Id>> = (0..8)
            .map(|seed| {
                walk(seed, None, &around)
                    .iter()
                    .map(|s| s.item.id)
                    .collect()
            })
            .collect();
        assert!(
            others.iter().any(|o| o != &others[0]),
            "other seeds, other walks"
        );
    }

    #[test]
    fn a_step_says_why_it_came_next() {
        let near = |id: Id| {
            if id == Id(uuid::Uuid::from_u128(1)) {
                vec![(Id(uuid::Uuid::from_u128(4)), 0.9)]
            } else {
                Vec::new()
            }
        };
        let around = Surroundings::new().with_near(&near);
        let start = Some(Id(uuid::Uuid::from_u128(1)));
        let mut reasons = HashSet::new();
        for seed in 0..60 {
            let first = walk(seed, start, &around).remove(0);
            assert_ne!(
                first.item.id,
                start.unwrap(),
                "the start isn't visited again"
            );
            match first.reason {
                Reason::Linked {
                    ref link_type,
                    back,
                    ..
                } => {
                    assert_eq!(first.item.id, Id(uuid::Uuid::from_u128(5)));
                    assert_eq!((link_type.as_str(), back), ("related", false));
                    assert_eq!(
                        first.preview, None,
                        "a restricted item's text is never shown"
                    );
                    reasons.insert("linked");
                }
                Reason::Near { score, .. } => {
                    assert_eq!(first.item.id, Id(uuid::Uuid::from_u128(4)));
                    assert!((score - 0.9).abs() < f32::EPSILON);
                    reasons.insert("near");
                }
                Reason::Neighbour { ref collection } => {
                    assert_eq!(collection, "Letters");
                    reasons.insert("neighbour");
                }
                Reason::Jump { ref collection, .. } => {
                    assert_eq!(collection, "Work", "away from where it is");
                    reasons.insert("jump");
                }
                Reason::Start { .. } => panic!("it started from an item"),
            }
        }
        assert_eq!(reasons.len(), 4, "{reasons:?}");
    }

    #[test]
    fn a_jump_goes_where_this_device_has_not_been_in_a_while() {
        let mut visits = Visits::default();
        visits.open("Letters", date("2026-10-30"));
        let around = Surroundings::new().with_visits(&visits);
        for seed in 0..10 {
            let first = walk(seed, None, &around).remove(0);
            let Reason::Start { collection } = first.reason else {
                panic!("{:?}", first.reason)
            };
            assert_eq!(collection, "Work", "Letters was opened lately");
        }
    }

    #[test]
    fn previews_are_cut_at_a_word() {
        assert_eq!(preview("  a\n b  "), Some("a b".into()));
        assert_eq!(preview(""), None);
        let long = "word ".repeat(200);
        let cut = preview(&long).unwrap();
        assert!(cut.ends_with("word…") && cut.chars().count() <= PREVIEW_CHARS + 1);
    }
}
