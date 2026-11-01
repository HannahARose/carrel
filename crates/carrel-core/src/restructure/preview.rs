//! What a proposal would leave, before it's approved (`docs/design/restructure.md`, "In each
//! interface"): the archive's collections and the tags it touches as they are, and as the
//! chosen changes would leave them, for the interfaces to show side by side. Nothing is
//! written.

use std::collections::{BTreeMap, BTreeSet};

use crate::card::Id;
use crate::graph::Graph;

use super::{Edit, Proposal};

/// A collection on one side of the preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewCollection {
    /// Its folder, relative to the archive root.
    pub path: String,
    /// How many items are directly in it.
    pub items: usize,
    /// Whether the chosen changes create it (after), or remove it (before).
    pub changed: bool,
}

/// A tag the proposal touches, on one side of the preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewTag {
    /// The tag.
    pub tag: String,
    /// How many items have it.
    pub items: usize,
}

/// The archive as it is and as the chosen changes would leave it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Preview {
    /// The collections as they are, by path.
    pub before: Vec<PreviewCollection>,
    /// The collections as the changes would leave them, by path.
    pub after: Vec<PreviewCollection>,
    /// The tags the proposal touches, as they are, by tag.
    pub tags_before: Vec<PreviewTag>,
    /// Those tags as the changes would leave them, by tag; a tag no item would have is gone.
    pub tags_after: Vec<PreviewTag>,
}

/// The archive as it is, in `graph`, and as the changes of `proposal` that `approved` names
/// would leave it.
#[must_use]
pub fn preview(graph: &Graph, proposal: &Proposal, approved: &[String]) -> Preview {
    let placements: BTreeMap<Id, String> = graph
        .items
        .iter()
        .map(|i| (i.id, i.collection.clone()))
        .collect();
    let mut tags: BTreeMap<Id, BTreeSet<String>> = graph
        .items
        .iter()
        .map(|i| (i.id, i.tags.iter().cloned().collect()))
        .collect();
    let touched_tags = mentioned_tags(proposal);
    let collections: BTreeSet<String> = graph.collections.iter().map(|c| c.path.clone()).collect();
    let tags_before = count_tags(&tags, &touched_tags);
    let mut after_placements = placements.clone();
    let mut after_collections = collections.clone();
    for change in proposal.changes.iter().filter(|c| approved.contains(&c.id)) {
        let items: Vec<Id> = change.items.iter().map(|t| t.id).collect();
        match &change.change {
            Edit::MergeTags { from, into } => {
                for id in &items {
                    if let Some(have) = tags.get_mut(id)
                        && from.iter().any(|t| have.remove(&t.0))
                    {
                        have.insert(into.0.clone());
                    }
                }
            }
            Edit::SplitTag { tag, into } => {
                for part in into {
                    for id in &part.items {
                        if let Some(have) = tags.get_mut(id)
                            && have.remove(&tag.0)
                        {
                            have.insert(part.tag.0.clone());
                        }
                    }
                }
            }
            Edit::RetireTag { tag } => {
                for id in &items {
                    if let Some(have) = tags.get_mut(id) {
                        have.remove(&tag.0);
                    }
                }
            }
            Edit::CreateCollection { path, .. } => {
                after_collections.insert(path.clone());
            }
            Edit::MoveItems { to } => {
                for id in &items {
                    if let Some(place) = after_placements.get_mut(id) {
                        place.clone_from(to);
                    }
                }
            }
            Edit::RenameCollection { from, to } => {
                rehome(&mut after_collections, &mut after_placements, from, to);
            }
            Edit::MergeCollection { from, into } => {
                rehome(&mut after_collections, &mut after_placements, from, into);
                after_collections.insert(into.clone());
            }
            Edit::RewriteDesign { .. } => {}
        }
    }
    Preview {
        before: sides(&collections, &placements, &after_collections),
        after: sides(&after_collections, &after_placements, &collections),
        tags_before,
        tags_after: count_tags(&tags, &touched_tags),
    }
}

/// Moves the collection at `from`, everything under it and its items, to `to`, in `collections`
/// and `placements`.
fn rehome(
    collections: &mut BTreeSet<String>,
    placements: &mut BTreeMap<Id, String>,
    from: &str,
    to: &str,
) {
    let moved = |path: &str| {
        if path == from {
            Some(to.to_owned())
        } else {
            path.strip_prefix(from)
                .and_then(|rest| rest.strip_prefix('/'))
                .map(|rest| format!("{to}/{rest}"))
        }
    };
    *collections = collections
        .iter()
        .map(|path| moved(path).unwrap_or_else(|| path.clone()))
        .collect();
    for place in placements.values_mut() {
        if let Some(new) = moved(place) {
            *place = new;
        }
    }
}

/// Every collection in `paths` with its items in `placements`, marked changed when `other`
/// (the other side) doesn't have it.
fn sides(
    paths: &BTreeSet<String>,
    placements: &BTreeMap<Id, String>,
    other: &BTreeSet<String>,
) -> Vec<PreviewCollection> {
    paths
        .iter()
        .map(|path| PreviewCollection {
            path: path.clone(),
            items: placements.values().filter(|p| *p == path).count(),
            changed: !other.contains(path),
        })
        .collect()
}

