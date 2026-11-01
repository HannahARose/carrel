//! Question value (`docs/design/librarian.md`, "Measuring it"): which of the questions Carrel
//! asks, once answered with the known answer, improve routing the most, and whether the
//! queue's ranking ([`crate::questions::rank`]) puts those first.
//!
//! [`value_questions`] ingests the test set in one batch, as ingest would, and ranks the
//! questions it raises. For each, [`known_answer`] gives the answer the expected destinations
//! imply: the collection most of its items belong in, or for a term, a glossary fact saying so.
//! Answering re-routes its items: a collection answer points them there, as answering does; a
//! fact is added to the glossary and its items are ingested again with it. The gain is how
//! many routing points that wins ([`routing_points`](super::routing_points)). [`summarize_value`]
//! compares the ranking with the gains as a rank correlation, and gives the routing after
//! answering the k highest-ranked questions beside the best any k could give.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Answers, HOLDING, routing_points};
use crate::archive::{Archive, Collection};
use crate::card::Id;
use crate::ingest::IntakeOptions;
use crate::ingest::run::Run;
use crate::providers::Registry;
use crate::questions::rank::{Context, Ranked, rank};
use crate::questions::{GLOSSARY, QuestionEntry, QuestionKind, add_fact};
use crate::review::Confidence;

/// Where an item was proposed to go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    /// The item's file name.
    pub item: String,
    /// The proposal's identifier, as questions name it.
    pub id: Id,
    /// The collection proposed.
    pub destination: String,
    /// How sure the proposal is.
    pub confidence: Confidence,
}

/// Each item's proposal in `run`.
#[must_use]
pub fn proposals_of(run: &Run) -> Vec<Proposal> {
    run.entries
        .iter()
        .flatten()
        .map(|e| Proposal {
            item: e
                .source
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            id: e.id,
            destination: e.destination.path.clone(),
            confidence: e.destination.confidence,
        })
        .collect()
}

/// The answer the test set's expected destinations give a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum KnownAnswer {
    /// A collection, for a question answered with one: its items go there.
    Collection {
        /// The collection's path.
        collection: String,
    },
    /// A glossary fact, for a term: its items are ingested again with it.
    Fact {
        /// The term.
        term: String,
        /// What it means, as the owner would say it.
        fact: String,
        /// The collection the fact points to.
        collection: String,
    },
}

impl KnownAnswer {
    /// The answer, for people.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Collection { collection } => collection.clone(),
            Self::Fact { term, fact, .. } => format!("{term}: {fact}"),
        }
    }
}

/// The proposals `question` is about, waiting on it or filed.
fn items_of<'p>(question: &QuestionEntry, proposals: &'p [Proposal]) -> Vec<&'p Proposal> {
    proposals
        .iter()
        .filter(|p| question.items.contains(&p.id) || question.about.contains(&p.id))
        .collect()
}

/// The known answer to `question`: the collection most of its items are expected in (the
/// first acceptable one of each; ties go to the first by path), as a collection for a
/// collection, group or sender question, or as a glossary fact for a term. None for an item
/// question, which doesn't change where anything goes, or when none of its items is answered.
#[must_use]
pub fn known_answer(
    question: &QuestionEntry,
    proposals: &[Proposal],
    answers: &Answers,
    collections: &[Collection],
) -> Option<KnownAnswer> {
    if question.kind == QuestionKind::Item {
        return None;
    }
    let mut votes: BTreeMap<&str, usize> = BTreeMap::new();
    for p in items_of(question, proposals) {
        if let Some(first) = answers
            .items
            .get(&p.item)
            .and_then(|a| a.destinations.first())
        {
            *votes.entry(first.as_str()).or_default() += 1;
        }
    }
    let most = votes.values().copied().max()?;
    let collection = votes
        .into_iter()
        .find(|(_, n)| *n == most)
        .map(|(c, _)| c.to_owned())?;
    if question.kind != QuestionKind::Term {
        return Some(KnownAnswer::Collection { collection });
    }
    let purpose = collections
        .iter()
        .find(|c| c.path == collection)
        .and_then(|c| {
            c.doc
                .description
                .clone()
                .or_else(|| Some(c.doc.title.clone()))
        })
        .filter(|p| !p.trim().is_empty() && *p != collection);
    let fact = match (collection.as_str(), purpose) {
        (HOLDING, _) => "nothing I file anywhere in particular; leave its items in Holding".into(),
        (c, Some(purpose)) => format!("items named for it belong in {c} ({})", purpose.trim()),
        (c, None) => format!("items named for it belong in {c}"),
    };
    Some(KnownAnswer::Fact {
        term: question.subject.clone(),
        fact,
        collection,
    })
}

