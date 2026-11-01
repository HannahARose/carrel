//! Building a whole-section restructure proposal (`docs/design/restructure.md`, "How a
//! proposal is built" and "When one is proposed"): from a section's candidate groups
//! ([`super::grouping`]) to a list of changes, each worth more than it costs by the margin the
//! section's eagerness asks for ([`super::eagerness`]), checked by the findability test
//! ([`super::findability`]) when a text model is at hand.
//!
//! Numbers decide what belongs together and whether it's worth it; a text model, when there is
//! one, names the new collections, may suggest renaming a collection whose name no longer fits,
//! and writes the rationale. Every answer it gives is checked, and anything it gets wrong is
//! replaced by a plain name built from the group's tags. Nothing here moves a file: applying
//! is [`super::applying`].
//!
//! Each candidate group becomes either a new collection its items move into (subdividing a
//! collection, gathering Holding's items, or gathering items from several branches); or, when
//! the group is a small collection's every item plus items of one other collection, merging
//! the small one into the other; or, when it belongs in a collection that's already there,
//! moving the members with reason to leave into it ([`super::rehoming`]). Single items far more
//! like another collection than their own, or that the crawler judged belong elsewhere, move on
//! their own. A rewrite of the home collection's design document, naming its new
//! subcollections, depends on the collections it names.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::eagerness::{self, Eagerness, Source, Thresholds};
use super::findability::{self, Tree};
use super::grouping::{CandidateGroup, GroupingOptions, group_section};
use super::rehoming::{self, CrawlerMove, CrawlerSignals};
use super::{
    Edit, Invasiveness, Proposal, ProposalKind, ProposedChange, RestructureError, Touched,
};
use crate::archive::{Archive, Collection, ROOT_RESERVED};
use crate::card::{Id, Sensitivity};
use crate::discover::Catalog;
use crate::graph::{Graph, is_within};
use crate::history::{History, Operation};
use crate::providers::{CompletionRequest, Registry};

/// The collection items go to when nowhere fits.
const HOLDING: &str = "Holding";

/// How a section proposal is built.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionOptions {
    /// How the section's items are grouped.
    pub grouping: GroupingOptions,
    /// How many cards the findability test holds out.
    pub sample: usize,
    /// The seed the findability test's sample is chosen with.
    pub seed: u64,
}

impl SectionOptions {
    /// The defaults for `archive`: its literary warrant, and the findability test's usual
    /// sample.
    #[must_use]
    pub fn for_archive(archive: &Archive) -> Self {
        Self {
            grouping: GroupingOptions::for_archive(archive),
            sample: findability::DEFAULT_SAMPLE,
            seed: 7,
        }
    }
}

/// How the findability test came out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindabilityResult {
    /// How many held-out cards were looked for.
    pub tried: usize,
    /// How many were found in the current tree.
    pub found_now: usize,
    /// How many were found in the proposed tree.
    pub found_after: usize,
    /// The model that walked the trees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The seed the sample was chosen with, to repeat the test.
    pub seed: u64,
}

impl FindabilityResult {
    /// Whether the proposed tree was found more often.
    #[must_use]
    pub fn is_better(&self) -> bool {
        self.found_after > self.found_now
    }
}

/// A group that was worth looking at but isn't in the proposal, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeldBack {
    /// What it would have done, such as `a new collection of 6 items in Reading`.
    pub what: String,
    /// How invasive it would have been.
    pub invasiveness: Invasiveness,
    /// How much its benefit beat its cost by.
    pub gain: f64,
    /// How much it needed to.
    pub required: f64,
    /// Why it's held back.
    pub why: String,
}

/// How a section proposal was judged: its cost, benefit and findability result, and the
/// eagerness it was judged by. Kept in the proposal file as `assessment`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assessment {
    /// The eagerness that applied to the section.
    pub eagerness: Eagerness,
    /// Where that eagerness was set.
    pub eagerness_from: Source,
    /// The numbers it stood for.
    pub thresholds: Thresholds,
    /// The proposal's benefit: its changes' benefits added up.
    pub benefit: f64,
    /// Its cost, weighted by how invasive each change is.
    pub cost: f64,
    /// The findability test, if it ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub findability: Option<FindabilityResult>,
    /// Why the findability test didn't run, if it didn't: then the proposal is a draft that a
    /// device with a text model can test again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub untested: Option<String>,
    /// Whether the section was restructured recently, so changes in it cost more.
    #[serde(default)]
    pub recently_restructured: bool,
    /// Whether a text model wrote the names and the rationale.
    #[serde(default)]
    pub worded_by_model: bool,
    /// How many of the section's items were grouped.
    pub items: usize,
    /// How many of those had no card vector, so were grouped by the graph alone.
    pub without_vectors: usize,
    /// Groups worth looking at that the proposal leaves out, and why.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub held_back: Vec<HeldBack>,
}

/// What a candidate group becomes.
#[derive(Debug, Clone, PartialEq)]
enum Shape {
    /// A new collection inside `home` (empty for the top level), which its items move into.
    New {
        /// Where it goes.
        home: String,
    },
    /// The small collection `from` merged into `into`.
    Merge {
        /// The small collection.
        from: String,
        /// The collection it joins.
        into: String,
    },
    /// Its items moved into `to`, a collection that's already there.
    Into {
        /// The collection.
        to: String,
    },
}

/// A candidate group, judged.
#[derive(Debug, Clone)]
struct Plan {
    group: CandidateGroup,
    shape: Shape,
    invasiveness: Invasiveness,
    benefit: f64,
    cost: f64,
    required: f64,
    /// For items moving into an existing collection, how much more similar they are to its
    /// items than to those where they are, from 0 to 1.
    improvement: f64,
    /// Items that would leave a collection the owner knows, for the move budget.
    known_moves: usize,
    name: String,
    description: String,
    reason: String,
}

impl Plan {
    fn gain(&self) -> f64 {
        self.benefit - self.cost
    }

