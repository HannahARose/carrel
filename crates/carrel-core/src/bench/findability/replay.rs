//! Replaying how search merges its rankings, from a findability run's `results.json`, without
//! the models (issue #249): the embedding models run on the owner's Mac, but how their
//! rankings are merged with the words can be tuned anywhere.
//!
//! A run since 2026-11 saves the whole word and meaning rankings for every description
//! ([`Lookup::places`]), so [`replay`] merges them exactly as search would. An earlier run
//! saved only the first five items and where the described item came; [`replay`] then fills
//! in what it doesn't know: the places in between with unknown items, and every item the
//! words found outside the meaning ranking's first five in the middle of it, as an item it
//! has no opinion of would be. That estimate is checked against the run's own merged ranking
//! in the tests.

use std::collections::{HashMap, HashSet};

use super::{DEPTH, Lookup, TOP};
use crate::search::{Merge, Worded, exact_terms, merge};

/// Whether `meaning`'s lookups hold whole rankings, so a replay of them is exact rather than
/// estimated.
#[must_use]
pub fn saved(meaning: &[Lookup]) -> bool {
    meaning.iter().any(|l| !l.places.is_empty())
}

/// Merges each description's word and meaning rankings from one run, `words` alone and
/// `meaning` alone, as search would with `weights`, and notes where each description's item
/// came back. `items` is how many items the run filed. Descriptions in only one of the two
/// are left out.
#[must_use]
pub fn replay(words: &[Lookup], meaning: &[Lookup], items: usize, weights: Merge) -> Vec<Lookup> {
    let whole = saved(meaning);
    let by_description: HashMap<(&str, &str), &Lookup> = meaning
        .iter()
        .map(|l| ((l.item.as_str(), l.description.as_str()), l))
        .collect();
    words
        .iter()
        .filter_map(|w| {
            let m = by_description.get(&(w.item.as_str(), w.description.as_str()))?;
            let (worded, near, others) = if whole {
                saved_rankings(w, m)
            } else {
                estimated_rankings(w, m, items)
            };
            let partial = whole && near.len() < DEPTH;
            let by_words = |key: &String| others.contains(key) || partial;
            // Search returns its first DEPTH hits, collections and glossary facts among
            // them, and only then are the items picked out.
            let found: Vec<String> = merge(&worded, Some(&near), &by_words, weights)
                .into_iter()
                .take(DEPTH)
                .map(|(key, _)| key)
                .filter(|key| !others.contains(key))
                .collect();
            Some(Lookup {
                item: w.item.clone(),
                description: w.description.clone(),
                rank: found.iter().position(|f| *f == w.item).map(|p| p + 1),
                top: found.into_iter().take(TOP).collect(),
                places: Vec::new(),
            })
        })
        .collect()
}

type Rankings = (Vec<Worded<String>>, Vec<String>, HashSet<String>);

/// The rankings a run saved whole.
fn saved_rankings(words: &Lookup, meaning: &Lookup) -> Rankings {
    let worded = words
        .places
        .iter()
        .map(|p| Worded {
            key: p.item.clone(),
            exact: p.exact,
        })
        .collect();
    let near = meaning.places.iter().map(|p| p.item.clone()).collect();
    let others = words
        .places
        .iter()
        .filter(|p| p.other)
        .map(|p| p.item.clone())
        .collect();
    (worded, near, others)
}

