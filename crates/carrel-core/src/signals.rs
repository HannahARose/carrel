//! Restructure signals (`docs/design/signals.md`): where the archive's structure shows signs of
//! not fitting what's in it. They're computed from the archive as it is, so they're never stale,
//! and they're information only until restructure proposals (v0.7) start from them.
//!
//! Two of them need the tag and link graph ([`crate::graph`]), and arrived with it in v0.7
//! (`docs/design/restructure.md`, "When one is proposed"): items in different branches often
//! linked or worked on together ([`Signal::Entangled`]), and sibling collections at very
//! different levels of detail ([`Signal::UnevenDetail`]).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::archive::{Archive, ArchiveError};
use crate::card::PlacementConfidence;
use crate::discover::Catalog;
use crate::graph::Graph;

/// The collection items go to when nowhere fits.
const HOLDING: &str = "Holding";

/// How many top-level collections a tag must appear in to suggest a missing group.
const SPREAD_TAG_COLLECTIONS: usize = 3;

/// How many times more items per collection one sibling must have than another for their
/// levels of detail to differ very much.
pub const DETAIL_RATIO: f64 = 4.0;

/// How finely a collection is divided: the items in it and below, and the collections they're
/// in, itself included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detail {
    /// The collection.
    pub collection: String,
    /// The items in it and every collection below it.
    pub items: usize,
    /// The collections those are in: itself and every collection below it.
    pub collections: usize,
}

impl Detail {
    /// Items per collection: the fewer, the finer it's divided.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // item counts
    pub fn per_collection(&self) -> f64 {
        self.items as f64 / self.collections.max(1) as f64
    }
}

/// A sign the structure may need changing.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "signal", rename_all = "kebab-case")]
pub enum Signal {
    /// Many items wait in Holding.
    Holding {
        /// How many.
        items: usize,
    },
    /// Many items in one collection were filed without routing being sure.
    Uncertain {
        /// The collection.
        collection: String,
        /// Items placed with low confidence.
        low: usize,
        /// Items placed with medium confidence.
        medium: usize,
    },
    /// A collection with no subcollections holds many loose items.
    Crowded {
        /// The collection.
        collection: String,
        /// How many items.
        items: usize,
    },
    /// A collection with no subcollections holds a single item.
    Single {
        /// The collection.
        collection: String,
    },
    /// A tag is used in several top-level collections, suggesting a group the structure doesn't
    /// capture.
    SpreadTag {
        /// The tag.
        tag: String,
        /// The top-level collections using it.
        collections: Vec<String>,
    },
    /// Items in two collections in different branches are often linked or worked on
    /// together, suggesting they belong nearer each other.
    Entangled {
        /// One collection.
        a: String,
        /// The other.
        b: String,
        /// How many links run between their items.
        links: usize,
        /// How many times an item in each was worked on the same day.
        days: usize,
    },
    /// Two sibling collections are divided at very different levels of detail: one into
    /// many small collections, the other hardly at all.
    UnevenDetail {
        /// The collection they're in; empty for the archive's top level.
        parent: String,
        /// The finely divided one.
        finer: Detail,
        /// The coarsely divided one.
        coarser: Detail,
    },
}

impl Signal {
    /// How strong the signal is, for putting the strongest first: roughly how many items it
    /// concerns.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // item counts
    pub fn weight(&self) -> f64 {
        match self {
            Self::Holding { items } | Self::Crowded { items, .. } => *items as f64,
            Self::Uncertain { low, medium, .. } => *low as f64 + *medium as f64 / 2.0,
            Self::Single { .. } => 1.0,
            Self::SpreadTag { collections, .. } => collections.len() as f64,
            Self::Entangled { links, days, .. } => (*links + *days) as f64,
            Self::UnevenDetail { coarser, .. } => coarser.items as f64,
        }
    }

