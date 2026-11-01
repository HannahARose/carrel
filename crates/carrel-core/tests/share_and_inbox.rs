//! What a Mac's share sheet sends (`docs/design/macos-integration.md`, "Share"): web addresses
//! filed as `url` items, the owner's note given to the drafter and kept in the card, and the
//! inbox a device uses while another is the writer (FORMAT.md §7.8).

use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::card::{Id, Kind};
use carrel_core::card_file::CardFile;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder, WriterLease};
use carrel_core::history::{History, Operation};
use carrel_core::inbox::{self, InboxAction};
use carrel_core::ingest::IntakeOptions;
use carrel_core::ingest::filing::Placement;
use carrel_core::intake::{add_url, add_with_note, process, waiting};
use carrel_core::providers::Registry;
use carrel_core::review::{REVIEW_DIR, ReviewEntry, ReviewQueue};
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
        for entry in std::fs::read_dir(root.join(REVIEW_DIR)).unwrap() {
            std::fs::remove_file(entry.unwrap().path()).unwrap();
        }
        let setup = Self {
            archive: Archive::open(&root).unwrap(),
            holder: Holder {
                person: Id::new_v7(),
                device: Id::new_v7(),
            },
            dir,
        };
        let lease = setup.lease(setup.holder);
        History::init(&setup.archive, &lease).unwrap();
        lease.release().unwrap();
        setup
    }

    fn lease(&self, holder: Holder) -> WriterLease {
        DiskLock::for_archive(&self.archive)
            .acquire(holder, "test", DEFAULT_LEASE)
            .unwrap()
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    /// Processes this device's one waiting entry with `registry`, and returns its proposal.
    fn draft_one(&self, registry: &Registry) -> ReviewEntry {
        let queued = waiting(&self.archive, self.holder.device, false).unwrap();
        assert_eq!(queued.len(), 1, "{queued:?}");
        let processed = process(
            &self.archive,
            registry,
            queued[0].clone(),
            self.holder,
            None,
            &mut |_| {},
        )
        .unwrap();
        assert!(processed.run.is_ok(), "{:?}", processed.run.err());
        let (proposals, _) = ReviewQueue::of(&self.archive).list().unwrap();
        assert_eq!(proposals.len(), 1);
        proposals.into_iter().next().unwrap()
    }

    /// Files `proposal` in Holding and returns its card's text.
    fn file_in_holding(&self, proposal: ReviewEntry) -> String {
        let outcome = carrel_core::actions::decide(
            &self.archive,
            vec![proposal],
            Placement::Holding,
            self.holder,
            None,
        )
        .unwrap();
        assert_eq!(outcome.filed.len(), 1, "{:?}", outcome.failed);
        std::fs::read_to_string(self.archive.root().join(&outcome.filed[0].card)).unwrap()
    }
}

#[test]
fn a_shared_address_is_drafted_from_itself_and_filed_as_a_url_item() {
    let s = Setup::new();
    let scripted = Scripted::new(
        "scripted",
        100,
        8192,
        vec![Ok(json!({
            "title": "Phase response curves explained",
            "summary": "A tutorial page on phase response curves.",
            "tags": []
        }))],
    );
    let asked = scripted.asked.clone();
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(scripted));

    let url = "https://example.org/prc?lang=en";
    let entry = add_url(
        &s.archive,
        url,
        Some("Phase response curves"),
        Some("For chapter two"),
        s.holder.device,
    )
    .unwrap()
    .expect("queued");
    assert_eq!(entry.url.as_deref(), Some(url));
    assert_eq!(
        add_url(&s.archive, url, None, None, s.holder.device).unwrap(),
        None,
        "already queued"
    );
    for wrong in ["example.org", "ftp://example.org/x", "https://"] {
        assert!(add_url(&s.archive, wrong, None, None, s.holder.device).is_err());
    }

    let proposal = s.draft_one(&registry);
    assert_eq!(proposal.kind, Kind::Url);
    assert_eq!(proposal.card.title, "Phase response curves explained");
    let prompt = &asked.lock().unwrap()[0].prompt;
    for said in [url, "Phase response curves", "For chapter two"] {
        assert!(prompt.contains(said), "{said} in {prompt}");
    }
    assert_eq!(
        add_url(&s.archive, url, None, None, s.holder.device).unwrap(),
        None,
        "already waiting for review"
    );

    let text = s.file_in_holding(proposal);
    let file = CardFile::parse(&text).unwrap();
    let card = file.card();
    assert_eq!(card.kind, Kind::Url);
    assert_eq!(card.url.as_deref(), Some(url));
    assert_eq!(card.item, None);
    assert!(file.body().contains("For chapter two"), "{text}");
    let holding = s.archive.root().join("Holding");
    assert!(holding.join("Phase response curves explained.md").is_file());
    assert!(
        !holding.join("Phase response curves.txt").exists(),
        "no file is kept for it"
    );
}

