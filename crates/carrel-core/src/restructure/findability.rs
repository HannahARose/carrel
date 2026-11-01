//! The findability test (`docs/design/restructure.md`, "When one is proposed"): whether a
//! proposed tree is easier to navigate than the current one. A sample of cards is held out,
//! and a text model tries to find each by walking a tree from the card's summary alone: at
//! each collection it's shown the collections inside, by name and description, and picks
//! where to look next, or says the item is here. A walk that ends in the item's collection
//! found it. The same cards walk the current tree and the proposed one, and a proposal goes
//! forward only if the proposed tree is found more often.
//!
//! It's the same idea as v0.4's findability benchmark (`crate::bench::findability`), used on
//! the owner's own archive with a model in place of search. The sample comes from a seed, so
//! a test can be repeated; the model runs through the text provider interface
//! ([`Registry::complete`]), so any text model on the device can do it, and tests use a
//! scripted one. Nothing here moves a file: the trees are plain data.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::card::{Id, Sensitivity};
use crate::discover::{Catalog, Rng};
use crate::graph::{Graph, is_within};
use crate::providers::{CompletionRequest, ProviderError, Registry};

/// The answer that says the item is in the collection the walk has reached.
pub const HERE: &str = "here";

/// How many cards are held out unless asked otherwise.
pub const DEFAULT_SAMPLE: usize = 40;

/// The deepest a walk goes, so a model that wanders can't walk forever.
pub const MAX_STEPS: usize = 12;

/// The longest answer a step needs, in tokens.
const ANSWER_TOKENS: u32 = 64;

/// A collection in a tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeCollection {
    /// Its folder, relative to the archive root.
    pub path: String,
    /// What it holds, as its design document says, shown to the model beside its name.
    pub description: Option<String>,
}

/// A shape for the archive: its collections, and where each item is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tree {
    /// The collections, sorted by path.
    pub collections: Vec<TreeCollection>,
    /// Each item's collection.
    pub placements: BTreeMap<Id, String>,
}

impl Tree {
    /// The archive's shape as it is now, from its graph.
    #[must_use]
    pub fn current(graph: &Graph) -> Self {
        let mut tree = Self::default();
        for collection in &graph.collections {
            tree.add_collection(&collection.path, collection.description.as_deref());
        }
        for item in &graph.items {
            tree.placements.insert(item.id, item.collection.clone());
        }
        tree
    }

    /// Adds the collection at `path`, or gives it `description` if it's there already.
    pub fn add_collection(&mut self, path: &str, description: Option<&str>) {
        let description = description.map(str::to_owned);
        match self
            .collections
            .binary_search_by(|c| c.path.as_str().cmp(path))
        {
            Ok(at) => self.collections[at].description = description,
            Err(at) => self.collections.insert(
                at,
                TreeCollection {
                    path: path.to_owned(),
                    description,
                },
            ),
        }
    }

    /// Puts the item `id` in the collection at `path`.
    pub fn place(&mut self, id: Id, path: &str) {
        self.placements.insert(id, path.to_owned());
    }

    /// The collections directly inside `path` (empty for the top level), as their names
    /// and descriptions.
    fn children(&self, path: &str) -> Vec<(&str, &TreeCollection)> {
        self.collections
            .iter()
            .filter_map(|c| {
                let rest = if path.is_empty() {
                    c.path.as_str()
                } else {
                    c.path.strip_prefix(path)?.strip_prefix('/')?
                };
                (!rest.is_empty() && !rest.contains('/')).then_some((rest, c))
            })
            .collect()
    }
}

/// A card held out for the test: the item, and the summary the model looks for it by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldOut {
    /// The item.
    pub id: Id,
    /// Its card's summary, or its title and tags when no card in the section has a summary.
    pub summary: String,
}

