//! The decision log and `carrel report` hold no contents (#330): real flows on the sample
//! archive (ingest, review, a correction, questions, search, Status), then a check that no
//! word of any card's title, summary or text, of any file's name, of any question's subject,
//! or of any search's query, is in the log or the report.
//!
//! One test, as the log is the process's: it starts logging to a temporary configuration
//! folder, never the owner's.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use carrel_core::actions;
use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
use carrel_core::decisions::{self, Event};
use carrel_core::history::History;
use carrel_core::ingest::IntakeOptions;
use carrel_core::ingest::card::Glossary;
use carrel_core::ingest::filing::Placement;
use carrel_core::local::Prefs;
use carrel_core::providers::Registry;
use carrel_core::questions::QuestionQueue;
use carrel_core::review::{REVIEW_DIR, ReviewQueue};

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

/// Every word of four letters or more in `text`, lowercased.
fn words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphabetic())
        .filter(|w| w.chars().count() >= 4)
        .map(str::to_lowercase)
        .collect()
}

/// `text` without its identifiers, whose hexadecimal digits can spell a word by chance.
fn without_ids(text: &str) -> String {
    let shape = [8, 4, 4, 4, 12];
    let mut out = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        let id = rest.get(..36).is_some_and(|candidate| {
            let groups: Vec<&str> = candidate.split('-').collect();
            groups.len() == 5
                && groups
                    .iter()
                    .zip(shape)
                    .all(|(g, n)| g.len() == n && g.chars().all(|c| c.is_ascii_hexdigit()))
        });
        if id {
            out.push(' ');
            rest = &rest[36..];
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// Every word Carrel itself writes in the log and the report: its sources' own words.
fn carrels_words() -> BTreeSet<String> {
    let mut known = BTreeSet::new();
    for source in [
        include_str!("../src/decisions.rs"),
        include_str!("../src/decisions/record.rs"),
        include_str!("../src/decisions/report.rs"),
        include_str!("../src/decisions/advice.rs"),
        include_str!("../src/decisions/hooks.rs"),
        include_str!("../src/formats.rs"),
    ] {
        known.extend(words(source));
    }
    known
}

/// The words of every card and proposal in `archive`, and of the files waiting in it.
fn archive_words(archive: &Archive) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for item in archive.items().unwrap() {
        let text = std::fs::read_to_string(archive.root().join(&item.card)).unwrap();
        found.extend(words(&text));
        if let Some(path) = &item.item {
            found.extend(words(path));
        }
    }
    let (entries, _) = ReviewQueue::of(archive).list().unwrap();
    for entry in entries {
        found.extend(words(&entry.card.title));
        found.extend(words(entry.card.summary.as_deref().unwrap_or_default()));
        found.extend(words(&entry.source.path.to_string_lossy()));
    }
    found
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
        std::fs::create_dir(dir.path().join("incoming")).unwrap();
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
        Self {
            dir,
            archive,
            holder,
        }
    }

    fn config(&self) -> PathBuf {
        self.dir.path().join("config")
    }

    /// Ingests a file named `name` holding `text` through the intake queue, as the worker
    /// does, and returns its proposal's identifier.
    fn ingest(&self, name: &str, text: &str) -> Id {
        let path = self.dir.path().join("incoming").join(name);
        std::fs::write(&path, text).unwrap();
        carrel_core::intake::add(
            &self.archive,
            &[path],
            IntakeOptions::default(),
            self.holder.device,
        )
        .unwrap();
        let entry = carrel_core::intake::waiting(&self.archive, self.holder.device, false)
            .unwrap()
            .remove(0);
        let processed = carrel_core::intake::process(
            &self.archive,
            &Registry::with_built_ins(),
            entry,
            self.holder,
            None,
            &mut |_| {},
        )
        .unwrap();
        processed.run.unwrap().entries[0].as_ref().unwrap().id
    }

    fn proposal(&self, id: Id) -> Vec<carrel_core::review::ReviewEntry> {
        actions::proposals(&self.archive, &[id.to_string()]).unwrap()
    }
}

/// Reviews, corrects, asks and searches in `s.archive`, as an owner would. Returns the
/// words searched for and said in a correction.
fn use_it(s: &Setup) -> BTreeSet<String> {
    let mut said = BTreeSet::new();
    let lamp = s.ingest(
        "Quorvel lamp notes.txt",
        "Notes on the brass Quorvel lamp\n",
    );
    let deed = s.ingest("Marrowfield deed.txt", "The Marrowfield deed, signed\n");
    let menu = s.ingest("Tisbury menu.txt", "Tisbury supper menu\n");
    let scan = s.ingest("Halvorsen letter.txt", "Dear Halvorsen, about the garden\n");

    actions::decide(
        &s.archive,
        s.proposal(lamp),
        Placement::Destination,
        s.holder,
        None,
    )
    .unwrap();
    actions::move_to(&s.archive, &deed.to_string(), "Reading").unwrap();
    actions::decide(
        &s.archive,
        s.proposal(deed),
        Placement::Destination,
        s.holder,
        None,
    )
    .unwrap();
    actions::decide(
        &s.archive,
        s.proposal(menu),
        Placement::Holding,
        s.holder,
        None,
    )
    .unwrap();
    actions::reject(&s.archive, s.proposal(scan)).unwrap();

    // A correction of a filed item, saved.
    let item = s
        .archive
        .items()
        .unwrap()
        .into_iter()
        .find(|i| i.item.is_some())
        .unwrap();
    let words_said = "title: Zanzibar ledger of Pemberton";
    said.extend(words(words_said));
    let prepared = carrel_core::correct::prepare::prepare_filed_lines(
        &s.archive,
        &Registry::with_built_ins(),
        item,
        words_said,
        "test/m",
    )
    .unwrap();
    let holder = s.holder;
    carrel_core::correct::prepare::save(&s.archive, prepared, || {
        Ok::<_, carrel_core::correct::prepare::PrepareError>(holder)
    })
    .unwrap();

    questions(s);

    // A search for words in the cards, and its first result opened.
    let typed = "brass quorvel lamp";
    said.extend(words(typed));
    let query = carrel_core::search::query::Query::parse(typed).unwrap();
    let found = carrel_core::search::run::search(
        &s.archive,
        &query,
        10,
        Registry::with_built_ins,
        &mut |_, _, _| {},
    )
    .unwrap();
    let first = found.results.hits.iter().find_map(|h| match &h.found {
        carrel_core::search::Found::Item(item) => Some(item.id),
        _ => None,
    });
    decisions::opened(&s.archive, first.expect("the lamp is found"));

    carrel_core::status::summary(
        &s.archive,
        &carrel_core::status::Looking::in_home(None, Some(s.dir.path().to_owned())),
    )
    .unwrap();
    said
}

