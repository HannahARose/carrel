//! The model benchmark (`docs/design/benchmark.md`): expected answers for a set of test
//! documents, and scores for an ingest run over them.
//!
//! The test set lives in `fixtures/bench`: an archive, the items to ingest, and
//! `answers.yaml`. [`check`] makes sure they agree; [`outcomes`] reads what a [`Run`] proposed;
//! [`score`] compares the two.
//!
//! [`findability`] measures search on the same test set, with hand-written descriptions of each
//! item in `descriptions.yaml`.
//!
//! [`librarian`] measures the librarian's answers to hand-written questions with known answers
//! in `librarian.yaml`, and [`question_value`] measures which questions, once answered, improve
//! routing most, and how well the question queue's ranking predicts it.

pub mod crawl;
pub mod findability;
pub mod librarian;
pub mod question_value;
pub mod restructure;

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::archive::Archive;
use crate::card::Sensitivity;
use crate::ingest::run::{Run, name};
use crate::ingest::sensitivity::floor;

/// The collection items go to when nowhere fits.
const HOLDING: &str = "Holding";

/// What a correct run proposes for one item.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    /// The collections it may go to.
    pub destinations: Vec<String>,
    /// Whether it fits more than one collection, so Holding is half right.
    #[serde(default)]
    pub ambiguous: bool,
    /// The level a correct run gives.
    pub sensitivity: Sensitivity,
    /// The creation date the text gives, as `YYYY-MM-DD`.
    pub date: Option<String>,
    /// Tags the card must have.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Other tags that are also right.
    #[serde(default)]
    pub may: Vec<String>,
    /// For an item that's easily mistaken for something else, words that say what it is: a
    /// reading is right when its title or summary has one of them, as a word or its plural.
    #[serde(default)]
    pub what: Vec<String>,
}

/// Every item's expected answer, by file name.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answers {
    /// The answers.
    pub items: BTreeMap<String, Answer>,
}

