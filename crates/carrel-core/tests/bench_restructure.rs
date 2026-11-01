//! The restructure benchmark (`docs/design/restructure.md`, "Measuring it"): where a
//! proposal's changes would leave each item, scoring the shape it recovers, the moves it takes
//! and its findability gain, on hand-made proposals and on one built from the muddled archive
//! with stub card vectors.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use carrel_core::bench::Answers;
use carrel_core::bench::crawl::{Judged, Misplaced, Muddle, build};
use carrel_core::bench::restructure::{
    after, at_eagerness, moves, score, shape, use_crawler_verdicts,
};
use carrel_core::card::{Extra, Id};
use carrel_core::restructure::eagerness::{Eagerness, Source};
use carrel_core::restructure::section::{
    Assessment, FindabilityResult, SectionOptions, propose_section,
};
use carrel_core::restructure::{
    Edit, Invasiveness, Proposal, ProposalKind, ProposedChange, Touched,
};

fn bench() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/bench")
}

fn id(n: u128) -> Id {
    Id(uuid::Uuid::from_u128(n))
}

fn change(n: usize, edit: Edit, invasiveness: Invasiveness, items: &[Id]) -> ProposedChange {
    ProposedChange {
        id: n.to_string(),
        change: edit,
        reason: String::new(),
        invasiveness,
        items: items
            .iter()
            .map(|i| Touched {
                id: *i,
                card: String::new(),
                title: String::new(),
            })
            .collect(),
        held: Vec::new(),
        documents: Vec::new(),
        depends_on: Vec::new(),
        extra: Extra::new(),
    }
}

#[test]
fn a_proposals_changes_carry_items_along_and_count_as_weighted_moves() {
    let before = BTreeMap::from([
        (id(1), "Home".to_owned()),
        (id(2), "Home".to_owned()),
        (id(3), "Manuals".to_owned()),
        (id(4), "Manuals/Kitchen".to_owned()),
        (id(5), "Work".to_owned()),
        (id(6), "Workshop".to_owned()),
    ]);
    let mut proposal = Proposal::new(ProposalKind::Section, "");
    proposal.changes = vec![
        change(
            1,
            Edit::CreateCollection {
                path: "Home/Cooking".into(),
                description: String::new(),
            },
            Invasiveness::Subdividing,
            &[],
        ),
        change(
            2,
            Edit::MoveItems {
                to: "Home/Cooking".into(),
            },
            Invasiveness::Subdividing,
            &[id(1)],
        ),
        change(
            3,
            Edit::MergeCollection {
                from: "Manuals".into(),
                into: "Home".into(),
            },
            Invasiveness::TopLevel,
            &[],
        ),
        change(
            4,
            Edit::RenameCollection {
                from: "Work".into(),
                to: "Job".into(),
            },
            Invasiveness::Renaming,
            &[],
        ),
    ];
    let places = after(&before, &proposal);
    assert_eq!(places[&id(1)], "Home/Cooking");
    assert_eq!(places[&id(2)], "Home");
    assert_eq!(places[&id(3)], "Home");
    assert_eq!(places[&id(4)], "Home/Kitchen", "subcollections come along");
    assert_eq!(places[&id(5)], "Job");
    assert_eq!(
        places[&id(6)],
        "Workshop",
        "a name that only starts the same isn't inside"
    );

    let counted = moves(&before, &proposal);
    assert_eq!(counted.moved, 4);
    // 1 subdividing (2), 2 merged top-level (5 each), 1 renamed (3).
    assert_eq!(counted.weighted, 2 + 10 + 3);
    assert_eq!(counted.by_invasiveness[&Invasiveness::TopLevel], 2);
}

