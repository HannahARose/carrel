//! The findability test (`docs/design/search.md`, "Measuring it"): for each item in the
//! benchmark's test set, a few ways a person might describe it, written by hand in
//! `descriptions.yaml`, and whether search brings the item back for each.
//!
//! [`build`] files every item of the test set into a copy of its archive, in its expected
//! collection, with a card made from the expected answers (and, if given, a model's drafted
//! title and summary). [`look_up`] runs each description through a ranking and notes where the
//! item came back; [`score`] gives top-five recall and mean reciprocal rank. The rankings
//! compared are words alone and both merged, as search gives them ([`by_search`]), and meaning
//! alone ([`by_meaning`]).

pub mod replay;

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::Answers;
use crate::archive::Archive;
use crate::card::{Checksum, Id};
use crate::card_file::CardFile;
use crate::search::query::Query;
use crate::search::{Found, Ranking, Results, Search};

/// How many places a description's item counts in: top-five recall.
pub const TOP: usize = 5;

/// How far down the results an item is looked for; one further down counts as not found.
pub const DEPTH: usize = 100;

/// The fewest descriptions an item has.
pub const FEWEST: usize = 2;

/// The most descriptions an item has.
pub const MOST: usize = 4;

/// Every item's descriptions, by file name, as `descriptions.yaml` gives them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptions {
    /// The descriptions.
    pub items: BTreeMap<String, Vec<String>>,
}

impl Descriptions {
    /// Reads `descriptions.yaml`.
    ///
    /// # Errors
    ///
    /// A message if it can't be read or parsed.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// How many descriptions there are in all.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.values().map(Vec::len).sum()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The problems with the descriptions: items with too few or too many, descriptions of items
/// that aren't answered, repeated ones, ones that would be read as filters, and ones that
/// copy the item's file name. Empty when they're fine.
#[must_use]
pub fn check(descriptions: &Descriptions, answers: &Answers) -> Vec<String> {
    let mut problems = Vec::new();
    for file in answers.items.keys() {
        let count = descriptions.items.get(file).map_or(0, Vec::len);
        if !(FEWEST..=MOST).contains(&count) {
            problems.push(format!(
                "{file} has {count} descriptions; each item needs {FEWEST} to {MOST}"
            ));
        }
    }
    for (file, list) in &descriptions.items {
        if !answers.items.contains_key(file) {
            problems.push(format!("{file} is described but isn't in the answers"));
        }
        let stem = stem(file).to_lowercase();
        for (n, description) in list.iter().enumerate() {
            if description.trim().is_empty() {
                problems.push(format!("{file}: description {} is empty", n + 1));
            } else if list[..n].contains(description) {
                problems.push(format!("{file}: “{description}” is there twice"));
            } else if description.to_lowercase().contains(&stem) {
                problems.push(format!("{file}: “{description}” copies the file name"));
            }
            match Query::parse(description) {
                Ok(query) if query.filters.is_empty() => {}
                _ => problems.push(format!("{file}: “{description}” would be read as a filter")),
            }
        }
    }
    problems
}

/// A file name without its extension.
fn stem(file: &str) -> &str {
    Path::new(file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(file)
}

/// A model's draft of an item's card, from a benchmark run's `results.json`, used in place
/// of the file name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drafted {
    /// The title drafted.
    pub title: String,
    /// The summary drafted.
    pub summary: Option<String>,
}

/// The cards drafted in a benchmark run's `results.json`, by item: from the first model in it
/// that has each item. Drafts without a title are left out.
#[must_use]
pub fn drafts(results: &serde_json::Value) -> BTreeMap<String, Drafted> {
    let mut found = BTreeMap::new();
    let models = results["models"].as_array().map_or(&[][..], Vec::as_slice);
    for model in models {
        let items = model["items"].as_array().map_or(&[][..], Vec::as_slice);
        for item in items {
            let outcome = &item["outcome"];
            let (Some(name), Some(title)) = (outcome["item"].as_str(), outcome["title"].as_str())
            else {
                continue;
            };
            if title.trim().is_empty() {
                continue;
            }
            found.entry(name.to_owned()).or_insert_with(|| Drafted {
                title: title.to_owned(),
                summary: outcome["summary"].as_str().map(str::to_owned),
            });
        }
    }
    found
}

/// Copies `from` into `to`, folders and all.
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// The identifier of the card at `path`, if there's one that parses.
fn existing_id(path: &Path) -> Option<Id> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(CardFile::parse(&text).ok()?.card().id)
}

