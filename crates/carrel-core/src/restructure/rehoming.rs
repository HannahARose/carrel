//! Rehoming: putting items back in collections that already exist (`docs/design/restructure.md`,
//! "What it can change": moving items between collections). Grouping finds what belongs
//! together, but an item scattered on its own, or a group whose collection is already there
//! (perhaps emptied), is better moved than given a new collection.
//!
//! Two kinds of evidence say where an item belongs:
//!
//! - **The numbers:** how similar it is to each collection's items ([`ItemFit`]). On their own
//!   they move an item only when it's far more like another collection's items than its own:
//!   by at least [`GroupingOptions::threshold`], and that much alike there.
//! - **The crawler's verdicts** (`docs/design/restructure.md`, "Crawlers": "its verdicts feed
//!   the signals"): an item the crawler judged not to fit where it still is, and where it would
//!   go instead. A verdict is enough when the numbers don't say otherwise: the item is at least
//!   as like its new collection's items as its own, or there's nothing to compare. A move the
//!   owner declined isn't proposed again.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::Invasiveness;
use super::grouping::{CandidateGroup, GroupSource, GroupingOptions, ItemFit};
use crate::archive::{Archive, Collection};
use crate::card::Id;
use crate::crawl::findings::Finding;
use crate::graph::{Graph, is_within};

/// The collection items go to when nowhere fits.
const HOLDING: &str = "Holding";

/// The crawler's verdict that an item belongs elsewhere.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CrawlerMove {
    /// The collection it would go to.
    pub to: String,
    /// How sure the crawler was, from 0 to 1.
    pub confidence: f64,
}

/// What the crawler's verdicts say about the items still where it judged them.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct CrawlerSignals {
    /// Items it judged fit where they are, which outweighs the numbers saying they don't.
    pub fitting: HashSet<Id>,
    /// Items it judged don't fit where they are.
    pub misfits: HashSet<Id>,
    /// Of those, the ones it would move to another existing collection, Holding aside, unless
    /// the owner declined that move.
    pub moves: HashMap<Id, CrawlerMove>,
}

impl CrawlerSignals {
    /// Sets aside where the crawler would put items when the numbers in `fits` say otherwise:
    /// the item is less like the items there than those where it is. That it doesn't fit where
    /// it is still counts.
    pub(super) fn set_aside_where_the_numbers_disagree(&mut self, fits: &[ItemFit]) {
        for fit in fits {
            let disagree = self.moves.get(&fit.id).is_some_and(|verdict| {
                matches!((fit.to(&verdict.to), fit.own), (Some(there), Some(own)) if there < own)
            });
            if disagree {
                self.moves.remove(&fit.id);
            }
        }
    }
}

/// The crawler's verdicts on the graph's items, as signals: only those given where the item
/// still is count.
pub(super) fn crawler_signals(
    archive: &Archive,
    graph: &Graph,
    collections: &[Collection],
) -> CrawlerSignals {
    let verdicts = crate::crawl::verdicts::all(archive);
    let exists =
        |path: &str| collections.iter().any(|c| c.path == path) && !is_within(path, HOLDING);
    let mut signals = CrawlerSignals::default();
    for item in graph.items.iter().filter(|i| !i.checked_out) {
        let Some(verdict) = verdicts.get(&item.id) else {
            continue;
        };
        if verdict.collection != item.collection {
            continue;
        }
        if verdict.fits {
            signals.fitting.insert(item.id);
            continue;
        }
        signals.misfits.insert(item.id);
        let Some(to) = verdict.better.as_deref() else {
            continue;
        };
        let declined = verdict
            .declined
            .iter()
            .any(|f| matches!(f, Finding::Move { to: t, .. } if t == to));
        if to != item.collection && exists(to) && !declined {
            signals.moves.insert(
                item.id,
                CrawlerMove {
                    to: to.to_owned(),
                    confidence: verdict.confidence.clamp(0.0, 1.0),
                },
            );
        }
    }
    signals
}

/// A single item worth moving into another existing collection.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ItemMove {
    /// The item, as a group of one.
    pub group: CandidateGroup,
    /// The collection it would go to.
    pub to: String,
    /// How much more similar it is to the items there than to those where it is, from 0 to 1;
    /// 0 when there's nothing to compare.
    pub improvement: f64,
}