    /// The signal in words, for `carrel status`.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Holding { items } => format!("Holding: {items} items waiting for a home"),
            Self::Uncertain {
                collection,
                low,
                medium,
            } => {
                let mut parts = Vec::new();
                if *low > 0 {
                    parts.push(format!("{low} placed with low confidence"));
                }
                if *medium > 0 {
                    parts.push(format!("{medium} with medium confidence"));
                }
                format!("{collection}: {}", parts.join(", "))
            }
            Self::Crowded { collection, items } => {
                format!("{collection}: {items} loose items and no subcollections")
            }
            Self::Single { collection } => format!("{collection}: only one item"),
            Self::SpreadTag { tag, collections } => {
                format!("Tag {tag} is used in {}", collections.join(", "))
            }
            Self::Entangled { a, b, links, days } => {
                let mut parts = Vec::new();
                if *links > 0 {
                    parts.push(format!("linked {links} times"));
                }
                if *days > 0 {
                    parts.push(format!("worked on together on {days} days"));
                }
                format!("{a} and {b}: items {}", parts.join(" and "))
            }
            Self::UnevenDetail { finer, coarser, .. } => format!(
                "{} and {} are divided very differently: {} items in {} collections, and {} in {}",
                finer.collection,
                coarser.collection,
                finer.items,
                finer.collections,
                coarser.items,
                coarser.collections
            ),
        }
    }
}

/// What a signal needs to know about one item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    /// Its tags.
    pub tags: Vec<String>,
    /// How sure filing was, if it wasn't.
    pub placement: Option<PlacementConfidence>,
}

/// The archive's signals, strongest first.
///
/// # Errors
///
/// [`ArchiveError`] if the collections or cards can't be read.
pub fn signals(archive: &Archive) -> Result<Vec<Signal>, ArchiveError> {
    let catalog = Catalog::read(archive)?;
    let contents: Vec<(String, Vec<Entry>)> = catalog
        .collections
        .iter()
        .map(|collection| {
            let entries = catalog
                .in_collection(&collection.path)
                .map(|e| Entry {
                    tags: e.tags.clone(),
                    placement: e.placement,
                })
                .collect();
            (collection.path.clone(), entries)
        })
        .collect();
    let warrant = archive.doc().min_collection_items.unwrap_or(5);
    let warrant = usize::try_from(warrant).unwrap_or(usize::MAX);
    let crowded = archive.settings().signals.crowded_items;
    let mut found = from_contents(
        &contents,
        warrant,
        usize::try_from(crowded).unwrap_or(usize::MAX),
    );
    found.extend(from_graph(&Graph::build(&catalog), warrant));
    found.sort_by(|a, b| b.weight().total_cmp(&a.weight()));
    Ok(found)
}

/// The signals the tag and link graph shows, given the least items worth a collection
/// (`warrant`): pairs of collections in different branches whose items are linked or worked
/// on together at least `warrant` times, and sibling collections whose items per collection
/// differ by [`DETAIL_RATIO`] or more, each with at least `warrant` items. Strongest first.
#[must_use]
pub fn from_graph(graph: &Graph, warrant: usize) -> Vec<Signal> {
    let mut found: Vec<Signal> = graph
        .connections()
        .into_iter()
        .filter(|c| c.separate_branches && c.a != HOLDING && c.b != HOLDING)
        .filter(|c| c.links + c.days >= warrant.max(1))
        .map(|c| Signal::Entangled {
            a: c.a,
            b: c.b,
            links: c.links,
            days: c.days,
        })
        .collect();
    found.extend(uneven_detail(graph, warrant));
    found.sort_by(|a, b| b.weight().total_cmp(&a.weight()));
    found
}

/// The parent of the collection at `path`: empty for a top-level one.
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(p, _)| p)
}

/// Sibling collections at very different levels of detail: for each parent, its most finely
/// and most coarsely divided children, if they differ by [`DETAIL_RATIO`].
fn uneven_detail(graph: &Graph, warrant: usize) -> Vec<Signal> {
    let detail = |path: &str| {
        let below: Vec<_> = graph
            .collections
            .iter()
            .filter(|c| crate::graph::is_within(&c.path, path))
            .collect();
        Detail {
            collection: path.to_owned(),
            items: below.iter().map(|c| c.items).sum(),
            collections: below.len(),
        }
    };
    let mut siblings: BTreeMap<&str, Vec<Detail>> = BTreeMap::new();
    for collection in &graph.collections {
        if collection.path == HOLDING {
            continue;
        }
        let d = detail(&collection.path);
        if d.items >= warrant {
            siblings
                .entry(parent(&collection.path))
                .or_default()
                .push(d);
        }
    }
    let mut found = Vec::new();
    for (parent, details) in siblings {
        let finest = details
            .iter()
            .filter(|d| d.collections > 1)
            .min_by(|a, b| a.per_collection().total_cmp(&b.per_collection()));
        let coarsest = details
            .iter()
            .max_by(|a, b| a.per_collection().total_cmp(&b.per_collection()));
        if let (Some(finer), Some(coarser)) = (finest, coarsest)
            && coarser.per_collection() >= DETAIL_RATIO * finer.per_collection()
        {
            found.push(Signal::UnevenDetail {
                parent: parent.to_owned(),
                finer: finer.clone(),
                coarser: coarser.clone(),
            });
        }
    }
    found
}

