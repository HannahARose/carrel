//! The crawler benchmark (`docs/design/restructure.md`, "Measuring it"): on a deliberately
//! muddled copy of the test set's archive, how many misplaced items each model finds, how
//! often it calls a well-placed item misplaced, and whether it would put what it finds in the
//! right place. It sets the crawler's default model per memory size (the catalog's `crawler`
//! flag, [`crate::crawl::model`]).
//!
//! The muddle ([`Muddle`], `muddle.yaml`) files every item where `answers.yaml` expects it,
//! except: a collection that's missing, its items waiting elsewhere; one collection's items
//! dumped into another, which ends up overfilled; and single items scattered into collections
//! they have nothing to do with. What `answers.yaml` expects is the intended shape, which the
//! restructure benchmark scores against too.
//!
//! [`score_item`] scores one model's judgment of one item, [`summarize`] adds a model's up, and
//! [`CRAWL_BAR`] is what a model must reach to be recommended.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{Answers, HOLDING, findability};
use crate::archive::Archive;

/// How the archive is muddled (`muddle.yaml`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Muddle {
    /// A collection that isn't there.
    pub missing: Missing,
    /// A collection that ends up overfilled with another's items.
    pub overfilled: Overfilled,
    /// Single items filed where they don't belong, each with the collection it's put in.
    #[serde(default)]
    pub scattered: BTreeMap<String, String>,
}

/// A collection that isn't there, and where its items wait instead.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Missing {
    /// The collection, by path.
    pub collection: String,
    /// Where its items are filed instead.
    pub into: String,
}

/// One collection's items dumped into another.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overfilled {
    /// The collection that ends up overfilled.
    pub collection: String,
    /// The collection whose items are dumped into it, left there empty.
    pub from: String,
}

/// How an item came to be misplaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Misplaced {
    /// Its collection is missing.
    Missing,
    /// It was dumped into an overfilled collection.
    Overfilled,
    /// It was scattered on its own.
    Scattered,
}

impl Misplaced {
    /// The kind, for people.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Missing => "its collection missing",
            Self::Overfilled => "dumped in an overfilled collection",
            Self::Scattered => "scattered",
        }
    }
}

impl Muddle {
    /// Reads `muddle.yaml`.
    ///
    /// # Errors
    ///
    /// A message if it can't be read or parsed.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Where each answered item is filed in the muddled archive, and how it was misplaced, if
    /// it was. An item whose collection is missing or dumped elsewhere, or that's scattered,
    /// is misplaced only when where it ends up isn't one of its acceptable collections.
    #[must_use]
    pub fn places(&self, answers: &Answers) -> BTreeMap<String, (String, Option<Misplaced>)> {
        answers
            .items
            .iter()
            .filter_map(|(file, answer)| {
                let intended = answer.destinations.first()?;
                let (place, kind) = if let Some(to) = self.scattered.get(file) {
                    (to.clone(), Misplaced::Scattered)
                } else if *intended == self.missing.collection {
                    (self.missing.into.clone(), Misplaced::Missing)
                } else if *intended == self.overfilled.from {
                    (self.overfilled.collection.clone(), Misplaced::Overfilled)
                } else {
                    return Some((file.clone(), (intended.clone(), None)));
                };
                let misplaced = !answer.destinations.contains(&place);
                Some((file.clone(), (place, misplaced.then_some(kind))))
            })
            .collect()
    }