    fn what(&self) -> String {
        match &self.shape {
            Shape::New { home } if home.is_empty() => format!(
                "a new top-level collection of {} items",
                self.group.members.len()
            ),
            Shape::New { home } => format!(
                "a new collection of {} items in {home}",
                self.group.members.len()
            ),
            Shape::Merge { from, into } => format!("merging {from} into {into}"),
            Shape::Into { to } if self.group.members.len() == 1 => {
                format!(
                    "moving an item from {} into {to}",
                    self.group.sources[0].collection
                )
            }
            Shape::Into { to } => format!("moving {} items into {to}", self.group.members.len()),
        }
    }

    fn held_back(&self, why: impl Into<String>) -> HeldBack {
        HeldBack {
            what: self.what(),
            invasiveness: self.invasiveness,
            gain: self.gain(),
            required: self.required,
            why: why.into(),
        }
    }
}

/// What the section looks like, for judging its groups.
struct Survey<'a> {
    archive: &'a Archive,
    collections: Vec<Collection>,
    catalog: Catalog,
    graph: Graph,
    /// Items with a placement filing wasn't sure of.
    unsure: BTreeSet<Id>,
    /// What the crawler judged of items still where it judged them.
    crawler: CrawlerSignals,
    crowded_items: usize,
    /// Collections restructured within each level's stability period, by the time of their
    /// latest restructure (seconds since 1970).
    restructured: Vec<(String, i64)>,
    now: i64,
}

impl<'a> Survey<'a> {
    fn read(archive: &'a Archive) -> Result<Self, RestructureError> {
        let catalog = Catalog::read(archive)?;
        let graph = Graph::build(&catalog);
        let unsure = catalog
            .entries
            .iter()
            .filter(|e| e.placement.is_some())
            .map(|e| e.item.id)
            .collect();
        let restructured = recent_restructures(archive);
        let collections = archive.collections()?;
        let crawler = rehoming::crawler_signals(archive, &graph, &collections);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        Ok(Self {
            archive,
            collections,
            catalog,
            graph,
            unsure,
            crawler,
            crowded_items: usize::try_from(archive.settings().signals.crowded_items)
                .unwrap_or(usize::MAX),
            restructured,
            now,
        })
    }

    fn thresholds(&self, path: &str) -> Thresholds {
        eagerness::thresholds_at(self.archive, &self.collections, path)
    }

    /// The collection's direct item count.
    fn items_in(&self, path: &str) -> usize {
        self.graph
            .collections
            .iter()
            .find(|c| c.path == path)
            .map_or(0, |c| c.items)
    }

    fn has_subcollections(&self, path: &str) -> bool {
        self.collections
            .iter()
            .any(|c| c.path != path && is_within(&c.path, path))
    }

    /// Each item's collection, by identifier.
    fn collection_of(&self) -> HashMap<Id, &str> {
        self.graph
            .items
            .iter()
            .map(|i| (i.id, i.collection.as_str()))
            .collect()
    }

    fn has_checked_out(&self, path: &str) -> bool {
        self.graph
            .items
            .iter()
            .any(|i| i.checked_out && is_within(&i.collection, path))
    }

    /// Whether `path` was restructured within `days`.
    fn recently(&self, path: &str, days: u32) -> bool {
        let since = self.now - i64::from(days) * 86_400;
        self.restructured.iter().any(|(p, at)| {
            *at >= since && (p.is_empty() || is_within(path, p) || is_within(p, path))
        })
    }
}

/// The collections touched by restructures in history, with when: each move's folders, and
/// the section a restructure's summary names. Best effort: no history reads as none.
fn recent_restructures(archive: &Archive) -> Vec<(String, i64)> {
    let Ok(log) = History::open(archive).and_then(|h| h.log()) else {
        return Vec::new();
    };
    // The folder a move happened in; a top-level collection moved or renamed counts as
    // itself, not as the whole archive.
    let folder = |p: &str| p.rsplit_once('/').map_or(p, |(f, _)| f).to_owned();
    let mut found = Vec::new();
    for entry in log
        .iter()
        .filter(|e| e.operation == Some(Operation::Restructure))
    {
        for m in &entry.moves {
            found.push((folder(&m.from), entry.time));
            found.push((folder(&m.to), entry.time));
        }
        let summary = entry.summary.trim_start_matches("Undo: ");
        if let Some(rest) = summary.strip_prefix("Restructure ")
            && let Some((section, _)) = rest.split_once(':')
            && section != "tags"
        {
            let section = if section == "the archive" {
                ""
            } else {
                section
            };
            found.push((section.to_owned(), entry.time));
        }
    }
    found
}

/// Weighs what a change costs, per item it moves, by how invasive it is.
fn move_weight(invasiveness: Invasiveness) -> f64 {
    match invasiveness {
        Invasiveness::Rewriting => 0.0,
        Invasiveness::Subdividing => 0.05,
        Invasiveness::Renaming => 0.1,
        Invasiveness::BetweenBranches => 0.25,
        Invasiveness::TopLevel => 0.4,
    }
}

fn in_holding(collection: &str) -> bool {
    is_within(collection, HOLDING)
}

/// What `group` becomes, in `section`: merging a small collection, when the group is all of one
/// small collection's items and some of one other's; moving the members with reason to leave
/// where they are into an existing collection, when it belongs there
/// ([`rehoming::existing_home`]); otherwise a new collection. Returns the group as narrowed to
/// the members that move.
fn shape_of(
    survey: &Survey,
    section: &str,
    group: CandidateGroup,
    options: &GroupingOptions,
) -> (CandidateGroup, Shape, Invasiveness) {
    if let Some(merge) = merge_shape(survey, &group, options.warrant) {
        return (group, merge.0, merge.1);
    }
    let collection_of = survey.collection_of();
    let exists = |path: &str| {
        survey.collections.iter().any(|c| c.path == path)
            && is_within(path, section)
            && !in_holding(path)
            && !survey.has_checked_out(path)
    };
    if let Some((to, members)) = rehoming::existing_home(
        &group,
        &collection_of,
        &survey.crawler,
        &|id| survey.unsure.contains(id) || survey.crawler.misfits.contains(id),
        &exists,
        options,
    ) {
        let group = rehoming::narrowed(&group, members, &to, &collection_of);
        let invasiveness = group.invasiveness;
        return (group, Shape::Into { to }, invasiveness);
    }
    let invasiveness = if group.home.is_empty()
        && group.sources.iter().any(|s| !in_holding(&s.collection))
        && group.invasiveness >= Invasiveness::BetweenBranches
    {
        Invasiveness::TopLevel
    } else {
        group.invasiveness
    };
    let home = group.home.clone();
    (group, Shape::New { home }, invasiveness)
}

