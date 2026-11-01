//! The restructure benchmark (`docs/design/restructure.md`, "Measuring it"): on the muddled
//! copy of the test set's archive ([`super::crawl::Muddle`]), whether a whole-archive
//! restructure proposal recovers the intended shape, how many moves it takes, weighted by how
//! invasive each is, and the findability gain its own test measured.
//!
//! The proposal is never applied: [`after`] works out where each item would end up from its
//! changes. The shape is scored by which items end up together ([`shape`]), so a new collection
//! the model names `Cooking` recovers the missing `Recipes` as well as one it names `Recipes`;
//! and by how many misplaced items end up in a collection `answers.yaml` accepts, which only
//! a move into an existing collection can get right.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{Answers, crawl::Misplaced};
use crate::card::Id;
use crate::restructure::{Edit, Invasiveness, Proposal};

/// Where each item in `before` would be once `proposal` is applied, by identifier. Moves,
/// merges and renames carry items along; nothing else moves anything. Items `before` doesn't
/// have, such as Holding's own, are left out.
#[must_use]
pub fn after(before: &BTreeMap<Id, String>, proposal: &Proposal) -> BTreeMap<Id, String> {
    let mut places = before.clone();
    for change in &proposal.changes {
        match &change.change {
            Edit::MoveItems { to } => {
                for item in &change.items {
                    if let Some(place) = places.get_mut(&item.id) {
                        place.clone_from(to);
                    }
                }
            }
            Edit::MergeCollection { from, into: to } | Edit::RenameCollection { from, to } => {
                for place in places.values_mut() {
                    if let Some(rest) = within(place, from) {
                        *place = format!("{to}{rest}");
                    }
                }
            }
            _ => {}
        }
    }
    places
}

/// What's left of `path` after `collection`, `""` or `/sub`, when `path` is `collection` or
/// inside it.
fn within<'a>(path: &'a str, collection: &str) -> Option<&'a str> {
    let rest = path.strip_prefix(collection)?;
    (rest.is_empty() || rest.starts_with('/')).then_some(rest)
}

/// How many items each change moves, weighted by how invasive it is (its rank, from 1 for
/// rewriting to 5 for moving top-level collections): what the proposal asks the owner to
/// check, and possibly undo. A merge or rename counts every item it carries along.
#[must_use]
pub fn moves(before: &BTreeMap<Id, String>, proposal: &Proposal) -> Moves {
    let mut counted = Moves::default();
    for change in &proposal.changes {
        let moved = match &change.change {
            Edit::MoveItems { .. } => change
                .items
                .iter()
                .filter(|i| before.contains_key(&i.id))
                .count(),
            Edit::MergeCollection { from, .. } | Edit::RenameCollection { from, .. } => before
                .values()
                .filter(|p| within(p, from).is_some())
                .count(),
            _ => 0,
        };
        *counted
            .by_invasiveness
            .entry(change.invasiveness)
            .or_default() += moved;
        counted.moved += moved;
        counted.weighted += moved * usize::from(change.invasiveness.rank());
    }
    counted
}

/// The moves a proposal makes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moves {
    /// Items moved, counting an item once for each change that moves it.
    pub moved: usize,
    /// Those moves, each weighted by its change's invasiveness rank.
    pub weighted: usize,
    /// Items moved, by how invasive the change was.
    pub by_invasiveness: BTreeMap<Invasiveness, usize>,
}

/// How closely a placement of items matches the intended shape, by pairs: of the pairs of
/// items it puts in the same collection, the share that belong together (precision), and of
/// the pairs that belong together, the share it puts together (recall).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    /// The share of pairs put together that belong together.
    pub precision: f64,
    /// The share of pairs that belong together that are put together.
    pub recall: f64,
    /// Their harmonic mean.
    pub f1: f64,
}

/// How `places` (each item's collection, by file name) matches the intended shape, each item's
/// first collection in `answers`. Items `answers` doesn't have are left out.
#[must_use]
#[allow(clippy::cast_precision_loss)] // pair counts
pub fn shape(places: &BTreeMap<String, String>, answers: &Answers) -> Shape {
    let items: Vec<(&str, &str)> = places
        .iter()
        .filter_map(|(file, place)| {
            let intended = answers.items.get(file)?.destinations.first()?;
            Some((place.as_str(), intended.as_str()))
        })
        .collect();
    let (mut together, mut belong, mut both) = (0_usize, 0_usize, 0_usize);
    for (i, (place, intended)) in items.iter().enumerate() {
        for (other_place, other_intended) in &items[i + 1..] {
            let put = place == other_place;
            let ought = intended == other_intended;
            together += usize::from(put);
            belong += usize::from(ought);
            both += usize::from(put && ought);
        }
    }
    let share = |part: usize, whole: usize| {
        if whole == 0 {
            0.0
        } else {
            part as f64 / whole as f64
        }
    };
    let precision = share(both, together);
    let recall = share(both, belong);
    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };
    Shape {
        precision,
        recall,
        f1,
    }
}

