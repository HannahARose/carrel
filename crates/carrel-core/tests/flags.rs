//! Flagging items and collections that need work (FORMAT.md §5.9): the field on cards and
//! design documents, undo, Status, search, the crawler's order and the signals.

use std::path::Path;

use carrel_core::archive::{Archive, ItemRef};
use carrel_core::card::Id;
use carrel_core::card_file::CardFile;
use carrel_core::coordinator::{Coordinator, DEFAULT_LEASE, DiskLock, Holder};
use carrel_core::crawl::Stage;
use carrel_core::flags::{self, Flaggable};
use carrel_core::history::History;
use carrel_core::signals::Signal;

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

/// The sample archive with its history started, and the holder flags are set as.
fn setup() -> (tempfile::TempDir, Archive, Holder) {
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
    let lease = DiskLock::for_archive(&archive)
        .acquire(holder, "test", DEFAULT_LEASE)
        .unwrap();
    History::start(&archive, &lease).unwrap();
    lease.release().unwrap();
    (dir, archive, holder)
}

fn origin(archive: &Archive) -> ItemRef {
    archive
        .items()
        .unwrap()
        .into_iter()
        .find(|i| i.card == "Reading/On the Origin of Species.md")
        .unwrap()
}

fn card_text(archive: &Archive, item: &ItemRef) -> String {
    std::fs::read_to_string(archive.root().join(&item.card)).unwrap()
}

#[test]
fn flagging_an_item_writes_its_card_and_undo_clears_it() {
    let (_dir, archive, holder) = setup();
    let item = origin(&archive);
    let before = card_text(&archive, &item);
    let body = CardFile::parse(&before).unwrap().body().to_owned();

    let flagged = flags::flag(
        &archive,
        &Flaggable::item(&item),
        Some("wrong title"),
        holder,
    )
    .unwrap();
    assert!(flagged.changed);
    assert!(flagged.commit.is_some());
    let file = CardFile::parse(&card_text(&archive, &item)).unwrap();
    let flag = file.card().needs_work.clone().unwrap();
    assert_eq!(flag.note.as_deref(), Some("wrong title"));
    assert_eq!(flag.person, Some(holder.person));
    assert!(flag.at.is_some());
    assert_eq!(file.body(), body, "the owner's text is kept");
    assert_eq!(flags::of_item(&archive, &item), Some(flag));

    // The same note again changes nothing.
    let again = flags::flag(
        &archive,
        &Flaggable::item(&item),
        Some("wrong title"),
        holder,
    )
    .unwrap();
    assert!(!again.changed);

    let lease = DiskLock::for_archive(&archive)
        .acquire(holder, "test", DEFAULT_LEASE)
        .unwrap();
    let history = History::open(&archive).unwrap();
    let log = history.log().unwrap();
    assert!(
        log[0].summary.contains("needing work"),
        "{}",
        log[0].summary
    );
    history.undo(&archive, &log[0].id, &lease).unwrap();
    lease.release().unwrap();
    assert_eq!(card_text(&archive, &item), before);
}

#[test]
fn unflagging_clears_it_and_unflagging_again_does_nothing() {
    let (_dir, archive, holder) = setup();
    let item = origin(&archive);
    flags::flag(&archive, &Flaggable::item(&item), None, holder).unwrap();
    let cleared = flags::unflag(&archive, &Flaggable::item(&item), holder).unwrap();
    assert!(cleared.changed);
    assert_eq!(flags::of_item(&archive, &item), None);
    let again = flags::unflag(&archive, &Flaggable::item(&item), holder).unwrap();
    assert!(!again.changed);
    assert!(again.commit.is_none());
}