/// The context the queue is ranked with: `archive`'s glossary, rules and collections, with
/// `proposals` open.
#[must_use]
pub fn ranking_context(
    archive: &Archive,
    proposals: &[Proposal],
    collections: &[Collection],
) -> Context {
    let mut context = Context::new(archive, &[], HashSet::new(), collections.to_vec());
    context.proposals = proposals.iter().map(|p| (p.id, p.confidence)).collect();
    context
}

/// One question, answered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Valued {
    /// The question's identifier.
    pub id: Id,
    /// What it asks.
    pub kind: QuestionKind,
    /// What it's about.
    pub subject: String,
    /// The question, in words.
    pub question: String,
    /// Where the ranking put it, from 1.
    pub rank: usize,
    /// What the ranking said an answer is worth.
    pub ranked_value: f64,
    /// How many items it's about.
    pub items: usize,
    /// The known answer, if it has one.
    pub answer: Option<KnownAnswer>,
    /// Where answering put its items, by file name, for those that moved.
    pub moved: BTreeMap<String, String>,
    /// The routing points answering won (or, below 0, lost).
    pub gain: f64,
}

/// Re-routes a question's items once it's answered: given the question, its known answer and
/// its items, where each item goes now, by file name.
pub type Reroute<'a> = dyn FnMut(&QuestionEntry, &KnownAnswer, &[&Proposal]) -> Result<BTreeMap<String, String>, String>
    + 'a;

/// Where a collection answer sends `items`: all of them to `collection`.
#[must_use]
pub fn send_to(collection: &str, items: &[&Proposal]) -> BTreeMap<String, String> {
    items
        .iter()
        .map(|p| (p.item.clone(), collection.to_owned()))
        .collect()
}

/// Answers each of `ranked`'s open questions with its known answer, alone, re-routing its
/// items with `reroute`, and notes the routing points each wins, in the ranking's order.
///
/// # Errors
///
/// What `reroute` gives when it fails.
pub fn value_each(
    ranked: &Ranked,
    proposals: &[Proposal],
    answers: &Answers,
    collections: &[Collection],
    reroute: &mut Reroute<'_>,
) -> Result<Vec<Valued>, String> {
    let mut valued = Vec::new();
    for (n, weighed) in ranked.open.iter().enumerate() {
        let question = &weighed.question;
        let items = items_of(question, proposals);
        let answer = known_answer(question, proposals, answers, collections);
        let mut moved = BTreeMap::new();
        let mut gain = 0.0;
        if let Some(answer) = &answer {
            for (item, destination) in reroute(question, answer, &items)? {
                let Some(before) = items.iter().find(|p| p.item == item) else {
                    continue;
                };
                if before.destination == destination {
                    continue;
                }
                if let Some(expected) = answers.items.get(&item) {
                    gain += routing_points(expected, Some(&destination))
                        - routing_points(expected, Some(&before.destination));
                }
                moved.insert(item, destination);
            }
        }
        valued.push(Valued {
            id: question.id,
            kind: question.kind,
            subject: question.subject.clone(),
            question: question.question.clone(),
            rank: n + 1,
            ranked_value: weighed.value,
            items: items.len(),
            answer,
            moved,
            gain,
        });
    }
    Ok(valued)
}

/// The routing after answering the first k of `order`, for each k.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopK {
    /// How many questions were answered.
    pub k: usize,
    /// The routing score after answering the k highest-ranked.
    pub ranked: f64,
    /// The best routing score answering any k gives, taking them by gain.
    pub best: f64,
}

/// What answering the questions does to routing, and how well the ranking predicts it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionValue {
    /// How many items were routed.
    pub items: usize,
    /// How many questions the batch raised and the ranking kept open.
    pub questions: usize,
    /// How many of them have a known answer.
    pub answerable: usize,
    /// How many questions the ranking set aside, merged or found answered.
    pub set_aside: usize,
    /// The routing score before any answer.
    pub routing_before: f64,
    /// The routing score after answering every question.
    pub routing_after: f64,
    /// The rank correlation (Spearman's) between what the ranking said each answer is worth
    /// and what it won; none with fewer than two questions, or when either doesn't vary.
    pub correlation: Option<f64>,
    /// The routing after answering the k highest-ranked, beside the best k, for each k.
    pub top_k: Vec<TopK>,
}

