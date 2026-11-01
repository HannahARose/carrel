//! Grouping a section's items (`docs/design/restructure.md`, "How a proposal is built"):
//! numerically, from their card embeddings and the tag and link graph, so what belongs
//! together doesn't hinge on one model's whim. A model names the groups later; this only says
//! which items they hold.
//!
//! Two items are as similar as their card vectors' cosine says, raised by their ties in the
//! graph (shared tags, links, days worked on together) as [`GroupingOptions::graph_weight`]
//! says; items without a vector are as similar as their ties alone. Items are
//! clustered by average linkage: the two groups whose items are most similar on average join,
//! until no two are similar enough ([`GroupingOptions::threshold`]). Literary warrant decides
//! which groups are worth a collection: at least `min_collection_items` members (FORMAT.md
//! §6.2).
//!
//! Each candidate says where it would go and how invasive placing it would be: a new
//! subcollection inside the collection all its members are in (subdividing), or a collection
//! gathering items from several branches (moving items between branches). Items in Holding
//! don't count against a group: filing them disturbs nothing the owner knows.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use super::Invasiveness;
use crate::card::Id;
use crate::graph::{Graph, ItemNode, is_within};
use crate::index::Index;

/// The collection items go to when nowhere fits.
const HOLDING: &str = "Holding";

/// The most items one grouping considers; the rest of a larger section are left out, and
/// counted in [`Grouping::left_out`]. Grouping keeps a similarity for every pair, so a
/// larger section is better grouped a collection at a time.
pub const MAX_GROUPED_ITEMS: usize = 3000;

/// How grouping weighs what it knows, and when it stops.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GroupingOptions {
    /// The fewest members a group needs to become a collection: the archive's
    /// `min_collection_items`.
    pub warrant: usize,
    /// How similar on average two groups' items must be to join, from 0 to 1.
    pub threshold: f64,
    /// How far the items' ties in the graph raise their similarity by meaning toward 1, from
    /// 0 (not at all) to 1 (a strong tie makes them as similar as can be).
    pub graph_weight: f64,
}

impl Default for GroupingOptions {
    fn default() -> Self {
        Self {
            warrant: 5,
            threshold: 0.5,
            graph_weight: 0.6,
        }
    }
}

impl GroupingOptions {
    /// The defaults, with literary warrant from the archive's `min_collection_items`.
    #[must_use]
    pub fn for_archive(archive: &crate::archive::Archive) -> Self {
        let warrant = archive.doc().min_collection_items.unwrap_or(5);
        Self {
            warrant: usize::try_from(warrant).unwrap_or(usize::MAX).max(2),
            ..Self::default()
        }
    }
}

/// Where some of a group's members are now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSource {
    /// The collection.
    pub collection: String,
    /// How many of the group's members are in it.
    pub items: usize,
}

/// A group of items that belong together, worth a collection of their own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateGroup {
    /// Its members, in the graph's order (by card path).
    pub members: Vec<Id>,
    /// How similar its members are on average, from 0 to 1.
    pub cohesion: f64,
    /// The tags at least half its members have, most used first: a hint for its name.
    pub tags: Vec<String>,
    /// Where its members are now, most first.
    pub sources: Vec<GroupSource>,
    /// The collection it would become a subcollection of; empty for the archive's top
    /// level.
    pub home: String,
    /// How invasive placing it would be.
    pub invasiveness: Invasiveness,
}

/// A section's candidate groups.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grouping {
    /// The section: a collection's path, or empty for the whole archive.
    pub section: String,
    /// How many of its items were grouped.
    pub items: usize,
    /// How many of those had no card vector, so were grouped by the graph alone.
    pub without_vectors: usize,
    /// How many were left out: checked out, or past [`MAX_GROUPED_ITEMS`].
    pub left_out: usize,
    /// The candidate groups, most cohesive first.
    pub groups: Vec<CandidateGroup>,
}

/// The card vectors `index` has for the graph's items in `section`.
#[must_use]
pub fn card_vectors(index: &Index, graph: &Graph, section: &str) -> HashMap<Id, Vec<f32>> {
    graph
        .items
        .iter()
        .filter(|i| is_within(&i.collection, section))
        .filter_map(|i| Some((i.id, index.card_vector(i.id)?)))
        .collect()
}

