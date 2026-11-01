//! The restructure benchmark (`docs/design/restructure.md`, "Measuring it"): where a
//! proposal's changes would leave each item, scoring the shape it recovers, the moves it takes
//! and its findability gain, on hand-made proposals and on one built from the muddled archive
//! with stub card vectors.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use carrel_core::bench::Answers;
use carrel_core::bench::crawl::{Muddle, build};
use carrel_core::bench::restructure::{after, at_eagerness, moves, score, shape};
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
    for (file, _) in &muddle.scattered {
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
