//! Every operation a device that isn't the writer sends through its inbox, applied by the
//! writer as the sender's, or skipped, with why, when it no longer applies; and the holding
//! folder a device without Kopia keeps replaced content in (`docs/design/sync-and-mobile.md`,
//! FORMAT.md §7.8 and §7.13).

use std::path::{Path, PathBuf};

use carrel_core::actions;
use carrel_core::archive::{Archive, ItemRef};
use carrel_core::backup::{BackupError, Backups, Snapshot};
use carrel_core::card::Id;
use carrel_core::card_file::CardFile;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
use carrel_core::correct::prepare::Corrects;
use carrel_core::correct::{Changes, Field};
use carrel_core::desk::{self, Recording};
use carrel_core::history::History;
use carrel_core::holding::{self, Why};
use carrel_core::inbox::{self, Answer, Applied, Applying, FlagTarget, Request};
use carrel_core::ingest::card::Glossary;
use carrel_core::providers::Registry;
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

/// An archive with a Mac that writes and a phone that doesn't.
struct Setup {
    dir: tempfile::TempDir,
    archive: Archive,
    mac: Holder,
    phone: Holder,
    sample: ReviewEntry,
    registry: Registry,
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
    let mac = Holder {
        person: Id::new_v7(),
        device: Id::new_v7(),
    };
    let phone = Holder {
        person: mac.person,
        device: Id::new_v7(),
    };
    let lease = DiskLock::for_archive(&archive)
        .acquire(mac, "test", DEFAULT_LEASE)
        .unwrap();
    History::start(&archive, &lease).unwrap();
    lease.release().unwrap();
    Setup {
        dir,
        archive,
        mac,
        phone,
        sample,
        registry: Registry::with_built_ins(),
    }
}

impl Setup {
    /// Applies every inbox as the Mac.
    fn apply(&self) -> Applied {
        let received = self.dir.path().join("received");
        let applying = Applying {
            writer: self.mac,
            received: &received,
            registry: &self.registry,
            backups: None,
        };
        let applied = inbox::apply(&self.archive, &applying).unwrap();
        assert!(applied.problems.is_empty(), "{:?}", applied.problems);
        applied
    }

    fn send(&self, request: Request) -> PathBuf {
        inbox::send(&self.archive, self.phone, request).unwrap()
    }

    fn item(&self, card: &str) -> ItemRef {
        self.archive
            .items()
            .unwrap()
            .into_iter()
            .find(|i| i.card == card)
            .unwrap()
    }

    fn card(&self, card: &str) -> CardFile {
        CardFile::parse(&std::fs::read_to_string(self.archive.root().join(card)).unwrap()).unwrap()
    }

    /// The last change in the history: its summary, and whether it's the phone's.
    fn last_change(&self) -> (String, bool) {
        let log = History::open(&self.archive).unwrap().log().unwrap();
        let last = &log[0];
        (
            last.summary.clone(),
            last.device == Some(self.phone.device) && last.person == Some(self.phone.person),
        )
    }

    fn skipped(&self) -> Vec<String> {
        inbox::skipped(&self.archive, self.phone.device)
            .into_iter()
            .map(|s| s.reason)
            .collect()
    }

    /// A saved proposal for a file named `name` that's on the Mac.
    fn proposal(&self, name: &str) -> ReviewEntry {
        let path = self.dir.path().join(name);
        std::fs::write(&path, name).unwrap();
        let mut entry = self.sample.clone();
        entry.id = Id::new_v7();
        entry.source.path.clone_from(&path);
        entry.source.device = self.mac.device;
        entry.source.checksum = carrel_core::card::Checksum::of_file(&path).unwrap();
        entry.source.size = name.len() as u64;
        entry.card.title = name.into();
        entry.destination.confidence = Confidence::Low;
        entry.proposed_tags.clear();
        ReviewQueue::of(&self.archive).save(&entry).unwrap();
        entry
    }
}

