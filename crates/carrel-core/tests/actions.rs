//! The owner's decisions on proposals and questions (`carrel_core::actions`).

use std::path::Path;

use carrel_core::actions::{self, ActionError};
use carrel_core::archive::Archive;
use carrel_core::card::{Id, Sensitivity};
use carrel_core::coordinator::Holder;
use carrel_core::history::History;
use carrel_core::ingest::card::Glossary;
use carrel_core::ingest::filing::Placement;
use carrel_core::questions::QuestionQueue;
use carrel_core::review::{Confidence, REVIEW_DIR, ReviewEntry, ReviewQueue, State};

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

struct Setup {
    _dir: tempfile::TempDir,
    archive: Archive,
    holder: Holder,
    sample: ReviewEntry,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("archive");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
        &root,
    );
    let sample_path = root
        .join(REVIEW_DIR)
        .join("01a18701-0500-776a-9867-08f000000001.json");
    let sample: ReviewEntry =
        serde_json::from_str(&std::fs::read_to_string(&sample_path).unwrap()).unwrap();
    std::fs::remove_file(sample_path).unwrap();
    let archive = Archive::open(&root).unwrap();
    let holder = Holder {
        person: Id::new_v7(),
        device: Id::new_v7(),
    };
    let lease = carrel_core::coordinator::Coordinator::acquire(
        &carrel_core::coordinator::DiskLock::for_archive(&archive),
        holder,
        "test",
        carrel_core::coordinator::DEFAULT_LEASE,
    )
    .unwrap();
    History::start(&archive, &lease).unwrap();
    lease.release().unwrap();
    Setup {
        _dir: dir,
        archive,
        holder,
        sample,
    }
}

impl Setup {
    /// A saved proposal for a file named `name` that exists on this device.
    fn proposal(&self, name: &str, confidence: Confidence) -> ReviewEntry {
        let path = self.archive.root().parent().unwrap().join(name);
        std::fs::write(&path, name).unwrap();
        let mut entry = self.sample.clone();
        entry.id = Id::new_v7();
        entry.source.path.clone_from(&path);
        entry.source.device = self.holder.device;
        entry.source.checksum = carrel_core::card::Checksum::of_file(&path).unwrap();
        entry.source.size = name.len() as u64;
        entry.card.title = name.into();
        entry.destination.confidence = confidence;
        entry.proposed_tags.clear();
        ReviewQueue::of(&self.archive).save(&entry).unwrap();
        entry
    }
}

#[test]
fn decisions_find_every_proposal_first_and_file_them_as_one_batch() {
    let s = setup();
    let sure = s.proposal("sure.txt", Confidence::High);
    let unsure = s.proposal("unsure.txt", Confidence::Low);
    let mut secret = s.proposal("secret.txt", Confidence::High);
    secret.card.sensitivity = Sensitivity::Sensitive;
    ReviewQueue::of(&s.archive).save(&secret).unwrap();

    let high: Vec<Id> = actions::all_high(&s.archive)
        .unwrap()
        .iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(high, [sure.id], "never sensitive, never low confidence");

    let typo = actions::proposals(&s.archive, &[sure.id.to_string(), "ffffffff".into()]);
    assert!(
        matches!(typo, Err(ActionError::Queue(_))),
        "a typo changes nothing"
    );

    let both = actions::proposals(
        &s.archive,
        &[
            sure.id.to_string(),
            unsure.id.to_string(),
            sure.id.to_string(),
        ],
    )
    .unwrap();
    assert_eq!(both.len(), 2, "each once");
    let outcome =
        actions::decide(&s.archive, both, Placement::Destination, s.holder, None).unwrap();
    assert_eq!(outcome.filed.len(), 2);
    assert!(outcome.commit.is_some());
    let log = History::open(&s.archive).unwrap().log().unwrap();
    assert_eq!(log[0].items.len(), 2, "one commit for both");

    let rejected = actions::reject(&s.archive, vec![secret.clone()]).unwrap();
    assert_eq!(rejected[0].state, State::Rejected);
    assert!(matches!(
        actions::proposals(&s.archive, &[secret.id.to_string()]),
        Err(ActionError::Invalid(_))
    ));
}

#[test]
fn moving_a_proposal_narrows_its_tags_and_files_nothing() {
    let s = setup();
    let mut entry = s.proposal("print.stl", Confidence::Low);
    entry.card.tags = vec![carrel_core::card::Tag("3d-printing".into())];
    ReviewQueue::of(&s.archive).save(&entry).unwrap();
    let moved = actions::move_to(&s.archive, &entry.id.to_string(), "/reading/").unwrap();
    assert_eq!(moved.to, "Reading");
    assert_eq!(moved.dropped, ["3d-printing"]);
    assert_eq!(moved.entry.destination.confidence, Confidence::High);
    assert!(
        entry.source.path.exists(),
        "nothing is filed until approval"
    );
    assert!(matches!(
        actions::move_to(&s.archive, &entry.id.to_string(), "Nowhere"),
        Err(ActionError::Invalid(_))
    ));
}

