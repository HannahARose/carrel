//! A whole ingest run (`docs/design/ingest.md`): intake, text, cards, routes and sensitivity
//! for each item, the proposals that result, and the questions they raise. Commands, the
//! interactive mode and the apps run ingest through [`run`] and only differ in how they show
//! the result.

use std::path::PathBuf;

use crate::archive::Archive;
use crate::card::Id;
use crate::ingest::card::{Context, Draft, Glossary, Vocabulary, draft};
use crate::ingest::filing::{Placement, files_itself};
use crate::ingest::questions::ask;
use crate::ingest::route::{Route, Router, RoutingRules};
use crate::ingest::sensitivity::{Assessment, assess};
use crate::ingest::text::{ItemText, read};
use crate::ingest::{Batch, Candidate, IngestError, IntakeOptions, Skipped, plan};
use crate::providers::Registry;
use crate::questions::{QuestionEntry, QuestionQueue, mark_waiting};
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
    let (candidates, skipped) = plan(archive, paths, options)?;
    let rules = archive.rules().map_err(|e| {
        IngestError::Archive(crate::archive::ArchiveError::Invalid {
            path: e.path,
            message: e.message,
        })
    })?;
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
        .map(|c| {
            (c.duplicate_of.is_none() && c.same_as.is_none()).then(|| read(registry, &rules, c))
        })
        .collect();
    let drafts: Vec<Option<Draft>> = candidates
        .iter()
        .zip(&texts)
        .map(|(c, t)| t.as_ref().map(|t| draft(registry, &context, c, t)))
        .collect();

    let routing_rules = RoutingRules::of(archive, &collections);
    let chooser = Router::new(archive, &collections, &routing_rules);
    let routes: Vec<Option<Route>> = candidates
        .iter()
        .zip(&drafts)
        .map(|(c, d)| {
            d.as_ref().map(|d| {
                let source = c.path.parent().unwrap_or(&c.path);
                chooser.route(registry, c, source, d)
            })
        })
        .collect();
    let batch = Batch {
        archive,
        collections: &collections,
        id: Id::new_v7(),
        device,
    };
    let assessments: Vec<Option<Assessment>> = candidates
        .iter()
        .zip(texts.iter().zip(&routes))
        .map(|(c, (t, r))| match (t, r) {
            (Some(t), Some(r)) => Some(assess(
                registry,
                &name(c),
                &t.text,
                batch.default_sensitivity(r),
            )),
            _ => None,
        })
        .collect();
    let mut entries: Vec<Option<ReviewEntry>> = candidates
        .iter()
        .zip(
            texts
                .iter()
                .zip(&drafts)
                .zip(routes.iter().zip(&assessments)),
        )
        .map(|(c, ((t, d), (r, s)))| match (t, d, r, s) {
            (Some(t), Some(d), Some(r), Some(s)) => batch.propose(c, d, r, s, &t.attempts),
            _ => None,
        })
        .collect();
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

impl Run {
    /// Saves the run's proposals and questions to their queues.
    ///
    /// # Errors
    ///
    /// [`ReviewError`] if a queue can't be written.
    pub fn save(&self, archive: &Archive) -> Result<(), ReviewError> {
        let queue = ReviewQueue::of(archive);
        for entry in self.entries.iter().flatten() {
            queue.save(entry)?;
        }
        let asked = QuestionQueue::of(archive);
        for question in &self.questions {
            asked.save(question)?;
        }
        Ok(())
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
