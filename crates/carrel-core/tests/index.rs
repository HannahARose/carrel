//! The embedding index (`docs/design/search.md`): what's embedded for each item, the files it's
//! kept in, finding what's behind, and search by meaning merged with search by words.

mod common;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use carrel_core::archive::Archive;
use carrel_core::card::{Checksum, Id};
use carrel_core::index::{
    ByMeaning, INDEX, Index, IndexError, Neighbours, Part, chunk, embed_query, plan,
};
use carrel_core::providers::{EmbeddingRequest, ProviderError, Purpose, Registry, priority};
use carrel_core::search::query::Query;
use carrel_core::search::texts::{fill, tidy};
use carrel_core::search::{Found, Meaning, Search, Why};
use common::ScriptedEmbed;

/// The sample archive's embedding model.
const MODEL: &str = "sample/embedding-model-v1";

fn sample_archive() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive")
}

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

/// Files `name` in Holding with `contents`, and a card with `fields` beside it.
fn file(root: &Path, name: &str, contents: &str, fields: &str) -> Id {
    let path = root.join("Holding").join(name);
    std::fs::write(&path, contents).unwrap();
    let checksum = Checksum::of_file(&path).unwrap();
    let id = Id::new_v7();
    let card = format!(
        "---\ncarrel: card\nformat: \"1.0\"\nid: {id}\nkind: file\nitem: \"{name}\"\n{fields}\
         checksum: \"{}\"\nsize: {}\n---\n",
        checksum.0,
        contents.len()
    );
    std::fs::write(root.join("Holding").join(format!("{name}.md")), card).unwrap();
    id
}

struct Setup {
    _dir: tempfile::TempDir,
    archive: Archive,
    lease: Id,
    statement: Id,
}

/// The sample archive with a long lease and a restricted bank statement filed in Holding, and
/// every item's text read into the cache.
fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("archive");
    copy_dir(&sample_archive(), &root);
    let mut lease = String::from("Tenancy agreement for the flat on Cowley Road, Oxford.\n\n");
    for n in 0..60 {
        write!(
            lease,
            "Clause {n}. The tenant keeps the garden tidy and pays the rent monthly. "
        )
        .unwrap();
    }
    lease.push_str("\n\n");
    lease.push_str(&"The landlord repairs the boiler and the radiators. ".repeat(20));
    let lease = file(
        &root,
        "agreement.txt",
        &lease,
        "title: \"Tenancy agreement\"\nsummary: \"A year's lease.\"\nsensitivity: personal\n",
    );
    let statement = file(
        &root,
        "statement.txt",
        "Account 4111 1111 1111 1111, the boiler repair paid.",
        "title: \"Bank statement\"\nsensitivity: restricted\n",
    );
    let archive = Archive::open(&root).unwrap();
    let (wanted, _) = tidy(&archive).unwrap();
    fill(
        &archive,
        &Registry::with_built_ins(),
        wanted,
        &mut |_, _, _| {},
    )
    .unwrap();
    Setup {
        _dir: dir,
        archive,
        lease,
        statement,
    }
}

/// What an embedding model was asked: the model and the request.
type Asked = Arc<Mutex<Vec<(String, EmbeddingRequest)>>>;

/// A registry with the built-in readers and an embedding model counting words.
fn registry(model: &str) -> (Registry, Asked) {
    let mut embed = ScriptedEmbed::new("words", model, priority::PORTABLE, 512);
    embed.by_words = true;
    let asked = embed.asked.clone();
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(embed));
    (registry, asked)
}

fn build(archive: &Archive, registry: &Registry) -> Index {
    let mut index = Index::open_or_new(archive, MODEL).unwrap();
    let plan = plan(archive, Some(&index)).unwrap();
    index
        .embed(archive, registry, plan, &mut |_, _, _| {})
        .unwrap();
    index
}