/// A card for `file`, `size` bytes with `checksum`, as its expected `answer` says, titled by
/// its draft or else its file name.
fn card_text(
    id: Id,
    file: &str,
    answer: &super::Answer,
    draft: Option<&Drafted>,
    checksum: &Checksum,
    size: u64,
) -> String {
    let quote = |s: &str| serde_json::Value::from(s).to_string();
    let title = draft.map_or_else(|| stem(file).to_owned(), |d| d.title.clone());
    let mut fields = format!(
        "---\ncarrel: card\nformat: \"1.0\"\nid: {id}\nkind: file\ntitle: {}\nitem: {}\n",
        quote(&title),
        quote(file)
    );
    if let Some(summary) = draft.and_then(|d| d.summary.as_deref()) {
        writeln!(fields, "summary: {}", quote(summary)).unwrap();
    }
    if !answer.tags.is_empty() {
        let tags: Vec<String> = answer.tags.iter().map(|t| quote(t)).collect();
        writeln!(fields, "tags: [{}]", tags.join(", ")).unwrap();
    }
    if let Some(date) = &answer.date {
        writeln!(fields, "dates:\n  document: {}", quote(date)).unwrap();
    }
    let level = serde_json::to_value(answer.sensitivity)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    writeln!(
        fields,
        "sensitivity: {level}\nchecksum: {}\nsize: {size}\n---",
        quote(&checksum.0)
    )
    .unwrap();
    fields
}

/// Files every answered item of the test set in `items` into `work`, a copy of the test
/// archive `fixture` made the first time, each in its first expected collection with a card
/// made from its answer and its draft, if `drafts` has one. Run again, it keeps each card's
/// identifier and rewrites only what changed, so an embedding index of `work` stays current
/// for items whose cards are the same.
///
/// # Errors
///
/// A message if a file can't be read or written, or the archive can't be opened.
pub fn build(
    fixture: &Path,
    items: &Path,
    answers: &Answers,
    drafts: &BTreeMap<String, Drafted>,
    work: &Path,
) -> Result<Archive, String> {
    let io = |path: &Path| {
        let path = path.to_owned();
        move |e: std::io::Error| format!("{}: {e}", path.display())
    };
    if !work.join("ARCHIVE.md").is_file() {
        copy_dir(fixture, work).map_err(io(work))?;
    }
    for (file, answer) in &answers.items {
        let Some(collection) = answer.destinations.first() else {
            continue;
        };
        let source = items.join(file);
        let target = work.join(collection).join(file);
        let checksum = Checksum::of_file(&source).map_err(io(&source))?;
        if Checksum::of_file(&target).ok().as_ref() != Some(&checksum) {
            std::fs::copy(&source, &target).map_err(io(&target))?;
        }
        let size = std::fs::metadata(&target).map_err(io(&target))?.len();
        let card_path = work.join(collection).join(format!("{file}.md"));
        let id = existing_id(&card_path).unwrap_or_else(Id::new_v7);
        let text = card_text(id, file, answer, drafts.get(file), &checksum, size);
        if std::fs::read_to_string(&card_path).ok().as_deref() != Some(text.as_str()) {
            std::fs::write(&card_path, text).map_err(io(&card_path))?;
        }
    }
    Archive::open(work).map_err(|e| e.to_string())
}

/// Each item's file name, by its identifier, in `archive`.
///
/// # Errors
///
/// A message if the archive can't be listed.
pub fn names(archive: &Archive) -> Result<HashMap<Id, String>, String> {
    Ok(archive
        .items()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(|item| {
            let name = item.item?.rsplit('/').next()?.to_owned();
            Some((item.id, name))
        })
        .collect())
}