/// The cosine of `a` and `b`, or none if either is empty or they differ in length.
fn cosine(a: &[f32], b: &[f32]) -> Option<f64> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let (mut dot, mut aa, mut bb) = (0.0_f64, 0.0_f64, 0.0_f64);
    for (x, y) in a.iter().zip(b) {
        let (x, y) = (f64::from(*x), f64::from(*y));
        dot += x * y;
        aa += x * x;
        bb += y * y;
    }
    (aa > 0.0 && bb > 0.0).then(|| dot / (aa.sqrt() * bb.sqrt()))
}

/// What the similarity of two items is made from.
struct Similarity<'a> {
    vectors: Vec<Option<&'a [f32]>>,
    ties: HashMap<(usize, usize), f64>,
    options: GroupingOptions,
}

impl Similarity<'_> {
    /// How similar the items at `i` and `j` are, from 0 to 1.
    fn between(&self, i: usize, j: usize) -> f64 {
        let key = if i < j { (i, j) } else { (j, i) };
        // A tie's weight, brought into 0..1: one tag only two items share is about 0.7.
        let tie = 1.0 - (-2.0 * self.ties.get(&key).copied().unwrap_or(0.0)).exp();
        let meaning = match (self.vectors[i], self.vectors[j]) {
            (Some(a), Some(b)) => cosine(a, b).map(|c| c.clamp(0.0, 1.0)),
            _ => None,
        };
        match meaning {
            // Ties raise similarity by meaning toward 1; they never lower it.
            Some(m) => m + self.options.graph_weight.clamp(0.0, 1.0) * tie * (1.0 - m),
            None => tie,
        }
    }
}

/// Clusters `n` items by average linkage, joining groups while their items' average
/// similarity is at least `threshold`. Uses the nearest-neighbour chain, which gives the same
/// groups as always joining the most similar pair, in quadratic time. Returns each group's
/// members, in ascending order.
#[allow(clippy::cast_precision_loss)] // group sizes
fn average_linkage(
    n: usize,
    similarity: &dyn Fn(usize, usize) -> f64,
    threshold: f64,
) -> Vec<Vec<usize>> {
    let mut matrix = vec![0.0_f32; n * n];
    for i in 0..n {
        for j in i + 1..n {
            #[allow(clippy::cast_possible_truncation)] // similarities are in 0..1
            let s = similarity(i, j) as f32;
            matrix[i * n + j] = s;
            matrix[j * n + i] = s;
        }
    }
    let mut members: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
    let mut active: Vec<bool> = vec![true; n];
    let mut done: Vec<Vec<usize>> = Vec::new();
    let mut chain: Vec<usize> = Vec::new();
    let mut remaining = n;
    while remaining > 0 {
        if chain.is_empty() {
            let first = active
                .iter()
                .position(|a| *a)
                .expect("an active group remains");
            chain.push(first);
        }
        let c = *chain.last().expect("the chain isn't empty");
        let previous = chain.len().checked_sub(2).map(|i| chain[i]);
        let mut best: Option<(usize, f32)> = None;
        for d in (0..n).filter(|&d| active[d] && d != c) {
            let s = matrix[c * n + d];
            let better = match best {
                None => true,
                // Ties go to the chain's previous group, so the chain always ends.
                Some((b, best_score)) => {
                    s > best_score
                        || (s.total_cmp(&best_score).is_eq()
                            && Some(d) == previous
                            && Some(b) != previous)
                }
            };
            if better {
                best = Some((d, s));
            }
        }
        match best {
            Some((d, s)) if f64::from(s) >= threshold => {
                if Some(d) == previous {
                    chain.truncate(chain.len() - 2);
                    // Join d into c: c's similarities become the size-weighted average.
                    let (size_c, size_d) = (members[c].len() as f32, members[d].len() as f32);
                    for k in (0..n).filter(|&k| active[k] && k != c && k != d) {
                        let joined = (size_c * matrix[c * n + k] + size_d * matrix[d * n + k])
                            / (size_c + size_d);
                        matrix[c * n + k] = joined;
                        matrix[k * n + c] = joined;
                    }
                    let moved = std::mem::take(&mut members[d]);
                    members[c].extend(moved);
                    active[d] = false;
                    remaining -= 1;
                } else {
                    chain.push(d);
                }
            }
            _ => {
                // Nothing is similar enough to c, and joining others can't change that: an
                // average of similarities below the threshold stays below it.
                chain.pop();
                active[c] = false;
                remaining -= 1;
                let mut group = std::mem::take(&mut members[c]);
                group.sort_unstable();
                done.push(group);
            }
        }
    }
    done.sort();
    done
}

