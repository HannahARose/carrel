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
//! collection, gathering Holding's items, or gathering items from several branches), or, when
//! the group is a small collection's every item plus items of one other collection, merging
//! the small one into the other. A rewrite of the home collection's design document, naming
//! its new subcollections, depends on the collections it names.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::eagerness::{self, Eagerness, Source, Thresholds};
use super::findability::{self, Tree};
use super::grouping::{CandidateGroup, GroupingOptions, group_section};
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
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        Ok(Self {
            archive,
            collections: archive.collections()?,
            catalog,
            graph,
            unsure,
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
    let folder = |p: &str| p.rsplit_once('/').map_or("", |(f, _)| f).to_owned();
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

/// What `group` becomes: merging a small collection, when the group is all of one small
/// collection's items and some of one other's; otherwise a new collection.
fn shape_of(survey: &Survey, group: &CandidateGroup, warrant: usize) -> (Shape, Invasiveness) {
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
                return (
                    Shape::Merge {
                        from: small.collection.clone(),
                        into: other.collection.clone(),
                    },
                    invasiveness,
                );
            }
        }
    }
    let invasiveness = if group.home.is_empty()
        && group.sources.iter().any(|s| !in_holding(&s.collection))
        && group.invasiveness >= Invasiveness::BetweenBranches
    {
        Invasiveness::TopLevel
    } else {
        group.invasiveness
    };
    (
        Shape::New {
            home: group.home.clone(),
        },
        invasiveness,
    )
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