    /// What's wrong with the muddle against the test set: items and collections it names that
    /// aren't there, items scattered into the missing collection or where they already belong,
    /// a muddle that misplaces nothing, and a missing or emptied collection with too few items
    /// for literary warrant. Empty when it's consistent.
    #[must_use]
    pub fn check(&self, answers: &Answers, archive: &Archive) -> Vec<String> {
        let collections: Vec<String> = archive
            .collections()
            .unwrap_or_default()
            .into_iter()
            .map(|c| c.path)
            .collect();
        let mut problems = Vec::new();
        let mut named = vec![
            &self.missing.collection,
            &self.missing.into,
            &self.overfilled.collection,
            &self.overfilled.from,
        ];
        named.extend(self.scattered.values());
        for collection in named {
            if !collections.contains(collection) {
                problems.push(format!("there's no collection {collection}"));
            }
        }
        for (file, to) in &self.scattered {
            match answers.items.get(file) {
                None => problems.push(format!("{file} is scattered but isn't in the answers")),
                Some(answer) if answer.destinations.contains(to) => {
                    problems.push(format!("{file} is scattered into {to}, where it belongs"));
                }
                Some(_) if *to == self.missing.collection => {
                    problems.push(format!("{file} is scattered into the missing {to}"));
                }
                Some(_) => {}
            }
        }
        if self
            .places(answers)
            .values()
            .all(|(_, kind)| kind.is_none())
        {
            problems.push("the muddle misplaces nothing".into());
        }
        // A restructure proposal groups items into a collection only with literary warrant.
        let warrant = archive.doc().min_collection_items.unwrap_or(5);
        for (what, collection) in [
            ("missing", &self.missing.collection),
            ("emptied", &self.overfilled.from),
        ] {
            let items = answers
                .items
                .iter()
                .filter(|(file, a)| {
                    a.destinations.first() == Some(collection)
                        && !self.scattered.contains_key(*file)
                })
                .count();
            if u32::try_from(items).unwrap_or(u32::MAX) < warrant {
                problems.push(format!(
                    "the {what} {collection} has {items} items, fewer than the archive's min_collection_items ({warrant}), so no restructure could gather them"
                ));
            }
        }
        problems
    }
}

/// Files the test set in `items` into `work`, a copy of the archive `fixture` made the first
/// time without the muddle's missing collection, each item where the muddle puts it, with a
/// card made from its answer. Run again, it keeps each card's identifier, as
/// [`findability::build`] does.
///
/// # Errors
///
/// A message if a file can't be read or written, or the archive can't be opened.
pub fn build(
    fixture: &Path,
    items: &Path,
    answers: &Answers,
    muddle: &Muddle,
    work: &Path,
) -> Result<Archive, String> {
    if !work.join("ARCHIVE.md").is_file() {
        findability::copy_dir(fixture, work).map_err(|e| format!("{}: {e}", work.display()))?;
        let missing = work.join(&muddle.missing.collection);
        std::fs::remove_dir_all(&missing).map_err(|e| format!("{}: {e}", missing.display()))?;
    }
    let mut muddled = answers.clone();
    for (file, (place, _)) in muddle.places(answers) {
        if let Some(answer) = muddled.items.get_mut(&file) {
            answer.destinations = vec![place];
        }
    }
    findability::build(fixture, items, &muddled, &BTreeMap::new(), work)
}

/// What a model concluded about one item, as a run saves it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Judged {
    /// Whether it fits where it is; `None` when the model's answer wasn't usable.
    pub fits: Option<bool>,
    /// Where it would go instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub better: Option<String>,
    /// How sure it was, from 0 to 1.
    #[serde(default)]
    pub confidence: f64,
    /// Why, in its words.
    #[serde(default)]
    pub reason: String,
    /// How long it took, in seconds.
    pub seconds: f64,
    /// Why it couldn't judge it, when it couldn't.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// How one model's judgment of one item scored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemScore {
    /// The item's file name.
    pub item: String,
    /// Where it was in the muddled archive.
    pub place: String,
    /// How it was misplaced, if it was.
    pub misplaced: Option<Misplaced>,
    /// Whether the model said it doesn't fit.
    pub flagged: bool,
    /// For a misplaced item whose collection is there, whether the model would put it in one
    /// of its acceptable collections; `None` otherwise.
    pub placed_right: Option<bool>,
    /// Whether the model's answer was usable.
    pub usable: bool,
    /// How long it took, in seconds.
    pub seconds: f64,
}

/// Scores `judged`, the model's judgment of the item `file`, at `place` in the muddled
/// archive and misplaced as `misplaced` says, against its `answer`. `existing` are the muddled
/// archive's collections, so an item whose collection is missing isn't scored on where it
/// would go.
#[must_use]
pub fn score_item(
    file: &str,
    (place, misplaced): (&str, Option<Misplaced>),
    answer: &super::Answer,
    judged: &Judged,
    existing: &[String],
) -> ItemScore {
    let flagged = judged.fits == Some(false);
    let placeable = misplaced.is_some() && answer.destinations.iter().any(|d| existing.contains(d));
    // A misplaced item it didn't flag, it wouldn't move at all.
    let placed_right = placeable.then(|| {
        flagged
            && judged
                .better
                .as_ref()
                .is_some_and(|b| answer.destinations.contains(b))
    });
    ItemScore {
        item: file.to_owned(),
        place: place.to_owned(),
        misplaced,
        flagged,
        placed_right,
        usable: judged.fits.is_some(),
        seconds: judged.seconds,
    }
}