/// The tags `proposal`'s changes name, before and after.
fn mentioned_tags(proposal: &Proposal) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for change in &proposal.changes {
        match &change.change {
            Edit::MergeTags { from, into } => {
                found.extend(from.iter().map(|t| t.0.clone()));
                found.insert(into.0.clone());
            }
            Edit::SplitTag { tag, into } => {
                found.insert(tag.0.clone());
                found.extend(into.iter().map(|p| p.tag.0.clone()));
            }
            Edit::RetireTag { tag } => {
                found.insert(tag.0.clone());
            }
            Edit::CreateCollection { .. }
            | Edit::MoveItems { .. }
            | Edit::RenameCollection { .. }
            | Edit::MergeCollection { .. }
            | Edit::RewriteDesign { .. } => {}
        }
    }
    found
}

/// How many items have each of `wanted`, leaving out those none has.
fn count_tags(tags: &BTreeMap<Id, BTreeSet<String>>, wanted: &BTreeSet<String>) -> Vec<PreviewTag> {
    wanted
        .iter()
        .map(|tag| PreviewTag {
            tag: tag.clone(),
            items: tags.values().filter(|have| have.contains(tag)).count(),
        })
        .filter(|t| t.items > 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::Sensitivity;
    use crate::card::Tag;
    use crate::graph::{CollectionNode, ItemNode};
    use crate::restructure::{ProposalKind, ProposedChange, Touched};

    fn item(id: Id, collection: &str, tags: &[&str]) -> ItemNode {
        ItemNode {
            id,
            title: format!("item in {collection}"),
            card: format!("{collection}/x.md"),
            collection: collection.into(),
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            sensitivity: Sensitivity::Personal,
            checked_out: false,
        }
    }

    fn change(id: &str, edit: Edit, items: &[Id]) -> ProposedChange {
        ProposedChange {
            id: id.into(),
            change: edit,
            reason: "because".into(),
            invasiveness: crate::restructure::Invasiveness::Rewriting,
            items: items
                .iter()
                .map(|id| Touched {
                    id: *id,
                    card: String::new(),
                    title: String::new(),
                })
                .collect(),
            held: Vec::new(),
            documents: Vec::new(),
            depends_on: Vec::new(),
            extra: crate::card::Extra::new(),
        }
    }

    #[test]
    fn the_chosen_tag_changes_show_after_and_the_rest_dont() {
        let (a, b, c) = (Id::new_v7(), Id::new_v7(), Id::new_v7());
        let graph = Graph {
            items: vec![
                item(a, "Finances", &["receipts"]),
                item(b, "Finances", &["receipt"]),
                item(c, "Projects", &["misc"]),
            ],
            collections: vec![
                CollectionNode {
                    path: "Finances".into(),
                    title: "Finances".into(),
                    description: None,
                    items: 2,
                },
                CollectionNode {
                    path: "Projects".into(),
                    title: "Projects".into(),
                    description: None,
                    items: 1,
                },
            ],
            edges: Vec::new(),
        };
        let mut proposal = Proposal::new(ProposalKind::Tags, "");
        proposal.changes = vec![
            change(
                "1",
                Edit::MergeTags {
                    from: vec![Tag("receipts".into())],
                    into: Tag("receipt".into()),
                },
                &[a],
            ),
            change(
                "2",
                Edit::RetireTag {
                    tag: Tag("misc".into()),
                },
                &[c],
            ),
        ];
        let tag = |t: &str, n| PreviewTag {
            tag: t.into(),
            items: n,
        };
        let only_first = preview(&graph, &proposal, &["1".into()]);
        assert_eq!(
            only_first.tags_before,
            [tag("misc", 1), tag("receipt", 1), tag("receipts", 1)]
        );
        assert_eq!(only_first.tags_after, [tag("misc", 1), tag("receipt", 2)]);
        let both = preview(&graph, &proposal, &["1".into(), "2".into()]);
        assert_eq!(both.tags_after, [tag("receipt", 2)]);
        assert_eq!(both.before, both.after, "tags move no files");
        assert_eq!(both.before[0].items, 2);
        assert!(!both.before[0].changed);
    }

    #[test]
    fn section_changes_move_items_and_collections_after() {
        let (a, b, c) = (Id::new_v7(), Id::new_v7(), Id::new_v7());
        let collection = |path: &str, items| CollectionNode {
            path: path.into(),
            title: path.into(),
            description: None,
            items,
        };
        let graph = Graph {
            items: vec![
                item(a, "Finances", &[]),
                item(b, "Finances", &[]),
                item(c, "Old/Notes", &[]),
            ],
            collections: vec![
                collection("Finances", 2),
                collection("Old", 0),
                collection("Old/Notes", 1),
            ],
            edges: Vec::new(),
        };
        let mut proposal = Proposal::new(ProposalKind::Tags, "");
        proposal.changes = vec![
            change(
                "1",
                Edit::CreateCollection {
                    path: "Finances/Receipts".into(),
                    description: "Receipts".into(),
                },
                &[],
            ),
            change(
                "2",
                Edit::MoveItems {
                    to: "Finances/Receipts".into(),
                },
                &[a],
            ),
            change(
                "3",
                Edit::RenameCollection {
                    from: "Old".into(),
                    to: "Archive".into(),
                },
                &[],
            ),
        ];
        let all: Vec<String> = ["1", "2", "3"].map(String::from).to_vec();
        let shown = preview(&graph, &proposal, &all);
        let after: BTreeMap<&str, (usize, bool)> = shown
            .after
            .iter()
            .map(|c| (c.path.as_str(), (c.items, c.changed)))
            .collect();
        assert_eq!(after.get("Finances"), Some(&(1, false)));
        assert_eq!(after.get("Finances/Receipts"), Some(&(1, true)));
        assert_eq!(after.get("Archive/Notes"), Some(&(1, true)));
        assert!(!after.contains_key("Old"));
        assert!(shown.before.iter().any(|c| c.path == "Old" && c.changed));
    }
}