#[test]
fn the_owners_note_reaches_the_drafter_and_stays_in_the_card() {
    let s = Setup::new();
    let scripted = Scripted::new(
        "scripted",
        100,
        8192,
        vec![Ok(
            json!({"title": "Lab notes", "summary": "Notes.", "tags": []}),
        )],
    );
    let asked = scripted.asked.clone();
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(scripted));
    let notes = s.file("notes.txt", "Damping measured at 0.3.\n");
    let queued = add_with_note(
        &s.archive,
        &[notes],
        IntakeOptions::default(),
        s.holder.device,
        Some("  From Tuesday's session  "),
    )
    .unwrap();
    assert_eq!(
        queued.added[0].note.as_deref(),
        Some("From Tuesday's session")
    );

    let proposal = s.draft_one(&registry);
    assert!(
        asked.lock().unwrap()[0]
            .prompt
            .contains("The owner's note about it: From Tuesday's session")
    );
    let text = s.file_in_holding(proposal);
    let file = CardFile::parse(&text).unwrap();
    assert_eq!(file.card().kind, Kind::File);
    assert!(file.body().contains("From Tuesday's session"), "{text}");
}

#[test]
fn another_devices_shares_wait_in_its_inbox_until_the_writer_applies_them() {
    let s = Setup::new();
    let other = Holder {
        person: s.holder.person,
        device: Id::new_v7(),
    };
    let writing = s.lease(s.holder);
    let lock = inbox::writer_elsewhere(&s.archive, other.device).expect("another writer");
    assert_eq!(lock.device, s.holder.device);
    assert!(
        inbox::writer_elsewhere(&s.archive, s.holder.device).is_none(),
        "not elsewhere for the writer itself"
    );

    let photo = s.file("photo.txt", "A photo's description.\n");
    let sent = inbox::send_paths(
        &s.archive,
        other,
        std::slice::from_ref(&photo),
        false,
        Some("Holiday"),
    )
    .unwrap();
    assert!(sent.starts_with(inbox::folder(&s.archive, other.device)));
    inbox::send_url(
        &s.archive,
        other,
        "https://example.org/a",
        Some("A page"),
        None,
    )
    .unwrap();
    assert!(photo.exists(), "the original stays where it was");
    let waiting_now = inbox::waiting(&s.archive);
    assert_eq!(
        waiting_now
            .iter()
            .map(|(_, e)| e.action)
            .collect::<Vec<_>>(),
        [InboxAction::Add, InboxAction::AddUrl],
        "in the order they were sent"
    );
    assert!(
        waiting(&s.archive, s.holder.device, true)
            .unwrap()
            .is_empty(),
        "nothing is queued until the writer applies them"
    );

    let received = s.dir.path().join("received");
    let applied = inbox::apply(&s.archive, &writing, &received).unwrap();
    assert_eq!(applied.entries, 2, "{:?}", applied.problems);
    assert!(applied.commit.is_some());
    let queued = waiting(&s.archive, s.holder.device, false).unwrap();
    assert_eq!(queued.len(), 2);
    let file = queued.iter().find(|e| e.url.is_none()).unwrap();
    assert!(
        file.path.starts_with(received.canonicalize().unwrap()),
        "{}",
        file.path.display()
    );
    assert_eq!(file.note.as_deref(), Some("Holiday"));
    assert!(
        queued
            .iter()
            .any(|e| e.url.as_deref() == Some("https://example.org/a"))
    );
    assert_eq!(inbox::waiting(&s.archive).len(), 0);
    let empty = std::fs::read_dir(inbox::folder(&s.archive, other.device))
        .unwrap()
        .count();
    assert_eq!(empty, 0, "the inbox is emptied");

    let history = History::open(&s.archive).unwrap();
    let last = history.log().unwrap();
    assert_eq!(last[0].operation, Some(Operation::Inbox));
    assert_eq!(last[0].summary, "Apply 2 inbox entries");
    writing.release().unwrap();
}

#[test]
fn a_written_note_is_queued_as_a_file_of_its_own_or_sent_to_the_inbox() {
    use carrel_core::written_note::{WrittenNoteAdded, add_written_note};
    let s = Setup::new();
    let notes = s.dir.path().join("notes");
    let added = add_written_note(
        &s.archive,
        &notes,
        "Ring the framer about the lamp print.",
        None,
        Some("For the hallway"),
        s.holder,
    )
    .unwrap();
    let WrittenNoteAdded::Queued { path, queued } = added else {
        panic!("queued here: {added:?}")
    };
    assert!(path.starts_with(&notes));
    assert_eq!(
        path.file_name().unwrap(),
        "Ring the framer about the lamp print.md"
    );
    assert_eq!(queued.added.len(), 1);
    assert_eq!(queued.added[0].path, path.canonicalize().unwrap());
    assert_eq!(queued.added[0].note.as_deref(), Some("For the hallway"));

    let other = Holder {
        person: s.holder.person,
        device: Id::new_v7(),
    };
    let writing = s.lease(s.holder);
    let sent =
        add_written_note(&s.archive, &notes, "Elsewhere", Some("Away"), None, other).unwrap();
    let WrittenNoteAdded::Sent {
        inbox: entry,
        writer,
    } = sent
    else {
        panic!("sent while another device writes: {sent:?}")
    };
    assert_eq!(writer, s.holder.device);
    assert!(entry.starts_with(inbox::folder(&s.archive, other.device)));
    assert_eq!(
        std::fs::read_dir(&notes).unwrap().count(),
        1,
        "the sent note's own copy is removed"
    );
    writing.release().unwrap();
}