const PARTS: &str = "Projects/Hardware/parts-list.csv.md";

#[test]
fn approvals_and_rejections_are_the_senders_and_a_decided_proposal_is_skipped() {
    let s = setup();
    let approved = s.proposal("approved.txt");
    let rejected = s.proposal("rejected.txt");
    s.send(Request::Approve {
        proposals: vec![approved.id],
        holding: false,
        keep: None,
    });
    s.send(Request::Reject {
        proposals: vec![rejected.id],
    });
    assert_eq!(
        ReviewQueue::of(&s.archive)
            .find(&approved.id.to_string())
            .unwrap()
            .state,
        State::Proposed,
        "nothing changes until the writer applies it"
    );
    let applied = s.apply();
    assert_eq!(applied.entries, 2);
    assert!(
        s.archive
            .items()
            .unwrap()
            .iter()
            .any(|i| i.card.ends_with("approved.txt.md")),
        "filed on the Mac, where the item is"
    );
    let log = History::open(&s.archive).unwrap().log().unwrap();
    assert!(
        log.iter()
            .any(|e| e.items.contains(&approved.id) || e.device == Some(s.phone.device)),
        "the filing is recorded"
    );
    assert_eq!(log[0].device, Some(s.phone.device), "as the phone's");
    let queue = ReviewQueue::of(&s.archive);
    assert_eq!(
        queue.find(&rejected.id.to_string()).unwrap().state,
        State::Rejected
    );

    // Holding the one already rejected no longer applies.
    s.send(Request::Approve {
        proposals: vec![rejected.id],
        holding: true,
        keep: None,
    });
    let applied = s.apply();
    assert_eq!(applied.skipped.len(), 1);
    assert_eq!(
        s.skipped(),
        ["the proposal was already decided elsewhere, or gone"]
    );
    assert!(
        inbox::waiting(&s.archive).is_empty(),
        "skipped entries leave"
    );
    let lines = inbox::status_lines(&s.archive, s.phone.device);
    assert!(
        lines[0].starts_with("Skipped “hold 1 proposal”"),
        "{lines:?}"
    );
}

#[test]
fn moving_and_choosing_for_a_proposal_change_only_the_queue() {
    let s = setup();
    let entry = s.proposal("print.stl");
    s.send(Request::MoveProposal {
        proposal: entry.id,
        collection: "Reading".into(),
    });
    s.apply();
    let moved = ReviewQueue::of(&s.archive)
        .find(&entry.id.to_string())
        .unwrap();
    assert_eq!(moved.destination.path, "Reading");
    s.send(Request::MoveProposal {
        proposal: entry.id,
        collection: "Nowhere".into(),
    });
    s.apply();
    assert_eq!(s.skipped().len(), 1, "a collection that isn't there");
}

#[test]
fn answers_teach_dismiss_and_skip_questions_answered_elsewhere() {
    let s = setup();
    let mut entries = vec![
        s.proposal("ABX-1 results.csv"),
        s.proposal("ABX summary.pdf"),
    ];
    let collections = s.archive.collections().unwrap();
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
    s.send(Request::Answer {
        question: term.id,
        answer: Answer::Teach {
            fact: "ABX is the assay kit.".into(),
        },
    });
    let applied = s.apply();
    assert_eq!(applied.entries, 1);
    assert_eq!(Glossary::of(&s.archive).facts[0].0, "ABX");
    assert!(queue.find(&term.id.to_string()).is_err());
    assert!(s.last_change().1, "the glossary change is the phone's");
    let requeued = carrel_core::intake::waiting(&s.archive, s.mac.device, false).unwrap();
    assert_eq!(
        requeued.len(),
        2,
        "drafted again on the Mac, where they are"
    );

    // Answered already: skipped.
    s.send(Request::Answer {
        question: term.id,
        answer: Answer::Dismiss,
    });
    s.apply();
    assert_eq!(
        s.skipped(),
        ["the question was answered elsewhere, or is gone"]
    );

    s.send(Request::Learn {
        about: "Meadow".into(),
        fact: "The meadow is behind the pond.".into(),
    });
    s.send(Request::AddRule {
        subject: "ABX".into(),
        collection: "Projects/Hardware".into(),
    });
    assert_eq!(s.apply().entries, 2);
    assert!(
        Glossary::of(&s.archive)
            .facts
            .iter()
            .any(|(about, _)| about == "Meadow")
    );
}