#[test]
fn cards_and_passages_are_embedded_and_kept_in_files() {
    let s = setup();
    let (registry, asked) = registry(MODEL);
    assert!(Index::open(&s.archive, MODEL).unwrap().is_none());
    let before = plan(&s.archive, None).unwrap();
    assert_eq!(before.current(), 0);
    let items = before.items;
    assert!(items > 5);

    let mut seen = Vec::new();
    let mut index = Index::open_or_new(&s.archive, MODEL).unwrap();
    index
        .embed(&s.archive, &registry, before, &mut |done, total, card| {
            seen.push((done, total, card.map(str::to_owned)));
        })
        .unwrap();
    assert_eq!(seen.last().unwrap(), &(items, items, None));
    assert_eq!(index.len(), items);
    assert_eq!(index.dimensions(), 512);
    assert!(
        asked
            .lock()
            .unwrap()
            .iter()
            .all(|(m, r)| m == MODEL && r.purpose == Purpose::Document)
    );

    // The lease's card, then its text in passages of about 500 tokens.
    let lease = index.get(s.lease).unwrap();
    assert_eq!(lease.parts[0], Part::Card);
    assert!(lease.parts.len() > 2, "{:?}", lease.parts);
    assert!(lease.text.is_some());
    // The restricted statement's card only.
    let statement = index.get(s.statement).unwrap();
    assert_eq!(statement.parts, [Part::Card]);
    assert_eq!(statement.text, None);

    // Plain files, one folder per model, that read back the same.
    let dir = s
        .archive
        .root()
        .join(INDEX)
        .join("sample_embedding-model-v1");
    assert_eq!(dir, Index::dir_of(&s.archive, MODEL));
    let head: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("index.json")).unwrap()).unwrap();
    assert_eq!(head["model"], MODEL);
    assert_eq!(head["dimensions"], 512);
    assert_eq!(head["format"], 1);
    let shards = std::fs::read_dir(&dir).unwrap().count() - 1;
    assert!(shards >= 1 && shards <= items, "{shards}");
    let again = Index::open(&s.archive, MODEL).unwrap().unwrap();
    assert_eq!(again.len(), items);
    assert_eq!(again.vectors(), index.vectors());
    assert_eq!(again.get(s.lease), index.get(s.lease));
    assert_eq!(plan(&s.archive, Some(&again)).unwrap().pending, []);

    // Another model's index is another folder.
    assert!(Index::open(&s.archive, "other-model").unwrap().is_none());

    // It's regenerable, so backups leave it out.
    assert!(
        carrel_core::backup::ignore_patterns(&s.archive).contains(&format!("/{INDEX}/")),
        "left out of backups"
    );
}

#[test]
fn changes_put_items_behind_until_embedded_again() {
    let s = setup();
    let (registry, _) = registry(MODEL);
    let mut index = build(&s.archive, &registry);
    let card = s.archive.root().join("Holding/agreement.txt.md");
    let text = std::fs::read_to_string(&card).unwrap();

    // A new title, or a refile to another collection, changes the card's text.
    std::fs::write(&card, text.replace("Tenancy agreement", "Flat lease")).unwrap();
    let behind = plan(&s.archive, Some(&index)).unwrap();
    assert_eq!(behind.pending.len(), 1);
    assert_eq!(behind.pending[0].id, s.lease);
    assert!(behind.pending[0].card_text.starts_with("Flat lease\n"));
    index
        .embed(&s.archive, &registry, behind, &mut |_, _, _| {})
        .unwrap();
    assert_eq!(plan(&s.archive, Some(&index)).unwrap().pending, []);

    // Restricting an item takes its passages out of the index.
    std::fs::write(
        &card,
        std::fs::read_to_string(&card)
            .unwrap()
            .replace("sensitivity: personal", "sensitivity: restricted"),
    )
    .unwrap();
    let behind = plan(&s.archive, Some(&index)).unwrap();
    assert_eq!(behind.pending.len(), 1);
    assert_eq!(behind.pending[0].contents, None);
    index
        .embed(&s.archive, &registry, behind, &mut |_, _, _| {})
        .unwrap();
    assert_eq!(index.get(s.lease).unwrap().parts, [Part::Card]);

    // An item that's gone is taken out.
    std::fs::remove_file(&card).unwrap();
    let behind = plan(&s.archive, Some(&index)).unwrap();
    assert_eq!(behind.gone, [s.lease]);
    let before = index.len();
    index
        .embed(&s.archive, &registry, behind, &mut |_, _, _| {})
        .unwrap();
    assert_eq!(index.len(), before - 1);
    assert!(index.get(s.lease).is_none());
}

#[test]
fn only_the_archives_model_embeds_and_a_failure_keeps_what_was_done() {
    let s = setup();
    let (other, _) = registry("another-model");
    let mut index = Index::open_or_new(&s.archive, MODEL).unwrap();
    let pending = plan(&s.archive, None).unwrap();
    let error = index
        .embed(&s.archive, &other, pending, &mut |_, _, _| {})
        .unwrap_err();
    assert!(
        matches!(error, IndexError::Provider(ProviderError::Unavailable(_))),
        "{error}"
    );
    assert!(error.to_string().contains(MODEL), "{error}");

    // A rebuild that stops part way is carried on, not started again.
    let (registry, _) = registry(MODEL);
    let mut index = build(&s.archive, &registry);
    index.start_rebuild();
    let mut failing = ScriptedEmbed::new("words", MODEL, priority::PORTABLE, 512);
    failing.failure = Some(ProviderError::Failed("out of memory".into()));
    let mut broken = Registry::with_built_ins();
    broken.add(Box::new(failing));
    let pending = plan(&s.archive, Some(&index)).unwrap();
    assert!(
        index
            .embed(&s.archive, &broken, pending, &mut |_, _, _| {})
            .is_err()
    );
    let reopened = Index::open(&s.archive, MODEL).unwrap().unwrap();
    assert!(reopened.rebuilding());
    assert!(reopened.is_empty());

    // Vectors of another length can't join the index.
    let mut wide = ScriptedEmbed::new("wide", MODEL, priority::PORTABLE + 1, 32);
    wide.by_words = true;
    let mut registry = Registry::with_built_ins();
    registry.add(Box::new(wide));
    let mut index = build(&s.archive, &self::registry(MODEL).0);
    let card = s.archive.root().join("Holding/agreement.txt.md");
    let text = std::fs::read_to_string(&card).unwrap();
    std::fs::write(&card, text.replace("Tenancy", "Rental")).unwrap();
    let pending = plan(&s.archive, Some(&index)).unwrap();
    let error = index
        .embed(&s.archive, &registry, pending, &mut |_, _, _| {})
        .unwrap_err();
    assert!(
        matches!(
            error,
            IndexError::Dimensions {
                index: 512,
                model: 32
            }
        ),
        "{error}"
    );
}