/// Merging a small collection, when `group` is all of one small collection's items and some
/// of one other's.
fn merge_shape(
    survey: &Survey,
    group: &CandidateGroup,
    warrant: usize,
) -> Option<(Shape, Invasiveness)> {
    if let [a, b] = group.sources.as_slice() {
        // The smaller joins the larger: sources come most items first.
        for (small, other) in [(b, a), (a, b)] {
            let whole = survey.items_in(&small.collection) == small.items;
            if whole
                && small.items < warrant
                && !in_holding(&small.collection)
                && !in_holding(&other.collection)
                && !survey.has_subcollections(&small.collection)
                && !survey.has_checked_out(&small.collection)
            {
                let top = !small.collection.contains('/') && !other.collection.contains('/');
                let invasiveness = if top {
                    Invasiveness::TopLevel
                } else {
                    Invasiveness::BetweenBranches
                };
                return Some((
                    Shape::Merge {
                        from: small.collection.clone(),
                        into: other.collection.clone(),
                    },
                    invasiveness,
                ));
            }
        }
    }
    None
}

/// The thresholds that apply to a plan: the least eager of where it goes and every collection
/// it takes items from, so a collection the owner knows by heart stays still.
fn plan_thresholds(survey: &Survey, group: &CandidateGroup, shape: &Shape) -> Thresholds {
    let mut places: Vec<&str> = group
        .sources
        .iter()
        .map(|s| s.collection.as_str())
        .collect();
    match shape {
        Shape::New { home } => places.push(home),
        Shape::Merge { into, .. } => places.push(into),
        Shape::Into { to } => places.push(to),
    }
    let all: Vec<Thresholds> = places.iter().map(|p| survey.thresholds(p)).collect();
    let mut strictest = all[0];
    for t in &all[1..] {
        if t.margin > strictest.margin {
            strictest.margin = t.margin;
        }
        if t.stability_penalty > strictest.stability_penalty {
            strictest.stability_penalty = t.stability_penalty;
        }
        strictest.stability_days = strictest.stability_days.max(t.stability_days);
        strictest.move_budget = match (strictest.move_budget, t.move_budget) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }
    strictest
}

/// Judges `group`, in `section`: what it becomes, what it's worth and what it costs.
///
/// - **Benefit:**
///   - the share of its items that have reason to leave where they are (0.5): those it
///     gathers from elsewhere that don't fit there ([`CandidateGroup::misfits`]), unless the
///     crawler judged they do; those in Holding or a placement filing wasn't sure of; the
///     crawler's misfits; and a merged collection's, too small to stand alone. Subdividing
///     needs most of them to have one, since the numbers can't tell a subtopic from items that
///     don't belong;
///   - how much more alike its items are to each other than to the rest of the collections
///     they'd leave, for the share of the items it moves that it gathers from elsewhere (0.3);
///     all of them when it subdivides a crowded collection;
///   - for items going into an existing collection, how much more alike they are to its items
///     than to those where they are (0.3);
///   - and 0.2 when it subdivides a crowded collection.
/// - **Cost:** each item moved out of a collection the owner knows, weighted by how invasive
///   the change is, plus the stability penalty when the section was restructured recently.
fn judge(survey: &Survey, section: &str, group: CandidateGroup, options: &GroupingOptions) -> Plan {
    let (group, shape, invasiveness) = shape_of(survey, section, group, options);
    // How alike the items are on average to the rest of where they are now.
    let apart = group.cohesion - group.separation;
    let improvement = match (&shape, &group.nearest) {
        (Shape::Into { to }, Some((nearest, likeness))) if nearest == to => {
            (likeness - apart).clamp(0.0, 1.0)
        }
        _ => 0.0,
    };
    weigh(survey, group, shape, invasiveness, improvement)
}

/// Judges a single item's move into another existing collection.
fn judge_move(survey: &Survey, item: rehoming::ItemMove) -> Plan {
    weigh(
        survey,
        item.group,
        Shape::Into { to: item.to },
        Invasiveness::BetweenBranches,
        item.improvement,
    )
}

