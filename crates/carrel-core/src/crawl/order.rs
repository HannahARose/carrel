//! The order the crawler takes items in (`docs/design/restructure.md`, "Crawlers"): what the
//! owner flagged as needing work first (FORMAT.md §5.9), items and the items of collections,
//! then Holding, then items filed with low confidence, then the items of crowded collections, then a
//! slow sweep of everything. Within each, items never judged come first, then those judged
//! longest ago. An item judged recently waits until it's due again, sooner the earlier its
//! stage; one with a finding waiting in Review waits for the owner.

use std::collections::{BTreeMap, HashSet};

use serde::Serialize;

use super::verdicts::Verdict;
use crate::archive::{Archive, ArchiveError};
use crate::card::{Id, Kind, PlacementConfidence, Sensitivity};
use crate::discover::Catalog;

/// Where an item is in the crawler's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    /// The owner flagged it, or its collection, as needing work.
    Flagged,
    /// It waits in Holding.
    Holding,
    /// It was filed with low confidence.
    Uncertain,
    /// It's in a crowded collection.
    Crowded,
    /// Everything else, slowly.
    Sweep,
}

impl Stage {
    /// The stage, for people.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Flagged => "flagged as needing work",
            Self::Holding => "in Holding",
            Self::Uncertain => "filed with low confidence",
            Self::Crowded => "in crowded collections",
            Self::Sweep => "in the sweep of everything",
        }
    }

    /// How long after it's judged an item in this stage is due again.
    #[must_use]
    pub fn again(self) -> jiff::SignedDuration {
        let days = match self {
            Self::Flagged | Self::Holding => 7,
            Self::Uncertain | Self::Crowded => 30,
            Self::Sweep => 180,
        };
        jiff::SignedDuration::from_hours(24 * days)
    }
}

/// What the order needs to know about one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its identifier.
    pub id: Id,
    /// Its card, relative to the archive root.
    pub card: String,
    /// Its collection, by path.
    pub collection: String,
    /// How sure filing was, if it wasn't.
    pub placement: Option<PlacementConfidence>,
    /// Whether the owner flagged it, or its collection, as needing work.
    pub flagged: bool,
}

/// The items the crawler may judge in `archive`: every item but restricted ones, whose text it
/// never reads, checked-out ones, which wait until they're returned, and removed ones.
///
/// # Errors
///
/// [`ArchiveError`] if the cards can't be read.
pub fn entries(archive: &Archive) -> Result<Vec<Entry>, ArchiveError> {
    let catalog = Catalog::read(archive)?;
    let flagged: HashSet<&str> = catalog
        .collections
        .iter()
        .filter(|c| c.flag.is_some())
        .map(|c| c.path.as_str())
        .collect();
    Ok(catalog
        .entries
        .iter()
        .filter(|e| {
            e.item.sensitivity != Sensitivity::Restricted
                && !e.checked_out
                && e.item.kind != Kind::Tombstone
        })
        .map(|e| Entry {
            id: e.item.id,
            card: e.item.card.clone(),
            collection: e.item.collection.clone(),
            placement: e.placement,
            flagged: e.flagged || flagged.contains(e.item.collection.as_str()),
        })
        .collect())
}

/// The collections the signals call crowded.
///
/// # Errors
///
/// [`ArchiveError`] if the archive can't be read.
pub fn crowded(archive: &Archive) -> Result<HashSet<String>, ArchiveError> {
    Ok(crate::signals::signals(archive)?
        .into_iter()
        .filter_map(|s| match s {
            crate::signals::Signal::Crowded { collection, .. } => Some(collection),
            _ => None,
        })
        .collect())
}

/// The stage `entry` is in, given the `crowded` collections.
fn stage_of<S: std::hash::BuildHasher>(entry: &Entry, crowded: &HashSet<String, S>) -> Stage {
    if entry.flagged {
        Stage::Flagged
    } else if entry.collection == "Holding" {
        Stage::Holding
    } else if entry.placement == Some(PlacementConfidence::Low) {
        Stage::Uncertain
    } else if crowded.contains(&entry.collection) {
        Stage::Crowded
    } else {
        Stage::Sweep
    }
}