/// Judges `group`: what it becomes, what it's worth and what it costs.
///
/// - **Benefit:** how much more alike its items are than grouping needed (half of it), the
///   share of them that leave Holding or a placement filing wasn't sure of (0.3), and 0.2 when
///   it subdivides a crowded collection.
/// - **Cost:** each item moved out of a collection the owner knows, weighted by how invasive
///   the change is, plus the stability penalty when the section was restructured recently.
#[allow(clippy::cast_precision_loss)] // item counts
fn judge(survey: &Survey, group: CandidateGroup, options: &GroupingOptions) -> Plan {
    let (shape, invasiveness) = shape_of(survey, &group, options.warrant);
    let thresholds = plan_thresholds(survey, &group, &shape);
    let members = group.members.len().max(1) as f64;
    let collection_of: HashMap<Id, &str> = survey
        .graph
        .items
        .iter()
        .map(|i| (i.id, i.collection.as_str()))
        .collect();
    let destination = match &shape {
        Shape::New { .. } => None,
        Shape::Merge { into, .. } => Some(into.as_str()),
    };
    let moving: Vec<&Id> = group
        .members
        .iter()
        .filter(|id| collection_of.get(id).copied() != destination)
        .collect();
    let resolved = moving
        .iter()
        .filter(|id| {
            survey.unsure.contains(id) || collection_of.get(id).is_some_and(|c| in_holding(c))
        })
        .count();
    let known_moves = moving
        .iter()
        .filter(|id| collection_of.get(id).is_some_and(|c| !in_holding(c)))
        .count();
    let tightness = ((group.cohesion - options.threshold) / (1.0 - options.threshold).max(1e-9))
        .clamp(0.0, 1.0);
    let crowded = matches!(&shape, Shape::New { home } if invasiveness == Invasiveness::Subdividing
        && survey.items_in(home) > survey.crowded_items);
    let benefit =
        0.5 * tightness + 0.3 * resolved as f64 / members + if crowded { 0.2 } else { 0.0 };
    let touched: Vec<&str> = group
        .sources
        .iter()
        .map(|s| s.collection.as_str())
        .collect();
    let recent = touched
        .iter()
        .any(|c| survey.recently(c, thresholds.stability_days));
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

/// Keeps the plans whose gain beats what their eagerness asks, best first, within the move
/// budget; the rest are held back, with why.
fn choose(
    survey: &Survey,
    section: &str,
    mut plans: Vec<Plan>,
    held: &mut Vec<HeldBack>,
) -> Vec<Plan> {
    plans.sort_by(|a, b| b.gain().total_cmp(&a.gain()));
    let budget = survey.thresholds(section).move_budget;
    let mut spent = 0;
    let mut merged: BTreeSet<String> = BTreeSet::new();
    let mut chosen = Vec::new();
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
    chosen
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
    let survey = Survey::read(archive)?;
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
    let plans: Vec<Plan> = grouping
        .groups
        .into_iter()
        .map(|g| judge(&survey, g, &options.grouping))
        .collect();
    let mut chosen = choose(&survey, &section, plans, &mut assessment.held_back);
    let wording = registry.and_then(|r| ask_for_wording(r, &survey, &section, &chosen));
    let renames = name_plans(&survey, &mut chosen, wording.as_ref());
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
        if assessment
            .findability
            .as_ref()
            .is_some_and(|f| !f.is_better())
        {
            for plan in &chosen {
                assessment.held_back.push(
                    plan.held_back("the new shape wasn't easier to find things in, as tested"),
                );
            }
            chosen.clear();
            proposal.changes.clear();
        }
    }
    assessment.benefit = chosen.iter().map(|p| p.benefit).sum();
    assessment.cost = chosen.iter().map(|p| p.cost).sum();
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
        assessment.untested = Some("no card in the section has a summary to test with".into());
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
    use std::fmt::Write as _;
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
    chosen: &mut [Plan],
    wording: Option<&Wording>,
) -> Vec<(String, String, String)> {
    let mut taken: BTreeSet<String> = BTreeSet::new();
    for (n, plan) in chosen.iter_mut().enumerate() {
        let worded = wording.and_then(|w| w.plans.get(&n));
        let home = match &plan.shape {
            Shape::New { home } => home.clone(),
            Shape::Merge { .. } => String::new(),
        };
        let base = worded
            .map(|(name, _, _)| name.clone())
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
        plan.reason = worded.map_or_else(|| plain_reason(plan), |(_, _, reason)| reason.clone());
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
                && from != HOLDING
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

fn plain_reason(plan: &Plan) -> String {
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
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// The proposal's changes for the chosen plans and renames, numbered from 1.
fn changes(
    survey: &Survey,
    chosen: &[Plan],
    renames: &[(String, String, String)],
) -> Vec<ProposedChange> {
    let mut list: Vec<ProposedChange> = Vec::new();
    let next = |list: &Vec<ProposedChange>| (list.len() + 1).to_string();
    let touched = |ids: &[Id]| -> Vec<Touched> {
        let nodes = survey.graph.positions();
        ids.iter()
            .filter_map(|id| nodes.get(id).map(|&i| &survey.graph.items[i]))
            .map(|i| Touched {
                id: i.id,
                card: i.card.clone(),
                title: i.title.clone(),
            })
            .collect()
    };
    let mut created: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();
    for plan in chosen {
        match &plan.shape {
            Shape::New { home } => {
                let path = join(home, &plan.name);
                let create = next(&list);
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
                let moving: Vec<Id> = plan.group.members.clone();
                let mut moves = change(
                    &next(&list),
                    Edit::MoveItems { to: path.clone() },
                    plan.reason.clone(),
                    plan.invasiveness,
                );
                moves.items = touched(&moving);
                moves.depends_on = vec![create.clone()];
                list.push(moves);
                if !home.is_empty() {
                    created.entry(home.clone()).or_default().push((
                        plan.name.clone(),
                        plan.description.clone(),
                        create,
                    ));
                }
            }
            Shape::Merge { from, into } => {
                let mut merge = change(
                    &next(&list),
                    Edit::MergeCollection {
                        from: from.clone(),
                        into: into.clone(),
                    },
                    plan.reason.clone(),
                    plan.invasiveness,
                );
                let ids: Vec<Id> = survey
                    .graph
                    .items
                    .iter()
                    .filter(|i| i.collection == *from)
                    .map(|i| i.id)
                    .collect();
                merge.items = touched(&ids);
                merge.documents = vec![format!("{into}/_collection.md")];
                list.push(merge);
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
    let mut text = format!(
        "{} change{} to {what}, moving {moved} item{}: benefit {:.2} against cost {:.2}, judged at {level}.",
        proposal.changes.len(),
        if proposal.changes.len() == 1 { "" } else { "s" },
        if moved == 1 { "" } else { "s" },
        assessment.benefit,
        assessment.cost,
    );
    match (&assessment.findability, &assessment.untested) {
        (Some(f), _) => text.push_str(&format!(
            " Findability: {} of {} held-out cards found now, {} after.",
            f.found_now, f.tried, f.found_after
        )),
        (None, Some(why)) => text.push_str(&format!(" Untested: {why}.")),
        (None, None) => {}
    }
    if !assessment.worded_by_model {
        text.push_str(" Names are plain ones from the groups' tags; a device with a text model can word it better.");
    }
    text
}