/// Up to `count` cards in `section` (a collection's path, or empty for the whole archive)
/// with a summary, chosen by `seed`: the same seed gives the same cards. When no card there
/// has a summary, as before any were drafted, each is looked for by its title and tags
/// instead, so the test still runs. Restricted items are never held out, since what they're
/// looked for by would go to a model.
#[must_use]
pub fn hold_out(catalog: &Catalog, section: &str, count: usize, seed: u64) -> Vec<HeldOut> {
    let open: Vec<&crate::discover::Entry> = catalog
        .entries
        .iter()
        .filter(|e| is_within(&e.item.collection, section))
        .filter(|e| e.item.sensitivity != Sensitivity::Restricted)
        .collect();
    let summarized = |e: &crate::discover::Entry| {
        let summary = e.summary.as_deref()?.trim();
        (!summary.is_empty()).then(|| summary.to_owned())
    };
    let titled = |e: &crate::discover::Entry| {
        let title = e.item.title.trim();
        (!title.is_empty()).then(|| {
            if e.tags.is_empty() {
                title.to_owned()
            } else {
                format!("{title} ({})", e.tags.join(", "))
            }
        })
    };
    let by_summary = open.iter().any(|e| summarized(e).is_some());
    let mut candidates: Vec<HeldOut> = open
        .iter()
        .filter_map(|e| {
            let said = if by_summary { summarized(e) } else { titled(e) }?;
            Some(HeldOut {
                id: e.item.id,
                summary: said,
            })
        })
        .collect();
    // A partial shuffle: the first `count` are a fair sample.
    let mut rng = Rng::new(seed);
    let count = count.min(candidates.len());
    for i in 0..count {
        let j = i + rng.below(candidates.len() - i);
        candidates.swap(i, j);
    }
    candidates.truncate(count);
    candidates
}

/// One card's walk through one tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Walk {
    /// The item looked for.
    pub id: Id,
    /// The collections the walk went into, in order.
    pub path: Vec<String>,
    /// Whether it ended where the item is.
    pub found: bool,
    /// Whether it ended on an answer that wasn't one of the choices.
    pub lost: bool,
}

/// How one tree did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeScore {
    /// How many cards were found.
    pub found: usize,
    /// How many were looked for.
    pub tried: usize,
    /// Each walk, in the sample's order.
    pub walks: Vec<Walk>,
}

impl TreeScore {
    /// The share of cards found, from 0 to 1; 0 when none were looked for.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // sample sizes
    pub fn rate(&self) -> f64 {
        if self.tried == 0 {
            0.0
        } else {
            self.found as f64 / self.tried as f64
        }
    }
}

/// The current tree against a proposed one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comparison {
    /// The seed the sample was chosen with, to repeat the test.
    pub seed: u64,
    /// The model that walked, as it named itself.
    pub model: Option<String>,
    /// How the current tree did.
    pub current: TreeScore,
    /// How the proposed tree did.
    pub proposed: TreeScore,
}

impl Comparison {
    /// How much more often the proposed tree's cards were found, from -1 to 1.
    #[must_use]
    pub fn gain(&self) -> f64 {
        self.proposed.rate() - self.current.rate()
    }

    /// Whether the proposed tree is easier to navigate: its cards were found more often.
    #[must_use]
    pub fn proposed_is_better(&self) -> bool {
        self.proposed.found > self.current.found
    }
}

/// The JSON Schema for one step's answer: one of `choices`.
fn schema(choices: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": { "choice": { "type": "string", "enum": choices } },
        "required": ["choice"],
        "additionalProperties": false,
    })
}

const SYSTEM: &str = "You are finding an item in a personal archive by walking its collections, \
knowing only the item's summary. At each step you are shown the collections inside the one \
you're in. Answer with the name of the collection to look in next, or \"here\" if the item \
belongs in the collection you're in. Answer in JSON: {\"choice\": \"...\"}.";