/// Asks a question about an unknown term, skips it and answers it, and dismisses another.
fn questions(s: &Setup) {
    let one = s.ingest("WEXLO-1 results.csv", "WEXLO,1\n");
    let two = s.ingest("WEXLO summary.csv", "WEXLO,2\n");
    let mut entries: Vec<_> = [one, two].into_iter().flat_map(|i| s.proposal(i)).collect();
    for entry in &mut entries {
        entry.destination.confidence = carrel_core::review::Confidence::Low;
    }
    let (mut asked, _) = QuestionQueue::of(&s.archive).list().unwrap();
    if asked.is_empty() {
        asked = carrel_core::ingest::questions::ask(
            &s.archive,
            &s.archive.collections().unwrap(),
            &Glossary::of(&s.archive),
            &entries,
        );
    }
    for entry in &mut entries {
        carrel_core::questions::mark_waiting(entry, &asked);
        ReviewQueue::of(&s.archive).save(entry).unwrap();
    }
    let term = asked
        .first()
        .expect("an unknown term is asked about")
        .clone();
    QuestionQueue::of(&s.archive).save(&term).unwrap();
    let skipped = carrel_core::questions::answer::skip(&s.archive, &term).unwrap();
    actions::teach(&s.archive, &skipped, "Wexlo is the assay kit.", s.holder).unwrap();
    let mut other = term.clone();
    other.id = Id::new_v7();
    QuestionQueue::of(&s.archive).save(&other).unwrap();
    actions::dismiss(&s.archive, &other).unwrap();
}

#[test]
fn the_log_and_the_report_hold_no_titles_text_names_or_queries() {
    let s = Setup::new();
    decisions::start_in(&s.config(), &Prefs::default());
    let said = use_it(&s);
    decisions::stop();

    let folder = decisions::folder_in(&s.config());
    let mut written = String::new();
    for entry in std::fs::read_dir(&folder).unwrap() {
        written.push_str(&std::fs::read_to_string(entry.unwrap().path()).unwrap());
        written.push('\n');
    }
    let since = jiff::Timestamp::now() - jiff::SignedDuration::from_hours(1);
    let report = decisions::report_for(&s.archive, &s.config(), since);
    written.push_str(&serde_json::to_string_pretty(&report).unwrap());
    written.push_str(&report.text(None));

    // Everything was recorded.
    let records = decisions::read(&folder, since);
    let has = |what: fn(&Event) -> bool| records.iter().any(|r| what(&r.event));
    assert!(has(|e| matches!(e, Event::Drafted { .. })), "{records:#?}");
    assert!(has(|e| matches!(e, Event::Decided { .. })));
    assert!(has(|e| matches!(e, Event::Moved { .. })));
    assert!(has(|e| matches!(e, Event::Corrected { .. })));
    assert!(has(|e| matches!(e, Event::Question { .. })));
    assert!(has(|e| matches!(e, Event::Searched { .. })));
    assert!(has(|e| matches!(e, Event::Opened { rank: 1, .. })));
    assert!(has(|e| matches!(e, Event::Snapshot { .. })));
    assert_eq!(report.search[0].opened, 1, "{report:#?}");

    // And nothing of what's in the archive, or what was typed.
    let carrels = carrels_words();
    let mut content = archive_words(&s.archive);
    content.extend(said);
    for word in [
        "quorvel",
        "marrowfield",
        "tisbury",
        "halvorsen",
        "wexlo",
        "zanzibar",
    ] {
        assert!(content.contains(word), "the check covers {word}");
    }
    let leaked: Vec<&String> = words(&without_ids(&written))
        .iter()
        .filter(|w| content.contains(*w) && !carrels.contains(*w))
        .map(|w| content.get(w).unwrap())
        .collect();
    assert!(leaked.is_empty(), "leaked {leaked:?} in:\n{written}");

    // Turned off, it writes nothing.
    let off = s.dir.path().join("off");
    let prefs = Prefs {
        report_log: Some(false),
        ..Prefs::default()
    };
    decisions::start_in(&off, &prefs);
    assert_eq!(decisions::logging_to(), None);
    s.ingest("Ostrander notes.txt", "more\n");
    assert!(!decisions::folder_in(&off).exists());
}
