//! Desk items: their cards drafted as ingest drafts them.

use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::card_file::CardFile;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder, WriterLease};
use carrel_core::desk::{Desk, add, archive, draft};
use carrel_core::entries::HistoryEntry;
use carrel_core::history::History;
use carrel_core::ingest::filing::Placement;
use carrel_core::intake::{process, waiting};
use carrel_core::providers::Registry;
use carrel_core::review::ReviewQueue;
use serde_json::json;

mod common;
use common::Scripted;

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
    dir: tempfile::TempDir,
    archive: Archive,
    holder: Holder,
}

impl Setup {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("archive");
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
            &root,
        );
        let archive = Archive::open(&root).unwrap();
        let holder = Holder {
            person: Id::new_v7(),
            device: Id::new_v7(),
        };
        let setup = Self {
            dir,
            archive,
            holder,
        };
        let lease = setup.lease();
        History::init(&setup.archive, &lease).unwrap();
        lease.release().unwrap();
        setup
    }

    /// The writer lock; release it before anything that takes it itself.
    fn lease(&self) -> WriterLease {
        DiskLock::for_archive(&self.archive)
            .acquire(self.holder, "test", DEFAULT_LEASE)
            .unwrap()
    }

    /// A folder on the desk with some notes in it.
    fn thesis(&self) -> PathBuf {
        let thesis = self.dir.path().join("Thesis");
        std::fs::create_dir_all(&thesis).unwrap();
        std::fs::write(
            thesis.join("notes.txt"),
            "Chapter two: coupled oscillators and their phase response.\n",
        )
        .unwrap();
        thesis
    }

    fn device(&self) -> Id {
        self.holder.device
    }
}

fn with_model(answers: Vec<serde_json::Value>) -> Registry {
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(Scripted::new(
        "scripted",
        100,
        8192,
        answers.into_iter().map(Ok).collect(),
    )));
    registry
}

