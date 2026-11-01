//! The tag and link graph as a discovery view (`docs/design/restructure.md`, "The tag and link
//! graph"), as `carrel discover graph` lists it: the archive's collections, the strongest
//! connections between them and what makes them, and the strongest ties between items in
//! different collections. The app draws it.

use carrel_core::discover::Catalog;
use carrel_core::graph::{Graph, ItemNode};

use crate::records::Sensitivity;
use crate::{ArchiveHandle, CarrelError};

/// A collection in the graph.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GraphCollection {
    /// Its folder, relative to the archive root.
    pub path: String,
    /// Its title.
    pub title: String,
    /// What it holds, as its design document says.
    pub description: Option<String>,
    /// How many items are directly in it.
    pub items: u64,
}

/// A tag two collections' items share.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GraphTag {
    /// The tag.
    pub tag: String,
    /// How many pairs of items, one in each collection, both have it.
    pub pairs: u64,
}

/// What ties two collections together.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GraphConnection {
    /// One collection: the lesser path of the two.
    pub a: String,
    /// The other.
    pub b: String,
    /// How strongly they're tied: the sum of their items' ties.
    pub weight: f64,
    /// How many pairs of items are tied.
    pub pairs: u64,
    /// The tags shared across them, most pairs first.
    pub tags: Vec<GraphTag>,
    /// How many links run between them.
    pub links: u64,
    /// How many times an item in each was worked on the same day.
    pub days: u64,
    /// Whether they're in different branches: neither is inside the other.
    pub separate_branches: bool,
}

/// An item at one end of a tie.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct GraphItem {
    /// Its identifier.
    pub id: String,
    /// Its card's title.
    pub title: String,
    /// Its card, relative to the archive root.
    pub card: String,
    /// The collection it's in.
    pub collection: String,
    /// How sensitive it is, so a restricted or sensitive one can be marked.
    pub sensitivity: Sensitivity,
}

impl From<&ItemNode> for GraphItem {
    fn from(item: &ItemNode) -> Self {
        Self {
            id: item.id.to_string(),
            title: item.title.clone(),
            card: item.card.clone(),
            collection: item.collection.clone(),
            sensitivity: item.sensitivity.into(),
        }
    }
}

/// What ties two items in different collections together.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct GraphItemTie {
    /// One item.
    pub a: GraphItem,
    /// The other.
    pub b: GraphItem,
    /// How strongly they're tied.
    pub weight: f64,
    /// The tags both have.
    pub tags: Vec<String>,
    /// The types of the links between them, either way.
    pub links: Vec<String>,
    /// The days both were worked on.
    pub days: u64,
}

/// The graph as Discover shows it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct DiscoveryGraph {
    /// Every collection, by path.
    pub collections: Vec<GraphCollection>,
    /// The strongest connections between collections, strongest first.
    pub connections: Vec<GraphConnection>,
    /// The strongest ties between items in different collections, strongest first.
    pub ties: Vec<GraphItemTie>,
}

fn count(n: usize) -> u64 {
    n as u64
}

#[uniffi::export]
impl ArchiveHandle {
    /// The tag and link graph, as `carrel discover graph` lists it: every collection, and at
    /// most `limit` of the strongest connections between collections and of the strongest
    /// ties between items in different collections.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive can't be read.
    pub fn discovery_graph(&self, limit: u32) -> Result<DiscoveryGraph, CarrelError> {
        let archive = self.fresh()?;
        let graph = Graph::build(&Catalog::read(&archive)?);
        let limit = limit as usize;
        let ties = graph
            .strongest_item_edges()
            .into_iter()
            .take(limit)
            .filter_map(|edge| {
                Some(GraphItemTie {
                    a: graph.item(edge.a)?.into(),
                    b: graph.item(edge.b)?.into(),
                    weight: edge.weight,
                    tags: edge.tags.clone(),
                    links: edge.links.clone(),
                    days: count(edge.days),
                })
            })
            .collect();
        Ok(DiscoveryGraph {
            collections: graph
                .collections
                .iter()
                .map(|c| GraphCollection {
                    path: c.path.clone(),
                    title: c.title.clone(),
                    description: c.description.clone(),
                    items: count(c.items),
                })
                .collect(),
            connections: graph
                .connections()
                .into_iter()
                .take(limit)
                .map(|c| GraphConnection {
                    a: c.a,
                    b: c.b,
                    weight: c.weight,
                    pairs: count(c.pairs),
                    tags: c
                        .tags
                        .into_iter()
                        .map(|t| GraphTag {
                            tag: t.tag,
                            pairs: count(t.pairs),
                        })
                        .collect(),
                    links: count(c.links),
                    days: count(c.days),
                    separate_branches: c.separate_branches,
                })
                .collect(),
            ties,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::Sample;

    #[test]
    fn the_sample_has_its_collections_and_connections() {
        let sample = Sample::new();
        let graph = sample.handle.discovery_graph(20).unwrap();
        assert!(graph.collections.iter().any(|c| c.path == "Projects"));
        assert!(graph.connections.len() <= 20);
        for connection in &graph.connections {
            assert!(connection.a < connection.b);
            assert!(connection.weight > 0.0);
        }
        for pair in graph.connections.windows(2) {
            assert!(pair[0].weight >= pair[1].weight, "strongest first");
        }
        for tie in &graph.ties {
            assert_ne!(tie.a.collection, tie.b.collection);
        }
        let none = sample.handle.discovery_graph(0).unwrap();
        assert!(none.connections.is_empty() && none.ties.is_empty());
    }
}