/// What `group`, becoming `shape`, is worth and costs; see [`judge`].
#[allow(clippy::cast_precision_loss)] // item counts
fn weigh(
    survey: &Survey,
    group: CandidateGroup,
    shape: Shape,
    invasiveness: Invasiveness,
    improvement: f64,
) -> Plan {
    let thresholds = plan_thresholds(survey, &group, &shape);
    let members = group.members.len().max(1) as f64;
    let collection_of = survey.collection_of();
    let destination = match &shape {
        Shape::New { .. } => None,
        Shape::Merge { into, .. } => Some(into.as_str()),
        Shape::Into { to } => Some(to.as_str()),
    };
    let moving: Vec<&Id> = group
        .members
        .iter()
        .filter(|id| collection_of.get(id).copied() != destination)
        .collect();
    // The numbers can't tell a subtopic from items that don't belong, since both are unlike
    // the rest of their collection: for a new collection they count only the misfits it
    // gathers from elsewhere, not those of the collection most of it comes from.
    let main = match &shape {
        Shape::New { .. } => group.sources.first().map(|s| s.collection.as_str()),
        _ => None,
    };
    let small = match &shape {
        Shape::Merge { from, .. } => Some(from.as_str()),
        _ => None,
    };
    let mut fixes = moving
        .iter()
        .filter(|id| {
            (group.misfits.contains(id)
                && collection_of.get(id).copied() != main
                && !survey.crawler.fitting.contains(id))
                || (small.is_some() && collection_of.get(id).copied() == small)
                || survey.unsure.contains(id)
                || survey.crawler.misfits.contains(id)
                || collection_of.get(id).is_some_and(|c| in_holding(c))
        })
        .count();
    // Subdividing says a group as a whole doesn't belong with the rest of its collection: a
    // few doubtful members don't make that case.
    if invasiveness == Invasiveness::Subdividing && fixes * 2 <= moving.len() {
        fixes = 0;
    }
    let known_moves = moving
        .iter()
        .filter(|id| collection_of.get(id).is_some_and(|c| !in_holding(c)))
        .count();
    let crowded = matches!(&shape, Shape::New { home } if invasiveness == Invasiveness::Subdividing
        && survey.items_in(home) > survey.crowded_items);
    // The share of the items it moves that it gathers from elsewhere, to join the rest; all
    // of them when it subdivides a crowded collection, putting like items together away from
    // the crowd.
    let gathered = if crowded {
        1.0
    } else {
        let main = group.sources.first().map(|s| s.collection.as_str());
        moving
            .iter()
            .filter(|id| collection_of.get(id).copied() != main)
            .count() as f64
            / moving.len().max(1) as f64
    };
    let benefit = 0.5 * fixes as f64 / members
        + 0.3 * group.separation * gathered
        + 0.3 * improvement
        + if crowded { 0.2 } else { 0.0 };
    let recent = group
        .sources
        .iter()
        .any(|s| survey.recently(&s.collection, thresholds.stability_days));
    let cost = move_weight(invasiveness) * known_moves as f64 / members
        + if recent {
            thresholds.stability_penalty
        } else {
            0.0
        };
    Plan {
        group,
        shape,
        invasiveness,
        benefit,
        cost,
        required: thresholds.required_gain(invasiveness),
        improvement,
        known_moves: if Thresholds::counts_against_budget(invasiveness) {
            known_moves
        } else {
            0
        },
        name: String::new(),
        description: String::new(),
        reason: String::new(),
    }
}

/// Adds to `chosen` the plans whose gain beats what their eagerness asks, best first, within
/// the move budget left; the rest are held back, with why.
fn choose(
    survey: &Survey,
    section: &str,
    mut plans: Vec<Plan>,
    chosen: &mut Vec<Plan>,
    held: &mut Vec<HeldBack>,
) {
    plans.sort_by(|a, b| b.gain().total_cmp(&a.gain()));
    let budget = survey.thresholds(section).move_budget;
    let mut spent: usize = chosen.iter().map(|p| p.known_moves).sum();
    let mut merged: BTreeSet<String> = BTreeSet::new();
    for plan in chosen.iter() {
        if let Shape::Merge { from, into } = &plan.shape {
            merged.insert(from.clone());
            merged.insert(into.clone());
        }
    }
    for plan in plans {
        if plan.gain() <= plan.required {
            let why = if plan.required > 0.0 {
                format!(
                    "its gain ({:.2}) isn't the {:.2} {} needs at this eagerness",
                    plan.gain(),
                    plan.required,
                    plan.invasiveness.describe()
                )
            } else {
                "it costs as much as it gains".to_owned()
            };
            held.push(plan.held_back(why));
            continue;
        }
        if let Some(limit) = budget
            && spent + plan.known_moves > limit
        {
            held.push(plan.held_back(format!(
                "it would take the moves between branches past this section's budget of {limit}"
            )));
            continue;
        }
        if let Shape::Merge { from, into } = &plan.shape
            && (merged.contains(from) || merged.contains(into))
        {
            held.push(plan.held_back("another change already merges one of its collections"));
            continue;
        }
        if let Shape::Merge { from, into } = &plan.shape {
            merged.insert(from.clone());
            merged.insert(into.clone());
        }
        spent += plan.known_moves;
        chosen.push(plan);
    }
}

/// Builds a restructure proposal for `section` (a collection's path, or empty for the whole
/// archive) from its items' card `vectors` and the tag and link graph, judged by the eagerness
/// that applies there. With a text model in `registry`, the model names the new collections,
/// may suggest renames and writes the rationale, and the findability test runs; without one,
/// the proposal says it's untested. A proposal without changes says in its rationale why
/// nothing is worth changing.
///
/// # Errors
///
/// [`RestructureError::Invalid`] if `section` isn't a collection, or an error reading the
/// archive.
pub fn propose_section<S: std::hash::BuildHasher>(
    archive: &Archive,
    section: &str,
    vectors: &HashMap<Id, Vec<f32>, S>,
    registry: Option<&Registry>,
    options: &SectionOptions,
) -> Result<Proposal, RestructureError> {
    let section = section.trim().trim_matches('/');
    let mut survey = Survey::read(archive)?;
    let section = match survey
        .collections
        .iter()
        .find(|c| c.path.eq_ignore_ascii_case(section))
    {
        Some(c) => c.path.clone(),
        None if section.is_empty() => String::new(),
        None => {
            return Err(RestructureError::Invalid(format!(
                "there's no collection {section}"
            )));
        }
    };
    let grouping = group_section(&survey.graph, &section, vectors, &options.grouping);
    let (eagerness, eagerness_from) = eagerness::at(archive, &survey.collections, &section);
    let thresholds = survey.thresholds(&section);
    let mut assessment = Assessment {
        eagerness,
        eagerness_from,
        thresholds,
        benefit: 0.0,
        cost: 0.0,
        findability: None,
        untested: None,
        recently_restructured: survey.recently(&section, thresholds.stability_days),
        worded_by_model: false,
        items: grouping.items,
        without_vectors: grouping.without_vectors,
        held_back: Vec::new(),
    };
    let mut chosen = choose_plans(
        &mut survey,
        &section,
        grouping,
        &options.grouping,
        &mut assessment.held_back,
    );
    let wording = registry.and_then(|r| ask_for_wording(r, &survey, &section, &chosen));
    let renames = name_plans(&survey, &section, &mut chosen, wording.as_ref());
    assessment.worded_by_model = wording.is_some();
    let mut proposal = Proposal::new(ProposalKind::Section, section.clone());
    proposal.changes = changes(&survey, &chosen, &renames);
    if !proposal.changes.is_empty() {
        test_findability(
            registry,
            &survey,
            &section,
            &chosen,
            options,
            &mut assessment,
        );
        // Even Eager won't propose a shape that's harder to navigate: with a model at hand,
        // a shape the test couldn't show is easier is held back too. Without one, it's a
        // draft that says it's untested.
        let held = match (&assessment.findability, &assessment.untested) {
            (Some(f), _) if !f.is_better() => {
                Some("the new shape wasn't easier to find things in, as tested".to_owned())
            }
            (None, Some(why)) if registry.is_some() => {
                Some(format!("it couldn't be tested for findability ({why})"))
            }
            _ => None,
        };
        if let Some(why) = held {
            for plan in &chosen {
                assessment.held_back.push(plan.held_back(why.clone()));
            }
            chosen.clear();
            proposal.changes.clear();
        }
    }
    assessment.benefit = chosen.iter().fold(0.0, |sum, p| sum + p.benefit);
    assessment.cost = chosen.iter().fold(0.0, |sum, p| sum + p.cost);
    proposal.invasiveness = proposal
        .changes
        .iter()
        .map(|c| c.invasiveness)
        .max()
        .unwrap_or(Invasiveness::Rewriting);
    proposal.rationale = wording
        .and_then(|w| w.rationale)
        .filter(|_| !proposal.changes.is_empty())
        .unwrap_or_else(|| rationale(&section, &proposal, &assessment));
    proposal.assessment = Some(assessment);
    Ok(proposal)
}

