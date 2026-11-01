//! Restructure signals (`docs/design/signals.md`): where the archive's structure shows signs of
//! not fitting what's in it. They're computed from the archive as it is, so they're never stale,
//! and they're information only until restructure proposals (v0.7) start from them.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::archive::{Archive, ArchiveError};
use crate::card::PlacementConfidence;

/// The collection items go to when nowhere fits.
const HOLDING: &str = "Holding";

/// How many top-level collections a tag must appear in to suggest a missing group.
const SPREAD_TAG_COLLECTIONS: usize = 3;

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
    let mut contents = Vec::new();
    for collection in archive.collections()? {
        let entries = archive
            .cards_in(&collection)?
            .into_iter()
            .map(|(_, card)| Entry {
                tags: card.tags.iter().map(|t| t.0.clone()).collect(),
                placement: card.placement.map(|p| p.confidence),
            })
            .collect();
        contents.push((collection.path, entries));
    }
    let warrant = archive.doc().min_collection_items.unwrap_or(5);
    let crowded = archive.settings().signals.crowded_items;
    Ok(from_contents(
        &contents,
        usize::try_from(warrant).unwrap_or(usize::MAX),
        usize::try_from(crowded).unwrap_or(usize::MAX),
    ))
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
}
