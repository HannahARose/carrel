//! The tag and link graph (`docs/design/restructure.md`, "The tag and link graph"): the
//! archive's items and collections as nodes, and what ties items together as weighted edges:
//! the tags they share, the links between them, and the days they were worked on together.
//!
//! It's built once, from a [`Catalog`], as plain data. Restructuring groups items with it
//! ([`crate::restructure::grouping`]), the Structure signals find branches tied together with
//! it ([`crate::signals`]), and `carrel discover graph` lists its strongest connections
//! between collections ([`Graph::connections`]).
//!
//! ```
//! use carrel_core::archive::{Archive, InitOptions};
//! use carrel_core::discover::Catalog;
//! use carrel_core::graph::Graph;
//!
//! let dir = tempfile::tempdir()?;
//! let archive = Archive::init(&dir.path().join("Archive"), &InitOptions::new("My archive"))?;
//! let graph = Graph::build(&Catalog::read(&archive)?);
//! assert!(graph.edges.is_empty());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::{BTreeMap, HashMap};

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::card::{Id, Sensitivity};
use crate::discover::Catalog;

/// How much one link between two items weighs.
pub const LINK_WEIGHT: f64 = 1.0;

/// How much one day two items were both worked on weighs, when only they were.
pub const SESSION_WEIGHT: f64 = 0.5;

/// A tag on more items than this says too little about any two of them to tie them.
pub const MAX_TAG_ITEMS: usize = 200;

/// A day more items than this were worked on (a bulk change, say) ties none of them.
pub const MAX_DAY_ITEMS: usize = 50;

/// An item in the graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemNode {
    /// Its identifier.
    pub id: Id,
    /// Its card's title.
    pub title: String,
    /// Its card, relative to the archive root.
    pub card: String,
    /// The collection it's in.
    pub collection: String,
    /// Its tags.
    pub tags: Vec<String>,
    /// How sensitive it is, so a restricted or sensitive one can be marked.
    pub sensitivity: Sensitivity,
    /// Whether it's checked out, so restructuring leaves it alone until it's returned.
    pub checked_out: bool,
}

/// A collection in the graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectionNode {
    /// Its folder, relative to the archive root.
    pub path: String,
    /// Its title.
    pub title: String,
    /// What it holds, as its design document says.
    pub description: Option<String>,
    /// How many items are directly in it.
    pub items: usize,
}

/// What ties two items together, and how strongly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// One item: the lesser identifier of the two.
    pub a: Id,
    /// The other.
    pub b: Id,
    /// How strongly they're tied: the sum of what's below, each weighed.
    pub weight: f64,
    /// The tags both have.
    pub tags: Vec<String>,
    /// The types of the links between them, either way.
    pub links: Vec<String>,
    /// The days both were worked on.
    pub days: usize,
}

/// A tag two collections' items share, and how many pairs of items share it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedTag {
    /// The tag.
    pub tag: String,
    /// How many pairs of items, one in each collection, both have it.
    pub pairs: usize,
}

/// What ties two collections together: every edge between an item in one and an item in the
/// other, added up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Connection {
    /// One collection: the lesser path of the two.
    pub a: String,
    /// The other.
    pub b: String,
    /// How strongly they're tied: the sum of their items' edges.
    pub weight: f64,
    /// How many pairs of items are tied.
    pub pairs: usize,
    /// The tags shared across them, most pairs first.
    pub tags: Vec<SharedTag>,
    /// How many links run between them.
    pub links: usize,
    /// How many times an item in each was worked on the same day.
    pub days: usize,
    /// Whether they're in different branches: neither is inside the other.
    pub separate_branches: bool,
}

/// The archive's items and collections, and the edges between items.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    /// The items, sorted by card path.
    pub items: Vec<ItemNode>,
    /// The collections, sorted by path.
    pub collections: Vec<CollectionNode>,
    /// The edges, sorted by their items.
    pub edges: Vec<Edge>,
}

/// Whether `inner` is `outer` or inside it.
#[must_use]
pub fn is_within(inner: &str, outer: &str) -> bool {
    outer.is_empty()
        || inner == outer
        || (inner.len() > outer.len()
            && inner.starts_with(outer)
            && inner[outer.len()..].starts_with('/'))
}

/// Whether the collections `a` and `b` are in different branches: neither is inside the other.
#[must_use]
pub fn separate_branches(a: &str, b: &str) -> bool {
    !is_within(a, b) && !is_within(b, a)
}

/// A tag or day shared by `n` items ties each pair of them this much: less the more items
/// share it, since a tag on everything says little about any two.
#[allow(clippy::cast_precision_loss)] // item counts
fn shared_weight(n: usize) -> f64 {
    1.0 / (1.0 + n as f64).log2()
}

/// A connection being gathered, with how many pairs share each tag.
type Gathered<'a> = (Connection, BTreeMap<&'a str, usize>);

/// The edges being gathered, by their pair of items.
#[derive(Default)]
struct Gathering {
    edges: BTreeMap<(Id, Id), Edge>,
}