/// The routing score over `proposals` with `placed` overriding where items go.
#[allow(clippy::cast_precision_loss)] // counts of test items
fn routing_with(
    proposals: &[Proposal],
    placed: &BTreeMap<String, String>,
    answers: &Answers,
) -> f64 {
    if proposals.is_empty() {
        return 0.0;
    }
    let points: f64 = proposals
        .iter()
        .filter_map(|p| {
            let answer = answers.items.get(&p.item)?;
            let destination = placed.get(&p.item).unwrap_or(&p.destination);
            Some(routing_points(answer, Some(destination)))
        })
        .sum();
    points / proposals.len() as f64
}

/// The routing score after answering `order`'s questions, one more each step: an item an
/// earlier answer placed stays where that put it.
fn cumulative(order: &[&Valued], proposals: &[Proposal], answers: &Answers) -> Vec<f64> {
    let mut placed = BTreeMap::new();
    order
        .iter()
        .map(|v| {
            for (item, destination) in &v.moved {
                placed
                    .entry(item.clone())
                    .or_insert_with(|| destination.clone());
            }
            routing_with(proposals, &placed, answers)
        })
        .collect()
}

/// Adds up `valued`, in the ranking's order, over `proposals`. `set_aside` is how many
/// questions the ranking didn't keep open.
#[must_use]
pub fn summarize_value(
    valued: &[Valued],
    proposals: &[Proposal],
    answers: &Answers,
    set_aside: usize,
) -> QuestionValue {
    let ranked: Vec<&Valued> = valued.iter().collect();
    let mut by_gain = ranked.clone();
    by_gain.sort_by(|a, b| b.gain.total_cmp(&a.gain).then(a.rank.cmp(&b.rank)));
    let along = cumulative(&ranked, proposals, answers);
    let best = cumulative(&by_gain, proposals, answers);
    let before = routing_with(proposals, &BTreeMap::new(), answers);
    QuestionValue {
        items: proposals.len(),
        questions: valued.len(),
        answerable: valued.iter().filter(|v| v.answer.is_some()).count(),
        set_aside,
        routing_before: before,
        routing_after: along.last().copied().unwrap_or(before),
        correlation: spearman(
            &valued.iter().map(|v| v.ranked_value).collect::<Vec<_>>(),
            &valued.iter().map(|v| v.gain).collect::<Vec<_>>(),
        ),
        top_k: along
            .iter()
            .zip(&best)
            .enumerate()
            .map(|(i, (r, b))| TopK {
                k: i + 1,
                ranked: *r,
                best: *b,
            })
            .collect(),
    }
}

/// Spearman's rank correlation between `xs` and `ys`, with tied values given their average
/// rank. None with fewer than two pairs, or when either doesn't vary.
#[must_use]
pub fn spearman(xs: &[f64], ys: &[f64]) -> Option<f64> {
    if xs.len() != ys.len() || xs.len() < 2 {
        return None;
    }
    let (rx, ry) = (ranks(xs), ranks(ys));
    // Both rank lists hold 1 to n, so their mean is the same.
    let mean = rx.iter().sum::<f64>() / f64::from(u32::try_from(rx.len()).ok()?);
    let (mx, my) = (mean, mean);
    let (mut cov, mut vx, mut vy) = (0.0_f64, 0.0_f64, 0.0_f64);
    for (a, b) in rx.iter().zip(&ry) {
        cov += (a - mx) * (b - my);
        vx += (a - mx).powi(2);
        vy += (b - my).powi(2);
    }
    (vx > 0.0 && vy > 0.0).then(|| cov / (vx * vy).sqrt())
}

/// Each value's rank, from 1 for the smallest, ties sharing their average rank.
#[allow(clippy::cast_precision_loss)] // a handful of questions
fn ranks(values: &[f64]) -> Vec<f64> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
    let mut ranks = vec![0.0; values.len()];
    let mut i = 0;
    while i < order.len() {
        let mut j = i;
        while j + 1 < order.len() && values[order[j + 1]] == values[order[i]] {
            j += 1;
        }
        let average = (i + j) as f64 / 2.0 + 1.0;
        for &k in &order[i..=j] {
            ranks[k] = average;
        }
        i = j + 1;
    }
    ranks
}