/// The file names of the items in `results`, best first: collections and glossary facts that
/// came back are left out, so a place is among items only.
#[must_use]
pub fn items_in(results: &Results) -> Vec<String> {
    results
        .hits
        .iter()
        .filter_map(|hit| match &hit.found {
            Found::Item(item) => item.item.as_ref()?.rsplit('/').next().map(str::to_owned),
            _ => None,
        })
        .collect()
}

/// The items `search` finds for `description`, best first: by words alone, or, with a
/// meaning ranking [added](Search::with_meaning), by both merged as search merges them.
#[must_use]
pub fn by_search(search: &Search, description: &str) -> Vec<String> {
    let Ok(query) = Query::parse(description) else {
        return Vec::new();
    };
    items_in(&search.search(&query, DEPTH))
}

/// Meaning alone: the items `ranking` puts nearest `description`, by their file `names`.
#[must_use]
pub fn by_meaning<S: std::hash::BuildHasher>(
    ranking: &dyn Ranking,
    names: &HashMap<Id, String, S>,
    description: &str,
) -> Vec<String> {
    ranking
        .rank(description, &|_| true, DEPTH)
        .into_iter()
        .filter_map(|ranked| names.get(&ranked.id).cloned())
        .collect()
}

/// One place in a ranking, as a run saves it so merges can be replayed without the models
/// ([`replay`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place {
    /// The item's file name; for a collection, glossary fact or card without contents, what
    /// it is.
    pub item: String,
    /// Its score in the ranking: BM25 for the words, cosine similarity for meaning.
    pub score: f64,
    /// How many of the query's rare exact names and numbers it has, among the words.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub exact: usize,
    /// Whether it isn't an item with contents, so it takes a place in the merge but doesn't
    /// count as one among the items found.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub other: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's signature
fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// The word ranking alone for `description`, every document it found up to [`DEPTH`] with
/// its score and exact matches, as [`replay`] needs it.
#[must_use]
pub fn word_places(search: &Search, description: &str) -> Vec<Place> {
    let Ok(query) = Query::parse(description) else {
        return Vec::new();
    };
    search
        .by_words(&query)
        .into_iter()
        .take(DEPTH)
        .map(|(found, score, exact)| {
            let (item, other) = match &found {
                Found::Item(item) => match item.item.as_ref().and_then(|i| i.rsplit('/').next()) {
                    Some(name) => (name.to_owned(), false),
                    None => (format!("card {}", item.title), true),
                },
                Found::Collection { path, .. } => (format!("collection {path}"), true),
                Found::Glossary { term } => (format!("glossary {term}"), true),
            };
            Place {
                item,
                score,
                exact,
                other,
            }
        })
        .collect()
}

/// The meaning ranking alone for `description`, up to [`DEPTH`] items by their file `names`,
/// each with its similarity, as [`replay`] needs it.
#[must_use]
pub fn meaning_places<S: std::hash::BuildHasher>(
    ranking: &dyn Ranking,
    names: &HashMap<Id, String, S>,
    description: &str,
) -> Vec<Place> {
    ranking
        .rank(description, &|_| true, DEPTH)
        .into_iter()
        .filter_map(|ranked| {
            Some(Place {
                item: names.get(&ranked.id)?.clone(),
                score: ranked.score,
                exact: 0,
                other: false,
            })
        })
        .collect()
}

/// Saves the ranking `places` gives for each of `lookups`' descriptions with it, so a later
/// [`replay`] can merge it.
pub fn keep_places(lookups: &mut [Lookup], places: &mut dyn FnMut(&str) -> Vec<Place>) {
    for lookup in lookups {
        lookup.places = places(&lookup.description);
    }
}

/// Where one description's item came back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lookup {
    /// The item described, by its file name.
    pub item: String,
    /// The description, as a query.
    pub description: String,
    /// Its place among the items found, from 1; `None` when it wasn't in the first [`DEPTH`].
    pub rank: Option<usize>,
    /// The items found first, up to [`TOP`], for seeing what came back instead.
    pub top: Vec<String>,
    /// The whole ranking, up to [`DEPTH`], for the words alone and meaning alone, so merges
    /// can be replayed ([`replay`]). Runs before 2026-11 didn't save it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub places: Vec<Place>,
}