impl Gathering {
    fn edge(&mut self, x: Id, y: Id) -> &mut Edge {
        let (a, b) = if x < y { (x, y) } else { (y, x) };
        self.edges.entry((a, b)).or_insert_with(|| Edge {
            a,
            b,
            weight: 0.0,
            tags: Vec::new(),
            links: Vec::new(),
            days: 0,
        })
    }

    /// Ties every pair of `ids`, as `add` says.
    fn pairs(&mut self, ids: &[Id], mut add: impl FnMut(&mut Edge)) {
        for (i, &x) in ids.iter().enumerate() {
            for &y in &ids[i + 1..] {
                add(self.edge(x, y));
            }
        }
    }
}

impl Graph {
    /// The graph of `catalog`'s items and collections.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // item counts
    pub fn build(catalog: &Catalog) -> Self {
        let mut gathering = Gathering::default();
        let mut by_tag: BTreeMap<&str, Vec<Id>> = BTreeMap::new();
        let mut by_day: BTreeMap<Date, Vec<Id>> = BTreeMap::new();
        for entry in &catalog.entries {
            let id = entry.item.id;
            for tag in &entry.tags {
                let ids = by_tag.entry(tag).or_default();
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            for (day, _) in &entry.sessions {
                by_day.entry(*day).or_default().push(id);
            }
            for (link_type, target) in &entry.links {
                if *target != id && catalog.get(*target).is_some() {
                    let edge = gathering.edge(id, *target);
                    edge.links.push(link_type.clone());
                    edge.weight += LINK_WEIGHT;
                }
            }
        }
        for (tag, ids) in &by_tag {
            if ids.len() > MAX_TAG_ITEMS {
                continue;
            }
            let weight = shared_weight(ids.len());
            gathering.pairs(ids, |edge| {
                edge.tags.push((*tag).to_owned());
                edge.weight += weight;
            });
        }
        for ids in by_day.values() {
            if ids.len() > MAX_DAY_ITEMS {
                continue;
            }
            let weight = SESSION_WEIGHT / (ids.len().saturating_sub(1).max(1)) as f64;
            gathering.pairs(ids, |edge| {
                edge.days += 1;
                edge.weight += weight;
            });
        }
        let mut edges: Vec<Edge> = gathering.edges.into_values().collect();
        for edge in &mut edges {
            edge.links.sort();
        }
        let items = catalog
            .entries
            .iter()
            .map(|e| ItemNode {
                id: e.item.id,
                title: e.item.title.clone(),
                card: e.item.card.clone(),
                collection: e.item.collection.clone(),
                tags: e.tags.clone(),
                sensitivity: e.item.sensitivity,
                checked_out: e.checked_out,
            })
            .collect();
        let collections = catalog
            .collections
            .iter()
            .map(|c| CollectionNode {
                path: c.path.clone(),
                title: c.title.clone(),
                description: c.description.clone(),
                items: catalog.in_collection(&c.path).count(),
            })
            .collect();
        Self {
            items,
            collections,
            edges,
        }
    }

    /// Each item's place in [`Graph::items`], by its identifier.
    #[must_use]
    pub fn positions(&self) -> HashMap<Id, usize> {
        self.items
            .iter()
            .enumerate()
            .map(|(i, item)| (item.id, i))
            .collect()
    }

    /// The item `id`, if the graph has it.
    #[must_use]
    pub fn item(&self, id: Id) -> Option<&ItemNode> {
        self.items.iter().find(|i| i.id == id)
    }

    /// The edges between items in different collections, strongest first.
    #[must_use]
    pub fn strongest_item_edges(&self) -> Vec<&Edge> {
        let positions = self.positions();
        let collection = |id: Id| positions.get(&id).map(|&i| &self.items[i].collection);
        let mut found: Vec<&Edge> = self
            .edges
            .iter()
            .filter(|e| collection(e.a) != collection(e.b))
            .collect();
        found.sort_by(|x, y| {
            y.weight
                .total_cmp(&x.weight)
                .then((x.a, x.b).cmp(&(y.a, y.b)))
        });
        found
    }

    /// What ties each pair of collections together, strongest first: every edge between an
    /// item in one and an item in the other, added up.
    #[must_use]
    pub fn connections(&self) -> Vec<Connection> {
        let positions = self.positions();
        let mut found: BTreeMap<(&str, &str), Gathered> = BTreeMap::new();
        for edge in &self.edges {
            let (Some(&x), Some(&y)) = (positions.get(&edge.a), positions.get(&edge.b)) else {
                continue;
            };
            let (x, y) = (
                self.items[x].collection.as_str(),
                self.items[y].collection.as_str(),
            );
            if x == y {
                continue;
            }
            let (a, b) = if x < y { (x, y) } else { (y, x) };
            let (connection, tags) = found.entry((a, b)).or_insert_with(|| {
                (
                    Connection {
                        a: a.to_owned(),
                        b: b.to_owned(),
                        weight: 0.0,
                        pairs: 0,
                        tags: Vec::new(),
                        links: 0,
                        days: 0,
                        separate_branches: separate_branches(a, b),
                    },
                    BTreeMap::new(),
                )
            });
            connection.weight += edge.weight;
            connection.pairs += 1;
            connection.links += edge.links.len();
            connection.days += edge.days;
            for tag in &edge.tags {
                *tags.entry(tag.as_str()).or_default() += 1;
            }
        }
        let mut connections: Vec<Connection> = found
            .into_values()
            .map(|(mut connection, tags)| {
                connection.tags = tags
                    .into_iter()
                    .map(|(tag, pairs)| SharedTag {
                        tag: tag.to_owned(),
                        pairs,
                    })
                    .collect();
                connection
                    .tags
                    .sort_by(|x, y| y.pairs.cmp(&x.pairs).then(x.tag.cmp(&y.tag)));
                connection
            })
            .collect();
        connections.sort_by(|x, y| {
            y.weight
                .total_cmp(&x.weight)
                .then((&x.a, &x.b).cmp(&(&y.a, &y.b)))
        });
        connections
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::Archive;
    use crate::discover::tests::{collection, date, entry};

    fn id(n: u128) -> Id {
        Id(uuid::Uuid::from_u128(n))
    }

    fn catalog() -> Catalog {
        let mut sim = entry(1, "sim", "Projects");
        sim.tags = vec!["simulation".into(), "physics".into()];
        sim.sessions = vec![(date("2026-01-02"), 1), (date("2026-01-03"), 2)];
        let mut paper = entry(2, "paper", "Reading");
        paper.tags = vec!["physics".into()];
        paper.links = vec![("cites".into(), id(1)), ("about".into(), id(99))];
        paper.sessions = vec![(date("2026-01-02"), 1)];
        let mut notes = entry(3, "notes", "Projects");
        notes.tags = vec!["simulation".into()];
        notes.sessions = vec![(date("2026-01-03"), 1)];
        let lone = entry(4, "lone", "Holding");
        Catalog::new(
            vec![sim, paper, notes, lone],
            vec![
                collection("Projects"),
                collection("Reading"),
                collection("Holding"),
            ],
        )
    }

    #[test]
    fn items_are_tied_by_tags_links_and_days_worked_together() {
        let graph = Graph::build(&catalog());
        assert_eq!(graph.items.len(), 4);
        assert_eq!(graph.collections.len(), 3);
        let sim_paper = graph
            .edges
            .iter()
            .find(|e| (e.a, e.b) == (id(1), id(2)))
            .unwrap();
        assert_eq!(sim_paper.tags, ["physics"]);
        assert_eq!(sim_paper.links, ["cites"], "the missing item isn't linked");
        assert_eq!(sim_paper.days, 1);
        let expected = LINK_WEIGHT + shared_weight(2) + SESSION_WEIGHT;
        assert!((sim_paper.weight - expected).abs() < 1e-9);
        let sim_notes = graph
            .edges
            .iter()
            .find(|e| (e.a, e.b) == (id(1), id(3)))
            .unwrap();
        assert_eq!((sim_notes.tags.len(), sim_notes.days), (1, 1));
        assert!(
            !graph.edges.iter().any(|e| e.a == id(4) || e.b == id(4)),
            "nothing ties the lone item"
        );
    }

    #[test]
    fn collections_connect_through_their_items() {
        let graph = Graph::build(&catalog());
        let connections = graph.connections();
        assert_eq!(
            connections.len(),
            1,
            "edges inside a collection don't count"
        );
        let c = &connections[0];
        assert_eq!((c.a.as_str(), c.b.as_str()), ("Projects", "Reading"));
        assert_eq!((c.pairs, c.links, c.days), (1, 1, 1));
        assert_eq!(c.tags[0].tag, "physics");
        assert!(c.separate_branches);
        assert_eq!(graph.strongest_item_edges().len(), 1);
    }

    #[test]
    fn branches_are_told_apart() {
        assert!(is_within("Projects/Hardware", "Projects"));
        assert!(is_within("Projects", ""));
        assert!(!is_within("Projectsx", "Projects"));
        assert!(!separate_branches("Projects", "Projects/Hardware"));
        assert!(separate_branches("Projects/Hardware", "Reading"));
    }

    #[test]
    fn the_sample_archive_has_a_graph() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive");
        let archive = Archive::open(&root).unwrap();
        let graph = Graph::build(&Catalog::read(&archive).unwrap());
        assert_eq!(graph.items.len(), 11);
        let sim = graph
            .items
            .iter()
            .find(|i| i.card == "Projects/oscillator-sim.md")
            .unwrap();
        assert!(
            graph
                .edges
                .iter()
                .any(|e| (e.a == sim.id || e.b == sim.id) && !e.links.is_empty()),
            "the simulation's derived-from link is an edge"
        );
        assert_ne!(graph.connections(), []);
    }
}
