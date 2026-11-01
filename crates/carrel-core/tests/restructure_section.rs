//! Whole-section restructure proposals (`docs/design/restructure.md`): built from a section's
//! groups and judged by the eagerness that applies there, approved all or in part as one
//! change with its moves, and undone as a unit, against temporary archives with fake
//! embeddings and a scripted text model.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Mutex;

use carrel_core::archive::{Archive, InitOptions};
use carrel_core::card::Id;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
use carrel_core::history::{History, Operation};
use carrel_core::providers::{
    Capability, Completion, CompletionRequest, Offer, Provider, ProviderError, Registry,
};
use carrel_core::restructure::eagerness::Eagerness;
use carrel_core::restructure::section::{SectionOptions, propose_section};
use carrel_core::restructure::{self, Edit, Invasiveness, Proposal, ProposalKind};
use serde_json::{Value, json};

const BIRDS: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
const STARS: [f32; 4] = [0.0, 1.0, 0.0, 0.0];
const GARDEN: [f32; 4] = [0.0, 0.0, 1.0, 0.0];

/// What kind of item a test card is.
#[derive(Clone, Copy, PartialEq)]
enum Shape {
    /// A file beside its card.
    File,
    /// A hard-sealed bundle: a folder with files inside, moved whole and never entered.
    Bundle,
    /// A file that's checked out.
    CheckedOut,
}

struct Item {
    collection: &'static str,
    name: String,
    tags: &'static [&'static str],
    vector: [f32; 4],
    shape: Shape,
    summary: String,
}