/// The items due to be judged at `now`, in the order the crawler takes them, each with its
/// stage. `judged` are the verdicts so far, `open` the items with a finding waiting.
#[must_use]
pub fn order_of<S: std::hash::BuildHasher, T: std::hash::BuildHasher>(
    entries: &[Entry],
    crowded: &HashSet<String, S>,
    judged: &BTreeMap<Id, Verdict>,
    open: &HashSet<Id, T>,
    now: jiff::Timestamp,
) -> Vec<(Id, Stage)> {
    let mut due: Vec<(Stage, Option<jiff::Timestamp>, &str, Id)> = entries
        .iter()
        .filter(|e| !open.contains(&e.id))
        .filter_map(|e| {
            let stage = stage_of(e, crowded);
            let last = judged.get(&e.id).and_then(Verdict::at);
            // An item that moved since it was judged is judged again where it is now.
            let moved = judged
                .get(&e.id)
                .is_some_and(|v| v.collection != e.collection);
            let waiting = last.is_some_and(|at| at + stage.again() > now) && !moved;
            (!waiting).then_some((stage, last.filter(|_| !moved), e.card.as_str(), e.id))
        })
        .collect();
    // `None` sorts before any time: never judged first.
    due.sort();
    due.into_iter()
        .map(|(stage, _, _, id)| (id, stage))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::Extra;

    fn id(n: u128) -> Id {
        Id(uuid::Uuid::from_u128(n))
    }

    fn entry(n: u128, collection: &str, placement: Option<PlacementConfidence>) -> Entry {
        Entry {
            id: id(n),
            card: format!("{collection}/item-{n}.md"),
            collection: collection.into(),
            placement,
            flagged: false,
        }
    }

    fn judged(collection: &str, at: &str) -> Verdict {
        Verdict {
            collection: collection.into(),
            fits: true,
            better: None,
            confidence: 0.8,
            reason: String::new(),
            model: "m/m".into(),
            judged: at.into(),
            declined: Vec::new(),
            extra: Extra::new(),
        }
    }

    #[test]
    fn holding_then_low_confidence_then_crowded_then_the_rest() {
        let entries = [
            entry(1, "Home", None),
            entry(2, "Receipts", None),
            entry(3, "Work", Some(PlacementConfidence::Low)),
            entry(4, "Holding", None),
            entry(5, "Work", Some(PlacementConfidence::Medium)),
            entry(6, "Holding", None),
        ];
        let crowded = HashSet::from(["Receipts".to_owned()]);
        let now: jiff::Timestamp = "2026-11-01T12:00:00Z".parse().unwrap();
        let mut verdicts = BTreeMap::new();
        // Item 4 was judged a while ago, so item 6, never judged, comes first.
        verdicts.insert(id(4), judged("Holding", "2026-10-01T12:00:00Z"));
        let order = order_of(&entries, &crowded, &verdicts, &HashSet::new(), now);
        let ids: Vec<(u128, Stage)> = order.iter().map(|(i, s)| (i.0.as_u128(), *s)).collect();
        assert_eq!(
            ids,
            [
                (6, Stage::Holding),
                (4, Stage::Holding),
                (3, Stage::Uncertain),
                (2, Stage::Crowded),
                (1, Stage::Sweep),
                (5, Stage::Sweep),
            ]
        );
    }

    #[test]
    fn recently_judged_items_and_those_awaiting_the_owner_wait() {
        let entries = [
            entry(1, "Holding", None),
            entry(2, "Home", None),
            entry(3, "Home", None),
            entry(4, "Work", None),
        ];
        let now: jiff::Timestamp = "2026-11-01T12:00:00Z".parse().unwrap();
        let mut verdicts = BTreeMap::new();
        // Holding is judged again after a week; the sweep after half a year.
        verdicts.insert(id(1), judged("Holding", "2026-10-30T12:00:00Z"));
        verdicts.insert(id(2), judged("Home", "2026-09-01T12:00:00Z"));
        // Item 4 moved since it was judged, so it's due where it is now.
        verdicts.insert(id(4), judged("Home", "2026-10-31T12:00:00Z"));
        let open = HashSet::from([id(3)]);
        let order = order_of(&entries, &HashSet::new(), &verdicts, &open, now);
        let ids: Vec<u128> = order.iter().map(|(i, _)| i.0.as_u128()).collect();
        assert_eq!(ids, [4]);
    }

    #[test]
    fn flagged_items_come_before_holding() {
        let mut flagged = entry(3, "Work", None);
        flagged.flagged = true;
        let entries = [entry(1, "Holding", None), entry(2, "Home", None), flagged];
        let now: jiff::Timestamp = "2026-11-01T12:00:00Z".parse().unwrap();
        let order = order_of(
            &entries,
            &HashSet::new(),
            &BTreeMap::new(),
            &HashSet::new(),
            now,
        );
        let ids: Vec<(u128, Stage)> = order.iter().map(|(i, s)| (i.0.as_u128(), *s)).collect();
        assert_eq!(
            ids,
            [(3, Stage::Flagged), (1, Stage::Holding), (2, Stage::Sweep)]
        );
    }
}