#[test]
fn the_shape_is_scored_by_which_items_end_up_together_whatever_the_names() {
    let answers: Answers = serde_saphyr::from_str(
        r#"
items:
  "a": {destinations: [Recipes], sensitivity: personal, date: null}
  "b": {destinations: [Recipes], sensitivity: personal, date: null}
  "c": {destinations: [Home], sensitivity: personal, date: null}
  "d": {destinations: [Home], sensitivity: personal, date: null}
"#,
    )
    .unwrap();
    let places = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(f, p)| ((*f).to_owned(), (*p).to_owned()))
            .collect()
    };
    let renamed = shape(
        &places(&[
            ("a", "Cooking"),
            ("b", "Cooking"),
            ("c", "Home"),
            ("d", "Home"),
        ]),
        &answers,
    );
    assert!((renamed.f1 - 1.0).abs() < 1e-9, "{renamed:?}");
    let lumped = shape(
        &places(&[("a", "Home"), ("b", "Home"), ("c", "Home"), ("d", "Home")]),
        &answers,
    );
    // Six pairs together, two of which belong: precision 1/3, recall 1.
    assert!((lumped.precision - 1.0 / 3.0).abs() < 1e-9);
    assert!((lumped.recall - 1.0).abs() < 1e-9);
    assert!(lumped.f1 < renamed.f1);
}

/// The muddled archive's items, by identifier and file name, built in `dir`.
fn muddled(dir: &Path) -> (carrel_core::archive::Archive, BTreeMap<Id, String>) {
    let archive = build(
        &bench().join("archive"),
        &bench().join("items"),
        &Answers::load(&bench().join("answers.yaml")).unwrap(),
        &Muddle::load(&bench().join("muddle.yaml")).unwrap(),
        &carrel_core::bench::findability::Descriptions::load(&bench().join("descriptions.yaml"))
            .unwrap(),
        &dir.join("archive"),
    )
    .unwrap();
    let files = archive
        .items()
        .unwrap()
        .into_iter()
        .filter_map(|i| Some((i.id, i.item?.rsplit('/').next()?.to_owned())))
        .collect();
    (archive, files)
}

#[test]
fn a_proposal_that_puts_the_muddle_right_recovers_the_shape_and_counts_its_moves() {
    let dir = tempfile::tempdir().unwrap();
    let answers = Answers::load(&bench().join("answers.yaml")).unwrap();
    let muddle = Muddle::load(&bench().join("muddle.yaml")).unwrap();
    let (_archive, files) = muddled(dir.path());
    let places = muddle.places(&answers);
    let ids: BTreeMap<&str, Id> = files.iter().map(|(i, f)| (f.as_str(), *i)).collect();

    // Every scattered item back where it belongs, and the missing collection's items gathered
    // under a name of the model's own; the dumped manuals are left.
    let mut proposal = Proposal::new(ProposalKind::Section, "");
    let mut n = 0;
    for file in muddle.scattered.keys() {
        n += 1;
        let to = answers.items[file].destinations[0].clone();
        proposal.changes.push(change(
            n,
            Edit::MoveItems { to },
            Invasiveness::BetweenBranches,
            &[ids[file.as_str()]],
        ));
    }
    let recipes: Vec<Id> = places
        .iter()
        .filter(|(_, (_, kind))| *kind == Some(carrel_core::bench::crawl::Misplaced::Missing))
        .map(|(file, _)| ids[file.as_str()])
        .collect();
    proposal.changes.push(change(
        n + 1,
        Edit::MoveItems {
            to: "Home/Cooking".into(),
        },
        Invasiveness::Subdividing,
        &recipes,
    ));
    let mut assessment = Assessment {
        eagerness: Eagerness::Balanced,
        eagerness_from: Source::Archive,
        thresholds: Eagerness::Balanced.preset(),
        benefit: 1.0,
        cost: 0.5,
        findability: None,
        untested: None,
        recently_restructured: false,
        worded_by_model: true,
        items: 113,
        without_vectors: 0,
        held_back: Vec::new(),
    };
    assessment.findability = Some(FindabilityResult {
        tried: 20,
        found_now: 12,
        found_after: 15,
        model: Some("stub/m".into()),
        seed: 7,
    });
    proposal.assessment = Some(assessment);

    let scored = score(&proposal, &files, &places, &answers, 1.5);
    assert!(scored.recovered, "{scored:?}");
    assert!(scored.after.f1 > scored.before.f1);
    assert_eq!(scored.misplaced, 18);
    assert_eq!(
        scored.fixed, 8,
        "the scattered ones; Cooking isn't a name answers.yaml knows"
    );
    assert_eq!(scored.broken, 0);
    assert_eq!(scored.moves.moved, 12);
    assert_eq!(scored.moves.weighted, 8 * 4 + 4 * 2);
    assert_eq!(scored.weighted_moves_per_fix, Some(5.0));
    assert_eq!(scored.findability, Some((20, 12, 15)));
    assert!((scored.findability_gain.unwrap() - 0.15).abs() < 1e-9);

    let nothing = score(
        &Proposal::new(ProposalKind::Section, ""),
        &files,
        &places,
        &answers,
        0.1,
    );
    assert!(!nothing.recovered);
    assert_eq!((nothing.fixed, nothing.moves.moved), (0, 0));
    assert_eq!(nothing.weighted_moves_per_fix, None);
    assert!((nothing.after.f1 - nothing.before.f1).abs() < 1e-9);
}