/// The plans worth proposing for `grouping`'s groups, then for single items the chosen groups
/// don't move; the rest are held back, with why. The crawler's verdicts the numbers disagree
/// with are set aside first.
fn choose_plans(
    survey: &mut Survey,
    section: &str,
    grouping: super::grouping::Grouping,
    options: &GroupingOptions,
    held: &mut Vec<HeldBack>,
) -> Vec<Plan> {
    survey
        .crawler
        .set_aside_where_the_numbers_disagree(&grouping.fits);
    let survey = &*survey;
    let plans: Vec<Plan> = grouping
        .groups
        .into_iter()
        .map(|g| judge(survey, section, g, options))
        .collect();
    let mut chosen = Vec::new();
    choose(survey, section, plans, &mut chosen, held);
    choose_item_moves(survey, section, &grouping.fits, options, &mut chosen, held);
    chosen
}

/// Adds to `chosen` the single items worth moving on their own ([`rehoming::item_moves`]),
/// among those in `fits` the chosen plans don't move already, as [`choose`] does.
fn choose_item_moves(
    survey: &Survey,
    section: &str,
    fits: &[super::grouping::ItemFit],
    options: &GroupingOptions,
    chosen: &mut Vec<Plan>,
    held: &mut Vec<HeldBack>,
) {
    let grouped: HashSet<Id> = chosen
        .iter()
        .flat_map(|p| p.group.members.iter().copied())
        .collect();
    let crawler: HashMap<Id, CrawlerMove> = survey
        .crawler
        .moves
        .iter()
        .filter(|(_, v)| is_within(&v.to, section))
        .map(|(id, v)| (*id, v.clone()))
        .collect();
    let moves = rehoming::item_moves(fits, &crawler, &survey.crawler.fitting, &grouped, options)
        .into_iter()
        .map(|m| judge_move(survey, m))
        .collect();
    choose(survey, section, moves, chosen, held);
}

/// Runs the findability test on the chosen plans, if a text model and cards with summaries
/// are at hand; otherwise says why not.
fn test_findability(
    registry: Option<&Registry>,
    survey: &Survey,
    section: &str,
    chosen: &[Plan],
    options: &SectionOptions,
    assessment: &mut Assessment,
) {
    let Some(registry) = registry else {
        assessment.untested =
            Some("no text model was at hand to run the findability test".to_owned());
        return;
    };
    let cards = findability::hold_out(&survey.catalog, section, options.sample, options.seed);
    if cards.is_empty() {
        assessment.untested =
            Some("no card in the section has a summary or title to test with".into());
        return;
    }
    let current = Tree::current(&survey.graph);
    let proposed = proposed_tree(survey, chosen);
    match findability::compare(registry, &current, &proposed, &cards, options.seed) {
        Ok(comparison) => {
            assessment.findability = Some(FindabilityResult {
                tried: comparison.current.tried,
                found_now: comparison.current.found,
                found_after: comparison.proposed.found,
                model: comparison.model,
                seed: comparison.seed,
            });
        }
        Err(e) => assessment.untested = Some(format!("the findability test couldn't run: {e}")),
    }
}

/// The archive's tree as the chosen plans would leave it.
fn proposed_tree(survey: &Survey, chosen: &[Plan]) -> Tree {
    let mut tree = Tree::current(&survey.graph);
    for plan in chosen {
        let to = match &plan.shape {
            Shape::New { home } => {
                let path = join(home, &plan.name);
                tree.add_collection(&path, Some(&plan.description));
                path
            }
            Shape::Merge { from, into } => {
                tree.collections.retain(|c| c.path != *from);
                for (_, place) in tree.placements.iter_mut().filter(|(_, p)| *p == from) {
                    place.clone_from(into);
                }
                into.clone()
            }
            Shape::Into { to } => to.clone(),
        };
        for id in &plan.group.members {
            tree.place(*id, &to);
        }
    }
    tree
}

fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{folder}/{name}")
    }
}

/// What a text model wrote for a proposal, already checked.
#[derive(Debug, Default)]
struct Wording {
    /// For each plan, by its place: a name, a description and a reason.
    plans: BTreeMap<usize, (String, String, String)>,
    /// Collections to rename: from, to, and why.
    renames: Vec<(String, String, String)>,
    rationale: Option<String>,
}