/// How one proposal scored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RestructureScore {
    /// The changes it proposed.
    pub changes: usize,
    /// The muddled archive's shape.
    pub before: Shape,
    /// Its shape once the proposal is applied.
    pub after: Shape,
    /// Whether the proposal brings the archive closer to the intended shape.
    pub recovered: bool,
    /// Misplaced items.
    pub misplaced: usize,
    /// Of those, the ones the proposal would put in a collection `answers.yaml` accepts, or a
    /// subcollection of one.
    pub fixed: usize,
    /// Items in an acceptable collection that it would move out of one.
    pub broken: usize,
    /// The moves it makes.
    pub moves: Moves,
    /// Weighted moves for each misplaced item it fixes; none when it fixes none.
    pub weighted_moves_per_fix: Option<f64>,
    /// Its own findability test: how many held-out cards were found in the current tree and
    /// the proposed one, of how many tried.
    pub findability: Option<(usize, usize, usize)>,
    /// The findability gain: cards found in the proposed tree less those found in the current
    /// one, as a share of those tried.
    pub findability_gain: Option<f64>,
    /// Why the findability test didn't run, if it didn't.
    pub untested: Option<String>,
    /// How long building the proposal took, in seconds.
    pub seconds: f64,
}

/// Scores `proposal`, built for the muddled archive where `files` names each item and
/// `places` says where each is and how it was misplaced, against `answers`.
#[must_use]
#[allow(clippy::cast_precision_loss)] // item counts
pub fn score(
    proposal: &Proposal,
    files: &BTreeMap<Id, String>,
    places: &BTreeMap<String, (String, Option<Misplaced>)>,
    answers: &Answers,
    seconds: f64,
) -> RestructureScore {
    let counted = |file: &String| answers.items.get(file).is_some_and(super::crawl::counts);
    let before_ids: BTreeMap<Id, String> = files
        .iter()
        .filter(|(_, file)| counted(file))
        .filter_map(|(id, file)| Some((*id, places.get(file)?.0.clone())))
        .collect();
    let after_ids = after(&before_ids, proposal);
    let by_file = |ids: &BTreeMap<Id, String>| -> BTreeMap<String, String> {
        ids.iter()
            .filter_map(|(id, place)| Some((files.get(id)?.clone(), place.clone())))
            .collect()
    };
    let (before, after) = (by_file(&before_ids), by_file(&after_ids));
    // A subcollection of an acceptable collection is still where you'd look for it.
    let acceptable = |file: &str, place: &str| {
        answers
            .items
            .get(file)
            .is_some_and(|a| a.destinations.iter().any(|d| within(place, d).is_some()))
    };
    let mut fixed = 0;
    let mut broken = 0;
    let mut misplaced = 0;
    for (file, now) in &after {
        let was = &before[file];
        let wrong = places.get(file).is_some_and(|(_, kind)| kind.is_some());
        if wrong {
            misplaced += 1;
            fixed += usize::from(acceptable(file, now));
        } else if acceptable(file, was) && !acceptable(file, now) {
            broken += 1;
        }
    }
    let moves = moves(&before_ids, proposal);
    let assessment = proposal.assessment.as_ref();
    let findability = assessment
        .and_then(|a| a.findability.as_ref())
        .map(|f| (f.tried, f.found_now, f.found_after));
    let shape_before = shape(&before, answers);
    let shape_after = shape(&after, answers);
    RestructureScore {
        changes: proposal.changes.len(),
        before: shape_before,
        after: shape_after,
        recovered: shape_after.f1 > shape_before.f1,
        misplaced,
        fixed,
        broken,
        weighted_moves_per_fix: (fixed > 0).then(|| moves.weighted as f64 / fixed as f64),
        moves,
        findability_gain: findability.and_then(|(tried, now, after)| {
            (tried > 0).then(|| (after as f64 - now as f64) / tried as f64)
        }),
        findability,
        untested: assessment.and_then(|a| a.untested.clone()),
        seconds,
    }
}

/// `archive` again with its `restructure.eagerness` set to `eagerness`, so the same muddled
/// archive is proposed for at each preset.
///
/// # Errors
///
/// A message if `settings.json` can't be written or the archive opened again.
pub fn at_eagerness(
    archive: &crate::archive::Archive,
    eagerness: crate::restructure::eagerness::Eagerness,
) -> Result<crate::archive::Archive, String> {
    let mut settings = archive.settings().clone();
    settings.restructure.eagerness = eagerness;
    let path = archive.root().join("_System/settings.json");
    let text = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())? + "\n";
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    crate::archive::Archive::open(archive.root()).map_err(|e| e.to_string())
}