#[test]
fn a_damaged_shard_is_embedded_again() {
    let s = setup();
    let (registry, _) = registry(MODEL);
    let index = build(&s.archive, &registry);
    let dir = Index::dir_of(&s.archive, MODEL);
    let shard = format!("{:02x}.shard", s.lease.0.as_bytes()[15]);
    let path = dir.join(shard);
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.truncate(bytes.len() - 3);
    std::fs::write(&path, bytes).unwrap();
    let reopened = Index::open(&s.archive, MODEL).unwrap().unwrap();
    assert!(reopened.len() < index.len());
    let behind = plan(&s.archive, Some(&reopened)).unwrap();
    assert!(behind.pending.iter().any(|p| p.id == s.lease));
}

#[test]
fn the_nearest_items_come_with_their_best_passage() {
    let s = setup();
    let (registry, _) = registry(MODEL);
    let index = build(&s.archive, &registry);
    let query = embed_query(&registry, MODEL, "landlord repairs boiler radiators").unwrap();
    let near = index.nearest(&query, &|_| true, 3);
    assert_eq!(near[0].id, s.lease);
    assert!(matches!(near[0].part, Part::Passage { .. }), "{near:?}");
    assert!(near[0].score > near[1].score);
    let without = index.nearest(&query, &|id| id != s.lease, 3);
    assert!(without.iter().all(|n| n.id != s.lease));

    // The restricted statement mentions the boiler too, but only its card is in the index.
    let near = index.nearest(&query, &|_| true, 50);
    let statement = near.iter().find(|n| n.id == s.statement).unwrap();
    assert_eq!(statement.part, Part::Card);
}

#[test]
fn search_merges_meaning_with_words_and_says_when_it_is_behind() {
    let s = setup();
    let (registry, _) = registry(MODEL);
    let index = build(&s.archive, &registry);
    let registry = std::rc::Rc::new(registry);
    let embedder = {
        let registry = registry.clone();
        Box::new(move |q: &str| embed_query(&registry, MODEL, q).ok())
    };
    let meaning = ByMeaning::new(&s.archive, Box::new(index), 0, embedder);
    let search = Search::build(&s.archive)
        .unwrap()
        .with_meaning(Box::new(meaning));
    let results = search.search(&Query::parse("radiators boiler repairs").unwrap(), 5);
    assert_eq!(results.meaning, Meaning::Current);
    let first = &results.hits[0];
    let Found::Item(item) = &first.found else {
        panic!("{first:?}")
    };
    assert_eq!(item.id, s.lease);
    assert!(first.why.contains(&Why::Meaning), "{:?}", first.why);
    assert!(first.why.contains(&Why::Text), "{:?}", first.why);
    assert!(
        first
            .passage
            .as_ref()
            .unwrap()
            .marked("[", "]")
            .contains("[boiler]")
    );

    // Filters narrow what meaning finds too.
    let results = search.search(&Query::parse("radiators boiler kind:folder").unwrap(), 5);
    assert!(results.hits.iter().all(|h| match &h.found {
        Found::Item(item) => item.id != s.lease,
        _ => true,
    }));

    // An index that's behind still ranks what it has, and says so.
    let index = Index::open(&s.archive, MODEL).unwrap().unwrap();
    let meaning = ByMeaning::new(&s.archive, Box::new(index), 2, Box::new(|_: &str| None));
    let search = Search::build(&s.archive)
        .unwrap()
        .with_meaning(Box::new(meaning));
    let results = search.search(&Query::parse("boiler").unwrap(), 5);
    assert_eq!(results.meaning, Meaning::Behind { items: 2 });
    assert!(!results.hits.is_empty(), "words still find it");
}