const WORDING_SYSTEM: &str = "You help reorganize a personal archive. Groups of items have been \
found that belong together. For each group, give a short folder name for the collection it \
becomes (a few words, no slashes), a one-line description of what belongs in it, and one \
sentence saying why the change helps. You may also suggest renaming an existing collection in \
the section whose name no longer fits what it holds. Then write a two-sentence rationale for \
the whole proposal. Answer in JSON.";

fn wording_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "groups": {"type": "array", "items": {
                "type": "object",
                "properties": {
                    "group": {"type": "integer"},
                    "name": {"type": "string"},
                    "description": {"type": "string"},
                    "reason": {"type": "string"},
                },
                "required": ["group", "name", "description", "reason"],
            }},
            "renames": {"type": "array", "items": {
                "type": "object",
                "properties": {
                    "from": {"type": "string"},
                    "to": {"type": "string"},
                    "reason": {"type": "string"},
                },
                "required": ["from", "to", "reason"],
            }},
            "rationale": {"type": "string"},
        },
        "required": ["groups", "rationale"],
    })
}

/// The prompt: the section's collections, and each group's items by title and tags. Restricted
/// items are left out, so nothing restricted reaches a model.
fn wording_prompt(survey: &Survey, section: &str, chosen: &[Plan]) -> String {
    let mut text = String::from("Collections in the section:\n");
    for c in survey
        .graph
        .collections
        .iter()
        .filter(|c| is_within(&c.path, section))
    {
        let description = c.description.as_deref().unwrap_or("");
        writeln!(text, "- {} ({} items): {description}", c.path, c.items).unwrap();
    }
    let nodes = survey.graph.positions();
    for (n, plan) in chosen.iter().enumerate() {
        writeln!(text, "\nGroup {n}: {}", plan.what()).unwrap();
        for id in plan.group.members.iter().take(12) {
            let Some(item) = nodes.get(id).map(|&i| &survey.graph.items[i]) else {
                continue;
            };
            if item.sensitivity == Sensitivity::Restricted {
                continue;
            }
            writeln!(text, "- {} [{}]", item.title, item.tags.join(", ")).unwrap();
        }
    }
    text
}

/// Asks the text model in `registry` for names, renames and a rationale, and keeps only what
/// checks out. `None` if no model answers.
fn ask_for_wording(
    registry: &Registry,
    survey: &Survey,
    section: &str,
    chosen: &[Plan],
) -> Option<Wording> {
    if chosen.is_empty() {
        return None;
    }
    let request = CompletionRequest {
        system: Some(WORDING_SYSTEM.to_owned()),
        prompt: wording_prompt(survey, section, chosen),
        schema: Some(wording_schema()),
        max_tokens: Some(1200),
    };
    let (completion, _, _) = registry.complete(&request).ok()?;
    let answer = completion
        .json
        .or_else(|| serde_json::from_str(&completion.text).ok())?;
    let mut wording = Wording::default();
    let text = |v: &Value, key: &str| {
        v[key]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    for group in answer["groups"].as_array().into_iter().flatten() {
        let (Some(n), Some(name), Some(description), Some(reason)) = (
            group["group"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok()),
            text(group, "name"),
            text(group, "description"),
            text(group, "reason"),
        ) else {
            continue;
        };
        if n < chosen.len() && crate::names::problems(&name).is_empty() && !name.contains('/') {
            wording.plans.insert(n, (name, description, reason));
        }
    }
    for rename in answer["renames"].as_array().into_iter().flatten() {
        if let (Some(from), Some(to), Some(reason)) = (
            text(rename, "from"),
            text(rename, "to"),
            text(rename, "reason"),
        ) {
            wording.renames.push((from, to, reason));
        }
    }
    wording.rationale = text(&answer, "rationale");
    Some(wording)
}

/// A plain name for a group without a model's: its most shared tag, in words, or a word most
/// of its titles share.
fn plain_name(survey: &Survey, group: &CandidateGroup) -> Option<String> {
    let words = |tag: &str| {
        let spaced = tag.replace('-', " ");
        let mut chars = spaced.chars();
        chars
            .next()
            .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
    };
    if let Some(tag) = group.tags.first() {
        return words(tag);
    }
    let nodes = survey.graph.positions();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for id in &group.members {
        let Some(item) = nodes.get(id).map(|&i| &survey.graph.items[i]) else {
            continue;
        };
        let seen: BTreeSet<String> = item
            .title
            .split(|c: char| !c.is_alphabetic())
            .filter(|w| w.chars().count() > 3)
            .map(str::to_lowercase)
            .collect();
        for w in seen {
            *counts.entry(w).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, n)| n * 2 >= group.members.len())
        .max_by_key(|(_, n)| *n)
        .and_then(|(w, _)| words(&w))
}

/// Whether `name` is free in `folder` (on disk and among `taken`), compared without case.
fn is_free(survey: &Survey, folder: &str, name: &str, taken: &BTreeSet<String>) -> bool {
    let wanted = join(folder, name).to_lowercase();
    if taken.contains(&wanted) {
        return false;
    }
    if folder.is_empty() && ROOT_RESERVED.iter().any(|r| r.eq_ignore_ascii_case(name)) {
        return false;
    }
    let dir = survey.archive.root().join(folder);
    !std::fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
    })
}