fn card_of(path: &Path) -> CardFile {
    CardFile::parse(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn a_desk_card_is_drafted_by_a_model_like_an_ingested_one() {
    let s = Setup::new();
    let thesis = s.thesis();
    let registry = with_model(vec![json!({
        "title": "Thesis on coupled oscillators",
        "summary": "Chapter drafts and notes for the thesis.",
        "tags": ["draft", "not-in-the-vocabulary"],
        "created": "2026-09-01"
    })]);
    let drafted = draft(&s.archive, &registry, &thesis, s.device()).unwrap();
    let entry = add(&s.archive, &thesis, None, None, Some(&drafted), &s.lease()).unwrap();

    let file = card_of(&entry.card);
    let card = file.card();
    assert_eq!(card.title, "Thesis on coupled oscillators");
    assert_eq!(
        card.summary.as_deref(),
        Some("Chapter drafts and notes for the thesis.")
    );
    assert_eq!(
        card.tags.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(),
        ["draft"],
        "only tags from the vocabulary"
    );
    let dates = card.dates.as_ref().unwrap();
    assert_eq!(dates.created.as_ref().unwrap().0, "2026-09-01");
    assert!(dates.added.is_some());
    let generated = card.generated.as_ref().unwrap();
    assert_eq!(generated.model.as_deref(), Some("scripted/m"));
    assert!(generated.fields.contains(&"title".to_owned()));
    assert_eq!(card.item.as_deref(), Some("Thesis"));
}

#[test]
fn the_owners_title_is_kept_and_left_out_of_what_the_model_wrote() {
    let s = Setup::new();
    let thesis = s.thesis();
    let registry = with_model(vec![json!({
        "title": "Thesis on coupled oscillators",
        "summary": "Chapter drafts and notes for the thesis.",
        "tags": []
    })]);
    let drafted = draft(&s.archive, &registry, &thesis, s.device()).unwrap();
    let entry = add(
        &s.archive,
        &thesis,
        Some("My thesis"),
        None,
        Some(&drafted),
        &s.lease(),
    )
    .unwrap();
    let file = card_of(&entry.card);
    let card = file.card();
    assert_eq!(card.title, "My thesis");
    assert_eq!(
        card.summary.as_deref(),
        Some("Chapter drafts and notes for the thesis.")
    );
    let fields = &card.generated.as_ref().unwrap().fields;
    assert!(!fields.contains(&"title".to_owned()), "{fields:?}");
    assert!(fields.contains(&"summary".to_owned()), "{fields:?}");
}

#[test]
fn without_a_model_the_title_is_the_items_name() {
    let s = Setup::new();
    let notes = s.dir.path().join("reading-notes.txt");
    std::fs::write(&notes, "Some notes.\n").unwrap();
    let drafted = draft(&s.archive, &Registry::with_built_ins(), &notes, s.device()).unwrap();
    assert_ne!(drafted.problems, Vec::<String>::new());
    let entry = add(&s.archive, &notes, None, None, Some(&drafted), &s.lease()).unwrap();
    let file = card_of(&entry.card);
    let card = file.card();
    assert_eq!(card.title, "reading-notes.txt");
    assert_eq!(card.summary, None);
    assert_eq!(card.tags, Vec::new());
    assert_eq!(card.generated, None);
}

#[test]
fn archiving_a_desk_item_queues_it_and_filing_keeps_its_card_and_history() {
    let s = Setup::new();
    let thesis = s.thesis();
    let lease = s.lease();
    let entry = add(&s.archive, &thesis, Some("My thesis"), None, None, &lease).unwrap();
    let mut file = card_of(&entry.card);
    file.card_mut().summary = Some("Chapters and notes.".into());
    let mut log = file.history().unwrap();
    log.add(HistoryEntry::pending(
        "2026-10-30",
        Some(70),
        vec!["Changed 1 file(s): notes.txt".into()],
    ));
    file.set_history(&log).unwrap();
    std::fs::write(&entry.card, file.to_text().unwrap()).unwrap();

    let (archived, intake) = archive(&s.archive, &thesis, &lease).unwrap();
    assert_eq!(archived.id, entry.id);
    assert!(
        Desk::load(&s.archive, s.device()).unwrap().items.is_empty(),
        "it's off the desk"
    );
    assert!(entry.card.exists(), "the card stays until it's filed");
    let again = archive(&s.archive, &thesis, &lease).unwrap_err();
    assert!(again.to_string().contains("isn't on the desk"), "{again}");
    lease.release().unwrap();

    let queued = waiting(&s.archive, s.device(), false).unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].id, intake.id);
    let processed = process(
        &s.archive,
        &Registry::with_built_ins(),
        queued[0].clone(),
        s.holder,
        None,
        &mut |_| {},
    )
    .unwrap();
    assert!(processed.run.is_ok());
    let (proposals, _) = ReviewQueue::of(&s.archive).list().unwrap();
    let proposal = proposals
        .into_iter()
        .find(|p| p.source.path == archived.path)
        .expect("a proposal for it");
    assert_eq!(proposal.card.title, "My thesis", "the desk card's fields");
    assert_eq!(
        proposal.card.summary.as_deref(),
        Some("Chapters and notes.")
    );

    let outcome = carrel_core::actions::decide(
        &s.archive,
        vec![proposal],
        Placement::Holding,
        s.holder,
        None,
    )
    .unwrap();
    assert_eq!(outcome.filed.len(), 1, "{:?}", outcome.failed);
    let filed = &outcome.filed[0];
    assert_eq!(filed.item, entry.id, "the desk card's identifier is kept");
    let card = std::fs::read_to_string(s.archive.root().join(&filed.card)).unwrap();
    assert!(card.contains("title: My thesis"), "{card}");
    assert!(card.contains("Changed 1 file(s): notes.txt"), "{card}");
    assert!(!entry.card.exists(), "the desk card moved into the archive");
}

/// Makes `path`'s modification time `days` ago.
fn age(path: &Path, days: u64) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_hours(24 * days))
        .unwrap();
}

#[test]
fn status_suggests_active_folders_to_track_and_idle_desk_items_to_archive() {
    let s = Setup::new();
    let code = s.dir.path().join("Code");
    let write = |path: &str, days: u64| {
        let file = code.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "x\n").unwrap();
        age(&file, days);
    };
    write("active/notes.md", 1);
    write("active/target/old.o", 1);
    write("repo/main.rs", 300);
    std::fs::create_dir_all(code.join("repo/.git")).unwrap();
    write("dormant/plan.md", 30);
    write("dormant/target/build.log", 1);
    write(".hidden/today.md", 0);
    write("on-desk/today.md", 0);
    write("idle/old.md", 90);

    let lease = s.lease();
    add(&s.archive, &code.join("on-desk"), None, None, None, &lease).unwrap();
    add(&s.archive, &code.join("idle"), None, None, None, &lease).unwrap();
    lease.release().unwrap();
    let desk = Desk::load(&s.archive, s.device()).unwrap();
    let found = carrel_core::desk::suggest(
        &s.archive,
        &desk,
        std::slice::from_ref(&code),
        std::time::SystemTime::now(),
    )
    .unwrap();
    let names: Vec<String> = found
        .to_track
        .iter()
        .map(|t| t.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        ["active", "repo"],
        "regenerable folders don't count as changes, and what's tracked isn't suggested"
    );
    assert!(found.to_track[1].git);
    assert_eq!(found.to_archive.len(), 1);
    assert!(found.to_archive[0].entry.path.ends_with("idle"));
}