/// The signals for collections with these items, given the least items worth a collection
/// (`warrant`, FORMAT.md §6.2) and how many make a collection crowded. Strongest first.
#[must_use]
#[allow(clippy::cast_precision_loss)] // item counts
pub fn from_contents(
    contents: &[(String, Vec<Entry>)],
    warrant: usize,
    crowded: usize,
) -> Vec<Signal> {
    let has_children = |path: &str| {
        let prefix = format!("{path}/");
        contents.iter().any(|(p, _)| p.starts_with(&prefix))
    };
    let mut found = Vec::new();
    let mut tag_places: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (path, entries) in contents {
        if path == HOLDING {
            if entries.len() >= warrant {
                found.push(Signal::Holding {
                    items: entries.len(),
                });
            }
            continue;
        }
        let low = entries
            .iter()
            .filter(|e| e.placement == Some(PlacementConfidence::Low))
            .count();
        let medium = entries
            .iter()
            .filter(|e| e.placement == Some(PlacementConfidence::Medium))
            .count();
        if low as f64 + medium as f64 / 2.0 >= warrant as f64 {
            found.push(Signal::Uncertain {
                collection: path.clone(),
                low,
                medium,
            });
        }
        if !has_children(path) {
            if entries.len() > crowded {
                found.push(Signal::Crowded {
                    collection: path.clone(),
                    items: entries.len(),
                });
            } else if entries.len() == 1 {
                found.push(Signal::Single {
                    collection: path.clone(),
                });
            }
        }
        let top = path.split('/').next().unwrap_or(path);
        for entry in entries {
            for tag in &entry.tags {
                tag_places.entry(tag).or_default().insert(top);
            }
        }
    }
    for (tag, places) in tag_places {
        if places.len() >= SPREAD_TAG_COLLECTIONS {
            found.push(Signal::SpreadTag {
                tag: tag.to_owned(),
                collections: places.into_iter().map(str::to_owned).collect(),
            });
        }
    }
    found.sort_by(|a, b| b.weight().total_cmp(&a.weight()));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(n: usize) -> Vec<Entry> {
        vec![Entry::default(); n]
    }

    fn placed(low: usize, medium: usize, rest: usize) -> Vec<Entry> {
        let mut entries = vec![
            Entry {
                tags: Vec::new(),
                placement: Some(PlacementConfidence::Low),
            };
            low
        ];
        entries.extend(vec![
            Entry {
                tags: Vec::new(),
                placement: Some(PlacementConfidence::Medium),
            };
            medium
        ]);
        entries.extend(items(rest));
        entries
    }

    fn tagged(tag: &str) -> Entry {
        Entry {
            tags: vec![tag.to_owned()],
            placement: None,
        }
    }

    #[test]
    fn a_tidy_archive_has_no_signals() {
        let contents = vec![
            ("Holding".to_owned(), items(2)),
            ("Reading".to_owned(), placed(2, 2, 10)),
            ("Projects".to_owned(), items(4)),
            ("Projects/Hardware".to_owned(), items(3)),
        ];
        assert_eq!(from_contents(&contents, 5, 30), []);
    }

    #[test]
    fn signals_appear_past_their_thresholds_strongest_first() {
        let contents = vec![
            ("Holding".to_owned(), items(12)),
            // 4 low and 2 medium count as 5: enough for a pattern.
            ("Receipts".to_owned(), placed(4, 2, 3)),
            ("Reading".to_owned(), items(31)),
            ("Recipes".to_owned(), items(1)),
            // A parent with one item of its own isn't single: it has a subcollection.
            ("Projects".to_owned(), items(1)),
            ("Projects/Hardware".to_owned(), items(4)),
        ];
        let found = from_contents(&contents, 5, 30);
        assert_eq!(
            found,
            [
                Signal::Crowded {
                    collection: "Reading".into(),
                    items: 31
                },
                Signal::Holding { items: 12 },
                Signal::Uncertain {
                    collection: "Receipts".into(),
                    low: 4,
                    medium: 2
                },
                Signal::Single {
                    collection: "Recipes".into()
                },
            ]
        );
        assert_eq!(
            found[2].describe(),
            "Receipts: 4 placed with low confidence, 2 with medium confidence"
        );
    }

    #[test]
    fn a_tag_across_three_top_level_collections_suggests_a_group() {
        let contents = vec![
            ("Work".to_owned(), vec![tagged("tax"), tagged("travel")]),
            ("Finances".to_owned(), vec![tagged("tax"), Entry::default()]),
            ("Home".to_owned(), vec![Entry::default()]),
            (
                "Home/Office".to_owned(),
                vec![tagged("tax"), tagged("travel")],
            ),
        ];
        let found = from_contents(&contents, 5, 30);
        assert!(found.contains(&Signal::SpreadTag {
            tag: "tax".into(),
            collections: vec!["Finances".into(), "Home".into(), "Work".into()],
        }));
        assert!(
            !found
                .iter()
                .any(|s| matches!(s, Signal::SpreadTag { tag, .. } if tag == "travel")),
            "two places isn't a pattern"
        );
    }

    fn graph_of(entries: Vec<crate::discover::Entry>, paths: &[&str]) -> Graph {
        use crate::discover::tests::collection;
        Graph::build(&Catalog::new(
            entries,
            paths.iter().map(|p| collection(p)).collect(),
        ))
    }

    #[test]
    fn items_linked_and_worked_on_across_branches_are_entangled() {
        use crate::discover::tests::{date, entry};
        let id = |n: u128| crate::card::Id(uuid::Uuid::from_u128(n));
        let mut entries = Vec::new();
        for n in 1..=3_u128 {
            let mut code = entry(n, &format!("code{n}"), "Projects/Sim");
            code.links = vec![("cites".into(), id(n + 10))];
            code.sessions = vec![(date("2026-03-01"), 1)];
            entries.push(code);
            let mut paper = entry(n + 10, &format!("paper{n}"), "Reading");
            paper.sessions = vec![(date("2026-03-01"), 1)];
            entries.push(paper);
        }
        // Linked within one branch: not a sign of anything.
        let mut parent = entry(30, "overview", "Projects");
        parent.links = (1..=3).map(|n| ("about".to_owned(), id(n))).collect();
        entries.push(parent);
        let graph = graph_of(entries, &["Projects", "Projects/Sim", "Reading"]);
        let found = from_graph(&graph, 5);
        let entangled: Vec<&Signal> = found
            .iter()
            .filter(|s| matches!(s, Signal::Entangled { .. }))
            .collect();
        assert_eq!(entangled.len(), 1);
        let Signal::Entangled { a, b, links, days } = entangled[0] else {
            unreachable!()
        };
        assert_eq!((a.as_str(), b.as_str()), ("Projects/Sim", "Reading"));
        assert_eq!(*links, 3);
        // Six items on one day: each code item with each paper, nine pairs.
        assert_eq!(*days, 9);
        assert!(
            entangled[0]
                .describe()
                .starts_with("Projects/Sim and Reading: items linked 3 times")
        );
        assert!(
            from_graph(&graph, 13)
                .iter()
                .all(|s| !matches!(s, Signal::Entangled { .. }))
        );
    }

    #[test]
    fn siblings_divided_very_differently_are_uneven() {
        use crate::discover::tests::entry;
        let mut entries = Vec::new();
        let mut n = 0_u128;
        let mut add = |collection: &str, count: usize| {
            for _ in 0..count {
                n += 1;
                entries.push(entry(n, &format!("i{n}"), collection));
            }
        };
        add("Work", 40);
        for sub in ["Home/A", "Home/B", "Home/C", "Home/D", "Home/E"] {
            add(sub, 2);
        }
        add("Home", 2);
        add("Holding", 50);
        let graph = graph_of(
            entries,
            &[
                "Work", "Home", "Home/A", "Home/B", "Home/C", "Home/D", "Home/E", "Holding",
            ],
        );
        let found = from_graph(&graph, 5);
        assert_eq!(
            found,
            [Signal::UnevenDetail {
                parent: String::new(),
                finer: Detail {
                    collection: "Home".into(),
                    items: 12,
                    collections: 6
                },
                coarser: Detail {
                    collection: "Work".into(),
                    items: 40,
                    collections: 1
                },
            }]
        );
        assert_eq!(
            found[0].describe(),
            "Home and Work are divided very differently: 12 items in 6 collections, and 40 in 1"
        );
    }

    #[test]
    fn the_sample_archive_has_signals_from_the_archive() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive");
        let archive = Archive::open(&root).unwrap();
        let found = signals(&archive).unwrap();
        for pair in found.windows(2) {
            assert!(pair[0].weight() >= pair[1].weight());
        }
    }
}
