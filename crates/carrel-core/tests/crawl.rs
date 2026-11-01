//! The crawler (`docs/design/restructure.md`, "Crawlers"), driven by a scripted model on the
//! sample archive: its order, its pace, its findings in Review with `crawler.changes` off, the
//! changes it makes itself with it on, and undoing them.

mod common;

use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
use carrel_core::crawl::findings::{self, Finding};
use carrel_core::crawl::verdicts::{self, Verdicts};
use carrel_core::crawl::{AGENT, Crawler, Device, Pace, Stage};
use carrel_core::history::History;
use carrel_core::librarian::changes::undo_since_by;
use carrel_core::providers::Registry;
use carrel_core::providers::device::AvailableModel;
use common::Scripted;
use serde_json::{Value, json};

/// The sample's one item in Holding: a scanned note whose summary a model wrote, and whose
/// title the owner wrote.
const SCAN: &str = "01a18701-0500-776a-9867-0810dc3b08d1";

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

fn holder() -> Holder {
    Holder {
        person: Id::new_v7(),
        device: Id::new_v7(),
    }
}

/// The sample archive with its history started, and `crawler.changes` as `changes` says.
fn sample(changes: bool) -> (tempfile::TempDir, Archive, Holder) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("archive");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
        &root,
    );
    if changes {
        let path = root.join("_System/settings.json");
        let mut settings: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        settings["crawler"] = json!({"changes": true});
        std::fs::write(&path, settings.to_string()).unwrap();
    }
    let archive = Archive::open(&root).unwrap();
    let holder = holder();
    let lease = DiskLock::for_archive(&archive)
        .acquire(holder, "test", DEFAULT_LEASE)
        .unwrap();
    History::start(&archive, &lease).unwrap();
    lease.release().unwrap();
    (dir, archive, holder)
}

/// The model's verdict: whether it fits, where it would go, how sure, and a title and summary.
fn verdict(fits: bool, collection: &str, confidence: f64) -> Value {
    json!({
        "fits": fits,
        "collection": collection,
        "confidence": confidence,
        "reason": "It's a note about the oscillators, read for reference.",
        "title": "Scanned meeting note",
        "summary": "A scanned note moving the oscillator meeting to Thursday.",
        "created": "",
        "tags": []
    })
}

/// A registry whose only text model gives `answers`, and that model.
fn scripted(answers: Vec<Value>) -> (Registry, AvailableModel) {
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(Scripted::new(
        "scripted",
        10,
        8_000,
        answers.into_iter().map(Ok).collect(),
    )));
    let model = AvailableModel {
        provider: "scripted".into(),
        offer: "m".into(),
        label: "scripted/m".into(),
    };
    (registry, model)
}

fn collection_of(archive: &Archive, id: &str) -> String {
    let item = archive
        .items()
        .unwrap()
        .into_iter()
        .find(|i| i.id.to_string() == id)
        .unwrap();
    item.card.rsplit_once('/').unwrap().0.to_owned()
}

fn never() -> bool {
    false
}

#[test]
fn holding_comes_first_and_a_pass_takes_only_its_share() {
    let (_dir, archive, holder) = sample(false);
    let (registry, model) = scripted(vec![
        verdict(false, "Reading", 0.7),
        verdict(true, "", 0.9),
        verdict(true, "", 0.9),
    ]);
    let crawler = Crawler {
        archive: &archive,
        registry: &registry,
        model,
        holder,
    };
    let did = crawler.pass(2, &never).unwrap();
    assert_eq!(did.judged, 2);
    let mine = Verdicts::load(&archive, holder.device);
    assert_eq!(mine.items.len(), 2);
    let scan = Id(SCAN.parse().unwrap());
    assert!(!mine.items[&scan].fits, "Holding was judged first");
    assert!(mine.last_pass.is_some());

    let status =
        carrel_core::crawl::status(&archive, holder.device, jiff::Timestamp::now()).unwrap();
    assert_eq!(status.judged, 2);
    assert!(
        status
            .waiting
            .iter()
            .all(|(stage, _)| *stage != Stage::Holding)
    );
    assert_eq!(status.findings, 2, "a move and a summary");
}