#[test]
fn a_proposal_built_from_the_muddled_archive_is_scored_without_a_model() {
    let dir = tempfile::tempdir().unwrap();
    let answers = Answers::load(&bench().join("answers.yaml")).unwrap();
    let muddle = Muddle::load(&bench().join("muddle.yaml")).unwrap();
    let (archive, files) = muddled(dir.path());
    // Stub card vectors that know the intended shape: one direction per intended collection.
    let collections: Vec<String> = archive
        .collections()
        .unwrap()
        .into_iter()
        .map(|c| c.path)
        .chain(["Recipes".to_owned()])
        .collect();
    let vectors: HashMap<Id, Vec<f32>> = files
        .iter()
        .filter_map(|(id, file)| {
            let intended = answers.items.get(file)?.destinations.first()?;
            let place = collections.iter().position(|c| c == intended)?;
            let mut v = vec![0.0_f32; collections.len()];
            v[place] = 1.0;
            Some((*id, v))
        })
        .collect();
    let started = std::time::Instant::now();
    let proposal = propose_section(
        &archive,
        "",
        &vectors,
        None,
        &SectionOptions::for_archive(&archive),
    )
    .unwrap();
    let scored = score(
        &proposal,
        &files,
        &muddle.places(&answers),
        &answers,
        started.elapsed().as_secs_f64(),
    );
    assert_eq!(scored.misplaced, 18);
    assert_eq!(scored.findability, None);
    assert!(
        scored
            .untested
            .as_deref()
            .is_some_and(|u| u.contains("no text model")),
        "{scored:?}"
    );
    // Grouping by vectors that know the shape subdivides collections without breaking any
    // item out of where it belongs, which brings the archive closer to the intended shape.
    assert!(scored.changes > 0, "{scored:?}");
    assert!(scored.recovered, "{scored:?}");
    assert_eq!(scored.broken, 0, "{scored:?}");
    assert_eq!(scored.changes, proposal.changes.len());

    // Eager takes on the more invasive changes Balanced holds back, so it recovers at least
    // as much of the shape; Settled at most as much.
    let mut by_level = Vec::new();
    for level in [Eagerness::Settled, Eagerness::Balanced, Eagerness::Eager] {
        let archive = at_eagerness(&archive, level).unwrap();
        let proposal = propose_section(
            &archive,
            "",
            &vectors,
            None,
            &SectionOptions::for_archive(&archive),
        )
        .unwrap();
        assert_eq!(proposal.assessment.as_ref().unwrap().eagerness, level);
        let scored = score(&proposal, &files, &muddle.places(&answers), &answers, 0.0);
        by_level.push(scored.after.f1);
    }
    assert!(
        by_level[0] <= by_level[1] && by_level[1] <= by_level[2],
        "{by_level:?}"
    );
}

/// Stub card vectors like an embedding model's: every one shares a strong common direction,
/// so even unrelated items have a cosine around 0.6, with a weaker direction for each item's
/// intended collection and a little noise. The four Allotment items, a series in Home, share
/// a direction of their own too, as items named alike do.
#[allow(clippy::cast_precision_loss)]
fn embedding_like_vectors(
    files: &BTreeMap<Id, String>,
    answers: &Answers,
    collections: &[String],
) -> HashMap<Id, Vec<f32>> {
    let dimensions = collections.len() + 34;
    let series = collections.len() + 1;
    let mut seed = 12_345_u64;
    let mut noise = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        ((seed % 10_000) as f32 / 10_000.0 - 0.5) * 0.25
    };
    files
        .iter()
        .filter_map(|(id, file)| {
            let intended = answers.items.get(file)?.destinations.first()?;
            let place = collections.iter().position(|c| c == intended)?;
            let mut v: Vec<f32> = (0..dimensions).map(|_| noise()).collect();
            v[collections.len()] = 1.1;
            v[place] = 0.8;
            if file.starts_with("Allotment") {
                v[series] = 0.8;
            }
            Some((*id, v))
        })
        .collect()
}