/// Gives each plan its name, description and reason, from the model's wording where it
/// checks out, else plain ones; returns the renames that check out.
fn name_plans(
    survey: &Survey,
    section: &str,
    chosen: &mut [Plan],
    wording: Option<&Wording>,
) -> Vec<(String, String, String)> {
    let mut taken: BTreeSet<String> = BTreeSet::new();
    for (n, plan) in chosen.iter_mut().enumerate() {
        // Items going into an existing collection keep its name, and their reason is the
        // numbers' and the crawler's.
        let worded = wording
            .and_then(|w| w.plans.get(&n))
            .filter(|_| !matches!(plan.shape, Shape::Into { .. }));
        let home = match &plan.shape {
            Shape::New { home } => home.clone(),
            Shape::Merge { .. } | Shape::Into { .. } => String::new(),
        };
        let base = worded
            .map(|(name, _, _)| name.clone())
            .filter(|name| !super::hidden_name(name))
            .or_else(|| plain_name(survey, &plan.group))
            .unwrap_or_else(|| format!("Group {}", n + 1));
        let mut name = base.clone();
        let mut k = 2;
        while matches!(plan.shape, Shape::New { .. }) && !is_free(survey, &home, &name, &taken) {
            name = format!("{base} {k}");
            k += 1;
        }
        taken.insert(join(&home, &name).to_lowercase());
        plan.name = name;
        plan.description = worded.map_or_else(
            || plain_description(&plan.group),
            |(_, description, _)| description.clone(),
        );
        plan.reason = worded.map_or_else(
            || plain_reason(survey, plan),
            |(_, _, reason)| reason.clone(),
        );
    }
    let Some(wording) = wording else {
        return Vec::new();
    };
    wording
        .renames
        .iter()
        .filter(|(from, to, _)| {
            let parent = from.rsplit_once('/').map_or("", |(p, _)| p);
            survey.collections.iter().any(|c| c.path == *from)
                && crate::graph::is_within(from, section)
                && from != HOLDING
                && !super::hidden_name(to)
                && !to.contains('/')
                && crate::names::problems(to).is_empty()
                && !survey.has_checked_out(from)
                && is_free(survey, parent, to, &taken)
                && !chosen.iter().any(|p| touches(p, from))
        })
        .cloned()
        .collect()
}

/// Whether `plan` changes the collection at `path` or takes items from it.
fn touches(plan: &Plan, path: &str) -> bool {
    plan.group.sources.iter().any(|s| s.collection == path)
        || matches!(&plan.shape, Shape::Merge { from, into } if from == path || into == path)
        || matches!(&plan.shape, Shape::New { home } if home == path)
        || matches!(&plan.shape, Shape::Into { to } if to == path)
}

fn plain_description(group: &CandidateGroup) -> String {
    match group.tags.as_slice() {
        [] => format!("{} items that belong together", group.members.len()),
        [one] => format!("Items about {}", one.replace('-', " ")),
        [one, two, ..] => format!(
            "Items about {} and {}",
            one.replace('-', " "),
            two.replace('-', " ")
        ),
    }
}

fn plain_reason(survey: &Survey, plan: &Plan) -> String {
    let n = plan.group.members.len();
    let why = format!(
        "these {n} items are alike (cohesion {:.2}) and belong together",
        plan.group.cohesion
    );
    match &plan.shape {
        Shape::Merge { from, into } => {
            format!("{from} holds too few items to stand alone, and they belong with {into}: {why}")
        }
        Shape::New { .. } => {
            let sources: Vec<String> = plan
                .group
                .sources
                .iter()
                .map(|s| format!("{} from {}", s.items, s.collection))
                .collect();
            format!("{}; they're now {}", capitalize(&why), sources.join(", "))
        }
        Shape::Into { to } => into_reason(survey, plan, to),
    }
}