/// One model's scores, added up.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CrawlScore {
    /// Items judged.
    pub items: usize,
    /// Of those, the misplaced ones.
    pub misplaced: usize,
    /// Misplaced items the model said don't fit.
    pub found: usize,
    /// The share of misplaced items found.
    pub recall: f64,
    /// Items the model said don't fit.
    pub flagged: usize,
    /// The share of those that were misplaced.
    pub precision: f64,
    /// Misplaced items whose collection is there.
    pub placeable: usize,
    /// Of those, the ones the model would put in an acceptable collection.
    pub placed_right: usize,
    /// The share of placeable items it would put right: what moving its findings would get
    /// right.
    pub placement: f64,
    /// Recall by how items were misplaced: found and misplaced.
    pub by_kind: BTreeMap<Misplaced, (usize, usize)>,
    /// Answers that weren't usable.
    pub unusable: usize,
    /// Seconds per item, on average.
    pub seconds_per_item: f64,
}

/// `scores` added up.
#[must_use]
#[allow(clippy::cast_precision_loss)] // item counts
pub fn summarize(scores: &[ItemScore]) -> CrawlScore {
    let share = |part: usize, whole: usize| {
        if whole == 0 {
            0.0
        } else {
            part as f64 / whole as f64
        }
    };
    let mut score = CrawlScore {
        items: scores.len(),
        ..CrawlScore::default()
    };
    for item in scores {
        if !item.usable {
            score.unusable += 1;
        }
        if item.flagged {
            score.flagged += 1;
        }
        if let Some(kind) = item.misplaced {
            score.misplaced += 1;
            let counts = score.by_kind.entry(kind).or_default();
            counts.1 += 1;
            if item.flagged {
                score.found += 1;
                counts.0 += 1;
            }
        }
        if let Some(right) = item.placed_right {
            score.placeable += 1;
            if right {
                score.placed_right += 1;
            }
        }
    }
    score.recall = share(score.found, score.misplaced);
    score.precision = share(score.found, score.flagged);
    score.placement = share(score.placed_right, score.placeable);
    score.seconds_per_item = if scores.is_empty() {
        0.0
    } else {
        scores.iter().map(|s| s.seconds).sum::<f64>() / scores.len() as f64
    };
    score
}

/// What a model must reach to be recommended as the crawler's.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CrawlBar {
    /// The least share of misplaced items it finds.
    pub recall: f64,
    /// The least share of what it flags that's misplaced.
    pub precision: f64,
    /// The least share of what it finds that it would put right.
    pub placement: f64,
    /// The most seconds an item may take on the reference machine.
    pub seconds_per_item: f64,
}

/// The bar, a starting point as the filing and librarian bars were: a crawler that finds most
/// misplaced items, rarely raises a false alarm (each one is a finding the owner rejects), and
/// would put most of what it finds right, at a pace the device's idle time allows.
pub const CRAWL_BAR: CrawlBar = CrawlBar {
    recall: 0.70,
    precision: 0.60,
    placement: 0.70,
    seconds_per_item: 30.0,
};

impl CrawlScore {
    /// Where it falls short of `bar`, in words; empty when it meets it.
    #[must_use]
    pub fn shortfalls(&self, bar: &CrawlBar) -> Vec<String> {
        let mut short = Vec::new();
        if self.recall < bar.recall {
            short.push(format!("recall {}", super::percent(self.recall)));
        }
        if self.precision < bar.precision {
            short.push(format!("precision {}", super::percent(self.precision)));
        }
        if self.placement < bar.placement {
            short.push(format!("placement {}", super::percent(self.placement)));
        }
        if self.seconds_per_item > bar.seconds_per_item {
            short.push(format!("{:.1} s an item", self.seconds_per_item));
        }
        short
    }
}

/// Whether an item expected in `answer`'s collections counts in the benchmark: items that
/// belong in Holding don't, since nothing fits there.
#[must_use]
pub fn counts(answer: &super::Answer) -> bool {
    answer.destinations.first().is_some_and(|d| d != HOLDING)
}