/// What [`value_questions`] found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionValueRun {
    /// Each item's first proposal.
    pub proposals: Vec<Proposal>,
    /// Each open question, answered, in the ranking's order.
    pub valued: Vec<Valued>,
    /// The scores.
    pub value: QuestionValue,
}

/// Measures question value with `registry`'s text model: ingests `items` (paths to the test
/// set's files) into `archive`, a copy of the test archive, in one batch; ranks the questions
/// it raises; and answers each alone with its known answer. A fact is added to the glossary
/// for the time its items are ingested again, then taken out. `device` names this device to
/// ingest.
///
/// # Errors
///
/// A message if ingest fails, or the glossary can't be written or put back.
pub fn value_questions(
    archive: &Archive,
    registry: &Registry,
    items: &[PathBuf],
    answers: &Answers,
    device: Id,
) -> Result<QuestionValueRun, String> {
    let run = crate::ingest::run::run(archive, registry, items, IntakeOptions::default(), device)
        .map_err(|e| e.to_string())?;
    let proposals = proposals_of(&run);
    let collections = archive.collections().map_err(|e| e.to_string())?;
    let context = ranking_context(archive, &proposals, &collections);
    let ranked = rank(run.questions.clone(), &context, jiff::Timestamp::now(), 0);
    let set_aside = run.questions.len() - ranked.open.len();
    let by_name: BTreeMap<String, &PathBuf> = items
        .iter()
        .filter_map(|p| Some((p.file_name()?.to_string_lossy().into_owned(), p)))
        .collect();
    let mut reroute = |_: &QuestionEntry, answer: &KnownAnswer, waiting: &[&Proposal]| match answer
    {
        KnownAnswer::Collection { collection } => Ok(send_to(collection, waiting)),
        KnownAnswer::Fact { term, fact, .. } => {
            let paths: Vec<PathBuf> = waiting
                .iter()
                .filter_map(|p| by_name.get(&p.item).map(|p| (*p).clone()))
                .collect();
            with_fact(archive, term, fact, || {
                let again = crate::ingest::run::run(
                    archive,
                    registry,
                    &paths,
                    IntakeOptions::default(),
                    device,
                )
                .map_err(|e| e.to_string())?;
                Ok(proposals_of(&again)
                    .into_iter()
                    .map(|p| (p.item, p.destination))
                    .collect())
            })
        }
    };
    let valued = value_each(&ranked, &proposals, answers, &collections, &mut reroute)?;
    let value = summarize_value(&valued, &proposals, answers, set_aside);
    Ok(QuestionValueRun {
        proposals,
        valued,
        value,
    })
}

/// Runs `then` with `term`'s `fact` in `archive`'s glossary, and puts the glossary back as it
/// was afterwards, whatever `then` gives.
fn with_fact<T>(
    archive: &Archive,
    term: &str,
    fact: &str,
    then: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let path: &Path = &archive.root().join(GLOSSARY);
    let before = std::fs::read(path).ok();
    add_fact(archive, term, fact).map_err(|e| format!("{}: {e}", path.display()))?;
    let result = then();
    let restored = match &before {
        Some(bytes) => std::fs::write(path, bytes),
        None => std::fs::remove_file(path),
    };
    restored.map_err(|e| format!("{}: {e}", path.display()))?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_correlation_handles_ties_and_flat_values() {
        let up = spearman(&[1.0, 2.0, 3.0, 4.0], &[10.0, 20.0, 30.0, 40.0]).unwrap();
        assert!((up - 1.0).abs() < 1e-9);
        let down = spearman(&[1.0, 2.0, 3.0], &[3.0, 2.0, 1.0]).unwrap();
        assert!((down + 1.0).abs() < 1e-9);
        assert_eq!(ranks(&[5.0, 1.0, 5.0, 2.0]), [3.5, 1.0, 3.5, 2.0]);
        assert!(spearman(&[1.0, 2.0], &[0.0, 0.0]).is_none());
        assert!(spearman(&[1.0], &[1.0]).is_none());
    }
}