impl Answers {
    /// Reads `answers.yaml`.
    ///
    /// # Errors
    ///
    /// A message if it can't be read or parsed.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_saphyr::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// The problems with a test set: items without answers and answers without items,
/// destinations or tags the archive doesn't have, and expected levels below what the rules
/// alone give. Empty when the set is consistent.
///
/// # Errors
///
/// A message if the archive or the items can't be read.
pub fn check(answers: &Answers, archive: &Archive, items: &Path) -> Result<Vec<String>, String> {
    let collections = archive.collections().map_err(|e| e.to_string())?;
    let mut problems = Vec::new();
    let mut files = BTreeMap::new();
    for entry in std::fs::read_dir(items).map_err(|e| format!("{}: {e}", items.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let file = entry.file_name().to_string_lossy().into_owned();
        if !file.starts_with('.') {
            files.insert(file, entry.path());
        }
    }
    for file in files.keys() {
        if !answers.items.contains_key(file) {
            problems.push(format!("{file} has no answer"));
        }
    }
    let default = archive
        .doc()
        .sensitivity_default
        .unwrap_or(Sensitivity::Personal);
    for (file, answer) in &answers.items {
        let Some(path) = files.get(file) else {
            problems.push(format!("{file} is answered but isn't in the items"));
            continue;
        };
        if answer.destinations.is_empty() {
            problems.push(format!("{file} has no destination"));
        }
        let mut vocabulary = Vec::new();
        for destination in &answer.destinations {
            match collections.iter().find(|c| c.path == *destination) {
                Some(c) => vocabulary.extend(c.doc.tags.iter().map(|t| t.name.0.clone())),
                None => problems.push(format!("{file}: no collection {destination}")),
            }
        }
        for tag in answer.tags.iter().chain(&answer.may) {
            if !vocabulary.contains(tag) {
                problems.push(format!("{file}: {tag} isn't a tag of its destinations"));
            }
        }
        if let Some(date) = &answer.date
            && !crate::card::DateOrTime(date.clone()).is_valid()
        {
            problems.push(format!("{file}: {date} isn't a date"));
        }
        let text = item_text(path).map_err(|e| format!("{file}: {e}"))?;
        let ruled = floor(file, &text, default).level;
        if answer.sensitivity < ruled {
            problems.push(format!(
                "{file}: expects {:?} but the rules alone give {ruled:?}",
                answer.sensitivity
            ));
        }
        if answer.sensitivity < default {
            problems.push(format!(
                "{file}: expects {:?}, below the archive's default",
                answer.sensitivity
            ));
        }
    }
    Ok(problems)
}

/// An item's text for checking its expected level: a compressed file's names and the text of
/// everything inside it, otherwise whatever text the file holds, so binary items such as images
/// and 3D models are checked too.
fn item_text(path: &Path) -> Result<String, String> {
    let Some(listing) = crate::compressed::inspect(path).and_then(|i| i.listing) else {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    };
    let mut text = String::new();
    for entry in listing.files() {
        text.push_str(&entry.path);
        text.push('\n');
        let bytes = crate::compressed::read_entry(path, listing.format, &entry.path, 1 << 16)
            .map_err(|e| e.to_string())?;
        text.push_str(&String::from_utf8_lossy(&bytes));
        text.push('\n');
    }
    Ok(text)
}

/// What a run proposed for one item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    /// The item's file name.
    pub item: String,
    /// The collection chosen.
    pub destination: Option<String>,
    /// How sure routing was.
    pub confidence: Option<String>,
    /// The level given.
    pub sensitivity: Option<Sensitivity>,
    /// Why it was given that level.
    #[serde(default)]
    pub reasons: Vec<String>,
    /// The creation date drafted, as `YYYY-MM-DD`.
    pub date: Option<String>,
    /// The tags drafted, with any new one proposed.
    pub tags: Vec<String>,
    /// The title drafted.
    pub title: String,
    /// The summary drafted.
    pub summary: Option<String>,
    /// When the model wasn't sure what the item is, each reading as `title: summary`, the
    /// card's first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub readings: Vec<String>,
    /// Whether every stage ended with model output that passed validation, retries allowed.
    pub valid: bool,
    /// Whether every model output passed validation the first time.
    pub first_try: bool,
    /// Whether a model declined the item, as a safety refusal.
    #[serde(default)]
    pub declined: bool,
    /// What was left for the owner to settle.
    pub problems: Vec<String>,
    /// Model calls that didn't succeed, as `stage: result: message`.
    pub failures: Vec<String>,
}

/// What `run` proposed for each item.
#[must_use]
pub fn outcomes(run: &Run) -> Vec<Outcome> {
    run.candidates
        .iter()
        .enumerate()
        .map(|(i, candidate)| {
            let draft = run.drafts[i].as_ref();
            let route = run.routes[i].as_ref();
            let assessment = run.assessments[i].as_ref();
            let attempts = draft
                .into_iter()
                .flat_map(|d| &d.attempts)
                .chain(route.into_iter().flat_map(|r| &r.attempts))
                .chain(assessment.into_iter().flat_map(|a| &a.attempts));
            // Each stage's last call says how it ended; earlier ones were retries.
            let mut last: BTreeMap<&str, &str> = BTreeMap::new();
            let mut failures = Vec::new();
            for attempt in attempts {
                last.insert(&attempt.stage, &attempt.result);
                if attempt.result != "ok" {
                    failures.push(format!(
                        "{}: {}: {}",
                        attempt.stage,
                        attempt.result,
                        attempt.message.as_deref().unwrap_or("")
                    ));
                }
            }
            let problems = draft.map(|d| d.problems.clone()).unwrap_or_default();
            let mut tags: Vec<String> = draft
                .into_iter()
                .flat_map(|d| d.tags.iter().chain(&d.new_tag))
                .map(|t| t.0.clone())
                .collect();
            tags.dedup();
            Outcome {
                item: name(candidate),
                destination: route.map(|r| r.path.clone()),
                confidence: route.map(|r| format!("{:?}", r.confidence).to_lowercase()),
                sensitivity: assessment.map(|a| a.level),
                reasons: assessment.map(|a| a.reasons.clone()).unwrap_or_default(),
                date: draft
                    .and_then(|d| d.created.as_ref())
                    .map(|c| c.0.chars().take(10).collect()),
                tags,
                title: draft.map(|d| d.title.clone()).unwrap_or_default(),
                summary: draft.and_then(|d| d.summary.clone()),
                readings: draft
                    .into_iter()
                    .flat_map(|d| &d.readings)
                    .map(|r| format!("{}: {}", r.title, r.summary))
                    .collect(),
                valid: draft.is_some()
                    && !last.is_empty()
                    && last.values().all(|r| *r == "ok")
                    && problems.is_empty(),
                first_try: !last.is_empty() && failures.is_empty() && problems.is_empty(),
                declined: failures.iter().any(|f| f.contains(": declined:")),
                problems,
                failures,
            }
        })
        .collect()
}

/// How a level compares with the expected one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Rating {
    /// As expected.
    Exact,
    /// Higher than expected: safe, but more for the owner to review.
    Over,
    /// Lower than expected: the error that matters.
    Under,
    /// No level was given.
    Missing,
}