#[test]
fn answers_teach_the_glossary_or_place_the_items_and_dismissing_frees_them() {
    let s = setup();
    let collections = s.archive.collections().unwrap();
    let mut entries = vec![
        s.proposal("ABX-1 results.csv", Confidence::Low),
        s.proposal("ABX summary.pdf", Confidence::Low),
    ];
    let questions = carrel_core::ingest::questions::ask(
        &s.archive,
        &collections,
        &Glossary::of(&s.archive),
        &entries,
    );
    let queue = QuestionQueue::of(&s.archive);
    for entry in &mut entries {
        carrel_core::questions::mark_waiting(entry, &questions);
        ReviewQueue::of(&s.archive).save(entry).unwrap();
    }
    let term = &questions[0];
    queue.save(term).unwrap();
    assert_eq!(actions::waiting(&s.archive, term).unwrap().len(), 2);

    let commit = actions::teach(&s.archive, term, "ABX is the assay kit.", s.holder).unwrap();
    assert!(commit.is_some());
    assert_eq!(Glossary::of(&s.archive).facts[0].0, "ABX");
    assert!(
        queue.find(&term.id.to_string()).is_err(),
        "answered questions leave"
    );
    // Its items go back on the queue, at the front, for the worker to draft again.
    let requeued = actions::requeue_waiting(&s.archive, term, s.holder.device).unwrap();
    assert_eq!(requeued.len(), 2);
    let waiting = carrel_core::intake::waiting(&s.archive, s.holder.device, false).unwrap();
    assert_eq!(waiting.len(), 2);
    assert_eq!(
        actions::waiting(&s.archive, term).unwrap().len(),
        0,
        "their proposals left the review queue"
    );

    // A known term whose items can't be placed asks where they go.
    let questions = carrel_core::ingest::questions::ask(
        &s.archive,
        &collections,
        &Glossary::of(&s.archive),
        &entries,
    );
    let place = &questions[0];
    queue.save(place).unwrap();
    for entry in &mut entries {
        carrel_core::questions::mark_waiting(entry, std::slice::from_ref(place));
        ReviewQueue::of(&s.archive).save(entry).unwrap();
    }
    let moved = actions::place(&s.archive, place, "Projects/Hardware").unwrap();
    assert_eq!(moved.len(), 2);
    assert!(moved.iter().all(|m| m.to == "Projects/Hardware"));

    let mut again = place.clone();
    again.id = Id::new_v7();
    queue.save(&again).unwrap();
    actions::dismiss(&s.archive, &again).unwrap();
    assert!(queue.find(&again.id.to_string()).is_err());
}

#[test]
fn an_edited_card_is_checked_and_becomes_the_owners() {
    let s = setup();
    let mut entry = s.proposal("lamp.stl", Confidence::Medium);
    entry.card.sensitivity = Sensitivity::Personal;
    entry.card.generated = s.sample.card.generated.clone();
    entry.state = State::NeedsOwner;
    entry.problems = vec![carrel_core::review::Problem {
        stage: "card".into(),
        message: "no text model on this device; write the card by hand".into(),
    }];
    let text = actions::draft_text(&s.archive, &entry, &[]).unwrap();
    assert!(text.contains("# Tags that apply there: 3d-printing"));
    assert_eq!(
        actions::apply_draft(&s.archive, &mut entry.clone(), &text).unwrap(),
        actions::Edited::Unchanged
    );

    let lower = text.replace("sensitivity: personal", "sensitivity: public");
    let bad_date = text.replace(
        "title: lamp.stl",
        "title: lamp.stl\ndates:\n  created: 2026-13-40",
    );
    for (edited, expected) in [
        (lower, "sensitivity can't go below personal"),
        (bad_date, "dates.created “2026-13-40” isn't a date"),
        (
            text.replace("title: lamp.stl", "title: ' '"),
            "the title is empty",
        ),
        (format!("{text}\nnot_a_field: 1\n"), "the YAML isn't valid"),
    ] {
        match actions::apply_draft(&s.archive, &mut entry.clone(), &edited).unwrap() {
            actions::Edited::Problems(problems) => {
                assert!(problems[0].starts_with(expected), "{problems:?}");
            }
            other => panic!("{other:?} for {edited}"),
        }
    }

    let good = text
        .replace("title: lamp.stl", "title: Lamp diffuser")
        .replace("sensitivity: personal", "sensitivity: sensitive");
    assert_eq!(
        actions::apply_draft(&s.archive, &mut entry, &good).unwrap(),
        actions::Edited::Changed
    );
    assert_eq!(entry.card.title, "Lamp diffuser");
    assert_eq!(entry.card.sensitivity, Sensitivity::Sensitive);
    assert_eq!(entry.state, State::Proposed, "the owner settled the card");
    assert!(entry.problems.is_empty(), "{:?}", entry.problems);
    let fields = &entry.card.generated.as_ref().unwrap().fields;
    assert!(!fields.contains(&"title".to_owned()) && !fields.contains(&"sensitivity".to_owned()));
    assert!(
        fields.contains(&"summary".to_owned()),
        "the model's summary is still the model's"
    );
}