#[test]
fn a_card_is_one_text_of_what_it_says_the_item_is() {
    let s = setup();
    let text = std::fs::read_to_string(s.archive.root().join("Holding/agreement.txt.md")).unwrap();
    let file = carrel_core::card_file::CardFile::parse(&text).unwrap();
    assert_eq!(
        chunk::card_text(file.card(), "Holding"),
        "Tenancy agreement\nA year's lease.\nCollection: Holding\nKind: file"
    );
}

#[test]
fn switching_models_builds_the_new_index_beside_the_old_then_switches() {
    use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
    use carrel_core::history::History;
    use carrel_core::index::switch::{self, Switch, SwitchError};

    const NEW: &str = "another/model:v2";
    let s = setup();
    let lease = DiskLock::for_archive(&s.archive)
        .acquire(
            Holder {
                person: Id::new_v7(),
                device: Id::new_v7(),
            },
            "test",
            DEFAULT_LEASE,
        )
        .unwrap();
    History::start(&s.archive, &lease).unwrap();
    let old = build(&s.archive, &registry(MODEL).0);
    let old_dir = Index::dir_of(&s.archive, MODEL);
    assert!(switch::pending(&s.archive).is_none());

    // The switch is recorded in the settings every device reads, and its index is built
    // in a folder of its own; a failure part way keeps what was done.
    let (mut index, begun) = switch::begin(&s.archive, NEW, &lease).unwrap();
    assert!(!begun.resumed);
    assert!(begun.commit.is_some());
    let archive = Archive::open(s.archive.root()).unwrap();
    assert_eq!(
        switch::pending(&archive),
        Some(Switch {
            from: Some(MODEL.into()),
            to: NEW.into(),
        })
    );
    assert_eq!(archive.settings().embedding_model(), Some(MODEL));
    let new_dir = Index::dir_of(&archive, NEW);
    assert_eq!(new_dir, archive.root().join(INDEX).join("another_model_v2"));
    let mut failing = ScriptedEmbed::new("words", NEW, priority::PORTABLE, 64);
    failing.failure = Some(ProviderError::Failed("interrupted".into()));
    let mut broken = Registry::with_built_ins();
    broken.add(Box::new(failing));
    let pending = plan(&archive, Some(&index)).unwrap();
    assert!(
        index
            .embed(&archive, &broken, pending, &mut |_, _, _| {})
            .is_err()
    );
    assert!(matches!(
        switch::finish(&archive, NEW, &lease),
        Err(SwitchError::Incomplete(_))
    ));
    // The old index is untouched meanwhile.
    let serving = Index::open(&archive, MODEL).unwrap().unwrap();
    assert_eq!(serving.len(), old.len());

    // Started again, it carries on, then switches and removes the old index.
    let (mut index, begun) = switch::begin(&archive, NEW, &lease).unwrap();
    assert!(begun.resumed);
    assert!(begun.commit.is_none());
    let (new_registry, _) = registry(NEW);
    let pending = plan(&archive, Some(&index)).unwrap();
    index
        .embed(&archive, &new_registry, pending, &mut |_, _, _| {})
        .unwrap();
    let finished = switch::finish(&archive, NEW, &lease).unwrap();
    assert_eq!(finished.from.as_deref(), Some(MODEL));
    assert!(finished.commit.is_some());
    let archive = Archive::open(s.archive.root()).unwrap();
    assert_eq!(archive.settings().embedding_model(), Some(NEW));
    assert!(switch::pending(&archive).is_none());
    assert!(!old_dir.exists());
    let current = Index::open(&archive, NEW).unwrap().unwrap();
    assert!(!current.rebuilding());
    assert_eq!(plan(&archive, Some(&current)).unwrap().pending, Vec::new());

    // Switching to a third model, then to a fourth, gives up the third's index; stopping
    // removes the fourth's and keeps the archive's.
    switch::begin(&archive, MODEL, &lease).unwrap();
    assert!(Index::dir_of(&archive, MODEL).exists());
    let archive = Archive::open(s.archive.root()).unwrap();
    let (_, begun) = switch::begin(&archive, "fourth", &lease).unwrap();
    assert_eq!(begun.replaced.as_deref(), Some(MODEL));
    assert!(!Index::dir_of(&archive, MODEL).exists());
    let archive = Archive::open(s.archive.root()).unwrap();
    assert!(matches!(
        switch::finish(&archive, MODEL, &lease),
        Err(SwitchError::NotSwitching { now: Some(_), .. })
    ));
    assert_eq!(
        switch::cancel(&archive, &lease).unwrap().as_deref(),
        Some("fourth")
    );
    let archive = Archive::open(s.archive.root()).unwrap();
    assert!(switch::pending(&archive).is_none());
    assert!(!Index::dir_of(&archive, "fourth").exists());
    assert!(new_dir.exists());
    assert_eq!(archive.settings().embedding_model(), Some(NEW));
    lease.release().unwrap();
}