/// The prompt for one step: the summary, where the walk is, and the choices.
fn prompt(summary: &str, here: &str, children: &[(&str, &TreeCollection)]) -> String {
    let mut text = format!("The item: {summary}\n\n");
    if here.is_empty() {
        text.push_str("You're at the top of the archive. Its collections:\n");
    } else {
        writeln!(text, "You're in {here}. The collections inside it:").unwrap();
    }
    for (name, collection) in children {
        match &collection.description {
            Some(d) if !d.trim().is_empty() => writeln!(text, "- {name}: {d}").unwrap(),
            _ => writeln!(text, "- {name}").unwrap(),
        }
    }
    if !here.is_empty() {
        writeln!(text, "- {HERE}: it belongs in {here} itself").unwrap();
    }
    text
}

/// The choice a completion makes: its JSON's `choice`, else its text.
fn choice_of(json: Option<&Value>, text: &str) -> String {
    json.and_then(|j| j["choice"].as_str())
        .map_or_else(|| text.trim().trim_matches('"').to_owned(), str::to_owned)
}

/// Walks `tree` looking for `card` with the text model `registry` ranks first. Returns the
/// walk and the model's name.
///
/// # Errors
///
/// The provider's error if no text model answers.
pub fn walk_tree(
    registry: &Registry,
    tree: &Tree,
    card: &HeldOut,
) -> Result<(Walk, String), ProviderError> {
    let mut here = String::new();
    let mut path = Vec::new();
    let mut model = String::new();
    let mut lost = false;
    for _ in 0..MAX_STEPS {
        let children = tree.children(&here);
        if children.is_empty() {
            break;
        }
        let mut choices: Vec<&str> = children.iter().map(|(n, _)| *n).collect();
        if !here.is_empty() {
            choices.push(HERE);
        }
        let request = CompletionRequest {
            system: Some(SYSTEM.to_owned()),
            prompt: prompt(&card.summary, &here, &children),
            schema: Some(schema(&choices)),
            max_tokens: Some(ANSWER_TOKENS),
        };
        let (completion, _, _) = registry.complete(&request)?;
        model.clone_from(&completion.model);
        let choice = choice_of(completion.json.as_ref(), &completion.text);
        if choice.eq_ignore_ascii_case(HERE) && !here.is_empty() {
            break;
        }
        let Some((_, next)) = children
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&choice))
        else {
            // Every answer is checked, as filing's are: one that isn't a choice ends the walk.
            lost = true;
            break;
        };
        here.clone_from(&next.path);
        path.push(here.clone());
    }
    let found = !lost && tree.placements.get(&card.id) == Some(&here);
    Ok((
        Walk {
            id: card.id,
            path,
            found,
            lost,
        },
        model,
    ))
}

/// Walks `tree` for every card in `cards`.
fn score(
    registry: &Registry,
    tree: &Tree,
    cards: &[HeldOut],
    model: &mut Option<String>,
) -> Result<TreeScore, ProviderError> {
    let mut score = TreeScore::default();
    for card in cards {
        let (walk, name) = walk_tree(registry, tree, card)?;
        if !name.is_empty() {
            model.get_or_insert(name);
        }
        score.tried += 1;
        score.found += usize::from(walk.found);
        score.walks.push(walk);
    }
    Ok(score)
}