#[test]
fn card_edits_merge_and_an_edit_that_clashes_is_skipped() {
    let s = setup();
    let item = s.item(PARTS);
    let base = actions::card_fields(s.card(PARTS).card());
    let mut mine = base.clone();
    mine.title = "Lamp parts".into();
    s.send(Request::EditFields {
        item: item.id,
        base: Box::new(base.clone()),
        fields: Box::new(mine),
    });
    s.apply();
    assert_eq!(s.card(PARTS).card().title, "Lamp parts");
    let (summary, phones) = s.last_change();
    assert!(summary.starts_with("Edit “Lamp parts”"), "{summary}");
    assert!(phones);

    // Made against the card before that edit, changing the title to something else.
    let mut stale = base.clone();
    stale.title = "Parts for the lamp".into();
    s.send(Request::EditFields {
        item: item.id,
        base: Box::new(base),
        fields: Box::new(stale),
    });
    s.apply();
    assert_eq!(s.skipped(), ["title changed elsewhere meanwhile"]);
    assert_eq!(s.card(PARTS).card().title, "Lamp parts");

    let body = s.card(PARTS).body().to_owned();
    s.send(Request::EditBody {
        item: item.id,
        base: body.clone(),
        body: format!("{body}\nOrdered the diffuser.\n"),
    });
    s.apply();
    assert!(s.card(PARTS).body().contains("Ordered the diffuser."));
}

#[test]
fn corrections_worked_out_on_the_phone_are_saved_without_a_model() {
    let s = setup();
    let item = s.item(PARTS);
    let changes = Changes {
        title: Some(Field {
            value: "Lamp bill of materials".into(),
            dictated: true,
        }),
        ..Changes::default()
    };
    s.send(Request::Correct {
        corrects: Corrects::Filed(item.id),
        words: "it's the lamp's bill of materials".into(),
        changes: Box::new(changes.clone()),
        model: None,
        by: None,
        answering: None,
    });
    s.apply();
    assert_eq!(s.card(PARTS).card().title, "Lamp bill of materials");
    assert!(s.last_change().1);

    let entry = s.proposal("scan.pdf");
    s.send(Request::Correct {
        corrects: Corrects::Proposal(entry.id),
        words: "title: Lamp bill of materials".into(),
        changes: Box::new(changes),
        model: None,
        by: None,
        answering: None,
    });
    s.apply();
    assert_eq!(
        ReviewQueue::of(&s.archive)
            .find(&entry.id.to_string())
            .unwrap()
            .card
            .title,
        "Lamp bill of materials"
    );
}

#[test]
fn flags_expansions_and_undo_are_the_senders() {
    let s = setup();
    let item = s.item(PARTS);
    s.send(Request::Flag {
        target: FlagTarget::Item { id: item.id },
        note: Some("Check the prices".into()),
    });
    s.apply();
    let flag = s.card(PARTS).card().needs_work.clone().unwrap();
    assert_eq!(flag.note.as_deref(), Some("Check the prices"));
    assert!(s.last_change().1);
    let flagged = History::open(&s.archive).unwrap().log().unwrap()[0]
        .id
        .clone();

    s.send(Request::Unflag {
        target: FlagTarget::Collection {
            path: "Projects".into(),
        },
    });
    s.send(Request::Undo { change: flagged });
    s.send(Request::Undo {
        change: "0123456789abcdef0123456789abcdef01234567".into(),
    });
    s.send(Request::Expand {
        item: item.id,
        anyway: false,
    });
    let applied = s.apply();
    assert_eq!(applied.entries, 2);
    assert!(
        s.card(PARTS).card().needs_work.is_none(),
        "the flag is undone"
    );
    assert_eq!(
        s.skipped().len(),
        2,
        "an unknown change, and a CSV isn't compressed"
    );
}