/// What a crawler like the benchmark's recommended one concludes on the muddled archive, by
/// file name: most misplaced items flagged and sent where they belong, with its usual mistakes.
/// The missing collection's items are flagged but sent to Projects, there being nowhere better,
/// and one of them passes; a dumped manual and a scattered paper are sent to the wrong place;
/// and a few well-placed items are flagged and sent where they don't belong.
fn crawler_like(answers: &Answers, muddle: &Muddle) -> BTreeMap<String, Judged> {
    let judged = |fits: bool, better: Option<&str>| Judged {
        fits: Some(fits),
        better: better.map(str::to_owned),
        confidence: 0.9,
        reason: String::new(),
        seconds: 1.0,
        error: None,
    };
    let false_alarms = [
        ("payslip october.txt", "Finances"),
        ("LIGO open data tutorial.md", "Projects"),
        ("Larkspur Road deposit return.txt", "Receipts"),
        ("conference registration.txt", "Projects"),
    ];
    muddle
        .places(answers)
        .into_iter()
        .map(|(file, (_, misplaced))| {
            let intended = answers.items[&file].destinations[0].as_str();
            let gave = match (misplaced, file.as_str()) {
                (Some(Misplaced::Missing), "IMG_1502.jpg") => judged(true, None),
                (Some(Misplaced::Missing), _) => judged(false, Some("Projects")),
                (_, "drill safety and warranty.txt") => judged(false, Some("Receipts")),
                (_, "paper kuramoto synchronization.txt") => judged(false, Some("Research")),
                (Some(_), _) => judged(false, Some(intended)),
                (None, _) => match false_alarms.iter().find(|(f, _)| *f == file) {
                    Some((_, to)) => judged(false, Some(to)),
                    None => judged(true, None),
                },
            };
            (file, gave)
        })
        .collect()
}