/// Whether a run read an item right, for an item whose answer says what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadingRating {
    /// The card's reading is right.
    Right,
    /// The card's is wrong, but another reading offered is right.
    Offered,
    /// No reading offered is right.
    Wrong,
}

/// How `outcome` read an item that `what` describes.
fn reading_rating(what: &[String], outcome: &Outcome) -> ReadingRating {
    let patterns: Vec<regex::Regex> = what
        .iter()
        .filter_map(|w| regex::Regex::new(&format!(r"(?i)\b{}s?\b", regex::escape(w))).ok())
        .collect();
    let right = |text: &str| patterns.iter().any(|p| p.is_match(text));
    let card = format!(
        "{} {}",
        outcome.title,
        outcome.summary.as_deref().unwrap_or_default()
    );
    if right(&card) {
        ReadingRating::Right
    } else if outcome.readings.iter().skip(1).any(|r| right(r)) {
        ReadingRating::Offered
    } else {
        ReadingRating::Wrong
    }
}

/// One item's scores.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[allow(clippy::struct_excessive_bools)] // independent yes-or-no scores, reported as they are
pub struct ItemScore {
    /// The item's file name.
    pub item: String,
    /// 1 for an acceptable destination, 0.5 for Holding when the item is ambiguous, else 0.
    pub routing: f64,
    /// The sensitivity rating against the expected level.
    pub sensitivity: Rating,
    /// Whether the date is the expected one, or absent when there is none.
    pub date: bool,
    /// Must-have tags found.
    pub tags_found: usize,
    /// Must-have tags expected.
    pub tags_expected: usize,
    /// Tags outside the must and may lists.
    pub tags_outside: Vec<String>,
    /// Whether every stage ended with model output that passed validation.
    pub valid: bool,
    /// Whether every model output passed the first time.
    pub first_try: bool,
    /// Whether a model declined it.
    pub declined: bool,
    /// Whether it was read right, when the answer says what it is.
    pub reading: Option<ReadingRating>,
    /// Whether the model offered other readings of it.
    pub readings_offered: bool,
}

/// A run's scores over every item.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Score {
    /// How many items were scored.
    pub items: usize,
    /// The share with valid output, retries allowed.
    pub valid: f64,
    /// The share whose output was valid the first time.
    pub first_try: f64,
    /// The share a model declined, as a safety refusal.
    pub declined: f64,
    /// The routing score, as a share.
    pub routing: f64,
    /// The share rated exactly.
    pub sensitivity_exact: f64,
    /// The share rated too low.
    pub under_rated: f64,
    /// How many restricted items were rated too low.
    pub under_rated_restricted: usize,
    /// The share with the expected date.
    pub date: f64,
    /// The share of must-have tags found.
    pub tags_found: f64,
    /// How many tags were outside the expected ones.
    pub tags_outside: usize,
    /// The share of items the model offered other readings of.
    pub readings_offered: f64,
    /// How many items whose answer says what they are had a wrong card.
    pub misread: usize,
    /// How many of those had the right reading among the others offered.
    pub misread_offered: usize,
    /// Seconds per item.
    pub seconds_per_item: f64,
}

/// The bar a model must meet to be recommended (`docs/design/benchmark.md`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    /// The least share with valid output.
    pub valid: f64,
    /// The least routing score.
    pub routing: f64,
    /// The most under-rated restricted items.
    pub under_rated_restricted: usize,
    /// The largest share under-rated.
    pub under_rated: f64,
    /// The most seconds per item.
    pub seconds_per_item: f64,
}

/// The bar the design sets.
pub const BAR: Bar = Bar {
    valid: 0.95,
    routing: 0.80,
    under_rated_restricted: 0,
    under_rated: 0.05,
    seconds_per_item: 30.0,
};