#[test]
fn likely_duplicates_are_filed_as_versions_or_beside_the_archived_item() {
    use carrel_core::ingest::likely::{Likely, Why};
    let s = setup();
    let archived = s
        .archive
        .items()
        .unwrap()
        .into_iter()
        .find(|i| i.card == "Projects/Hardware/parts-list.csv.md")
        .unwrap();
    let likely = |why: Why| Likely {
        id: archived.id,
        card: archived.card.clone(),
        why: vec![why],
    };
    let card_of = |filed: &carrel_core::ingest::filing::Filed| {
        let text = std::fs::read_to_string(s.archive.root().join(&filed.card)).unwrap();
        carrel_core::card_file::CardFile::parse(&text)
            .unwrap()
            .card()
            .clone()
    };

    // A newer version is linked as one without the owner choosing, and isn't approved in bulk.
    let mut newer = s.proposal("parts-list v2.csv", Confidence::High);
    newer.likely_duplicates = vec![likely(Why::NewerVersion)];
    ReviewQueue::of(&s.archive).save(&newer).unwrap();
    assert!(!actions::is_high(&newer));
    let outcome = actions::decide(
        &s.archive,
        vec![newer],
        Placement::Destination,
        s.holder,
        None,
    )
    .unwrap();
    let card = card_of(&outcome.filed[0]);
    assert_eq!(card.links.len(), 1);
    assert_eq!(card.links[0].link_type.0, "version-of");
    assert_eq!(card.links[0].id, archived.id);
    let path = card.links[0].path.as_deref().unwrap();
    assert!(
        !path.starts_with("Projects/") && path.ends_with("parts-list.csv.md"),
        "the path is relative to the new card: {path}"
    );

    // The same name has no default link; the owner keeps both with a `related` one.
    let mut same = s.proposal("parts-list (1).csv", Confidence::High);
    same.likely_duplicates = vec![likely(Why::SameName)];
    actions::keep(&s.archive, &mut same, actions::Kept::Both, None).unwrap();
    let outcome = actions::decide(
        &s.archive,
        vec![same],
        Placement::Destination,
        s.holder,
        None,
    )
    .unwrap();
    assert_eq!(card_of(&outcome.filed[0]).links[0].link_type.0, "related");

    // With nothing to link to, or a target that isn't archived, nothing changes.
    let mut plain = s.proposal("plain.txt", Confidence::High);
    assert!(matches!(
        actions::keep(&s.archive, &mut plain, actions::Kept::AsVersion, None),
        Err(ActionError::Invalid(_))
    ));
    assert!(matches!(
        actions::keep(
            &s.archive,
            &mut plain,
            actions::Kept::AsVersion,
            Some("ffffffff")
        ),
        Err(ActionError::Invalid(_))
    ));
    let id = archived.id.0.to_string();
    let end = &id[id.len() - 8..];
    actions::keep(&s.archive, &mut plain, actions::Kept::AsVersion, Some(end)).unwrap();
    assert_eq!(
        plain.card.links[0].id, archived.id,
        "the end of an identifier is enough"
    );

    let comparisons = actions::compare(&s.archive, &{
        let mut e = s.proposal("parts-list (2).csv", Confidence::High);
        e.likely_duplicates = vec![likely(Why::SameName)];
        e
    })
    .unwrap();
    assert_eq!(comparisons.len(), 1);
    assert_eq!(comparisons[0].why, ["same file name"]);
    let row = |label: &str| {
        comparisons[0]
            .rows
            .iter()
            .find(|r| r.label == label)
            .unwrap()
    };
    assert_eq!(row("file name").archived, "parts-list.csv");
    assert!(row("file name").differs);
}