fn item(collection: &'static str, name: &str, tags: &'static [&'static str], v: [f32; 4]) -> Item {
    Item {
        collection,
        name: name.to_owned(),
        tags,
        vector: v,
        shape: Shape::File,
        summary: format!("A note called {name}"),
    }
}

/// Reading holds six bird notes (one a sealed bundle) and six star notes (one checked out);
/// Notes holds two garden notes that belong with Projects's four.
fn items() -> Vec<Item> {
    let mut items = Vec::new();
    for n in 1..=6 {
        let mut bird = item("Reading", &format!("bird-{n}"), &["birds"], BIRDS);
        if n == 6 {
            bird.shape = Shape::Bundle;
        }
        items.push(bird);
        let mut star = item("Reading", &format!("star-{n}"), &["stars"], STARS);
        if n == 6 {
            star.shape = Shape::CheckedOut;
        }
        items.push(star);
    }
    for n in 1..=4 {
        items.push(item("Projects", &format!("plot-{n}"), &["garden"], GARDEN));
    }
    for n in 1..=2 {
        items.push(item("Notes", &format!("seed-{n}"), &["garden"], GARDEN));
    }
    items
}

struct Setup {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    holder: Holder,
    ids: BTreeMap<String, Id>,
    vectors: HashMap<Id, Vec<f32>>,
}

impl Setup {
    fn archive(&self) -> Archive {
        Archive::open(&self.root).unwrap()
    }

    fn propose(&self) -> Proposal {
        self.propose_with(None, "")
    }

    fn propose_with(&self, registry: Option<&Registry>, section: &str) -> Proposal {
        let archive = self.archive();
        let options = SectionOptions::for_archive(&archive);
        propose_section(&archive, section, &self.vectors, registry, &options).unwrap()
    }

    fn set_eagerness(&self, settings: Value) {
        let path = self.root.join("_System/settings.json");
        let mut json: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
            .unwrap_or_else(|_| json!({}));
        json["restructure"] = settings;
        std::fs::write(&path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
    }
}

fn write_collection(root: &Path, path: &str, extra: &str) {
    std::fs::create_dir_all(root.join(path)).unwrap();
    std::fs::write(
        root.join(path).join("_collection.md"),
        format!(
            "---\ncarrel: collection\nformat: \"1.0\"\nid: {}\ntitle: \"{path}\"\n{extra}---\n\n# {path}\n\n## Organization\n\nThe owner's words.\n",
            Id::new_v7()
        ),
    )
    .unwrap();
}

const CHECKSUM: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

fn card_text(item: &Item, id: Id, links: &str) -> String {
    let tags: Vec<String> = item.tags.iter().map(|t| format!("\"{t}\"")).collect();
    let (kind, extra) = match item.shape {
        Shape::File => ("file", format!("checksum: \"{CHECKSUM}\"\n")),
        Shape::Bundle => (
            "bundle",
            format!(
                "sealed: hard\nmanifest:\n  checksum: \"{CHECKSUM}\"\n  path: \"_System/manifests/{id}.tsv\"\n"
            ),
        ),
        Shape::CheckedOut => (
            "file",
            format!(
                "checksum: \"{CHECKSUM}\"\nchecked_out:\n  to: \"~/Desktop/{}\"\n  device: {}\n  person: {}\n  since: \"2026-10-29T09:12:00-07:00\"\n",
                item.name,
                Id::new_v7(),
                Id::new_v7()
            ),
        ),
    };
    let item_name = match item.shape {
        Shape::Bundle => item.name.clone(),
        _ => format!("{}.txt", item.name),
    };
    format!(
        "---\ncarrel: card\nformat: \"1.0\"\nid: {id}\nkind: {kind}\ntitle: \"{}\"\n\
         item: \"{item_name}\"\nsummary: \"{}\"\ntags: [{}]\nsensitivity: personal\nsize: 5\n\
         {extra}{links}generated:\n  fields: [title, tags]\n  by: \"carrel test\"\n  at: \"2026-11-01\"\n---\n\n\
         The owner's notes on {}.\n",
        item.name,
        item.summary,
        tags.join(", "),
        item.name
    )
}

fn setup_with(items: &[Item], collections: &[(&str, &str)]) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("archive");
    Archive::init(&root, &InitOptions::new("Restructure")).unwrap();
    for (path, extra) in collections {
        write_collection(&root, path, extra);
    }
    let mut ids = BTreeMap::new();
    let mut vectors = HashMap::new();
    for item in items {
        ids.insert(item.name.clone(), Id::new_v7());
    }
    for item in items {
        let id = ids[&item.name];
        let folder = root.join(item.collection);
        match item.shape {
            Shape::Bundle => {
                let bundle = folder.join(&item.name);
                std::fs::create_dir_all(bundle.join(".git")).unwrap();
                std::fs::write(bundle.join("notes.txt"), "inside\n").unwrap();
                std::fs::write(bundle.join(".git/HEAD"), "ref\n").unwrap();
            }
            _ => std::fs::write(folder.join(format!("{}.txt", item.name)), "words").unwrap(),
        }
        // plot-1 links to bird-1, so its link has to follow when bird-1 moves.
        let links = if item.name == "plot-1" && ids.contains_key("bird-1") {
            format!(
                "links:\n  - type: related\n    id: {}\n    path: \"../Reading/bird-1.txt.md\"\n",
                ids["bird-1"]
            )
        } else {
            String::new()
        };
        let card = match item.shape {
            Shape::Bundle => format!("{}.md", item.name),
            _ => format!("{}.txt.md", item.name),
        };
        std::fs::write(folder.join(card), card_text(item, id, &links)).unwrap();
        vectors.insert(id, item.vector.to_vec());
    }
    let archive = Archive::open(&root).unwrap();
    carrel_core::listing::refresh(&archive).unwrap();
    let holder = Holder {
        person: Id::new_v7(),
        device: Id::new_v7(),
    };
    let lease = DiskLock::for_archive(&archive)
        .acquire(holder, "test", DEFAULT_LEASE)
        .unwrap();
    History::init(&archive, &lease).unwrap();
    lease.release().unwrap();
    Setup {
        _dir: dir,
        root,
        holder,
        ids,
        vectors,
    }
}

fn setup() -> Setup {
    setup_with(
        &items(),
        &[("Reading", ""), ("Projects", ""), ("Notes", "")],
    )
}

/// Every file and folder under the archive outside `_System`, with each file's contents.
fn tree(root: &Path) -> BTreeMap<String, Option<String>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if relative.starts_with("_System") {
                continue;
            }
            if path.is_dir() {
                out.insert(relative, None);
                stack.push(path);
            } else {
                out.insert(relative, Some(std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    out
}

fn change_of(proposal: &Proposal, wanted: impl Fn(&Edit) -> bool) -> &str {
    &proposal
        .changes
        .iter()
        .find(|c| wanted(&c.change))
        .unwrap_or_else(|| panic!("no such change in {:#?}", proposal.changes))
        .id
}

fn creates(proposal: &Proposal, path: &str) -> String {
    change_of(
        proposal,
        |e| matches!(e, Edit::CreateCollection { path: p, .. } if p == path),
    )
    .to_owned()
}

fn moves_to(proposal: &Proposal, to: &str) -> String {
    change_of(
        proposal,
        |e| matches!(e, Edit::MoveItems { to: t } if t == to),
    )
    .to_owned()
}

#[test]
fn it_proposes_subdividing_and_merging_with_reasons_cost_benefit_and_eagerness() {
    let setup = setup();
    let proposal = setup.propose();
    assert_eq!(proposal.kind, ProposalKind::Section);
    assert_eq!(proposal.section, "");
    let assessment = proposal.assessment.as_ref().unwrap();
    assert_eq!(assessment.eagerness, Eagerness::Balanced);
    assert!(assessment.untested.is_some(), "no model, so untested");
    assert!(assessment.benefit > assessment.cost);
    assert!(
        proposal.rationale.contains("Untested"),
        "{}",
        proposal.rationale
    );

    // Reading is subdivided: the birds and the stars each get a subcollection named for
    // their tag, and Reading's design document names them.
    let birds = moves_to(&proposal, "Reading/Birds");
    let move_birds = proposal.change(&birds).unwrap();
    assert_eq!(move_birds.invasiveness, Invasiveness::Subdividing);
    assert_eq!(move_birds.items.len(), 6, "the sealed bundle moves whole");
    assert_eq!(move_birds.depends_on, [creates(&proposal, "Reading/Birds")]);
    assert_ne!(move_birds.reason, "");
    let stars = proposal
        .change(&moves_to(&proposal, "Reading/Stars"))
        .unwrap();
    assert_eq!(stars.items.len(), 5, "the checked-out star stays");
    assert!(stars.items.iter().all(|t| t.id != setup.ids["star-6"]));
    let rewrite = change_of(&proposal, |e| matches!(e, Edit::RewriteDesign { .. }));
    let rewrite = proposal.change(rewrite).unwrap();
    assert_eq!(rewrite.invasiveness, Invasiveness::Rewriting);
    assert_eq!(rewrite.depends_on.len(), 2);

    // Notes is too small to stand alone and belongs with Projects.
    let merge = change_of(
        &proposal,
        |e| matches!(e, Edit::MergeCollection { from, into } if from == "Notes" && into == "Projects"),
    );
    assert_eq!(
        proposal.change(merge).unwrap().invasiveness,
        Invasiveness::TopLevel
    );
    assert_eq!(proposal.invasiveness, Invasiveness::TopLevel);

    // It round-trips through the queue as the file FORMAT.md describes.
    let archive = setup.archive();
    restructure::queue(&archive, &proposal).unwrap();
    let file = std::fs::read_to_string(
        setup
            .root
            .join(format!("_System/queue/proposals/{}.json", proposal.id)),
    )
    .unwrap();
    let json: Value = serde_json::from_str(&file).unwrap();
    assert_eq!(json["kind"], "section");
    assert_eq!(json["assessment"]["eagerness"], "balanced");
    assert_eq!(json["changes"][0]["change"]["kind"], "create-collection");
    let back = restructure::find_any(&archive, None).unwrap();
    assert_eq!(back, proposal);
}

#[test]
fn approving_it_all_moves_everything_in_one_change_and_undo_puts_it_back() {
    let setup = setup();
    let before = tree(&setup.root);
    let proposal = setup.propose();
    let archive = setup.archive();
    let approved = restructure::approve(&archive, &proposal, None, setup.holder).unwrap();
    let commit = approved.commit.clone().unwrap();
    assert_eq!(approved.selection.dropped.len(), 0);

    let root = &setup.root;
    // The bundle moved whole, untouched inside.
    assert_eq!(
        std::fs::read_to_string(root.join("Reading/Birds/bird-6/notes.txt")).unwrap(),
        "inside\n"
    );
    assert!(root.join("Reading/Birds/bird-6/.git/HEAD").is_file());
    assert!(root.join("Reading/Birds/bird-1.txt").is_file());
    assert!(root.join("Reading/Stars/star-1.txt.md").is_file());
    // The checked-out item stayed where it was.
    assert!(root.join("Reading/star-6.txt.md").is_file());
    // Notes merged into Projects, and its folder went with it.
    assert!(root.join("Projects/seed-1.txt").is_file());
    assert!(!root.join("Notes").exists());
    // The link to bird-1 follows it.
    let plot = std::fs::read_to_string(root.join("Projects/plot-1.txt.md")).unwrap();
    assert!(plot.contains("../Reading/Birds/bird-1.txt.md"), "{plot}");
    // The owner's notes and fields are as they were.
    let bird = std::fs::read_to_string(root.join("Reading/Birds/bird-2.txt.md")).unwrap();
    assert!(bird.contains("The owner's notes on bird-2."));
    let design = std::fs::read_to_string(root.join("Reading/_collection.md")).unwrap();
    assert!(design.contains("- **Birds**"), "{design}");
    assert!(design.contains("The owner's words."));

    // One change in history, with every move.
    let history = History::open(&archive).unwrap();
    let log = history.log().unwrap();
    assert_eq!(log[0].id, commit);
    assert_eq!(log[0].operation, Some(Operation::Restructure));
    assert_eq!(log[0].moves.len(), 6 + 5 + 2);
    assert!(log[0].summary.starts_with("Restructure the archive:"));
    assert_eq!(restructure::waiting(&archive, None).unwrap().len(), 0);

    // Undo puts every item, folder, design document and link back.
    let lease = DiskLock::for_archive(&archive)
        .acquire(setup.holder, "test", DEFAULT_LEASE)
        .unwrap();
    history.undo(&archive, &commit, &lease).unwrap();
    lease.release().unwrap();
    let after = tree(&setup.root);
    let missing: Vec<&String> = before.keys().filter(|k| !after.contains_key(*k)).collect();
    let extra: Vec<&String> = after.keys().filter(|k| !before.contains_key(*k)).collect();
    assert!(missing.is_empty(), "missing after undo: {missing:?}");
    assert!(extra.is_empty(), "left behind by undo: {extra:?}");
    for (path, contents) in &before {
        assert_eq!(&after[path], contents, "{path} differs after undo");
    }
}

#[test]
fn approving_part_drops_what_depends_on_the_rest_and_says_so() {
    let setup = setup();
    let proposal = setup.propose();
    let birds_create = creates(&proposal, "Reading/Birds");
    let birds_move = moves_to(&proposal, "Reading/Birds");
    let stars_create = creates(&proposal, "Reading/Stars");
    let stars_move = moves_to(&proposal, "Reading/Stars");
    let rewrite = change_of(&proposal, |e| matches!(e, Edit::RewriteDesign { .. })).to_owned();

    // The birds' move without its collection, and the rewrite without the stars' collection.
    let only = vec![
        birds_move.clone(),
        stars_create.clone(),
        stars_move.clone(),
        rewrite.clone(),
    ];
    let selection = restructure::select(&proposal, Some(&only)).unwrap();
    assert_eq!(
        selection.approved,
        [stars_create.clone(), stars_move.clone()]
    );
    assert!(selection.rejected.contains(&birds_create));
    assert!(
        selection
            .dropped
            .contains(&(birds_move.clone(), birds_create.clone()))
    );
    assert!(
        selection
            .dropped
            .iter()
            .any(|(id, missing)| *id == rewrite && *missing == birds_create)
    );

    let archive = setup.archive();
    let approved = restructure::approve(&archive, &proposal, Some(&only), setup.holder).unwrap();
    assert!(approved.summary.contains("2 of"), "{}", approved.summary);
    assert!(setup.root.join("Reading/Stars/star-1.txt").is_file());
    assert!(
        setup.root.join("Reading/bird-1.txt").is_file(),
        "birds stay"
    );
    assert!(!setup.root.join("Reading/Birds").exists());
    assert!(
        setup.root.join("Notes/seed-1.txt").is_file(),
        "the merge was left out"
    );
    let body = History::open(&archive).unwrap().log().unwrap()[0].clone();
    assert_eq!(body.moves.len(), 5);
}

#[test]
fn an_item_checked_out_after_the_proposal_is_left_alone_and_reported() {
    let setup = setup();
    let proposal = setup.propose();
    let card = setup.root.join("Reading/bird-2.txt.md");
    let text = std::fs::read_to_string(&card).unwrap().replacen(
        "generated:",
        &format!(
            "checked_out:\n  to: \"~/Desktop/bird-2\"\n  device: {}\n  person: {}\n  since: \"2026-11-01T09:00:00-07:00\"\ngenerated:",
            Id::new_v7(),
            Id::new_v7()
        ),
        1,
    );
    std::fs::write(&card, text).unwrap();
    // A sealed bundle can't be merged away either: a collection holding a checked-out item
    // stays where it is.
    let archive = setup.archive();
    let birds_create = creates(&proposal, "Reading/Birds");
    let birds_move = moves_to(&proposal, "Reading/Birds");
    let only = [birds_create, birds_move.clone()];
    let approved = restructure::approve(&archive, &proposal, Some(&only), setup.holder).unwrap();
    let outcome = approved
        .outcomes
        .iter()
        .find(|o| o.change == birds_move)
        .unwrap();
    assert_eq!(outcome.items.len(), 5);
    assert_eq!(outcome.skipped.len(), 1);
    assert_eq!(outcome.skipped[0].0, setup.ids["bird-2"]);
    assert!(outcome.skipped[0].1.contains("checked out"));
    assert!(card.is_file(), "it stayed where it was");
}

#[test]
fn eagerness_presets_scale_what_each_kind_of_change_needs() {
    let merges = |p: &Proposal| {
        p.changes
            .iter()
            .any(|c| matches!(c.change, Edit::MergeCollection { .. }))
    };
    let subdivides = |p: &Proposal| {
        p.changes
            .iter()
            .any(|c| matches!(&c.change, Edit::MoveItems { to } if to == "Reading/Birds"))
    };
    let setup = setup();

    // Settled still subdivides for a modest gain, but won't merge top-level collections.
    setup.set_eagerness(json!({"eagerness": "settled"}));
    let settled = setup.propose();
    assert!(subdivides(&settled));
    assert!(!merges(&settled));
    let held = &settled.assessment.as_ref().unwrap().held_back;
    assert!(
        held.iter()
            .any(|h| h.what.contains("merging Notes") && h.why.contains("isn't the")),
        "{held:#?}"
    );

    // Balanced and Eager do both.
    setup.set_eagerness(json!({"eagerness": "balanced"}));
    assert!(merges(&setup.propose()));
    setup.set_eagerness(json!({"eagerness": "eager"}));
    let eager = setup.propose();
    assert!(merges(&eager) && subdivides(&eager));

    // The numbers behind them are settings too: a margin set directly wins over the preset.
    setup.set_eagerness(json!({"eagerness": "eager", "margin": 0.6}));
    let strict = setup.propose();
    assert!(!merges(&strict));
    assert!(
        subdivides(&strict),
        "subdividing needs only half the margin"
    );

    // The move budget caps moves between branches and top-level collections only.
    setup.set_eagerness(json!({"eagerness": "eager", "move_budget": 1}));
    let budgeted = setup.propose();
    assert!(!merges(&budgeted));
    assert!(subdivides(&budgeted));
    assert!(
        budgeted.assessment.as_ref().unwrap().held_back[0]
            .why
            .contains("budget")
    );
}

#[test]
fn a_collection_can_set_its_own_eagerness_in_its_design_document() {
    let setup = setup_with(
        &items(),
        &[
            ("Reading", ""),
            ("Projects", "restructure:\n  eagerness: settled\n"),
            ("Notes", ""),
        ],
    );
    setup.set_eagerness(json!({"eagerness": "eager"}));
    let proposal = setup.propose();
    assert!(
        !proposal
            .changes
            .iter()
            .any(|c| matches!(c.change, Edit::MergeCollection { .. })),
        "Projects is settled, so Notes isn't merged into it"
    );
    assert!(
        proposal
            .changes
            .iter()
            .any(|c| matches!(&c.change, Edit::MoveItems { to } if to == "Reading/Birds"))
    );
    // Proposing for Projects alone reports where its eagerness comes from.
    let projects = setup.propose_with(None, "Projects");
    let assessment = projects.assessment.unwrap();
    assert_eq!(assessment.eagerness, Eagerness::Settled);
    assert_eq!(
        assessment.eagerness_from,
        carrel_core::restructure::eagerness::Source::Collection("Projects".into())
    );
}

#[test]
fn a_section_must_be_a_collection_and_a_proposal_covers_only_it() {
    let setup = setup();
    let archive = setup.archive();
    let options = SectionOptions::for_archive(&archive);
    let err = propose_section(&archive, "Nowhere", &setup.vectors, None, &options).unwrap_err();
    assert!(err.to_string().contains("no collection Nowhere"));
    let reading = setup.propose_with(None, "reading");
    assert_eq!(reading.section, "Reading");
    assert!(
        !reading
            .changes
            .iter()
            .any(|c| matches!(c.change, Edit::MergeCollection { .. }))
    );
    assert_ne!(reading.changes.len(), 0);
}

/// A scripted text model: it names groups after their first tag when asked for wording, and
/// walks a tree by choosing the collection whose name the summary mentions, or, when none
/// does, an answer that isn't a choice, which loses the walk. If `lost`, every walk is lost.
struct Walker {
    offers: Vec<Offer>,
    lost: bool,
    asked: Mutex<Vec<String>>,
}

impl Walker {
    fn registry(lost: bool) -> Registry {
        let mut registry = Registry::new();
        registry.add(Box::new(Self {
            offers: vec![Offer {
                capability: Capability::TextModel,
                id: "walker".into(),
                label: None,
                priority: 10,
                available: true,
                reason: None,
                media_types: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            lost,
            asked: Mutex::new(Vec::new()),
        }));
        registry
    }
}

impl Provider for Walker {
    fn name(&self) -> &'static str {
        "walker"
    }

    fn offers(&self) -> &[Offer] {
        &self.offers
    }

    fn complete(
        &self,
        _offer: &str,
        request: &CompletionRequest,
    ) -> Result<Completion, ProviderError> {
        self.asked.lock().unwrap().push(request.prompt.clone());
        let schema = request.schema.as_ref().unwrap();
        let json = if schema["properties"].get("groups").is_some() {
            json!({
                "groups": [{"group": 0, "name": "Field birds", "description": "Notes on birds seen in the field", "reason": "They're all bird notes."}],
                "renames": [{"from": "Nowhere", "to": "Somewhere", "reason": "Not a collection, so dropped."}],
                "rationale": "Birds get a home of their own."
            })
        } else {
            walk_choice(request, self.lost)
        };
        Ok(Completion {
            text: json.to_string(),
            json: Some(json),
            model: "walker-1".into(),
            usage: None,
        })
    }
}

/// A findability walk's answer: the collection the card's description names, else `here`;
/// or, `lost`, an answer that isn't one of the choices.
fn walk_choice(request: &CompletionRequest, lost: bool) -> Value {
    let schema = request.schema.as_ref().unwrap();
    let summary = request
        .prompt
        .lines()
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let choices: Vec<String> = schema["properties"]["choice"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_owned())
        .collect();
    let choice = choices
        .iter()
        .find(|c| !lost && c.as_str() != "here" && summary.contains(&c.to_lowercase()))
        .cloned()
        .unwrap_or_else(|| {
            if !lost && choices.iter().any(|c| c == "here") {
                "here".into()
            } else {
                "nowhere at all".into()
            }
        });
    json!({"choice": choice})
}

/// Six bird notes waiting in Holding beside a star note, which the walker can only find under a collection whose
/// name their summaries mention.
fn holding_birds() -> Setup {
    let mut birds: Vec<Item> = (1..=6)
        .map(|n| Item {
            summary: format!("Field birds seen on walk {n}"),
            ..item("Holding", &format!("bird-{n}"), &["birds"], BIRDS)
        })
        .collect();
    // Holding holds something else too, so the birds aren't all it holds already.
    birds.push(item("Holding", "star-1", &["stars"], STARS));
    setup_with(&birds, &[])
}

#[test]
fn a_model_names_the_groups_and_the_findability_test_lets_a_better_shape_through() {
    let setup = holding_birds();
    let registry = Walker::registry(false);
    let proposal = setup.propose_with(Some(&registry), "");
    let assessment = proposal.assessment.as_ref().unwrap();
    assert!(assessment.worded_by_model);
    let findability = assessment.findability.as_ref().unwrap();
    assert_eq!(findability.tried, 7);
    assert_eq!(findability.found_now, 0);
    assert_eq!(findability.found_after, 6);
    assert_eq!(findability.model.as_deref(), Some("walker-1"));
    assert_eq!(proposal.rationale, "Birds get a home of their own.");
    // The model's name is used, and its rename of a collection that isn't there is dropped.
    moves_to(&proposal, "Field birds");
    assert!(
        !proposal
            .changes
            .iter()
            .any(|c| matches!(c.change, Edit::RenameCollection { .. }))
    );
}

#[test]
fn even_eager_wont_propose_a_shape_that_isnt_easier_to_navigate() {
    let setup = holding_birds();
    setup.set_eagerness(json!({"eagerness": "eager"}));
    let registry = Walker::registry(true);
    let proposal = setup.propose_with(Some(&registry), "");
    assert!(proposal.changes.is_empty(), "{:#?}", proposal.changes);
    let assessment = proposal.assessment.unwrap();
    assert!(!assessment.findability.unwrap().is_better());
    assert!(
        assessment.held_back[0]
            .why
            .contains("wasn't easier to find things in")
    );
    assert!(
        proposal.rationale.contains("held back"),
        "{}",
        proposal.rationale
    );
}

/// Backups that record each snapshot they're asked for.
#[derive(Default)]
struct Snapshots {
    taken: Mutex<Vec<String>>,
}

impl carrel_core::backup::Backups for Snapshots {
    fn describe(&self) -> String {
        "test".into()
    }
    fn snapshot(
        &self,
        _root: &Path,
        description: &str,
    ) -> Result<carrel_core::backup::Snapshot, carrel_core::backup::BackupError> {
        self.taken.lock().unwrap().push(description.to_owned());
        Ok(carrel_core::backup::Snapshot {
            id: "k0001".into(),
            time: "2026-11-01T12:00:00Z".into(),
            description: Some(description.to_owned()),
            pinned: true,
            size: None,
            files: None,
        })
    }
    fn list(
        &self,
        _root: &Path,
    ) -> Result<Vec<carrel_core::backup::Snapshot>, carrel_core::backup::BackupError> {
        Ok(Vec::new())
    }
    fn verify(&self, _percent: u8) -> Result<String, carrel_core::backup::BackupError> {
        Ok(String::new())
    }
    fn restore(
        &self,
        _snapshot: &str,
        _path: &str,
        _to: &Path,
    ) -> Result<(), carrel_core::backup::BackupError> {
        Ok(())
    }
    fn forget(&self, _snapshot: &str) -> Result<(), carrel_core::backup::BackupError> {
        Ok(())
    }
}

#[test]
fn renaming_a_collection_keeps_its_id_takes_one_snapshot_and_undoes_as_one() {
    let setup = setup();
    let archive = setup.archive();
    let before = tree(&setup.root);
    let id = archive
        .collections()
        .unwrap()
        .into_iter()
        .find(|c| c.path == "Projects")
        .unwrap()
        .doc
        .id;
    let mut proposal = Proposal::new(ProposalKind::Section, "");
    proposal.invasiveness = Invasiveness::Renaming;
    proposal.rationale = "Clearer names.".into();
    proposal.changes = vec![
        serde_json::from_value(json!({
            "id": "1",
            "change": {"kind": "rename-collection", "from": "Projects", "to": "Garden"},
            "reason": "It's all the garden now.",
            "invasiveness": "renaming",
        }))
        .unwrap(),
        serde_json::from_value(json!({
            "id": "2",
            "change": {"kind": "rewrite-design", "collection": "Garden", "description": "Plots and plans"},
            "reason": "Says what it holds.",
            "invasiveness": "rewriting",
            "depends_on": ["1"],
        }))
        .unwrap(),
        // Reading holds a checked-out item, so it stays as it is.
        serde_json::from_value(json!({
            "id": "3",
            "change": {"kind": "rename-collection", "from": "Reading", "to": "Library"},
            "reason": "It holds more than reading now.",
            "invasiveness": "renaming",
        }))
        .unwrap(),
    ];
    let backups = Snapshots::default();
    let approved =
        restructure::approve_with_snapshot(&archive, &proposal, None, setup.holder, Some(&backups))
            .unwrap();
    assert_eq!(approved.snapshot.as_deref(), Some("kopia:k0001"));
    assert_eq!(backups.taken.lock().unwrap().len(), 1);
    let refused = approved.outcomes[2].not_applied.as_deref().unwrap();
    assert!(refused.contains("checked-out"), "{refused}");
    assert!(setup.root.join("Reading/star-6.txt.md").is_file());
    let archive = setup.archive();
    let garden = archive
        .collections()
        .unwrap()
        .into_iter()
        .find(|c| c.path == "Garden")
        .unwrap();
    assert_eq!(garden.doc.id, id, "its permanent ID doesn't change");
    assert_eq!(garden.doc.title, "Garden");
    assert_eq!(garden.doc.description.as_deref(), Some("Plots and plans"));
    assert!(!setup.root.join("Projects").exists());
    assert!(setup.root.join("Garden/plot-2.txt").is_file());
    let archive_md = std::fs::read_to_string(setup.root.join("ARCHIVE.md")).unwrap();
    assert!(
        archive_md.contains("[Garden](Garden/_collection.md)"),
        "{archive_md}"
    );
    let history = History::open(&archive).unwrap();
    let entry = history.log().unwrap()[0].clone();
    assert_eq!(entry.snapshot.as_deref(), Some("kopia:k0001"));
    assert_eq!(entry.moves.len(), 1);
    assert_eq!(entry.moves[0].item, id);

    let lease = DiskLock::for_archive(&archive)
        .acquire(setup.holder, "test", DEFAULT_LEASE)
        .unwrap();
    history.undo(&archive, &entry.id, &lease).unwrap();
    lease.release().unwrap();
    assert_eq!(tree(&setup.root), before);
}

#[test]
fn rewriting_alone_takes_no_snapshot() {
    let setup = setup();
    let archive = setup.archive();
    let mut proposal = Proposal::new(ProposalKind::Section, "Reading");
    proposal.changes = vec![
        serde_json::from_value(json!({
            "id": "1",
            "change": {"kind": "rewrite-design", "collection": "Reading", "organization": "- By subject."},
            "reason": "Says how it's organized.",
            "invasiveness": "rewriting",
        }))
        .unwrap(),
    ];
    let backups = Snapshots::default();
    let approved =
        restructure::approve_with_snapshot(&archive, &proposal, None, setup.holder, Some(&backups))
            .unwrap();
    assert!(approved.snapshot.is_none());
    assert!(backups.taken.lock().unwrap().is_empty());
    let design = std::fs::read_to_string(setup.root.join("Reading/_collection.md")).unwrap();
    assert!(design.contains("- By subject."));
}

#[test]
fn a_collections_own_eagerness_reaches_its_subcollections_and_undo_takes_it_back() {
    use carrel_core::restructure::eagerness::{self, Source};
    let setup = setup_with(
        &items(),
        &[
            ("Reading", "restructure:\n  keep: this\n"),
            ("Reading/Birds", ""),
            ("Projects", ""),
            ("Notes", ""),
        ],
    );
    let archive = setup.archive();
    let design = setup.root.join("Reading/_collection.md");
    let before = std::fs::read_to_string(&design).unwrap();
    let lease = DiskLock::for_archive(&archive)
        .acquire(setup.holder, "test", DEFAULT_LEASE)
        .unwrap();

    let commit = eagerness::set_for_collection(&archive, "Reading", Some(Eagerness::Eager), &lease)
        .unwrap()
        .expect("a change");
    let archive = setup.archive();
    let collections = archive.collections().unwrap();
    // A subcollection takes the level of the nearest collection above it that sets one.
    assert_eq!(
        eagerness::at(&archive, &collections, "Reading/Birds"),
        (Eagerness::Eager, Source::Collection("Reading".into()))
    );
    assert_eq!(
        eagerness::at(&archive, &collections, "Projects"),
        (Eagerness::Balanced, Source::Archive)
    );
    assert_eq!(
        eagerness::thresholds_at(&archive, &collections, "Reading/Birds").move_budget,
        None,
        "eager has no move budget"
    );
    let written = std::fs::read_to_string(&design).unwrap();
    assert!(
        written.contains("keep: this"),
        "other restructure keys stay: {written}"
    );

    // The same level again changes nothing; an unknown collection is refused.
    assert_eq!(
        eagerness::set_for_collection(&archive, "reading", Some(Eagerness::Eager), &lease).unwrap(),
        None
    );
    assert!(
        eagerness::set_for_collection(&archive, "Nowhere", Some(Eagerness::Eager), &lease).is_err()
    );

    // Undo puts the design document back as it was.
    History::open(&archive)
        .unwrap()
        .undo(&archive, &commit, &lease)
        .unwrap();
    lease.release().unwrap();
    assert_eq!(std::fs::read_to_string(&design).unwrap(), before);
    let archive = setup.archive();
    assert_eq!(
        eagerness::at(&archive, &archive.collections().unwrap(), "Reading/Birds").1,
        Source::Archive
    );
}

#[test]
fn a_proposal_is_found_by_its_identifier_or_alone_and_a_section_replaces_its_own() {
    let setup = setup();
    let archive = setup.archive();
    let none = restructure::find_any(&archive, None).unwrap_err();
    assert!(
        none.to_string()
            .contains("no restructure proposal is waiting"),
        "{none}"
    );

    let mut reading = Proposal::new(ProposalKind::Section, "Reading");
    reading.rationale = "Tidy Reading.".into();
    let mut projects = Proposal::new(ProposalKind::Section, "Projects");
    projects.rationale = "Tidy Projects.".into();
    // Proposals change nothing until approved, so a proposal with no changes is enough here.
    assert_eq!(restructure::queue(&archive, &reading).unwrap(), 0);
    assert_eq!(
        restructure::find_any(&archive, None).unwrap().id,
        reading.id,
        "the only one"
    );
    assert_eq!(restructure::queue(&archive, &projects).unwrap(), 0);
    let several = restructure::find_any(&archive, None).unwrap_err();
    assert!(
        several
            .to_string()
            .contains("2 restructure proposals are waiting"),
        "{several}"
    );
    assert_eq!(
        restructure::find_any(&archive, Some(&projects.id.to_string()))
            .unwrap()
            .section,
        "Projects"
    );

    // A new proposal for Reading replaces the one waiting for it, and only that one.
    let mut again = Proposal::new(ProposalKind::Section, "Reading");
    again.rationale = "Tidy Reading again.".into();
    assert_eq!(restructure::queue(&archive, &again).unwrap(), 1);
    let sections: Vec<String> = restructure::waiting(&archive, None)
        .unwrap()
        .into_iter()
        .map(|p| p.rationale)
        .collect();
    assert_eq!(sections.len(), 2, "{sections:?}");
    assert!(sections.contains(&"Tidy Reading again.".to_owned()));
    assert!(sections.contains(&"Tidy Projects.".to_owned()));

    restructure::reject(&archive, &projects).unwrap();
    assert_eq!(restructure::find_any(&archive, None).unwrap().id, again.id);
}

#[test]
fn an_item_whose_card_cant_be_written_keeps_its_contents_with_it() {
    let setup = setup();
    let root = &setup.root;
    let id = setup.ids["bird-1"];
    // Writing the card's temporary file in Projects fails (as a full disk or a permission
    // error would): here, because something is already in its place.
    std::fs::create_dir(root.join(format!("Projects/.bird-1.txt.md.tmp-{id}"))).unwrap();
    let archive = setup.archive();
    let mut proposal = Proposal::new(ProposalKind::Section, "");
    proposal.changes = vec![
        serde_json::from_value(json!({
            "id": "1",
            "change": {"kind": "move-items", "to": "Projects"},
            "reason": "It belongs there.",
            "invasiveness": "between-branches",
            "items": [{"id": setup.ids["bird-1"], "card": "Reading/bird-1.txt.md", "title": "bird-1"}],
        }))
        .unwrap(),
    ];
    let approved = restructure::approve(&archive, &proposal, None, setup.holder).unwrap();
    let outcome = &approved.outcomes[0];
    let card_in_reading = root.join("Reading/bird-1.txt.md").is_file();
    let contents_in_reading = root.join("Reading/bird-1.txt").is_file();
    let recorded = outcome.moves.iter().any(|m| m.item == id);
    assert!(
        (card_in_reading && contents_in_reading) || recorded,
        "card in Reading: {card_in_reading}, contents in Reading: {contents_in_reading}, \
         contents in Projects: {}, move recorded for undo: {recorded}; outcome {outcome:#?}",
        root.join("Projects/bird-1.txt").is_file()
    );
}

/// A merge that fails part way leaves each item whole, on one side or the other, with every
/// item it did move recorded, so undo puts the merge back exactly (#377).
#[test]
fn a_merge_that_fails_part_way_leaves_each_item_whole_and_undoes_exactly() {
    let setup = setup();
    let root = &setup.root;
    let before = tree(root);
    let second = setup.ids["seed-2"];
    std::fs::create_dir(root.join(format!("Projects/.seed-2.txt.md.tmp-{second}"))).unwrap();
    let archive = setup.archive();
    let mut proposal = Proposal::new(ProposalKind::Section, "");
    proposal.changes = vec![
        serde_json::from_value(json!({
            "id": "1",
            "change": {"kind": "merge-collection", "from": "Notes", "into": "Projects"},
            "reason": "They're the same garden.",
            "invasiveness": "top-level",
        }))
        .unwrap(),
    ];
    let approved = restructure::approve(&archive, &proposal, None, setup.holder).unwrap();
    let outcome = &approved.outcomes[0];
    assert!(outcome.not_applied.is_some(), "{outcome:#?}");
    // The first seed moved, card and contents, and its move is recorded.
    assert!(root.join("Projects/seed-1.txt").is_file());
    assert!(root.join("Projects/seed-1.txt.md").is_file());
    assert!(outcome.moves.iter().any(|m| m.item == setup.ids["seed-1"]));
    // The second stayed whole in Notes.
    assert!(root.join("Notes/seed-2.txt").is_file());
    assert!(root.join("Notes/seed-2.txt.md").is_file());
    assert!(!root.join("Projects/seed-2.txt").exists());

    // Undo puts the first seed back.
    std::fs::remove_dir(root.join(format!("Projects/.seed-2.txt.md.tmp-{second}"))).unwrap();
    let lease = DiskLock::for_archive(&archive)
        .acquire(setup.holder, "test", DEFAULT_LEASE)
        .unwrap();
    History::open(&archive)
        .unwrap()
        .undo(&archive, &approved.commit.clone().unwrap(), &lease)
        .unwrap();
    lease.release().unwrap();
    let after = tree(root);
    let missing: Vec<&String> = before.keys().filter(|k| !after.contains_key(*k)).collect();
    let extra: Vec<&String> = after.keys().filter(|k| !before.contains_key(*k)).collect();
    assert!(missing.is_empty(), "missing after undo: {missing:?}");
    assert!(extra.is_empty(), "left behind by undo: {extra:?}");
    for (path, contents) in &before {
        assert_eq!(&after[path], contents, "{path} differs after undo");
    }
}

// Found reviewing #350: a section proposal stays in its section, never hides a collection,
// and approving it is safe when changes fail, the lease runs out or a rename changes case.

/// A text model that names no groups, suggests the renames it's given, and walks trees as
/// [`Walker`] does.
struct Renamer {
    offers: Vec<Offer>,
    renames: Value,
}

impl Renamer {
    fn registry(renames: Value) -> Registry {
        let mut registry = Registry::new();
        registry.add(Box::new(Self {
            offers: vec![Offer {
                capability: Capability::TextModel,
                id: "renamer".into(),
                label: None,
                priority: 10,
                available: true,
                reason: None,
                media_types: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            renames,
        }));
        registry
    }
}

impl Provider for Renamer {
    fn name(&self) -> &'static str {
        "renamer"
    }

    fn offers(&self) -> &[Offer] {
        &self.offers
    }

    fn complete(
        &self,
        _offer: &str,
        request: &CompletionRequest,
    ) -> Result<Completion, ProviderError> {
        let walking = request
            .schema
            .as_ref()
            .is_some_and(|s| s["properties"].get("choice").is_some());
        let json = if walking {
            walk_choice(request, false)
        } else {
            json!({"groups": [], "renames": self.renames, "rationale": "Tidier."})
        };
        Ok(Completion {
            text: json.to_string(),
            json: Some(json),
            model: "renamer-1".into(),
            usage: None,
        })
    }
}

/// `items()` without summaries, so the findability test looks for cards by their titles and
/// tags.
fn items_without_summaries() -> Vec<Item> {
    items()
        .into_iter()
        .map(|i| Item {
            summary: String::new(),
            ..i
        })
        .collect()
}

#[test]
fn a_section_proposal_only_renames_collections_in_its_section() {
    let setup = setup_with(
        &items_without_summaries(),
        &[("Reading", ""), ("Projects", ""), ("Notes", "")],
    );
    let registry = Renamer::registry(json!([
        {"from": "Projects", "to": "Garden plots", "reason": "It's all the garden."}
    ]));
    let proposal = setup.propose_with(Some(&registry), "Reading");
    assert_eq!(proposal.section, "Reading");
    let outside: Vec<&Edit> = proposal
        .changes
        .iter()
        .map(|c| &c.change)
        .filter(|e| matches!(e, Edit::RenameCollection { from, .. } if from == "Projects"))
        .collect();
    assert!(
        outside.is_empty(),
        "a proposal for Reading renames a collection outside it: {outside:?}"
    );
}

#[test]
fn a_rename_never_hides_a_collection_behind_a_dot() {
    let mut items = items_without_summaries();
    let mut lone = item("Misc", "lone", &["odd"], [0.0, 0.0, 0.0, 1.0]);
    lone.summary = String::new();
    items.push(lone);
    let setup = setup_with(
        &items,
        &[
            ("Reading", ""),
            ("Projects", ""),
            ("Notes", ""),
            ("Misc", ""),
        ],
    );
    let registry = Renamer::registry(json!([
        {"from": "Misc", "to": ".Misc", "reason": "Out of the way."}
    ]));
    let proposal = setup.propose_with(Some(&registry), "");
    assert!(
        !proposal.changes.iter().any(
            |c| matches!(&c.change, Edit::RenameCollection { to, .. } if to.contains(".Misc"))
        ),
        "the model's rename is dropped: {:#?}",
        proposal.changes
    );
    // Nor is one applied when a proposal asks for it.
    let mut by_hand = Proposal::new(ProposalKind::Section, "");
    by_hand.changes = vec![
        serde_json::from_value(json!({
            "id": "1",
            "change": {"kind": "rename-collection", "from": "Misc", "to": ".Misc"},
            "reason": "Out of the way.",
            "invasiveness": "renaming",
        }))
        .unwrap(),
    ];
    let archive = setup.archive();
    let approved = restructure::approve(&archive, &by_hand, None, setup.holder).unwrap();
    assert!(
        approved.outcomes[0].not_applied.is_some(),
        "{:?}",
        approved.outcomes
    );
    let archive = setup.archive();
    let ids: Vec<Id> = archive.items().unwrap().iter().map(|i| i.id).collect();
    assert!(
        ids.contains(&setup.ids["lone"]),
        "the item in Misc is no longer in the archive; outcomes: {:#?}",
        approved.outcomes
    );
}

#[test]
fn a_rename_that_only_changes_case_can_be_undone() {
    let setup = setup();
    let archive = setup.archive();
    let before = tree(&setup.root);
    let mut proposal = Proposal::new(ProposalKind::Section, "");
    proposal.changes = vec![
        serde_json::from_value(json!({
            "id": "1",
            "change": {"kind": "rename-collection", "from": "Projects", "to": "projects"},
            "reason": "Lower case.",
            "invasiveness": "renaming",
        }))
        .unwrap(),
    ];
    let approved = restructure::approve(&archive, &proposal, None, setup.holder).unwrap();
    assert!(
        approved.outcomes[0].not_applied.is_none(),
        "{:?}",
        approved.outcomes
    );
    let commit = approved.commit.unwrap();
    let history = History::open(&archive).unwrap();
    let lease = DiskLock::for_archive(&archive)
        .acquire(setup.holder, "test", DEFAULT_LEASE)
        .unwrap();
    let undone = history.undo(&archive, &commit, &lease);
    lease.release().unwrap();
    undone.unwrap();
    assert_eq!(tree(&setup.root), before);
}

#[test]
fn a_change_whose_dependency_wasnt_applied_doesnt_run() {
    let setup = setup();
    let proposal = setup.propose();
    // Since the proposal, something else named Birds turned up in Reading.
    std::fs::create_dir(setup.root.join("Reading/birds")).unwrap();
    std::fs::write(setup.root.join("Reading/birds/list.txt"), "owner's\n").unwrap();
    let archive = setup.archive();
    let approved = restructure::approve(&archive, &proposal, None, setup.holder).unwrap();
    let create = creates(&proposal, "Reading/Birds");
    let created = approved
        .outcomes
        .iter()
        .find(|o| o.change == create)
        .unwrap();
    assert!(created.not_applied.is_some(), "{created:?}");
    let design = std::fs::read_to_string(setup.root.join("Reading/_collection.md")).unwrap();
    assert!(
        !design.contains("- **Birds**"),
        "Reading's design document names a subcollection that was never made:\n{design}"
    );
}

/// Backups whose snapshot takes longer than the writer lease lasts.
struct SlowSnapshots;

impl carrel_core::backup::Backups for SlowSnapshots {
    fn describe(&self) -> String {
        "slow".into()
    }
    fn snapshot(
        &self,
        root: &Path,
        description: &str,
    ) -> Result<carrel_core::backup::Snapshot, carrel_core::backup::BackupError> {
        // As if the snapshot took six minutes: the lease's expiry is now in the past.
        let lock = root.join("_System/writer.lock");
        let mut info: Value =
            serde_json::from_str(&std::fs::read_to_string(&lock).unwrap()).unwrap();
        info["expires"] = json!("2000-01-01T00:00:00Z");
        std::fs::write(&lock, serde_json::to_string_pretty(&info).unwrap()).unwrap();
        Ok(carrel_core::backup::Snapshot {
            id: "k0002".into(),
            time: "2026-11-01T12:00:00Z".into(),
            description: Some(description.to_owned()),
            pinned: true,
            size: None,
            files: None,
        })
    }
    fn list(
        &self,
        _root: &Path,
    ) -> Result<Vec<carrel_core::backup::Snapshot>, carrel_core::backup::BackupError> {
        Ok(Vec::new())
    }
    fn verify(&self, _percent: u8) -> Result<String, carrel_core::backup::BackupError> {
        Ok(String::new())
    }
    fn restore(
        &self,
        _snapshot: &str,
        _path: &str,
        _to: &Path,
    ) -> Result<(), carrel_core::backup::BackupError> {
        Ok(())
    }
    fn forget(&self, _snapshot: &str) -> Result<(), carrel_core::backup::BackupError> {
        Ok(())
    }
}

#[test]
fn a_lease_that_runs_out_during_the_snapshot_changes_nothing() {
    let setup = setup();
    let before = tree(&setup.root);
    let proposal = setup.propose();
    let archive = setup.archive();
    restructure::queue(&archive, &proposal).unwrap();
    let result = restructure::approve_with_snapshot(
        &archive,
        &proposal,
        None,
        setup.holder,
        Some(&SlowSnapshots),
    );
    let waiting = restructure::waiting(&archive, None).unwrap().len();
    let after = tree(&setup.root);
    assert!(
        result.is_ok() || (after == before && waiting == 1),
        "approve failed with {:?}, after moving files ({} paths gone) and taking the proposal \
         off the queue ({waiting} waiting); nothing in history records it",
        result.err().map(|e| e.to_string()),
        before.keys().filter(|k| !after.contains_key(*k)).count()
    );
}

#[test]
fn renaming_one_top_level_collection_doesnt_make_the_whole_archive_recent() {
    let setup = setup();
    let archive = setup.archive();
    let mut proposal = Proposal::new(ProposalKind::Section, "Projects");
    proposal.changes = vec![
        serde_json::from_value(json!({
            "id": "1",
            "change": {"kind": "rename-collection", "from": "Projects", "to": "Garden"},
            "reason": "It's the garden.",
            "invasiveness": "renaming",
        }))
        .unwrap(),
    ];
    restructure::approve(&archive, &proposal, None, setup.holder).unwrap();
    let reading = setup.propose_with(None, "Reading");
    assert!(
        !reading.assessment.unwrap().recently_restructured,
        "Reading was never restructured, but counts as recently restructured"
    );
}