/// The muddled archive's items moved by `proposal`'s changes, each change's sorted.
fn moved_together(proposal: &Proposal) -> Vec<(String, Vec<Id>)> {
    proposal
        .changes
        .iter()
        .filter_map(|c| match &c.change {
            Edit::MoveItems { to } => {
                let mut ids: Vec<Id> = c.items.iter().map(|t| t.id).collect();
                ids.sort();
                Some((to.clone(), ids))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn the_crawlers_verdicts_bring_back_the_missing_and_emptied_collections() {
    let dir = tempfile::tempdir().unwrap();
    let answers = Answers::load(&bench().join("answers.yaml")).unwrap();
    let muddle = Muddle::load(&bench().join("muddle.yaml")).unwrap();
    let (archive, files) = muddled(dir.path());
    let places = muddle.places(&answers);
    let collections: Vec<String> = archive
        .collections()
        .unwrap()
        .into_iter()
        .map(|c| c.path)
        .chain(["Recipes".to_owned()])
        .collect();
    let vectors = embedding_like_vectors(&files, &answers, &collections);
    let intended = |collection: &str| -> Vec<Id> {
        files
            .iter()
            .filter(|(_, f)| {
                answers.items[*f].destinations.first().map(String::as_str) == Some(collection)
            })
            .map(|(id, _)| *id)
            .collect()
    };
    let propose = |level| {
        let archive = at_eagerness(&archive, level).unwrap();
        propose_section(
            &archive,
            "",
            &vectors,
            None,
            &SectionOptions::for_archive(&archive),
        )
        .unwrap()
    };

    // The numbers alone can't tell the missing Recipes, waiting in Home, from a part of Home,
    // so they leave both alone, and break nothing.
    let alone = propose(Eagerness::Eager);
    let scored = score(&alone, &files, &places, &answers, 0.0);
    assert_eq!(scored.broken, 0, "{scored:?}");
    assert!(
        moved_together(&alone)
            .iter()
            .all(|(_, ids)| *ids != intended("Recipes")),
        "{:?}",
        scored.changed
    );

    let crawled = crawler_like(&answers, &muddle);
    use_crawler_verdicts(&archive, &files, &places, Some(("crawler", &crawled))).unwrap();
    for level in [Eagerness::Balanced, Eagerness::Eager] {
        let proposal = propose(level);
        let scored = score(&proposal, &files, &places, &answers, 0.0);
        assert!(scored.recovered, "{level:?}: {scored:?}");
        assert_eq!(scored.broken, 0, "{level:?}: {scored:?}");
        let moved = moved_together(&proposal);
        // The crawler said most of Recipes doesn't fit Home but had nowhere better for it, so
        // it becomes a collection of its own inside Home.
        assert!(
            moved
                .iter()
                .any(|(to, ids)| to.starts_with("Home/") && *ids == intended("Recipes")),
            "{level:?}: {:?}",
            scored.changed
        );
        // The manuals go back to the emptied Manuals: those the crawler sends there, and the
        // one it sent to Receipts, which the numbers say is like them and nothing in Home.
        assert!(
            moved
                .iter()
                .any(|(to, ids)| to == "Manuals" && *ids == intended("Manuals")),
            "{level:?}: {:?}",
            scored.changed
        );
        // Items scattered on their own go back, but its false alarms stay, being more like
        // the items where they are.
        for file in ["receipt desk chair.txt", "letter from grandma.txt"] {
            let item = files.iter().find(|(_, f)| *f == file).unwrap().0;
            assert!(
                moved.iter().any(|(_, ids)| ids.contains(item)),
                "{level:?}: {file}: {:?}",
                scored.changed
            );
        }
        assert!(scored.fixed >= 10, "{level:?}: {scored:?}");
    }
    // Without them again, it's the numbers' proposal again.
    use_crawler_verdicts(&archive, &files, &places, None).unwrap();
    assert_eq!(propose(Eagerness::Eager).changes, alone.changes);
}

/// Card vectors made from the words of the text each card's embedding is made from
/// (`index::chunk::card_text`), its collection included, with a direction they all share:
/// cruder than an embedding model's, but with the same failings that grouping on real ones
/// showed on the muddled archive. Items in one collection look alike for being there, and
/// items about one subject in other words don't look alike at all.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn word_vectors(archive: &carrel_core::archive::Archive) -> HashMap<Id, Vec<f32>> {
    const DIMENSIONS: usize = 1024;
    let mut vectors = HashMap::new();
    for item in archive.items().unwrap() {
        let text = std::fs::read_to_string(archive.root().join(&item.card)).unwrap();
        let card = carrel_core::card_file::CardFile::parse(&text).unwrap();
        let collection = item.card.rsplit_once('/').map_or("", |(c, _)| c);
        let embedded = carrel_core::index::chunk::card_text(card.card(), collection);
        let mut v = vec![0.0_f32; DIMENSIONS + 1];
        for word in embedded
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 2)
        {
            // Words alike at the start count as one, as a crude stem.
            let stem: String = word.to_lowercase().chars().take(6).collect();
            let hash = stem.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
            });
            v[(hash % DIMENSIONS as u64) as usize] += 1.0;
        }
        let length = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
        for x in &mut v {
            *x /= length;
        }
        v[DIMENSIONS] = 0.9;
        vectors.insert(item.id, v);
    }
    vectors
}

#[test]
fn on_vectors_that_cant_see_the_shape_numbers_alone_break_nothing_and_the_crawler_helps() {
    let dir = tempfile::tempdir().unwrap();
    let answers = Answers::load(&bench().join("answers.yaml")).unwrap();
    let muddle = Muddle::load(&bench().join("muddle.yaml")).unwrap();
    let (archive, files) = muddled(dir.path());
    let places = muddle.places(&answers);
    let vectors = word_vectors(&archive);
    let propose = |level| {
        let archive = at_eagerness(&archive, level).unwrap();
        let proposal = propose_section(
            &archive,
            "",
            &vectors,
            None,
            &SectionOptions::for_archive(&archive),
        )
        .unwrap();
        score(&proposal, &files, &places, &answers, 0.0)
    };
    for level in Eagerness::ALL {
        let scored = propose(level);
        assert_eq!(scored.broken, 0, "{level:?}: {scored:?}");
        assert!(scored.after.f1 >= scored.before.f1, "{level:?}: {scored:?}");
    }
    let crawled = crawler_like(&answers, &muddle);
    use_crawler_verdicts(&archive, &files, &places, Some(("crawler", &crawled))).unwrap();
    for level in Eagerness::ALL {
        let scored = propose(level);
        assert_eq!(scored.broken, 0, "{level:?}: {scored:?}");
        assert!(scored.recovered, "{level:?}: {scored:?}");
    }
    let balanced = propose(Eagerness::Balanced);
    assert!(balanced.fixed >= 5, "{balanced:?}");
}

