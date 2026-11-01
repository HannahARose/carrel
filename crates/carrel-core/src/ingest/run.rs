//! A whole ingest run (`docs/design/ingest.md`): intake, text, cards, routes and sensitivity
//! for each item, the proposals that result, and the questions they raise. Commands, the
//! interactive mode and the apps run ingest through [`run`] and only differ in how they show
//! the result.

use std::path::PathBuf;

use crate::archive::Archive;
use crate::card::{Id, Sensitivity};
use crate::ingest::card::{Context, Draft, Glossary, Vocabulary, draft};
use crate::ingest::filing::{Placement, files_itself};
use crate::ingest::questions::ask;
use crate::ingest::route::{Route, Router, RoutingRules};
use crate::ingest::sensitivity::{Assessment, assess};
use crate::ingest::text::{ItemText, read};
use crate::ingest::{Batch, Candidate, IngestError, IntakeOptions, Skipped, plan_with_index};
use crate::providers::Registry;
use crate::questions::rank::{merge, same};
use crate::questions::{QuestionEntry, QuestionQueue, clear_waiting, mark_waiting};
use crate::review::{ReviewEntry, ReviewError, ReviewQueue};

/// What an ingest run found and proposed. The lists of per-item results line up with
/// `candidates`; an item has no text, draft or proposal when it's a duplicate.
#[derive(Debug)]
pub struct Run {
    /// The batch's identifier.
    pub batch: Id,
    /// The items, in order.
    pub candidates: Vec<Candidate>,
    /// Paths left out, and why.
    pub skipped: Vec<Skipped>,
    /// Each item's text.
    pub texts: Vec<Option<ItemText>>,
    /// Each item's drafted card.
    pub drafts: Vec<Option<Draft>>,
    /// Where each item would go.
    pub routes: Vec<Option<Route>>,
    /// How sensitive each item is.
    pub assessments: Vec<Option<Assessment>>,
    /// Each item's proposal.
    pub entries: Vec<Option<ReviewEntry>>,
    /// The questions the batch raises; the proposals they name are marked as waiting.
    pub questions: Vec<QuestionEntry>,
    /// Routing rules that couldn't be used, and why.
    pub routing_problems: Vec<String>,
}

/// An item's file or folder name.
#[must_use]
pub fn name(candidate: &Candidate) -> String {
    candidate.path.file_name().map_or_else(
        || candidate.path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// A stage of an ingest run. Each goes through every item before the next starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Reading each item's text.
    Reading,
    /// Drafting each card with a model.
    Drafting,
    /// Choosing each item's collection.
    Routing,
    /// Rating each item's sensitivity.
    Rating,
}

impl Stage {
    /// How many stages a run has.
    pub const COUNT: usize = 4;

    /// The stage's place in the run, from 1.
    #[must_use]
    pub fn number(self) -> usize {
        match self {
            Self::Reading => 1,
            Self::Drafting => 2,
            Self::Routing => 3,
            Self::Rating => 4,
        }
    }

    /// What it does, for a progress line.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Reading => "Reading",
            Self::Drafting => "Drafting cards",
            Self::Routing => "Choosing collections",
            Self::Rating => "Rating sensitivity",
        }
    }
}

/// How far a run has got: `done` of `total` items through `stage`, and the one starting now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress<'a> {
    /// The stage.
    pub stage: Stage,
    /// Items finished in this stage.
    pub done: usize,
    /// Items in the run.
    pub total: usize,
    /// The item being worked on, or `None` once the stage is finished.
    pub item: Option<&'a str>,
}

/// Runs ingest over `paths` on `device`, using `registry`'s providers. Nothing is saved; see
/// [`Run::save`].
///
/// # Errors
///
/// [`IngestError`] if the paths can't be planned or the archive can't be read.
pub fn run(
    archive: &Archive,
    registry: &Registry,
    paths: &[PathBuf],
    options: IntakeOptions,
    device: Id,
) -> Result<Run, IngestError> {
    run_with_progress(archive, registry, paths, options, device, &mut |_| {})
}