impl Score {
    /// The ways this score falls short of `bar`; empty when it meets it.
    #[must_use]
    pub fn shortfalls(&self, bar: &Bar) -> Vec<String> {
        let mut short = Vec::new();
        if self.valid < bar.valid {
            short.push(format!("valid output {}", percent(self.valid)));
        }
        if self.routing < bar.routing {
            short.push(format!("routing {}", percent(self.routing)));
        }
        if self.under_rated_restricted > bar.under_rated_restricted {
            short.push(format!(
                "{} restricted items under-rated",
                self.under_rated_restricted
            ));
        }
        if self.under_rated > bar.under_rated {
            short.push(format!("{} under-rated", percent(self.under_rated)));
        }
        if self.seconds_per_item > bar.seconds_per_item {
            short.push(format!("{:.1} s an item", self.seconds_per_item));
        }
        short
    }
}

/// A share as a whole percentage.
#[must_use]
pub fn percent(share: f64) -> String {
    format!("{:.0}%", share * 100.0)
}

/// How right filing an item with `answer` in `destination` is: 1 for an acceptable
/// collection, ½ for Holding when the item is ambiguous, otherwise 0.
#[must_use]
pub fn routing_points(answer: &Answer, destination: Option<&str>) -> f64 {
    match destination {
        Some(d) if answer.destinations.iter().any(|a| a == d) => 1.0,
        Some(HOLDING) if answer.ambiguous => 0.5,
        _ => 0.0,
    }
}