/// The rankings as well as a run that saved only the first five and the item's place tells
/// them: unknown items in the places between, and the items the words found that the meaning
/// ranking's first five don't have in the middle of it. When the description has an exact
/// name or number, the words' first item is taken to have it.
fn estimated_rankings(words: &Lookup, meaning: &Lookup, items: usize) -> Rankings {
    let mut word_keys = words.top.clone();
    if let Some(rank) = words.rank
        && rank > word_keys.len()
        && word_keys.len() == TOP
    {
        word_keys.extend((word_keys.len() + 1..rank).map(|p| format!("?words {p}")));
        word_keys.push(words.item.clone());
    }
    let exact = usize::from(!exact_terms(&words.description).is_empty());
    let worded = word_keys
        .iter()
        .enumerate()
        .map(|(place, key)| Worded {
            key: key.clone(),
            exact: if place == 0 { exact } else { 0 },
        })
        .collect();

    let length = items
        .max(meaning.rank.unwrap_or(0))
        .max(meaning.top.len() + word_keys.len());
    let mut slots: Vec<Option<String>> = vec![None; length];
    for (place, key) in meaning.top.iter().enumerate() {
        slots[place] = Some(key.clone());
    }
    if let Some(rank) = meaning.rank {
        slots[rank - 1] = Some(meaning.item.clone());
    }
    let placed: HashSet<String> = slots.iter().flatten().cloned().collect();
    let middle = usize::midpoint(meaning.top.len(), length);
    let mut free: Vec<usize> = (0..length).filter(|&s| slots[s].is_none()).collect();
    free.sort_by_key(|&s| (s.abs_diff(middle), s));
    let unplaced = word_keys
        .iter()
        .filter(|k| !placed.contains(*k) && **k != meaning.item);
    for (key, slot) in unplaced.zip(free) {
        slots[slot] = Some(key.clone());
    }
    let near = slots
        .into_iter()
        .enumerate()
        .map(|(place, key)| key.unwrap_or_else(|| format!("?meaning {}", place + 1)))
        .collect();
    (worded, near, HashSet::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::findability::Place;

    fn place(item: &str, exact: usize, other: bool) -> Place {
        Place {
            item: item.into(),
            score: 1.0,
            exact,
            other,
        }
    }

    fn lookup(description: &str, rank: Option<usize>, top: &[&str], places: Vec<Place>) -> Lookup {
        Lookup {
            item: "a.txt".into(),
            description: description.into(),
            rank,
            top: top.iter().map(|&t| t.to_owned()).collect(),
            places,
        }
    }

    #[test]
    fn saved_rankings_are_merged_as_search_merges_them() {
        let words = lookup(
            "invoice 2057-4038",
            Some(2),
            &[],
            vec![
                place("collection Receipts", 0, true),
                place("b.txt", 0, false),
                place("a.txt", 2, false),
            ],
        );
        let meaning = lookup(
            "invoice 2057-4038",
            Some(3),
            &[],
            ["c.txt", "b.txt", "a.txt"]
                .iter()
                .map(|i| place(i, 0, false))
                .collect(),
        );
        assert!(saved(std::slice::from_ref(&meaning)));
        let merged = replay(
            std::slice::from_ref(&words),
            std::slice::from_ref(&meaning),
            3,
            Merge::STRONG,
        );
        assert_eq!(merged[0].rank, Some(1), "the exact match first");
        assert_eq!(merged[0].top, ["a.txt", "b.txt", "c.txt"], "{merged:?}");
        let words = lookup(
            "the invoice",
            Some(2),
            &[],
            vec![place("b.txt", 0, false), place("a.txt", 0, false)],
        );
        let meaning = Lookup {
            description: "the invoice".into(),
            ..meaning
        };
        let merged = replay(&[words], &[meaning], 3, Merge::STRONG);
        assert_eq!(merged[0].top, ["b.txt", "a.txt", "c.txt"], "{merged:?}");
    }

    #[test]
    fn an_older_run_is_estimated_from_its_first_five() {
        let words = lookup(
            "the lease",
            Some(7),
            &["b.txt", "c.txt", "d.txt", "e.txt", "f.txt"],
            Vec::new(),
        );
        let meaning = lookup(
            "the lease",
            Some(1),
            &["a.txt", "g.txt", "h.txt", "i.txt", "j.txt"],
            Vec::new(),
        );
        assert!(!saved(std::slice::from_ref(&meaning)));
        let (worded, near, others) = estimated_rankings(&words, &meaning, 20);
        let keys: Vec<&str> = worded.iter().map(|w| w.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "b.txt", "c.txt", "d.txt", "e.txt", "f.txt", "?words 6", "a.txt"
            ]
        );
        assert_eq!(near.len(), 20);
        assert_eq!(near[..5], ["a.txt", "g.txt", "h.txt", "i.txt", "j.txt"]);
        let middle = near.iter().position(|k| k == "b.txt").unwrap();
        assert!((10..=14).contains(&middle), "{near:?}");
        assert!(others.is_empty());
        let merged = replay(
            std::slice::from_ref(&words),
            std::slice::from_ref(&meaning),
            20,
            Merge::STRONG,
        );
        assert_eq!(merged[0].rank, Some(1));
        let merged = replay(&[words], &[meaning], 20, Merge::WEAK);
        assert_eq!(merged[0].top[0], "b.txt");
    }
}