impl Lookup {
    /// Whether the item was in the top five.
    #[must_use]
    pub fn found(&self) -> bool {
        self.rank.is_some_and(|r| r <= TOP)
    }
}

/// Runs every description through `find`, which gives the items found for a query, best
/// first, and notes where each description's item came.
pub fn look_up(
    descriptions: &Descriptions,
    find: &mut dyn FnMut(&str) -> Vec<String>,
) -> Vec<Lookup> {
    let mut lookups = Vec::new();
    for (item, list) in &descriptions.items {
        for description in list {
            let found = find(description);
            lookups.push(Lookup {
                item: item.clone(),
                description: description.clone(),
                rank: found
                    .iter()
                    .take(DEPTH)
                    .position(|f| f == item)
                    .map(|p| p + 1),
                top: found.into_iter().take(TOP).collect(),
                places: Vec::new(),
            });
        }
    }
    lookups
}

/// A ranking's scores over every description.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Findability {
    /// How many descriptions were looked up.
    pub descriptions: usize,
    /// The share whose item came back in the top five.
    pub top_five: f64,
    /// The mean of 1 / the item's place, 0 for one not found.
    pub reciprocal_rank: f64,
}

/// Top-five recall and mean reciprocal rank over `lookups`.
#[must_use]
#[allow(clippy::cast_precision_loss)] // counts of descriptions
pub fn score(lookups: &[Lookup]) -> Findability {
    let n = lookups.len().max(1) as f64;
    Findability {
        descriptions: lookups.len(),
        top_five: lookups.iter().filter(|l| l.found()).count() as f64 / n,
        reciprocal_rank: lookups
            .iter()
            .filter_map(|l| l.rank)
            .map(|r| 1.0 / r as f64)
            .sum::<f64>()
            / n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(rank: Option<usize>) -> Lookup {
        Lookup {
            item: "a.txt".into(),
            description: "d".into(),
            rank,
            top: Vec::new(),
            places: Vec::new(),
        }
    }

    #[test]
    fn recall_counts_the_top_five_and_reciprocal_rank_every_place() {
        let s = score(&[
            lookup(Some(1)),
            lookup(Some(2)),
            lookup(Some(6)),
            lookup(None),
        ]);
        assert_eq!(s.descriptions, 4);
        assert!((s.top_five - 0.5).abs() < 1e-9);
        let expected = (1.0 + 0.5 + 1.0 / 6.0) / 4.0;
        assert!((s.reciprocal_rank - expected).abs() < 1e-9);
        let none = score(&[]);
        assert!(none.top_five.abs() < f64::EPSILON);
    }

    #[test]
    fn look_up_finds_each_items_place() {
        let descriptions = Descriptions {
            items: BTreeMap::from([
                ("a.txt".into(), vec!["first".into(), "second".into()]),
                ("b.txt".into(), vec!["third".into()]),
            ]),
        };
        let mut find = |q: &str| match q {
            "first" => vec!["a.txt".to_owned()],
            "second" => vec!["b.txt".into(), "c.txt".into(), "a.txt".into()],
            _ => Vec::new(),
        };
        let lookups = look_up(&descriptions, &mut find);
        let ranks: Vec<Option<usize>> = lookups.iter().map(|l| l.rank).collect();
        assert_eq!(ranks, [Some(1), Some(3), None]);
        assert_eq!(lookups[1].top, ["b.txt", "c.txt", "a.txt"]);
    }

    #[test]
    fn drafts_come_from_the_first_model_that_has_the_item() {
        let results = serde_json::json!({"models": [
            {"items": [{"outcome": {"item": "a.txt", "title": "First", "summary": "S"}}]},
            {"items": [
                {"outcome": {"item": "a.txt", "title": "Second", "summary": null}},
                {"outcome": {"item": "b.txt", "title": "", "summary": null}},
                {"outcome": {"item": "c.txt", "title": "C", "summary": null}},
            ]},
        ]});
        let found = drafts(&results);
        assert_eq!(found["a.txt"].title, "First");
        assert_eq!(found["a.txt"].summary.as_deref(), Some("S"));
        assert!(!found.contains_key("b.txt"), "no title, no draft");
        assert_eq!(found["c.txt"].summary, None);
    }
}