#[test]
fn desk_changes_are_made_here_and_recorded_by_the_writer() {
    let s = setup();
    let work = s.dir.path().join("Work in progress");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(work.join("draft.md"), "A draft.\n").unwrap();
    let entry = desk::add_recording(
        &s.archive,
        &work,
        (Some("Work in progress"), Some("A draft.")),
        None,
        Recording::Inbox(s.phone),
    )
    .unwrap();
    assert!(
        desk::Desk::load(&s.archive, s.phone.device)
            .unwrap()
            .find(&entry.path)
            .is_some(),
        "the phone's desk is its own"
    );
    assert_eq!(inbox::waiting_from(&s.archive, s.phone.device).len(), 1);
    s.apply();
    let (summary, phones) = s.last_change();
    assert_eq!(summary, "Put “Work in progress” on the desk");
    assert!(phones);

    let (_, sent) = desk::archive_through_inbox(&s.archive, &work, s.phone).unwrap();
    assert!(sent.exists());
    let applied = s.apply();
    assert_eq!(applied.queued.len(), 1, "queued on the Mac");
    assert!(
        desk::carried_card(&applied.queued[0].extra).is_some(),
        "keeping its desk card"
    );
}

#[test]
fn additions_from_a_desk_and_a_url_are_recorded_as_the_senders() {
    let s = setup();
    inbox::send_url(&s.archive, s.phone, "https://example.org/b", None, None).unwrap();
    let applied = s.apply();
    assert_eq!(applied.queued.len(), 1);
    assert!(s.last_change().1);
}

/// Backups that always succeed.
struct Taken;

impl Backups for Taken {
    fn describe(&self) -> String {
        "taken".into()
    }
    fn snapshot(&self, _: &Path, description: &str) -> Result<Snapshot, BackupError> {
        Ok(Snapshot {
            id: "k1".into(),
            time: "2026-11-01T09:00:00Z".into(),
            description: Some(description.to_owned()),
            pinned: false,
            size: None,
            files: None,
        })
    }
    fn list(&self, _: &Path) -> Result<Vec<Snapshot>, BackupError> {
        Ok(Vec::new())
    }
    fn verify(&self, _: u8) -> Result<String, BackupError> {
        Ok(String::new())
    }
    fn restore(&self, _: &str, _: &str, _: &Path) -> Result<(), BackupError> {
        Ok(())
    }
    fn forget(&self, _: &str) -> Result<(), BackupError> {
        Ok(())
    }
}