/// Why items go into the existing collection `to`: what the numbers say, and what the crawler
/// said.
fn into_reason(survey: &Survey, plan: &Plan, to: &str) -> String {
    let n = plan.group.members.len();
    let (them, are) = if n == 1 {
        ("it", "is")
    } else {
        ("they", "are")
    };
    let mut why = Vec::new();
    if plan.improvement > 0.0 {
        why.push(format!(
            "{them} {are} more like {to}'s items than those where {them} {are} (by {:.2})",
            plan.improvement
        ));
    }
    let sent = plan
        .group
        .members
        .iter()
        .filter(|id| survey.crawler.moves.get(id).is_some_and(|v| v.to == to))
        .count();
    if sent > 0 {
        why.push(if n == 1 {
            format!("the crawler judged it belongs in {to}")
        } else {
            format!("the crawler judged {sent} of them belong in {to}")
        });
    }
    if why.is_empty() {
        why.push(if n == 1 {
            format!("it doesn't fit where it is, and belongs with {to}")
        } else {
            format!("they don't fit where they are, and belong with {to}")
        });
    }
    let what = if n == 1 {
        "This item belongs elsewhere".to_owned()
    } else {
        format!("These {n} items belong elsewhere")
    };
    format!("{what}: {}", why.join("; "))
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// The proposal's changes for the chosen plans and renames, numbered from 1.
/// The items `ids` as a change lists them.
fn touched(survey: &Survey, ids: &[Id]) -> Vec<Touched> {
    let nodes = survey.graph.positions();
    ids.iter()
        .filter_map(|id| nodes.get(id).map(|&i| &survey.graph.items[i]))
        .map(|i| Touched {
            id: i.id,
            card: i.card.clone(),
            title: i.title.clone(),
        })
        .collect()
}

/// The two changes a new collection makes: creating it, then moving its items in, which
/// needs it. Returns the creating change's number.
fn new_collection_changes(
    survey: &Survey,
    plan: &Plan,
    home: &str,
    list: &mut Vec<ProposedChange>,
) -> String {
    let path = join(home, &plan.name);
    let create = (list.len() + 1).to_string();
    // A new subcollection subdivides; a new top-level collection reshapes the archive.
    let creating = if home.is_empty() && plan.invasiveness == Invasiveness::TopLevel {
        Invasiveness::TopLevel
    } else {
        Invasiveness::Subdividing
    };
    list.push(change(
        &create,
        Edit::CreateCollection {
            path: path.clone(),
            description: plan.description.clone(),
        },
        format!("A home for the group: {}", plan.description),
        creating,
    ));
    let mut moves = change(
        &(list.len() + 1).to_string(),
        Edit::MoveItems { to: path },
        plan.reason.clone(),
        plan.invasiveness,
    );
    moves.items = touched(survey, &plan.group.members);
    moves.depends_on = vec![create.clone()];
    list.push(moves);
    create
}

/// The change merging `from` into `into`, listing every item it moves.
fn merge_change(survey: &Survey, plan: &Plan, from: &str, into: &str, n: usize) -> ProposedChange {
    let mut merge = change(
        &n.to_string(),
        Edit::MergeCollection {
            from: from.to_owned(),
            into: into.to_owned(),
        },
        plan.reason.clone(),
        plan.invasiveness,
    );
    let ids: Vec<Id> = survey
        .graph
        .items
        .iter()
        .filter(|i| i.collection == from)
        .map(|i| i.id)
        .collect();
    merge.items = touched(survey, &ids);
    merge.documents = vec![format!("{into}/_collection.md")];
    merge
}

/// The proposal's changes for the chosen plans and renames, numbered from 1.
fn changes(
    survey: &Survey,
    chosen: &[Plan],
    renames: &[(String, String, String)],
) -> Vec<ProposedChange> {
    let mut list: Vec<ProposedChange> = Vec::new();
    let next = |list: &Vec<ProposedChange>| (list.len() + 1).to_string();
    let mut created: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();
    for plan in chosen {
        match &plan.shape {
            Shape::New { home } => {
                let create = new_collection_changes(survey, plan, home, &mut list);
                if !home.is_empty() {
                    created.entry(home.clone()).or_default().push((
                        plan.name.clone(),
                        plan.description.clone(),
                        create,
                    ));
                }
            }
            Shape::Merge { from, into } => {
                let merge = merge_change(survey, plan, from, into, list.len() + 1);
                list.push(merge);
            }
            Shape::Into { to } => {
                let mut moves = change(
                    &next(&list),
                    Edit::MoveItems { to: to.clone() },
                    plan.reason.clone(),
                    plan.invasiveness,
                );
                moves.items = touched(survey, &plan.group.members);
                list.push(moves);
            }
        }
    }
    for (home, subcollections) in created {
        let organization = subcollections
            .iter()
            .map(|(name, description, _)| format!("- **{name}**: {description}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut rewrite = change(
            &next(&list),
            Edit::RewriteDesign {
                collection: home.clone(),
                description: None,
                organization: Some(organization),
            },
            format!(
                "{home}'s design document names its new subcollections, so filing knows what each holds"
            ),
            Invasiveness::Rewriting,
        );
        rewrite.documents = vec![format!("{home}/_collection.md")];
        rewrite.depends_on = subcollections.into_iter().map(|(_, _, id)| id).collect();
        list.push(rewrite);
    }
    for (from, to, reason) in renames {
        let parent = from.rsplit_once('/').map_or("", |(p, _)| p);
        let mut rename = change(
            &next(&list),
            Edit::RenameCollection {
                from: from.clone(),
                to: join(parent, to),
            },
            reason.clone(),
            Invasiveness::Renaming,
        );
        rename.documents = vec![format!("{}/_collection.md", join(parent, to))];
        list.push(rename);
    }
    list
}

fn change(id: &str, edit: Edit, reason: String, invasiveness: Invasiveness) -> ProposedChange {
    ProposedChange {
        id: id.to_owned(),
        change: edit,
        reason,
        invasiveness,
        items: Vec::new(),
        held: Vec::new(),
        documents: Vec::new(),
        depends_on: Vec::new(),
        extra: crate::card::Extra::new(),
    }
}

/// The rationale in plain words, when no model wrote one.
fn rationale(section: &str, proposal: &Proposal, assessment: &Assessment) -> String {
    let what = if section.is_empty() {
        "the archive".to_owned()
    } else {
        section.to_owned()
    };
    let level = match &assessment.eagerness_from {
        Source::Archive => format!("{} (the archive's setting)", assessment.eagerness),
        Source::Collection(c) => format!("{} (set by {c})", assessment.eagerness),
    };
    if proposal.changes.is_empty() {
        let held = assessment.held_back.len();
        return if held == 0 {
            format!(
                "Nothing in {what} is worth reorganizing: no group of its {} items belongs together more than it does now.",
                assessment.items
            )
        } else {
            format!(
                "Nothing in {what} is worth reorganizing at {level}: {held} possible change{} {} held back.",
                if held == 1 { "" } else { "s" },
                if held == 1 { "is" } else { "are" }
            )
        };
    }
    let moved: usize = proposal
        .changes
        .iter()
        .filter(|c| {
            matches!(
                c.change,
                Edit::MoveItems { .. } | Edit::MergeCollection { .. }
            )
        })
        .map(|c| c.items.len())
        .sum();
    let kinds = |wanted: fn(&Edit) -> bool| {
        proposal
            .changes
            .iter()
            .filter(|c| wanted(&c.change))
            .count()
    };
    let created = kinds(|e| matches!(e, Edit::CreateCollection { .. }));
    let merged = kinds(|e| matches!(e, Edit::MergeCollection { .. }));
    let renamed = kinds(|e| matches!(e, Edit::RenameCollection { .. }));
    let mut parts = vec![format!(
        "moves {moved} item{}",
        if moved == 1 { "" } else { "s" }
    )];
    for (n, what) in [
        (created, "new collection"),
        (merged, "merged collection"),
        (renamed, "renamed collection"),
    ] {
        if n > 0 {
            parts.push(format!("{n} {what}{}", if n == 1 { "" } else { "s" }));
        }
    }
    let mut text = format!(
        "A better shape for {what}, judged at {level}: {}. Its benefit ({:.2}) beats its cost ({:.2}).",
        parts.join(", "),
        assessment.benefit,
        assessment.cost,
    );
    match (&assessment.findability, &assessment.untested) {
        (Some(f), _) => {
            let _ = write!(
                text,
                " The findability test found {} of {} held-out cards with it, against {} now.",
                f.found_after, f.tried, f.found_now
            );
        }
        (None, Some(_)) => {
            text.push_str(" Untested: a device with a text model can run the findability test.");
        }
        (None, None) => {}
    }
    if !assessment.worded_by_model {
        text.push_str(" Names are plain ones from the groups' tags; a device with a text model can word it better.");
    }
    text
}