#[test]
fn desk_cards_are_mirrored_in_active_and_notes_written_there_go_back_to_the_card() {
    let s = Setup::new();
    let thesis = s.thesis();
    let lease = s.lease();
    let entry = add(&s.archive, &thesis, Some("My thesis"), None, None, &lease).unwrap();
    let mirror = s.archive.root().join("Active/On desk/My thesis.md");
    let text = std::fs::read_to_string(&mirror).unwrap();
    assert!(text.contains("carrel: desk-mirror"), "{text}");
    assert!(text.contains("# My thesis"), "{text}");
    let index = std::fs::read_to_string(s.archive.root().join("Active/_index.md")).unwrap();
    assert!(
        index.contains("[My thesis](On%20desk/My%20thesis.md)"),
        "{index}"
    );

    // A session is recorded on the card while the owner writes a note in the mirror.
    let mut file = card_of(&entry.card);
    let mut log = file.history().unwrap();
    log.add(HistoryEntry::pending(
        "2026-10-30",
        Some(70),
        vec!["Changed 1 file(s): notes.txt".into()],
    ));
    file.set_history(&log).unwrap();
    std::fs::write(&entry.card, file.to_text().unwrap()).unwrap();
    let note = "Ask Sam about chapter three.";
    let edited = text.replace(
        "<!-- /carrel:mirror -->\n",
        &format!("<!-- /carrel:mirror -->\n{note}\n"),
    );
    std::fs::write(&mirror, edited).unwrap();

    let done = carrel_core::active::regenerate(&s.archive).unwrap();
    assert_eq!(done.copied, std::slice::from_ref(&entry.card));
    let card = std::fs::read_to_string(&entry.card).unwrap();
    assert!(card.contains(note), "{card}");
    assert!(
        card.contains("Changed 1 file(s): notes.txt"),
        "the card's history is kept: {card}"
    );
    let text = std::fs::read_to_string(&mirror).unwrap();
    assert!(text.contains(note) && text.contains("notes.txt"), "{text}");
    let again = carrel_core::active::regenerate(&s.archive).unwrap();
    assert_eq!(again.copied, Vec::<PathBuf>::new(), "copied once");
    assert_eq!(again.changed, Vec::<String>::new());

    // Both changed: the mirror's note is added, and nothing on the card is lost.
    std::fs::write(&entry.card, card.replace(note, "Sam replied.")).unwrap();
    std::fs::write(&mirror, text.replace(note, "Send it Friday.")).unwrap();
    carrel_core::active::regenerate(&s.archive).unwrap();
    let card = std::fs::read_to_string(&entry.card).unwrap();
    assert!(
        card.contains("Sam replied.") && card.contains("Send it Friday."),
        "{card}"
    );

    carrel_core::desk::remove(&s.archive, &thesis, false, &lease).unwrap();
    lease.release().unwrap();
    assert!(!mirror.exists(), "its mirror goes when it's off the desk");
}

#[test]
fn a_mirror_of_another_devices_desk_card_is_kept() {
    let s = Setup::new();
    let other = Id::new_v7();
    let card = PathBuf::from("/elsewhere/Thesis.md");
    let entry = carrel_core::desk::DeskEntry {
        id: Id::new_v7(),
        path: "/elsewhere/Thesis".into(),
        card: card.clone(),
        added: carrel_core::card::DateOrTime("2026-10-01T09:00:00-04:00".into()),
        extra: carrel_core::card::Extra::new(),
    };
    let desk = Desk {
        items: vec![entry.clone()],
        extra: carrel_core::card::Extra::new(),
    };
    let path = Desk::path(&s.archive, other);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_string(&desk).unwrap()).unwrap();
    let mirror = s.archive.root().join("Active/On desk/Their thesis.md");
    std::fs::create_dir_all(mirror.parent().unwrap()).unwrap();
    let text = format!(
        "---\ncarrel: desk-mirror\nid: {}\ncard: {}\nmirrored: sha256:00\n---\n<!-- carrel:mirror -->\n# Their thesis\n<!-- /carrel:mirror -->\n",
        entry.id,
        card.display()
    );
    std::fs::write(&mirror, &text).unwrap();

    carrel_core::active::regenerate(&s.archive).unwrap();
    assert_eq!(std::fs::read_to_string(&mirror).unwrap(), text);
    let index = std::fs::read_to_string(s.archive.root().join("Active/_index.md")).unwrap();
    assert!(index.contains("[Their thesis]"), "{index}");
}