#[test]
fn a_checkout_and_return_from_the_phone_keep_the_old_version_until_a_snapshot() {
    let s = setup();
    let item = s.item(PARTS);
    let archived = s.archive.root().join("Projects/Hardware/parts-list.csv");
    let before = std::fs::read(&archived).unwrap();
    let out = s.dir.path().join("Checked out");
    let (working, _) = inbox::send_checkout(&s.archive, s.phone, &item, &out, None).unwrap();
    assert!(working.exists(), "the copy is made on the phone");
    assert!(s.card(PARTS).card().checked_out.is_none());
    s.apply();
    let checked_out = s.card(PARTS).card().checked_out.clone().unwrap();
    assert_eq!(checked_out.device, s.phone.device, "out to the phone");
    assert_eq!(checked_out.to, working.display().to_string());

    // The same item can't come out twice.
    let elsewhere = s.dir.path().join("Elsewhere");
    assert!(inbox::send_checkout(&s.archive, s.mac, &item, &elsewhere, None).is_err());

    let _ = carrel_core::checkout::unlock(&working);
    std::fs::write(&working, "part,count\nLED strip,2\n").unwrap();
    inbox::send_return(&s.archive, s.phone, item.id, &working).unwrap();
    s.apply();
    assert!(s.card(PARTS).card().checked_out.is_none());
    assert_eq!(
        std::fs::read_to_string(&archived).unwrap(),
        "part,count\nLED strip,2\n"
    );
    let (summary, phones) = s.last_change();
    assert!(summary.starts_with("Return"), "{summary}");
    assert!(phones);

    // Without Kopia here, the version it replaced waits in the holding folder.
    let held = holding::list(&s.archive);
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].why, Why::Return);
    assert_eq!(held[0].item, Some(item.id));
    assert_eq!(held[0].path, "Projects/Hardware/parts-list.csv");
    let waiting = holding::waiting(&s.archive);
    assert_eq!(waiting.count, 1);
    assert_eq!(waiting.size, before.len() as u64);
    assert_eq!(waiting.warnings.len(), 0);

    let found = holding::find(&s.archive, &held[0].id.to_string()[..8]).unwrap();
    let to = holding::restore_beside(&s.archive, &found);
    holding::restore(&s.archive, &found, &to).unwrap();
    assert_eq!(std::fs::read(&to).unwrap(), before, "restored as it was");
    assert!(
        holding::restore(&s.archive, &found, &to).is_err(),
        "never over a file"
    );

    let cleared = holding::snapshot_and_clear(&s.archive, &Taken).unwrap();
    assert_eq!(cleared.cleared.len(), 1);
    assert!(cleared.snapshot.is_some());
    assert_eq!(holding::list(&s.archive).len(), 0);
    assert!(
        !History::tracked_paths(&s.archive)
            .unwrap()
            .iter()
            .any(|p| p.starts_with("_System/holding") || p.starts_with("_System/inbox/")),
        "history never tracks what passes through"
    );
}

#[test]
fn a_checkout_of_an_item_that_changed_meanwhile_is_skipped() {
    let s = setup();
    let item = s.item(PARTS);
    let (working, _) =
        inbox::send_checkout(&s.archive, s.phone, &item, &s.dir.path().join("out"), None).unwrap();
    std::fs::write(
        s.archive.root().join("Projects/Hardware/parts-list.csv"),
        "changed on the Mac\n",
    )
    .unwrap();
    s.apply();
    assert_eq!(s.skipped(), ["the item changed after the copy was taken"]);
    assert!(s.card(PARTS).card().checked_out.is_none());
    assert!(working.exists(), "the phone's copy is left to the owner");

    // Cancelling a checkout that isn't the phone's is skipped too.
    s.send(Request::CancelCheckout { item: item.id });
    s.apply();
    assert_eq!(s.skipped().len(), 2);
}

#[test]
fn old_or_large_holdings_warn() {
    let s = setup();
    let file = s.archive.root().join("Projects/Hardware/notes.md");
    let keeping = holding::Keeping {
        item: None,
        title: "Notes",
        why: Why::Expand,
        name: None,
    };
    let held = holding::keep(&s.archive, &file, &keeping, s.phone).unwrap();
    assert!(!file.exists());
    assert!(held.content(&s.archive).exists());
    let record = s
        .archive
        .root()
        .join(holding::HOLDING_DIR)
        .join(held.id.to_string())
        .join("held.json");
    let text = std::fs::read_to_string(&record)
        .unwrap()
        .replace(&held.at, "2026-01-01T00:00:00Z");
    std::fs::write(&record, text).unwrap();
    let waiting = holding::waiting(&s.archive);
    assert!(waiting.oldest_days.unwrap() >= holding::OLD_DAYS);
    assert_eq!(waiting.warnings.len(), 1, "{:?}", waiting.warnings);
}