/// How sensitive `candidate` is: its text's rating, and at least `personal` when the file
/// records where it was made.
fn rate(
    registry: &Registry,
    candidate: &Candidate,
    text: &ItemText,
    default: Sensitivity,
) -> Assessment {
    let mut assessment = assess(registry, &name(candidate), &text.text, default);
    if text.located {
        assessment.located();
    }
    assessment
}

/// [`run`], calling `progress` as each item starts and as each stage finishes.
///
/// # Errors
///
/// As [`run`].
pub fn run_with_progress(
    archive: &Archive,
    registry: &Registry,
    paths: &[PathBuf],
    options: IntakeOptions,
    device: Id,
    progress: &mut dyn FnMut(Progress<'_>),
) -> Result<Run, IngestError> {
    let (candidates, skipped, index) = plan_with_index(archive, paths, options)?;
    let names: Vec<String> = candidates.iter().map(name).collect();
    let total = candidates.len();
    let mut report = |stage: Stage, done: usize| {
        progress(Progress {
            stage,
            done,
            total,
            item: names.get(done).map(String::as_str),
        });
    };
    let rules = rules_of(archive)?;
    let collections = archive.collections()?;
    let vocabulary = Vocabulary::of(archive, &collections);
    let glossary = Glossary::of(archive);
    let context = Context {
        archive_name: &archive.doc().name,
        vocabulary: &vocabulary,
        glossary: &glossary,
        device,
    };
    // Duplicates won't be filed by default, so their text isn't needed yet.
    let texts: Vec<Option<ItemText>> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            report(Stage::Reading, i);
            (c.duplicate_of.is_none() && c.same_as.is_none()).then(|| read(registry, &rules, c))
        })
        .collect();
    report(Stage::Reading, total);
    let drafts: Vec<Option<Draft>> = candidates
        .iter()
        .zip(&texts)
        .enumerate()
        .map(|(i, (c, t))| {
            report(Stage::Drafting, i);
            t.as_ref().map(|t| draft(registry, &context, c, t))
        })
        .collect();
    report(Stage::Drafting, total);

    let routing_rules = RoutingRules::of(archive, &collections);
    let chooser = Router::new(archive, &collections, &routing_rules);
    let routes: Vec<Option<Route>> = candidates
        .iter()
        .zip(&drafts)
        .enumerate()
        .map(|(i, (c, d))| {
            report(Stage::Routing, i);
            d.as_ref().map(|d| {
                let source = c.path.parent().unwrap_or(&c.path);
                chooser.route(registry, c, source, d)
            })
        })
        .collect();
    report(Stage::Routing, total);
    let batch = Batch {
        archive,
        collections: &collections,
        id: Id::new_v7(),
        device,
    };
    let assessments: Vec<Option<Assessment>> = candidates
        .iter()
        .zip(texts.iter().zip(&routes))
        .enumerate()
        .map(|(i, (c, (t, r)))| {
            report(Stage::Rating, i);
            match (t, r) {
                (Some(t), Some(r)) => Some(rate(registry, c, t, batch.default_sensitivity(r))),
                _ => None,
            }
        })
        .collect();
    report(Stage::Rating, total);
    let mut entries = propose_all(&batch, &candidates, &texts, &drafts, &routes, &assessments);
    for entry in entries.iter_mut().flatten() {
        index.add_by_title(entry);
    }
    let proposed: Vec<ReviewEntry> = entries.iter().flatten().cloned().collect();
    let questions = ask(archive, &collections, &glossary, &proposed);
    for entry in entries.iter_mut().flatten() {
        mark_waiting(entry, &questions);
    }
    Ok(Run {
        batch: batch.id,
        candidates,
        skipped,
        texts,
        drafts,
        routes,
        assessments,
        entries,
        questions,
        routing_problems: routing_rules.problems.clone(),
    })
}