#[test]
fn it_stops_for_the_owner_and_keeps_its_pace() {
    let (_dir, archive, holder) = sample(false);
    let (registry, model) = scripted(vec![verdict(true, "", 0.9)]);
    let crawler = Crawler {
        archive: &archive,
        registry: &registry,
        model,
        holder,
    };
    let did = crawler.pass(4, &|| true).unwrap();
    assert!(did.stopped);
    assert_eq!(did.judged, 0);

    let now = std::time::SystemTime::now();
    assert!(Pace::Steady.due(None, now));
    assert!(!Pace::Steady.due(Some(now), now + std::time::Duration::from_mins(10)));
    assert!(Pace::Steady.due(Some(now), now + std::time::Duration::from_mins(20)));
    assert!(!Pace::Off.due(None, now));
    let idle = |minutes, on_power| Device {
        idle: Some(std::time::Duration::from_mins(minutes)),
        on_power: Some(on_power),
    };
    assert!(Pace::Steady.may_run(&idle(10, true)).is_ok());
    assert!(
        Pace::Steady
            .may_run(&idle(1, true))
            .unwrap_err()
            .contains("in use")
    );
    assert!(
        Pace::Steady
            .may_run(&idle(10, false))
            .unwrap_err()
            .contains("battery")
    );
    assert!(Pace::Brisk.may_run(&idle(3, true)).is_ok());
    assert!(Pace::Gentle.may_run(&idle(10, true)).is_err());
    assert!(Pace::Off.may_run(&idle(60, true)).is_err());
    assert!(
        Pace::Steady.may_run(&Device::default()).is_ok(),
        "what the device can't tell doesn't stop it"
    );
    assert!(Pace::Gentle.items() < Pace::Steady.items());
    assert!(Pace::Steady.items() < Pace::Brisk.items());
}

#[test]
fn with_changes_off_its_findings_wait_in_review_and_leave_the_owners_fields_alone() {
    let (_dir, archive, holder) = sample(false);
    let (registry, model) = scripted(vec![verdict(false, "Reading", 0.97)]);
    let crawler = Crawler {
        archive: &archive,
        registry: &registry,
        model: model.clone(),
        holder,
    };
    let did = crawler.pass(1, &never).unwrap();
    assert_eq!(did.proposed, 2, "{did:?}");
    assert_eq!(
        did.made,
        Vec::<String>::new(),
        "with changes off it only proposes"
    );
    assert_eq!(collection_of(&archive, SCAN), "Holding");

    let found = findings::list(&archive).unwrap();
    let kinds: Vec<&Finding> = found.iter().map(|f| &f.finding).collect();
    assert!(kinds.contains(&&Finding::Move {
        from: "Holding".into(),
        to: "Reading".into()
    }));
    assert!(
        kinds
            .iter()
            .any(|f| matches!(f, Finding::Correct { field, .. } if field == "summary"))
    );
    assert!(
        !kinds
            .iter()
            .any(|f| matches!(f, Finding::Correct { field, .. } if field == "title")),
        "the owner wrote the title, so it's left alone"
    );

    // Approving the move refiles it as the owner's change.
    let mv = found
        .iter()
        .find(|f| matches!(f.finding, Finding::Move { .. }))
        .unwrap();
    let lease = DiskLock::for_archive(&archive)
        .acquire(holder, "test", DEFAULT_LEASE)
        .unwrap();
    findings::approve(&archive, &registry, mv, &lease).unwrap();
    lease.release().unwrap();
    assert_eq!(collection_of(&archive, SCAN), "Reading");
    let log = History::open(&archive).unwrap().log().unwrap();
    assert_eq!(log[0].agent, None, "the owner's change");

    // Rejecting the summary removes it, and it isn't proposed again.
    let summary = findings::list(&archive).unwrap().remove(0);
    findings::reject(&archive, &summary, holder.device).unwrap();
    assert_eq!(findings::list(&archive).unwrap().len(), 0);
    let scan = Id(SCAN.parse().unwrap());
    let mut mine = Verdicts::load(&archive, holder.device);
    let judged = mine.items.get_mut(&scan).unwrap();
    assert_eq!(judged.declined.len(), 1);
    // Judged long ago in Holding, so it's due again where it is now.
    judged.judged = "2020-01-01T00:00:00Z".into();
    mine.save(&archive, holder.device).unwrap();
    let (registry, model) = scripted(vec![verdict(true, "", 0.97); 12]);
    let again = Crawler {
        archive: &archive,
        registry: &registry,
        model,
        holder,
    }
    .pass(12, &never)
    .unwrap();
    assert!(
        !findings::list(&archive)
            .unwrap()
            .iter()
            .any(|f| f.item == scan
                && matches!(&f.finding, Finding::Correct { field, .. } if field == "summary")),
        "the declined summary isn't proposed again: {again:?}"
    );
}