#[test]
fn flagging_a_collection_writes_its_design_document_and_feeds_the_signals() {
    let (_dir, archive, holder) = setup();
    let document = archive.root().join("Projects/_collection.md");
    let body_before = {
        let text = std::fs::read_to_string(&document).unwrap();
        carrel_core::frontmatter::split(&text)
            .unwrap()
            .body
            .to_owned()
    };
    flags::flag(
        &archive,
        &Flaggable::collection("Projects"),
        Some("split this folder"),
        holder,
    )
    .unwrap();
    let text = std::fs::read_to_string(&document).unwrap();
    assert!(text.contains("needs_work:"), "{text}");
    assert_eq!(
        carrel_core::frontmatter::split(&text).unwrap().body,
        body_before
    );

    let all = flags::all(&archive).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].what, Flaggable::collection("Projects"));
    assert_eq!(all[0].flag.note.as_deref(), Some("split this folder"));

    let signals = carrel_core::signals::signals(&archive).unwrap();
    assert_eq!(
        signals[0],
        Signal::Flagged {
            collection: "Projects".into(),
            note: Some("split this folder".into()),
        },
        "the owner's flag comes first"
    );

    // The crawler takes the flagged collection's items first.
    let entries = carrel_core::crawl::order::entries(&archive).unwrap();
    let order = carrel_core::crawl::order::order_of(
        &entries,
        &std::collections::HashSet::new(),
        &std::collections::BTreeMap::new(),
        &std::collections::HashSet::new(),
        jiff::Timestamp::now(),
    );
    let first = entries.iter().find(|e| e.id == order[0].0).unwrap();
    assert_eq!(order[0].1, Stage::Flagged);
    assert!(first.collection.starts_with("Projects"), "{first:?}");
}

#[test]
fn status_and_search_list_flagged_items() {
    let (dir, archive, holder) = setup();
    let item = origin(&archive);
    flags::flag(
        &archive,
        &Flaggable::item(&item),
        Some("wrong title"),
        holder,
    )
    .unwrap();

    let looking = carrel_core::status::Looking::in_home(None, Some(dir.path().join("home")));
    let summary = carrel_core::status::summary(&archive, &looking).unwrap();
    assert_eq!(summary.flagged.len(), 1);
    assert!(
        summary.flagged[0]
            .title
            .starts_with("On the Origin of Species")
    );

    let search = carrel_core::search::Search::build(&archive).unwrap();
    let query = carrel_core::search::query::Query::parse("flagged").unwrap();
    let results = search.search(&query, 10);
    assert_eq!(results.total, 1);
    let carrel_core::search::Found::Item(found) = &results.hits[0].found else {
        panic!("an item")
    };
    assert_eq!(found.id, item.id);
}

#[test]
fn a_hand_written_flag_is_read_and_kept_in_its_short_form() {
    let (_dir, archive, _holder) = setup();
    let item = origin(&archive);
    let text = card_text(&archive, &item).replacen(
        "\nsensitivity:",
        "\nneeds_work: \"check the edition\"\nsensitivity:",
        1,
    );
    let mut file = CardFile::parse(&text).unwrap();
    assert_eq!(
        file.card().needs_work.as_ref().unwrap().note.as_deref(),
        Some("check the edition")
    );
    file.card_mut().title.push_str(" (1859)");
    let written = file.to_text().unwrap();
    assert!(
        written.contains("needs_work: check the edition")
            || written.contains("needs_work: \"check the edition\""),
        "{written}"
    );

    let off = card_text(&archive, &item).replacen(
        "\nsensitivity:",
        "\nneeds_work: false\nsensitivity:",
        1,
    );
    assert_eq!(CardFile::parse(&off).unwrap().card().needs_work, None);
    let on = card_text(&archive, &item).replacen(
        "\nsensitivity:",
        "\nneeds_work: true\nsensitivity:",
        1,
    );
    assert!(CardFile::parse(&on).unwrap().card().needs_work.is_some());
}

#[test]
fn a_checked_out_item_is_flagged_when_it_is_returned() {
    let (_dir, archive, holder) = setup();
    let item = origin(&archive);
    let path = archive.root().join(&item.card);
    let text = std::fs::read_to_string(&path).unwrap().replacen(
        "\nsensitivity:",
        &format!(
            "\nchecked_out:\n  to: \"~/Desktop/x\"\n  device: {}\n  person: {}\n  since: \"2026-11-01\"\nsensitivity:",
            holder.device, holder.person
        ),
        1,
    );
    std::fs::write(&path, text).unwrap();
    let error = flags::flag(&archive, &Flaggable::item(&item), None, holder).unwrap_err();
    assert!(error.to_string().contains("checked out"), "{error}");
}