/// The archive's rules, or why they can't be read.
fn rules_of(archive: &Archive) -> Result<crate::detect::Rules, IngestError> {
    archive.rules().map_err(|e| {
        IngestError::Archive(crate::archive::ArchiveError::Invalid {
            path: e.path,
            message: e.message,
        })
    })
}

/// Each item's proposal, for those that have everything a proposal needs.
fn propose_all(
    batch: &Batch<'_>,
    candidates: &[Candidate],
    texts: &[Option<ItemText>],
    drafts: &[Option<Draft>],
    routes: &[Option<Route>],
    assessments: &[Option<Assessment>],
) -> Vec<Option<ReviewEntry>> {
    candidates
        .iter()
        .zip(texts.iter().zip(drafts).zip(routes.iter().zip(assessments)))
        .map(|(c, ((t, d), (r, s)))| match (t, d, r, s) {
            (Some(t), Some(d), Some(r), Some(s)) => batch.propose(c, d, r, s, &t.attempts),
            _ => None,
        })
        .collect()
}

impl Run {
    /// Saves the run's proposals and questions to their queues. A question that says the same
    /// as one already asked merges into it, and its proposals wait on that one instead; one the
    /// owner declined isn't asked again.
    ///
    /// # Errors
    ///
    /// [`ReviewError`] if a queue can't be read or written.
    pub fn save(&self, archive: &Archive) -> Result<(), ReviewError> {
        let asked = QuestionQueue::of(archive);
        let (mut existing, _) = asked.list()?;
        let mut entries: Vec<ReviewEntry> = self.entries.iter().flatten().cloned().collect();
        for question in &self.questions {
            let kept = match existing.iter_mut().find(|e| same(e, question)) {
                Some(earlier) if earlier.declined.is_some() => None,
                Some(earlier) => {
                    merge(earlier, question);
                    Some(earlier.clone())
                }
                None => {
                    existing.push(question.clone());
                    Some(question.clone())
                }
            };
            if let Some(kept) = &kept {
                asked.save(kept)?;
            }
            if kept.as_ref().is_some_and(|k| k.id == question.id) {
                continue;
            }
            for entry in entries
                .iter_mut()
                .filter(|e| question.items.contains(&e.id))
            {
                clear_waiting(entry, question);
                if let Some(kept) = &kept {
                    mark_waiting(entry, std::slice::from_ref(kept));
                }
            }
        }
        let queue = ReviewQueue::of(archive);
        for entry in &entries {
            queue.save(entry)?;
        }
        self.keep_texts(archive);
        Ok(())
    }

    /// Keeps each proposed item's text for search, so it's never read again. Restricted items'
    /// text isn't kept, and a cache that can't be written is only a slower search later.
    fn keep_texts(&self, archive: &Archive) {
        let cache = crate::search::texts::TextCache::of(archive);
        for ((candidate, text), (entry, assessment)) in self
            .candidates
            .iter()
            .zip(&self.texts)
            .zip(self.entries.iter().zip(&self.assessments))
        {
            if let (Some(text), Some(_), Some(assessment)) = (text, entry, assessment)
                && assessment.level != Sensitivity::Restricted
                && text.unread != Some(crate::ingest::text::Unread::NoReader)
            {
                let _ = cache.put(&candidate.checksum, &text.text);
            }
        }
    }

    /// The proposals an owner rule files without review (design decision 4).
    #[must_use]
    pub fn filed_by_rule(&self) -> Vec<(ReviewEntry, Placement)> {
        self.entries
            .iter()
            .zip(&self.routes)
            .filter_map(|(e, r)| match (e, r) {
                (Some(e), Some(r)) if files_itself(e, r) => {
                    Some((e.clone(), Placement::Destination))
                }
                _ => None,
            })
            .collect()
    }
}