#[test]
fn with_changes_on_it_makes_what_its_sure_of_and_undo_takes_it_back() {
    let (_dir, archive, holder) = sample(true);
    let (registry, model) = scripted(vec![verdict(false, "Reading", 0.95)]);
    let crawler = Crawler {
        archive: &archive,
        registry: &registry,
        model,
        holder,
    };
    let did = crawler.pass(1, &never).unwrap();
    assert_eq!(did.made.len(), 2, "{did:?}");
    assert_eq!(did.proposed, 0);
    assert_eq!(collection_of(&archive, SCAN), "Reading");
    let log = History::open(&archive).unwrap().log().unwrap();
    let mine: Vec<_> = log
        .iter()
        .filter(|e| e.agent.as_deref() == Some(AGENT))
        .collect();
    assert_eq!(mine.len(), 2, "each change is the crawler's");
    assert!(
        mine.iter().all(|e| e.summary.contains("Crawler")),
        "{mine:?}"
    );

    let lease = DiskLock::for_archive(&archive)
        .acquire(holder, "test", DEFAULT_LEASE)
        .unwrap();
    let undone = undo_since_by(&archive, AGENT, 0, &lease).unwrap();
    lease.release().unwrap();
    assert_eq!(undone.undone.len(), 2, "{:?}", undone.failed);
    assert_eq!(collection_of(&archive, SCAN), "Holding");
}

#[test]
fn with_changes_on_what_it_isnt_sure_of_still_waits_in_review() {
    let (_dir, archive, holder) = sample(true);
    let (registry, model) = scripted(vec![verdict(false, "Reading", 0.6)]);
    let did = Crawler {
        archive: &archive,
        registry: &registry,
        model,
        holder,
    }
    .pass(1, &never)
    .unwrap();
    assert_eq!(did.made, Vec::<String>::new());
    assert_eq!(did.proposed, 2);
    assert_eq!(collection_of(&archive, SCAN), "Holding");
}

#[test]
fn a_collection_whose_items_keep_failing_becomes_a_signal() {
    let (_dir, archive, holder) = sample(false);
    let (registry, model) = scripted(vec![
        verdict(false, "Projects", 0.6),
        verdict(false, "Projects", 0.6),
        verdict(false, "Projects", 0.6),
        verdict(false, "Projects", 0.6),
        verdict(false, "Projects", 0.6),
    ]);
    Crawler {
        archive: &archive,
        registry: &registry,
        model,
        holder,
    }
    .pass(5, &never)
    .unwrap();
    let misfits = verdicts::misfits(&verdicts::all(&archive));
    assert!(!misfits.is_empty(), "{:?}", verdicts::all(&archive));
    let signals = carrel_core::signals::signals(&archive).unwrap();
    for (collection, _, _) in &misfits {
        assert!(signals.iter().any(|s| matches!(
            s,
            carrel_core::signals::Signal::Misfits { collection: c, .. } if c == collection
        )));
    }
}