/// Scores `outcomes` against `answers`, given the run's total time. Items without an answer
/// are left out.
#[must_use]
#[allow(clippy::cast_precision_loss)] // counts of test items
pub fn score(answers: &Answers, outcomes: &[Outcome], seconds: f64) -> (Score, Vec<ItemScore>) {
    let mut items = Vec::new();
    for outcome in outcomes {
        let Some(answer) = answers.items.get(&outcome.item) else {
            continue;
        };
        let routing = routing_points(answer, outcome.destination.as_deref());
        let sensitivity = match outcome.sensitivity {
            None => Rating::Missing,
            Some(level) if level == answer.sensitivity => Rating::Exact,
            Some(level) if level > answer.sensitivity => Rating::Over,
            Some(_) => Rating::Under,
        };
        let tags_found = answer
            .tags
            .iter()
            .filter(|t| outcome.tags.contains(t))
            .count();
        let tags_outside = outcome
            .tags
            .iter()
            .filter(|t| !answer.tags.contains(t) && !answer.may.contains(t))
            .cloned()
            .collect();
        items.push((
            answer,
            ItemScore {
                item: outcome.item.clone(),
                routing,
                sensitivity,
                date: outcome.date == answer.date,
                tags_found,
                tags_expected: answer.tags.len(),
                tags_outside,
                valid: outcome.valid,
                first_try: outcome.first_try,
                declined: outcome.declined,
                reading: (!answer.what.is_empty()).then(|| reading_rating(&answer.what, outcome)),
                readings_offered: !outcome.readings.is_empty(),
            },
        ));
    }
    let n = items.len().max(1) as f64;
    let share =
        |f: &dyn Fn(&ItemScore) -> bool| items.iter().filter(|(_, s)| f(s)).count() as f64 / n;
    let under = |s: &ItemScore| matches!(s.sensitivity, Rating::Under | Rating::Missing);
    let expected: usize = items.iter().map(|(_, s)| s.tags_expected).sum();
    let found: usize = items.iter().map(|(_, s)| s.tags_found).sum();
    let summary = Score {
        items: items.len(),
        valid: share(&|s| s.valid),
        first_try: share(&|s| s.first_try),
        declined: share(&|s| s.declined),
        routing: items.iter().map(|(_, s)| s.routing).sum::<f64>() / n,
        sensitivity_exact: share(&|s| s.sensitivity == Rating::Exact),
        under_rated: share(&under),
        under_rated_restricted: items
            .iter()
            .filter(|(a, s)| a.sensitivity == Sensitivity::Restricted && under(s))
            .count(),
        date: share(&|s| s.date),
        tags_found: if expected == 0 {
            1.0
        } else {
            found as f64 / expected as f64
        },
        tags_outside: items.iter().map(|(_, s)| s.tags_outside.len()).sum(),
        readings_offered: share(&|s| s.readings_offered),
        misread: items
            .iter()
            .filter(|(_, s)| {
                matches!(
                    s.reading,
                    Some(ReadingRating::Offered | ReadingRating::Wrong)
                )
            })
            .count(),
        misread_offered: items
            .iter()
            .filter(|(_, s)| s.reading == Some(ReadingRating::Offered))
            .count(),
        seconds_per_item: seconds / n,
    };
    (summary, items.into_iter().map(|(_, s)| s).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answers() -> Answers {
        serde_saphyr::from_str(
            r#"
items:
  "a.txt":
    destinations: [Finances]
    sensitivity: restricted
    date: 2025-03-31
    tags: [bank]
  "b.txt":
    destinations: [Receipts, Manuals]
    ambiguous: true
    sensitivity: personal
    date: null
    tags: []
    may: [warranty]
"#,
        )
        .unwrap()
    }

    fn outcome(item: &str, destination: &str, level: Sensitivity, tags: &[&str]) -> Outcome {
        Outcome {
            item: item.into(),
            destination: Some(destination.into()),
            confidence: Some("medium".into()),
            sensitivity: Some(level),
            reasons: Vec::new(),
            date: None,
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            title: "t".into(),
            summary: None,
            readings: Vec::new(),
            valid: true,
            first_try: true,
            declined: false,
            problems: Vec::new(),
            failures: Vec::new(),
        }
    }

    #[test]
    fn a_perfect_run_meets_the_bar() {
        let mut a = outcome("a.txt", "Finances", Sensitivity::Restricted, &["bank"]);
        a.date = Some("2025-03-31".into());
        let b = outcome("b.txt", "Manuals", Sensitivity::Personal, &["warranty"]);
        let (score, items) = score(&answers(), &[a, b], 10.0);
        assert_eq!(score.items, 2);
        assert!((score.routing - 1.0).abs() < f64::EPSILON);
        assert!((score.date - 1.0).abs() < f64::EPSILON);
        assert!((score.tags_found - 1.0).abs() < f64::EPSILON);
        assert_eq!(score.tags_outside, 0);
        assert!((score.seconds_per_item - 5.0).abs() < f64::EPSILON);
        assert_eq!(score.shortfalls(&BAR), Vec::<String>::new());
        assert_eq!(items[1].sensitivity, Rating::Exact);
    }

    #[test]
    fn holding_is_half_right_only_when_ambiguous_and_under_rating_counts() {
        let a = outcome("a.txt", "Holding", Sensitivity::Sensitive, &["tax"]);
        let b = outcome("b.txt", "Holding", Sensitivity::Sensitive, &[]);
        let (score, items) = score(&answers(), &[a, b], 100.0);
        assert!((score.routing - 0.25).abs() < f64::EPSILON);
        assert_eq!(items[0].sensitivity, Rating::Under);
        assert_eq!(items[1].sensitivity, Rating::Over);
        assert_eq!(score.under_rated_restricted, 1);
        assert_eq!(items[0].tags_outside, ["tax"]);
        assert!((score.tags_found - 0.0).abs() < f64::EPSILON);
        let short = score.shortfalls(&BAR);
        assert!(
            short.iter().any(|s| s.starts_with("routing 25%")),
            "{short:?}"
        );
        assert!(short.iter().any(|s| s.contains("restricted")), "{short:?}");
        assert!(short.iter().any(|s| s.contains("s an item")), "{short:?}");
    }

    #[test]
    fn a_misread_item_counts_whether_the_right_reading_was_offered() {
        let mut answers = answers();
        answers.items.get_mut("b.txt").unwrap().what = vec!["keychain".into()];
        answers.items.get_mut("a.txt").unwrap().what = vec!["statement".into()];
        let mut a = outcome("a.txt", "Finances", Sensitivity::Restricted, &["bank"]);
        a.title = "Bank statement".into();
        let mut b = outcome("b.txt", "Manuals", Sensitivity::Personal, &[]);
        b.title = "Fish model".into();
        b.readings = vec![
            "Fish model: a fish".into(),
            "Keychain plate: a plate for a keychain".into(),
        ];
        let (score, items) = score(&answers, &[a, b.clone()], 1.0);
        assert_eq!(items[0].reading, Some(ReadingRating::Right));
        assert_eq!(items[1].reading, Some(ReadingRating::Offered));
        assert_eq!((score.misread, score.misread_offered), (1, 1));
        assert!((score.readings_offered - 0.5).abs() < f64::EPSILON);

        b.readings.pop();
        let (score, items) = score(&answers, &[b], 1.0);
        assert_eq!(items[0].reading, Some(ReadingRating::Wrong));
        assert_eq!((score.misread, score.misread_offered), (1, 0));
    }

    #[test]
    fn unknown_fields_in_answers_are_refused() {
        let wrong: Result<Answers, _> = serde_saphyr::from_str(
            "items:\n  a.txt:\n    destination: [X]\n    sensitivity: public\n    date: null\n",
        );
        assert!(wrong.is_err());
    }
}