/// A text model that names no groups and gets lost in every tree, so no shape is easier to
/// find things in for it; or, `failing`, fails every walk.
struct LostWalker {
    offers: Vec<carrel_core::providers::Offer>,
    failing: bool,
}

impl LostWalker {
    fn registry(failing: bool) -> carrel_core::providers::Registry {
        let mut registry = carrel_core::providers::Registry::new();
        registry.add(Box::new(Self {
            offers: vec![carrel_core::providers::Offer {
                capability: carrel_core::providers::Capability::TextModel,
                id: "lost".into(),
                label: None,
                priority: 10,
                available: true,
                reason: None,
                media_types: Vec::new(),
                extra: serde_json::Map::new(),
            }],
            failing,
        }));
        registry
    }
}

impl carrel_core::providers::Provider for LostWalker {
    fn name(&self) -> &'static str {
        "lost"
    }

    fn offers(&self) -> &[carrel_core::providers::Offer] {
        &self.offers
    }

    fn complete(
        &self,
        _offer: &str,
        request: &carrel_core::providers::CompletionRequest,
    ) -> Result<carrel_core::providers::Completion, carrel_core::providers::ProviderError> {
        let schema = request.schema.as_ref().unwrap();
        let json = if schema["properties"].get("groups").is_some() {
            serde_json::json!({"groups": [], "rationale": "Tidier."})
        } else if self.failing {
            return Err(carrel_core::providers::ProviderError::Failed(
                "the walk was cut off".into(),
            ));
        } else {
            serde_json::json!({"choice": "nowhere at all"})
        };
        Ok(carrel_core::providers::Completion {
            text: json.to_string(),
            json: Some(json),
            model: "lost-1".into(),
            usage: None,
        })
    }
}

#[test]
fn with_a_model_the_findability_test_runs_and_holds_back_a_shape_no_easier_to_navigate() {
    let dir = tempfile::tempdir().unwrap();
    let answers = Answers::load(&bench().join("answers.yaml")).unwrap();
    let muddle = Muddle::load(&bench().join("muddle.yaml")).unwrap();
    let (archive, files) = muddled(dir.path());
    let archive = at_eagerness(&archive, Eagerness::Eager).unwrap();
    let collections: Vec<String> = archive
        .collections()
        .unwrap()
        .into_iter()
        .map(|c| c.path)
        .chain(["Recipes".to_owned()])
        .collect();
    let vectors = embedding_like_vectors(&files, &answers, &collections);
    let registry = LostWalker::registry(false);
    let proposal = propose_section(
        &archive,
        "",
        &vectors,
        Some(&registry),
        &SectionOptions::for_archive(&archive),
    )
    .unwrap();
    let assessment = proposal.assessment.as_ref().unwrap();
    // The muddled archive's cards have summaries, so the test runs.
    let findability = assessment.findability.as_ref().expect("the test ran");
    assert!(findability.tried > 0);
    assert!(!findability.is_better());
    assert_eq!(proposal.changes.len(), 0, "even Eager holds it back");
    assert!(
        assessment
            .held_back
            .iter()
            .any(|h| h.why.contains("wasn't easier to find things in")),
        "{:?}",
        assessment.held_back
    );
    let scored = score(&proposal, &files, &muddle.places(&answers), &answers, 0.0);
    assert!(
        (scored.after.f1 - scored.before.f1).abs() < 1e-9,
        "nothing moved"
    );

    // A test that can't run with a model at hand holds it back too.
    let proposal = propose_section(
        &archive,
        "",
        &vectors,
        Some(&LostWalker::registry(true)),
        &SectionOptions::for_archive(&archive),
    )
    .unwrap();
    assert_eq!(proposal.changes.len(), 0);
    let assessment = proposal.assessment.unwrap();
    assert!(
        assessment
            .held_back
            .iter()
            .any(|h| h.why.contains("couldn't be tested for findability")),
        "{:?}",
        assessment.held_back
    );
}