/// The deepest collection all of `collections` are in, or empty for the archive's top level.
fn common_home<'a>(collections: impl Iterator<Item = &'a str>) -> String {
    let mut home: Option<Vec<&str>> = None;
    for path in collections {
        let parts: Vec<&str> = path.split('/').collect();
        home = Some(match home {
            None => parts,
            Some(h) => h
                .iter()
                .zip(&parts)
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| *a)
                .collect(),
        });
    }
    home.unwrap_or_default().join("/")
}

/// Where a group of items in `collections` would go, and how invasive that would be.
fn placement(collections: &[&str]) -> (String, Invasiveness) {
    let known: Vec<&str> = collections
        .iter()
        .copied()
        .filter(|c| *c != HOLDING && !is_within(c, HOLDING))
        .collect();
    if known.is_empty() {
        // Holding's items filed together: nothing the owner knows moves.
        return (String::new(), Invasiveness::Subdividing);
    }
    let home = common_home(known.iter().copied());
    if known.iter().all(|c| *c == home) {
        (home, Invasiveness::Subdividing)
    } else {
        (home, Invasiveness::BetweenBranches)
    }
}

/// The candidate group of `members` (their places in `similarity`), which are `items`, or none
/// if they're exactly a collection's items already. `per_collection` counts each collection's
/// items.
#[allow(clippy::cast_precision_loss)] // item counts
fn candidate(
    members: &[usize],
    items: &[&ItemNode],
    similarity: &Similarity,
    per_collection: &HashMap<&str, usize>,
) -> Option<CandidateGroup> {
    let mut sources: BTreeMap<&str, usize> = BTreeMap::new();
    for item in items {
        *sources.entry(item.collection.as_str()).or_default() += 1;
    }
    if let [(only, count)] = sources.iter().collect::<Vec<_>>()[..]
        && per_collection.get(only) == Some(count)
    {
        return None;
    }
    let pairs = members.len() * (members.len() - 1) / 2;
    let total: f64 = members
        .iter()
        .enumerate()
        .flat_map(|(a, &x)| members[a + 1..].iter().map(move |&y| (x, y)))
        .map(|(x, y)| similarity.between(x, y))
        .sum();
    let mut tag_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for item in items {
        for tag in &item.tags {
            *tag_counts.entry(tag.as_str()).or_default() += 1;
        }
    }
    let mut tags: Vec<(&str, usize)> = tag_counts
        .into_iter()
        .filter(|(_, n)| n * 2 >= members.len())
        .collect();
    tags.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let collections: Vec<&str> = sources.keys().copied().collect();
    let (home, invasiveness) = placement(&collections);
    let mut sources: Vec<GroupSource> = sources
        .into_iter()
        .map(|(collection, items)| GroupSource {
            collection: collection.to_owned(),
            items,
        })
        .collect();
    sources.sort_by(|a, b| b.items.cmp(&a.items).then(a.collection.cmp(&b.collection)));
    Some(CandidateGroup {
        members: items.iter().map(|i| i.id).collect(),
        cohesion: total / pairs as f64,
        tags: tags.into_iter().map(|(t, _)| t.to_owned()).collect(),
        sources,
        home,
        invasiveness,
    })
}