/// Compares the `current` tree with a `proposed` one: the text model `registry` ranks first
/// walks each for every held-out card in `cards` (from [`hold_out`] with `seed`).
///
/// # Errors
///
/// The provider's error if no text model answers.
pub fn compare(
    registry: &Registry,
    current: &Tree,
    proposed: &Tree,
    cards: &[HeldOut],
    seed: u64,
) -> Result<Comparison, ProviderError> {
    let mut model = None;
    let current = score(registry, current, cards, &mut model)?;
    let proposed = score(registry, proposed, cards, &mut model)?;
    Ok(Comparison {
        seed,
        model,
        current,
        proposed,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::archive::Archive;
    use crate::discover::tests::{collection, entry};
    use crate::providers::{Capability, Completion, Offer, Provider, priority};

    /// A scripted text model: it picks the first choice whose name the summary mentions, or
    /// "here", or, if `stray`, an answer that isn't a choice. It keeps every prompt.
    struct Scripted {
        offers: Vec<Offer>,
        stray: bool,
        prompts: Mutex<Vec<String>>,
    }

    impl Scripted {
        fn new(stray: bool) -> Self {
            Self {
                offers: vec![Offer {
                    capability: Capability::TextModel,
                    id: "scripted".into(),
                    label: None,
                    priority: priority::BUILT_IN,
                    available: true,
                    reason: None,
                    media_types: Vec::new(),
                    extra: serde_json::Map::new(),
                }],
                stray,
                prompts: Mutex::new(Vec::new()),
            }
        }
    }

    impl Provider for Scripted {
        fn name(&self) -> &'static str {
            "scripted"
        }

        fn offers(&self) -> &[Offer] {
            &self.offers
        }

        fn complete(
            &self,
            _offer: &str,
            request: &CompletionRequest,
        ) -> Result<crate::providers::Completion, ProviderError> {
            self.prompts.lock().unwrap().push(request.prompt.clone());
            let summary = request
                .prompt
                .lines()
                .next()
                .unwrap_or_default()
                .to_lowercase();
            let choices: Vec<String> =
                request.schema.as_ref().unwrap()["properties"]["choice"]["enum"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| c.as_str().unwrap().to_owned())
                    .collect();
            let choice = if self.stray {
                "Nowhere".to_owned()
            } else {
                choices
                    .iter()
                    .find(|c| *c != HERE && summary.contains(&c.to_lowercase()))
                    .cloned()
                    .unwrap_or_else(|| HERE.to_owned())
            };
            Ok(Completion {
                text: String::new(),
                json: Some(json!({ "choice": choice })),
                model: "scripted-1".into(),
                usage: None,
            })
        }
    }

    fn registry(stray: bool) -> Registry {
        let mut registry = Registry::new();
        registry.add(Box::new(Scripted::new(stray)));
        registry
    }

    fn id(n: u128) -> Id {
        Id(uuid::Uuid::from_u128(n))
    }

    /// Bird notes muddled into Holding, star notes in Reading; summaries mention where a
    /// reader would look.
    fn catalog() -> Catalog {
        let mut entries = Vec::new();
        for n in 1..=4_u128 {
            let mut bird = entry(n, &format!("bird{n}"), "Holding");
            bird.summary = Some(format!("Reading notes on birds, number {n}"));
            entries.push(bird);
            let mut star = entry(n + 10, &format!("star{n}"), "Reading");
            star.summary = Some(format!("Reading notes on stars, number {n}"));
            entries.push(star);
        }
        let mut secret = entry(20, "secret", "Reading");
        secret.summary = Some("Reading a private letter".into());
        secret.item.sensitivity = Sensitivity::Restricted;
        entries.push(secret);
        entries.push(entry(21, "untitled", "Reading"));
        Catalog::new(entries, vec![collection("Holding"), collection("Reading")])
    }

    fn proposed(current: &Tree) -> Tree {
        let mut tree = current.clone();
        tree.add_collection("Reading/Birds", Some("Notes on birds"));
        tree.add_collection("Reading/Stars", Some("Notes on stars"));
        for n in 1..=4 {
            tree.place(id(n), "Reading/Birds");
            tree.place(id(n + 10), "Reading/Stars");
        }
        tree
    }

    #[test]
    fn held_out_cards_have_summaries_and_come_from_the_seed() {
        let catalog = catalog();
        let all = hold_out(&catalog, "", 100, 7);
        assert_eq!(
            all.len(),
            8,
            "no restricted item, and none without a summary"
        );
        let some = hold_out(&catalog, "", 3, 7);
        assert_eq!(some, hold_out(&catalog, "", 3, 7));
        assert_eq!(some.len(), 3);
        assert_eq!(hold_out(&catalog, "Holding", 100, 1).len(), 4);

        // With no summaries at all, cards are looked for by their titles and tags.
        let mut bare = catalog.clone();
        for entry in &mut bare.entries {
            entry.summary = None;
        }
        bare.entries[0].tags = vec!["birds".into()];
        let titled = hold_out(&bare, "", 100, 7);
        assert_eq!(titled.len(), 9, "still no restricted item");
        assert!(
            titled
                .iter()
                .any(|h| h.id == bare.entries[0].item.id && h.summary.ends_with("(birds)")),
            "{titled:?}"
        );
    }

    #[test]
    fn a_tree_that_puts_things_where_theyd_be_looked_for_is_found_more_often() {
        let catalog = catalog();
        let current = Tree::current(&Graph::build(&catalog));
        let proposed = proposed(&current);
        let cards = hold_out(&catalog, "", DEFAULT_SAMPLE, 42);
        let comparison = compare(&registry(false), &current, &proposed, &cards, 42).unwrap();
        assert_eq!((comparison.current.found, comparison.current.tried), (4, 8));
        assert_eq!(
            (comparison.proposed.found, comparison.proposed.tried),
            (8, 8)
        );
        assert!(comparison.proposed_is_better());
        assert!((comparison.gain() - 0.5).abs() < 1e-9);
        assert_eq!(comparison.model.as_deref(), Some("scripted-1"));
        let bird = comparison
            .proposed
            .walks
            .iter()
            .find(|w| w.id == id(1))
            .unwrap();
        assert_eq!(bird.path, ["Reading", "Reading/Birds"]);
    }

    #[test]
    fn the_model_sees_names_and_descriptions_but_never_other_cards() {
        let catalog = catalog();
        let current = Tree::current(&Graph::build(&catalog));
        let tree = proposed(&current);
        let model = Scripted::new(false);
        let mut registry = Registry::new();
        let card = HeldOut {
            id: id(1),
            summary: "Reading notes on birds".into(),
        };
        registry.add(Box::new(model));
        let (walk, _) = walk_tree(&registry, &tree, &card).unwrap();
        assert!(walk.found);
        let text = prompt(&card.summary, "Reading", &tree.children("Reading"));
        assert!(text.contains("- Birds: Notes on birds"));
        assert!(text.contains("- here: it belongs in Reading itself"));
        assert!(!text.contains("stars, number"));
    }

    #[test]
    fn an_answer_that_isnt_a_choice_loses_the_walk() {
        let catalog = catalog();
        let current = Tree::current(&Graph::build(&catalog));
        let cards = hold_out(&catalog, "", 2, 1);
        let comparison = compare(&registry(true), &current, &current, &cards, 1).unwrap();
        assert_eq!(comparison.current.found, 0);
        assert!(comparison.current.walks.iter().all(|w| w.lost));
        assert!(!comparison.proposed_is_better(), "the same tree is no gain");
    }

    #[test]
    fn without_a_text_model_the_test_says_so() {
        let catalog = catalog();
        let current = Tree::current(&Graph::build(&catalog));
        let cards = hold_out(&catalog, "", 1, 1);
        assert!(compare(&Registry::new(), &current, &current, &cards, 1).is_err());
    }

    #[test]
    fn the_sample_archive_walks_its_own_tree() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive");
        let archive = Archive::open(&root).unwrap();
        let catalog = Catalog::read(&archive).unwrap();
        let tree = Tree::current(&Graph::build(&catalog));
        assert!(tree.children("").iter().any(|(n, _)| *n == "Projects"));
        assert!(
            tree.children("Projects")
                .iter()
                .any(|(n, _)| *n == "Hardware")
        );
        let cards = hold_out(&catalog, "", 5, 3);
        let comparison = compare(&registry(false), &tree, &tree, &cards, 3).unwrap();
        assert_eq!(comparison.current, comparison.proposed, "the same walks");
    }
}