/// The items in `fits` worth moving on their own: by the numbers, unless the crawler judged
/// they fit where they are (`fitting`), or by the crawler's verdict (`crawler`, already without
/// those the numbers disagree with: [`CrawlerSignals::set_aside_where_the_numbers_disagree`]). Items in Holding are filing's to place, and items in `grouped` are already moved
/// with a group.
pub(super) fn item_moves(
    fits: &[ItemFit],
    crawler: &HashMap<Id, CrawlerMove>,
    fitting: &HashSet<Id>,
    grouped: &HashSet<Id>,
    options: &GroupingOptions,
) -> Vec<ItemMove> {
    let threshold = options.threshold;
    fits.iter()
        .filter(|fit| !is_within(&fit.collection, HOLDING) && !grouped.contains(&fit.id))
        .filter_map(|fit| {
            let own = fit.own.unwrap_or(0.0);
            let by_numbers = fit
                .others
                .first()
                .filter(|_| !fitting.contains(&fit.id))
                .filter(|(_, there)| *there >= threshold && there - own >= threshold)
                .map(|(to, _)| to.clone());
            let by_crawler = crawler.get(&fit.id).map(|verdict| verdict.to.clone());
            let to = by_crawler.or(by_numbers)?;
            let improvement = fit
                .to(&to)
                .map_or(0.0, |there| (there - own).clamp(0.0, 1.0));
            Some(ItemMove {
                group: CandidateGroup {
                    members: vec![fit.id],
                    cohesion: fit.to(&to).unwrap_or(0.0),
                    separation: 0.0,
                    misfits: if own < threshold {
                        vec![fit.id]
                    } else {
                        Vec::new()
                    },
                    nearest: fit.to(&to).map(|there| (to.clone(), there)),
                    tags: Vec::new(),
                    sources: vec![GroupSource {
                        collection: fit.collection.clone(),
                        items: 1,
                    }],
                    home: to.clone(),
                    invasiveness: Invasiveness::BetweenBranches,
                },
                to,
                improvement,
            })
        })
        .collect()
}

/// The existing collection `group` belongs in, if any, with the members that would move
/// there: the collection most of it comes from, when that's more than half the group and more
/// than half the collection's items (the group is that collection, with a few from elsewhere);
/// where more than half its members' crawler verdicts say they belong; or, by the numbers, the collection they're as similar to on average as grouping asks of a group
/// ([`CandidateGroup::nearest`]). Either way, a collection with items the group is less similar
/// to than that is no home for it: a group that alike to itself is a collection of its own.
/// Only members with reason to leave where they are move: those the crawler sends there,
/// those that don't fit where they are (unless the crawler judged they do), those in Holding,
/// and those `doubtful`: a placement filing wasn't sure of, or one the crawler judged doesn't
/// fit.
/// `exists` says whether a collection may take them.
pub(super) fn existing_home(
    group: &CandidateGroup,
    collection_of: &HashMap<Id, &str>,
    crawler: &CrawlerSignals,
    doubtful: &dyn Fn(&Id) -> bool,
    exists: &dyn Fn(&str) -> bool,
    options: &GroupingOptions,
) -> Option<(String, Vec<Id>)> {
    let has_items = |to: &str| collection_of.values().any(|c| *c == to);
    let members = group.members.len();
    if let [main, _, ..] = group.sources.as_slice() {
        let all = collection_of
            .values()
            .filter(|c| **c == main.collection)
            .count();
        if main.items * 2 > members && main.items * 2 > all && exists(&main.collection) {
            let to = main.collection.clone();
            let moving = with_reason(group, &to, collection_of, crawler, doubtful);
            return (!moving.is_empty()).then_some((to, moving));
        }
    }
    let mut votes: BTreeMap<&str, usize> = BTreeMap::new();
    for id in &group.members {
        if let Some(verdict) = crawler.moves.get(id) {
            *votes.entry(verdict.to.as_str()).or_default() += 1;
        }
    }
    let sent_to = votes
        .into_iter()
        .filter(|(_, n)| n * 2 > group.members.len())
        .max_by_key(|(_, n)| *n)
        .map(|(to, _)| to.to_owned());
    // The nearest collection is the most alike of those with items: any other has items the
    // group is less alike to, and one that isn't there has none.
    let alike = |to: &str| match &group.nearest {
        Some((nearest, likeness)) if nearest == to => *likeness >= options.threshold,
        _ => !has_items(to),
    };
    let near = group.nearest.as_ref().map(|(to, _)| to.clone());
    let to = sent_to.or(near).filter(|to| {
        exists(to) && alike(to) && !group.sources.iter().any(|s| s.collection == *to)
    })?;
    let moving = with_reason(group, &to, collection_of, crawler, doubtful);
    (!moving.is_empty()).then_some((to, moving))
}