/// The candidate groups among the graph's items in `section` (a collection's path, or empty
/// for the whole archive), from their card `vectors` (missing ones are grouped by the graph
/// alone) and the graph's edges. Checked-out items are left out. A group that is exactly a
/// collection's items already is no change, so isn't a candidate.
#[must_use]
pub fn group_section<S: std::hash::BuildHasher>(
    graph: &Graph,
    section: &str,
    vectors: &HashMap<Id, Vec<f32>, S>,
    options: &GroupingOptions,
) -> Grouping {
    let in_section: Vec<usize> = (0..graph.items.len())
        .filter(|&i| is_within(&graph.items[i].collection, section))
        .collect();
    let mut chosen: Vec<usize> = in_section
        .iter()
        .copied()
        .filter(|&i| !graph.items[i].checked_out)
        .collect();
    chosen.truncate(MAX_GROUPED_ITEMS);
    let left_out = in_section.len() - chosen.len();
    let local: HashMap<Id, usize> = chosen
        .iter()
        .enumerate()
        .map(|(k, &i)| (graph.items[i].id, k))
        .collect();
    let mut ties = HashMap::new();
    for edge in &graph.edges {
        if let (Some(&x), Some(&y)) = (local.get(&edge.a), local.get(&edge.b)) {
            ties.insert(if x < y { (x, y) } else { (y, x) }, edge.weight);
        }
    }
    let similarity = Similarity {
        vectors: chosen
            .iter()
            .map(|&i| vectors.get(&graph.items[i].id).map(Vec::as_slice))
            .collect(),
        ties,
        options: *options,
    };
    let without_vectors = similarity.vectors.iter().filter(|v| v.is_none()).count();
    let clusters = average_linkage(
        chosen.len(),
        &|i, j| similarity.between(i, j),
        options.threshold,
    );
    let mut per_collection: HashMap<&str, usize> = HashMap::new();
    for item in &graph.items {
        *per_collection.entry(item.collection.as_str()).or_default() += 1;
    }
    let mut groups: Vec<CandidateGroup> = clusters
        .into_iter()
        .filter(|members| members.len() >= options.warrant.max(2))
        .filter_map(|members| {
            let items: Vec<&ItemNode> = members.iter().map(|&k| &graph.items[chosen[k]]).collect();
            candidate(&members, &items, &similarity, &per_collection)
        })
        .collect();
    groups.sort_by(|a, b| {
        b.cohesion
            .total_cmp(&a.cohesion)
            .then(b.members.len().cmp(&a.members.len()))
            .then(a.members.cmp(&b.members))
    });
    Grouping {
        section: section.to_owned(),
        items: chosen.len(),
        without_vectors,
        left_out,
        groups,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::Archive;
    use crate::discover::Catalog;
    use crate::discover::tests::{collection, entry};

    fn id(n: u128) -> Id {
        Id(uuid::Uuid::from_u128(n))
    }

    /// A fake card vector pointing mostly along `axis`, a little off by `n`.
    #[allow(clippy::cast_precision_loss)]
    fn vector(axis: usize, n: u128) -> Vec<f32> {
        let mut v = vec![0.05_f32; 4];
        v[axis] = 1.0;
        v[(axis + 1) % 4] += (n % 3) as f32 * 0.1;
        v
    }

    /// Twelve items in Reading: six about birds and six about stars, by meaning; and five
    /// in Projects and Holding tagged `garden`.
    fn sample() -> (Graph, HashMap<Id, Vec<f32>>) {
        let mut entries = Vec::new();
        let mut vectors = HashMap::new();
        for n in 1..=12_u128 {
            entries.push(entry(n, &format!("r{n:02}"), "Reading"));
            vectors.insert(id(n), vector(usize::from(n > 6), n));
        }
        for n in 20..25_u128 {
            let place = if n < 23 { "Projects" } else { "Holding" };
            let mut e = entry(n, &format!("g{n}"), place);
            e.tags = vec!["garden".into(), "seeds".into()];
            entries.push(e);
        }
        let mut busy = entry(30, "busy", "Projects");
        busy.checked_out = true;
        entries.push(busy);
        let catalog = Catalog::new(
            entries,
            vec![
                collection("Reading"),
                collection("Projects"),
                collection("Holding"),
            ],
        );
        (Graph::build(&catalog), vectors)
    }

    #[test]
    fn a_collection_is_subdivided_by_meaning() {
        let (graph, vectors) = sample();
        let grouping = group_section(&graph, "Reading", &vectors, &GroupingOptions::default());
        assert_eq!((grouping.items, grouping.without_vectors), (12, 0));
        assert_eq!(grouping.groups.len(), 2);
        for group in &grouping.groups {
            assert_eq!(group.members.len(), 6);
            assert_eq!(group.home, "Reading");
            assert_eq!(group.invasiveness, Invasiveness::Subdividing);
            assert!(group.cohesion > 0.9, "{}", group.cohesion);
        }
        let birds: Vec<Id> = (1..=6).map(id).collect();
        assert!(grouping.groups.iter().any(|g| g.members == birds));
    }

    #[test]
    fn groups_need_literary_warrant() {
        let (graph, vectors) = sample();
        let options = GroupingOptions {
            warrant: 7,
            ..GroupingOptions::default()
        };
        assert_eq!(
            group_section(&graph, "Reading", &vectors, &options).groups,
            []
        );
    }

    #[test]
    fn tags_group_items_across_branches_without_vectors() {
        let (graph, vectors) = sample();
        let grouping = group_section(&graph, "", &vectors, &GroupingOptions::default());
        assert_eq!(grouping.left_out, 1, "the checked-out item is left alone");
        assert_eq!(grouping.without_vectors, 5);
        let garden = grouping
            .groups
            .iter()
            .find(|g| g.tags.first().map(String::as_str) == Some("garden"))
            .expect("the garden items are a group");
        assert_eq!(garden.members.len(), 5);
        assert_eq!(
            garden.sources,
            [
                GroupSource {
                    collection: "Projects".into(),
                    items: 3
                },
                GroupSource {
                    collection: "Holding".into(),
                    items: 2
                },
            ]
        );
        // Holding's items don't count: the group subdivides Projects.
        assert_eq!(
            (garden.home.as_str(), garden.invasiveness),
            ("Projects", Invasiveness::Subdividing)
        );
    }

    #[test]
    fn grouping_is_the_same_every_time() {
        let (graph, vectors) = sample();
        let options = GroupingOptions::default();
        assert_eq!(
            group_section(&graph, "", &vectors, &options),
            group_section(&graph, "", &vectors, &options)
        );
    }

    #[test]
    fn a_group_across_collections_moves_items_between_branches() {
        assert_eq!(
            placement(&["Projects/Hardware", "Projects/Software"]),
            ("Projects".into(), Invasiveness::BetweenBranches)
        );
        assert_eq!(
            placement(&["Projects", "Reading", "Holding"]),
            (String::new(), Invasiveness::BetweenBranches)
        );
        assert_eq!(
            placement(&["Holding"]),
            (String::new(), Invasiveness::Subdividing)
        );
    }

    #[test]
    fn average_linkage_matches_joining_the_closest_pair() {
        // Points on a line: 0, 1, 2 close; 10, 11 close; 30 alone.
        let points = [0.0_f64, 1.0, 2.0, 10.0, 11.0, 30.0];
        let similarity = |i: usize, j: usize| 1.0 / (1.0 + (points[i] - points[j]).abs());
        let groups = average_linkage(points.len(), &similarity, 0.3);
        assert_eq!(groups, [vec![0, 1, 2], vec![3, 4], vec![5]]);
    }

    #[test]
    fn the_sample_archive_is_grouped_without_an_index() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive");
        let archive = Archive::open(&root).unwrap();
        let graph = Graph::build(&Catalog::read(&archive).unwrap());
        let options = GroupingOptions {
            warrant: 2,
            ..GroupingOptions::for_archive(&archive)
        };
        let grouping = group_section(&graph, "", &HashMap::new(), &options);
        assert_eq!(grouping.without_vectors, grouping.items);
        for group in &grouping.groups {
            assert!(group.members.len() >= 2);
            assert!((0.0..=1.0).contains(&group.cohesion));
        }
    }
}