/// The members of `group` not in `to` with reason to leave where they are; see
/// [`existing_home`].
fn with_reason(
    group: &CandidateGroup,
    to: &str,
    collection_of: &HashMap<Id, &str>,
    crawler: &CrawlerSignals,
    doubtful: &dyn Fn(&Id) -> bool,
) -> Vec<Id> {
    group
        .members
        .iter()
        .copied()
        .filter(|id| collection_of.get(id).is_some_and(|c| *c != to))
        .filter(|id| {
            crawler.moves.get(id).is_some_and(|v| v.to == to)
                || (group.misfits.contains(id) && !crawler.fitting.contains(id))
                || doubtful(id)
                || collection_of.get(id).is_some_and(|c| is_within(c, HOLDING))
        })
        .collect()
}

/// `group` narrowed to `members`, going into the existing collection `to`.
pub(super) fn narrowed(
    group: &CandidateGroup,
    members: Vec<Id>,
    to: &str,
    collection_of: &HashMap<Id, &str>,
) -> CandidateGroup {
    let mut sources: BTreeMap<&str, usize> = BTreeMap::new();
    for id in &members {
        if let Some(c) = collection_of.get(id) {
            *sources.entry(c).or_default() += 1;
        }
    }
    let mut sources: Vec<GroupSource> = sources
        .into_iter()
        .map(|(collection, items)| GroupSource {
            collection: collection.to_owned(),
            items,
        })
        .collect();
    sources.sort_by(|a, b| b.items.cmp(&a.items).then(a.collection.cmp(&b.collection)));
    let filing = sources.iter().all(|s| is_within(&s.collection, HOLDING));
    CandidateGroup {
        misfits: group
            .misfits
            .iter()
            .copied()
            .filter(|id| members.contains(id))
            .collect(),
        members,
        sources,
        home: to.to_owned(),
        invasiveness: if filing {
            Invasiveness::Subdividing
        } else {
            Invasiveness::BetweenBranches
        },
        ..group.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Id {
        Id(uuid::Uuid::from_u128(n))
    }

    fn fit(n: u128, collection: &str, own: Option<f64>, others: &[(&str, f64)]) -> ItemFit {
        ItemFit {
            id: id(n),
            collection: collection.to_owned(),
            own,
            others: others.iter().map(|(c, l)| ((*c).to_owned(), *l)).collect(),
        }
    }

    fn sent(to: &str) -> CrawlerMove {
        CrawlerMove {
            to: to.to_owned(),
            confidence: 0.9,
        }
    }

    fn moved(moves: &[ItemMove]) -> Vec<(Id, &str)> {
        moves
            .iter()
            .map(|m| (m.group.members[0], m.to.as_str()))
            .collect()
    }

    #[test]
    fn the_numbers_alone_move_an_item_only_when_its_far_more_like_another_collection() {
        let fits = [
            fit(1, "Reading", Some(0.1), &[("Receipts", 0.9)]),
            fit(2, "Reading", Some(0.3), &[("Letters", 0.6)]),
            fit(3, "Holding", None, &[("Receipts", 0.9)]),
            fit(4, "Reading", Some(0.1), &[("Receipts", 0.9)]),
        ];
        let options = GroupingOptions::default();
        let grouped = HashSet::from([id(4)]);
        let moves = item_moves(&fits, &HashMap::new(), &HashSet::new(), &grouped, &options);
        // The second is more like Letters, but not by enough; Holding's are filing's to place;
        // the fourth moves with its group.
        assert_eq!(moved(&moves), [(id(1), "Receipts")]);
        assert!((moves[0].improvement - 0.8).abs() < 1e-9);
        assert_eq!(moves[0].group.misfits, [id(1)]);
        assert_eq!(moves[0].group.invasiveness, Invasiveness::BetweenBranches);
        // The crawler judging it fits where it is outweighs the numbers.
        let fitting = HashSet::from([id(1)]);
        assert_eq!(
            item_moves(&fits, &HashMap::new(), &fitting, &grouped, &options),
            Vec::<ItemMove>::new()
        );
    }

    #[test]
    fn the_crawlers_verdict_moves_an_item_unless_the_numbers_say_otherwise() {
        let fits = [
            fit(1, "Letters", Some(0.3), &[("Health", 0.4)]),
            fit(2, "Work", Some(0.6), &[("Finances", 0.2)]),
            fit(3, "Home", Some(0.4), &[]),
        ];
        let mut signals = CrawlerSignals {
            moves: HashMap::from([
                (id(1), sent("Health")),
                (id(2), sent("Finances")),
                (id(3), sent("Manuals")),
            ]),
            misfits: HashSet::from([id(1), id(2), id(3)]),
            ..CrawlerSignals::default()
        };
        signals.set_aside_where_the_numbers_disagree(&fits);
        assert!(
            signals.misfits.contains(&id(2)),
            "it still doesn't fit Work"
        );
        let moves = item_moves(
            &fits,
            &signals.moves,
            &HashSet::new(),
            &HashSet::new(),
            &GroupingOptions::default(),
        );
        // The second is more like Work's items than Finances's; the third goes to a collection
        // with nothing to compare.
        assert_eq!(moved(&moves), [(id(1), "Health"), (id(3), "Manuals")]);
        assert!(moves[1].improvement.abs() < f64::EPSILON);
    }

    /// A group of `members` (by number) from `sources`, most like `nearest`.
    fn group(
        members: &[u128],
        sources: &[(&str, usize)],
        nearest: Option<(&str, f64)>,
        misfits: &[u128],
    ) -> CandidateGroup {
        CandidateGroup {
            members: members.iter().map(|n| id(*n)).collect(),
            cohesion: 0.9,
            separation: 0.6,
            misfits: misfits.iter().map(|n| id(*n)).collect(),
            nearest: nearest.map(|(c, l)| (c.to_owned(), l)),
            tags: Vec::new(),
            sources: sources
                .iter()
                .map(|(c, n)| GroupSource {
                    collection: (*c).to_owned(),
                    items: *n,
                })
                .collect(),
            home: "Home".into(),
            invasiveness: Invasiveness::Subdividing,
        }
    }

    #[test]
    fn a_group_goes_back_to_the_collection_the_crawler_sends_most_of_it_to() {
        // Five manuals in Home, and Projects with items of its own; Manuals is empty.
        let mut collection_of: HashMap<Id, &str> = (1..=5).map(|n| (id(n), "Home")).collect();
        collection_of.insert(id(10), "Projects");
        let exists = |c: &str| ["Home", "Manuals", "Projects"].contains(&c);
        let options = GroupingOptions::default();
        let manuals = group(
            &[1, 2, 3, 4, 5],
            &[("Home", 5)],
            Some(("Projects", 0.2)),
            &[],
        );
        let mut crawler = CrawlerSignals::default();
        for n in 1..=3 {
            crawler.moves.insert(id(n), sent("Manuals"));
        }
        crawler.misfits.insert(id(4));
        let found = existing_home(
            &manuals,
            &collection_of,
            &crawler,
            &|_| false,
            &exists,
            &options,
        );
        // The fifth has no reason to leave Home, so it stays.
        assert_eq!(found, Some(("Manuals".into(), vec![id(1), id(2), id(3)])));
        let doubtful = |i: &Id| crawler.misfits.contains(i);
        let found = existing_home(
            &manuals,
            &collection_of,
            &crawler,
            &doubtful,
            &exists,
            &options,
        );
        assert_eq!(found.unwrap().1.len(), 4);

        // Sent to Projects, whose items the group is unlike, it's a collection of its own.
        let mut to_projects = CrawlerSignals::default();
        for n in 1..=3 {
            to_projects.moves.insert(id(n), sent("Projects"));
        }
        assert_eq!(
            existing_home(
                &manuals,
                &collection_of,
                &to_projects,
                &|_| true,
                &exists,
                &options
            ),
            None
        );
    }

    #[test]
    fn a_group_thats_most_of_a_collection_takes_in_the_rest_with_reason_to_move() {
        // Four of Insurance's five items, with one from Travel and one from Health.
        let mut collection_of: HashMap<Id, &str> = (1..=5).map(|n| (id(n), "Insurance")).collect();
        collection_of.insert(id(6), "Travel");
        collection_of.insert(id(7), "Health");
        let policies = group(
            &[1, 2, 3, 4, 6, 7],
            &[("Insurance", 4), ("Health", 1), ("Travel", 1)],
            None,
            &[6],
        );
        let found = existing_home(
            &policies,
            &collection_of,
            &CrawlerSignals::default(),
            &|_| false,
            &|_| true,
            &GroupingOptions::default(),
        );
        // The one from Health is like them but fits where it is.
        assert_eq!(found, Some(("Insurance".into(), vec![id(6)])));
        let narrowed = narrowed(&policies, vec![id(6)], "Insurance", &collection_of);
        assert_eq!(narrowed.sources.len(), 1);
        assert_eq!(narrowed.home, "Insurance");
        assert_eq!(narrowed.invasiveness, Invasiveness::BetweenBranches);
    }
}
