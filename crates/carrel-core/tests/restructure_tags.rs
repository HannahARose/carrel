//! Tag restructuring (`docs/design/restructure.md`): proposals to merge, split and retire
//! tags, approved all or in part as one change that undo reverses, against temporary archives
//! with fake embeddings.

use std::collections::BTreeMap;
use std::path::Path;

use carrel_core::archive::{Archive, InitOptions};
use carrel_core::card::{Id, Tag};
use carrel_core::card_file::CardFile;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
use carrel_core::documents::ArchiveDoc;
use carrel_core::history::{History, Operation};
use carrel_core::restructure::tags::{TagOptions, propose};
use carrel_core::restructure::{
    self, Edit, Proposal, ProposalKind, ProposalQueue, ProposedChange, RestructureError,
};

/// A card in `collection`, its tags written by a model unless `owner` wrote them.
struct Item {
    collection: &'static str,
    name: &'static str,
    tags: &'static [&'static str],
    owner: bool,
    vector: [f32; 4],
}

const KITCHEN: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
const COMPUTERS: [f32; 4] = [0.0, 1.0, 0.0, 0.0];
const INVOICES: [f32; 4] = [0.0, 0.0, 1.0, 0.0];
const RECEIPTS: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

fn item(
    collection: &'static str,
    name: &'static str,
    tags: &'static [&'static str],
    vector: [f32; 4],
) -> Item {
    Item {
        collection,
        name,
        tags,
        owner: false,
        vector,
    }
}

fn items() -> Vec<Item> {
    vec![
        item("Finances", "r1", &["receipts"], RECEIPTS),
        item("Finances", "r2", &["receipts", "tax"], RECEIPTS),
        item("Finances", "r3", &["Receipt"], RECEIPTS),
        Item {
            owner: true,
            ..item("Finances", "r4", &["Receipt", "mine"], RECEIPTS)
        },
        item("Finances", "i1", &["invoice"], INVOICES),
        item("Finances", "i2", &["invoice"], [0.0, 0.0, 0.98, 0.05]),
        item("Finances", "b1", &["bill"], [0.0, 0.0, 0.99, 0.02]),
        item("Finances", "b2", &["bill"], INVOICES),
        item(
            "Kitchen",
            "k1",
            &["apple", "recipes", "misc-thing"],
            KITCHEN,
        ),
        item(
            "Kitchen",
            "k2",
            &["apple", "recipes"],
            [0.95, 0.05, 0.0, 0.0],
        ),
        item("Kitchen", "k3", &["apple", "recipes"], KITCHEN),
        item("Computers", "c1", &["apple", "hardware"], COMPUTERS),
        item(
            "Computers",
            "c2",
            &["apple", "hardware"],
            [0.05, 0.95, 0.0, 0.0],
        ),
        item("Computers", "c3", &["apple", "hardware"], COMPUTERS),
    ]
}

struct Setup {
    _dir: tempfile::TempDir,
    archive: Archive,
    holder: Holder,
    ids: BTreeMap<&'static str, Id>,
    vectors: BTreeMap<Id, Vec<f32>>,
}

impl Setup {
    fn card(&self, name: &str) -> CardFile {
        let item = items().into_iter().find(|i| i.name == name).unwrap();
        let text = std::fs::read_to_string(
            self.archive
                .root()
                .join(item.collection)
                .join(format!("{name}.md")),
        )
        .unwrap();
        CardFile::parse(&text).unwrap()
    }

    fn tags(&self, name: &str) -> Vec<String> {
        self.card(name)
            .card()
            .tags
            .iter()
            .map(|t| t.0.clone())
            .collect()
    }

    fn propose(&self) -> Proposal {
        let archive = Archive::open(self.archive.root()).unwrap();
        let vectors = |id: Id| self.vectors.get(&id).cloned();
        propose(&archive, &vectors, &TagOptions::default()).unwrap()
    }

    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.archive.root().join(path)).unwrap()
    }
}

fn write_collection(root: &Path, path: &str, vocabulary: &str) {
    std::fs::create_dir_all(root.join(path)).unwrap();
    std::fs::write(
        root.join(path).join("_collection.md"),
        format!(
            "---\ncarrel: collection\nformat: \"1.0\"\nid: {}\ntitle: \"{path}\"\n{vocabulary}---\n\n# {path}\n\nThe owner's words.\n",
            Id::new_v7()
        ),
    )
    .unwrap();
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("archive");
    let archive = Archive::init(&root, &InitOptions::new("Tags")).unwrap();
    // The archive's vocabulary has `receipt`; Finances's has `receipts`.
    let text = std::fs::read_to_string(root.join("ARCHIVE.md")).unwrap();
    let split = carrel_core::frontmatter::split(&text).unwrap();
    let mut doc: ArchiveDoc = serde_saphyr::from_str(split.yaml).unwrap();
    doc.tags.push(carrel_core::documents::TagDef {
        name: Tag("receipt".into()),
        description: "Proof of a purchase".into(),
        extra: carrel_core::card::Extra::new(),
    });
    let yaml = serde_saphyr::to_string(&doc).unwrap();
    std::fs::write(
        root.join("ARCHIVE.md"),
        carrel_core::frontmatter::join(&yaml, split.body),
    )
    .unwrap();
    write_collection(
        &root,
        "Finances",
        "tags:\n  - name: receipts\n    description: \"Receipts for purchases\"\n",
    );
    write_collection(
        &root,
        "Kitchen",
        "tags:\n  - name: apple\n    description: \"Apples\"\n",
    );
    write_collection(&root, "Computers", "");
    let mut ids = BTreeMap::new();
    let mut vectors = BTreeMap::new();
    for item in items() {
        let id = Id::new_v7();
        let tags: Vec<String> = item.tags.iter().map(|t| format!("\"{t}\"")).collect();
        let generated = if item.owner {
            String::new()
        } else {
            "generated:\n  fields: [title, tags]\n  by: \"carrel test\"\n  at: \"2026-11-01\"\n"
                .to_owned()
        };
        std::fs::write(
            root.join(item.collection).join(format!("{}.md", item.name)),
            format!(
                "---\ncarrel: card\nformat: \"1.0\"\nid: {id}\nkind: physical\ntitle: \"{}\"\n\
                 tags: [{}]\nsensitivity: personal\nlocation: \"Shelf\"\n{generated}---\n\n\
                 Notes on {}.\n",
                item.name,
                tags.join(", "),
                item.name
            ),
        )
        .unwrap();
        ids.insert(item.name, id);
        vectors.insert(id, item.vector.to_vec());
    }
    let archive = Archive::open(archive.root()).unwrap();
    let holder = Holder {
        person: Id::new_v7(),
        device: Id::new_v7(),
    };
    let lease = DiskLock::for_archive(&archive)
        .acquire(holder, "test", DEFAULT_LEASE)
        .unwrap();
    History::init(&archive, &lease).unwrap();
    lease.release().unwrap();
    Setup {
        _dir: dir,
        archive,
        holder,
        ids,
        vectors,
    }
}

fn tags(names: &[&str]) -> Vec<Tag> {
    names.iter().map(|n| Tag((*n).into())).collect()
}

fn find<'a>(proposal: &'a Proposal, wanted: impl Fn(&Edit) -> bool) -> &'a ProposedChange {
    proposal
        .changes
        .iter()
        .find(|c| wanted(&c.change))
        .unwrap_or_else(|| panic!("no such change in {:#?}", proposal.changes))
}

fn merge_into<'a>(proposal: &'a Proposal, tag: &str) -> &'a ProposedChange {
    find(
        proposal,
        |e| matches!(e, Edit::MergeTags { into, .. } if into.0 == tag),
    )
}

#[test]
fn it_proposes_merges_a_split_and_retirements_each_with_why_and_what_it_touches() {
    let setup = setup();
    let proposal = setup.propose();
    assert_eq!(proposal.kind, ProposalKind::Tags);
    assert_eq!(proposal.changes.len(), 5, "{:#?}", proposal.changes);

    // Capitals, plurals and the vocabulary's own spelling become the archive's commonest.
    let receipts = merge_into(&proposal, "receipts");
    let Edit::MergeTags { from, .. } = &receipts.change else {
        unreachable!()
    };
    assert_eq!(*from, tags(&["Receipt", "receipt"]));
    assert!(receipts.reason.contains("plural"), "{}", receipts.reason);
    assert!(receipts.reason.contains("capitals"), "{}", receipts.reason);
    assert_eq!(receipts.items.len(), 1, "only r3 is changed");
    assert_eq!(receipts.items[0].id, setup.ids["r3"]);
    // The owner's own tags are held, and the proposal says so.
    assert_eq!(receipts.held.len(), 1);
    assert_eq!(receipts.held[0].id, setup.ids["r4"]);
    assert!(receipts.held[0].why.contains("you wrote"));
    assert_eq!(receipts.documents, ["ARCHIVE.md"]);

    // Unrelated names on items the index puts together.
    let bill = merge_into(&proposal, "bill");
    assert!(bill.reason.contains("close in meaning"), "{}", bill.reason);
    assert_eq!(bill.items.len(), 2);

    // One tag over two groups that don't belong together.
    let split = find(&proposal, |e| matches!(e, Edit::SplitTag { .. }));
    let Edit::SplitTag { tag, into } = &split.change else {
        unreachable!()
    };
    assert_eq!(tag.0, "apple");
    let mut parts: Vec<&str> = into.iter().map(|p| p.tag.0.as_str()).collect();
    parts.sort_unstable();
    assert_eq!(parts, ["apple-hardware", "apple-recipes"]);
    assert_eq!(split.items.len(), 6);
    assert_eq!(split.documents, ["Kitchen/_collection.md"]);

    // Rare tags in no vocabulary; the owner's own rare tag isn't proposed at all.
    let mut retired: Vec<&str> = proposal
        .changes
        .iter()
        .filter_map(|c| match &c.change {
            Edit::RetireTag { tag } => Some(tag.0.as_str()),
            _ => None,
        })
        .collect();
    retired.sort_unstable();
    assert_eq!(retired, ["misc-thing", "tax"]);
    assert!(proposal.rationale.contains("nothing moves"));
}

#[test]
fn approving_everything_rewrites_cards_and_vocabularies_as_one_change_that_undo_reverses() {
    let setup = setup();
    let before: Vec<String> = ["ARCHIVE.md", "Kitchen/_collection.md", "Kitchen/k1.md"]
        .iter()
        .map(|p| setup.read(p))
        .collect();
    let proposal = setup.propose();
    ProposalQueue::of(&setup.archive).save(&proposal).unwrap();
    let approved = restructure::approve(&setup.archive, &proposal, None, setup.holder).unwrap();
    assert!(approved.summary.starts_with("Restructure tags: 5 changes"));
    assert!(approved.commit.is_some());

    assert_eq!(setup.tags("r3"), ["receipts"]);
    assert_eq!(
        setup.tags("r4"),
        ["Receipt", "mine"],
        "the owner's tags stay"
    );
    assert_eq!(setup.tags("r2"), ["receipts"]);
    assert_eq!(setup.tags("i1"), ["bill"]);
    assert_eq!(setup.tags("k1"), ["apple-recipes", "recipes"]);
    assert_eq!(setup.tags("c2"), ["apple-hardware", "hardware"]);
    // The card's body is the owner's, kept exactly.
    assert!(setup.card("k1").body().contains("Notes on k1."));

    let archive = setup.read("ARCHIVE.md");
    assert!(archive.contains("name: receipts") && !archive.contains("name: receipt\n"));
    let kitchen = setup.read("Kitchen/_collection.md");
    assert!(kitchen.contains("apple-recipes") && kitchen.contains("apple-hardware"));
    assert!(!kitchen.contains("name: apple\n"));
    assert!(kitchen.contains("The owner's words."));
    assert!(setup.read("Kitchen/CHANGELOG.md").contains("split “apple”"));
    assert!(
        ProposalQueue::of(&setup.archive)
            .list()
            .unwrap()
            .0
            .is_empty(),
        "the proposal leaves the queue"
    );

    let history = History::open(&setup.archive).unwrap();
    let log = history.log().unwrap();
    assert_eq!(log[0].operation, Some(Operation::Restructure));
    assert!(log[0].touches(setup.ids["k1"]));
    let lease = DiskLock::for_archive(&setup.archive)
        .acquire(setup.holder, "undo", DEFAULT_LEASE)
        .unwrap();
    history
        .undo(&setup.archive, approved.commit.as_deref().unwrap(), &lease)
        .unwrap();
    lease.release().unwrap();
    for (path, was) in ["ARCHIVE.md", "Kitchen/_collection.md", "Kitchen/k1.md"]
        .iter()
        .zip(&before)
    {
        assert_eq!(setup.read(path), *was, "{path} is back as it was");
    }
    assert_eq!(setup.tags("r3"), ["Receipt"]);
}

#[test]
fn approving_part_applies_only_that_part() {
    let setup = setup();
    let proposal = setup.propose();
    let split = find(&proposal, |e| matches!(e, Edit::SplitTag { .. }));
    let only = vec![split.id.clone()];
    ProposalQueue::of(&setup.archive).save(&proposal).unwrap();
    let approved =
        restructure::approve(&setup.archive, &proposal, Some(&only), setup.holder).unwrap();
    assert_eq!(approved.selection.approved, only);
    assert_eq!(approved.selection.rejected.len(), 4);
    assert!(approved.summary.contains("1 of 5 changes"));
    assert_eq!(setup.tags("k2"), ["apple-recipes", "recipes"]);
    assert_eq!(setup.tags("r3"), ["Receipt"], "the merge was left out");
    assert_eq!(
        setup.tags("r2"),
        ["receipts", "tax"],
        "so was the retirement"
    );
}

#[test]
fn a_card_the_owner_changed_since_is_left_alone_and_reported() {
    let setup = setup();
    let proposal = setup.propose();
    // The owner edits r3's tags themselves after the proposal was made.
    let path = setup.archive.root().join("Finances/r3.md");
    let mut file = setup.card("r3");
    file.card_mut().generated = None;
    std::fs::write(&path, file.to_text().unwrap()).unwrap();
    let merge = merge_into(&proposal, "receipts");
    let only = vec![merge.id.clone()];
    let approved =
        restructure::approve(&setup.archive, &proposal, Some(&only), setup.holder).unwrap();
    assert!(approved.outcomes[0].items.is_empty());
    assert_eq!(approved.outcomes[0].skipped[0].0, setup.ids["r3"]);
    assert_eq!(setup.tags("r3"), ["Receipt"]);
}

#[test]
fn without_an_index_only_names_and_rare_tags_are_proposed() {
    let setup = setup();
    let archive = Archive::open(setup.archive.root()).unwrap();
    let proposal = propose(&archive, &|_| None, &TagOptions::default()).unwrap();
    assert!(
        proposal
            .changes
            .iter()
            .all(|c| !matches!(c.change, Edit::SplitTag { .. }))
    );
    merge_into(&proposal, "receipts");
    // `bill` and `invoice` are rare now that nothing says they're synonyms.
    let retired = proposal
        .changes
        .iter()
        .filter(|c| matches!(c.change, Edit::RetireTag { .. }))
        .count();
    assert_eq!(retired, 4);
}

#[test]
fn leaving_out_a_change_drops_the_ones_that_need_it() {
    let setup = setup();
    let mut proposal = setup.propose();
    proposal.changes[2].depends_on = vec!["1".into()];
    let selection = restructure::select(&proposal, Some(&["2".into(), "3".into()])).unwrap();
    assert_eq!(selection.approved, ["2"]);
    assert_eq!(selection.dropped, [("3".to_owned(), "1".to_owned())]);
    assert!(matches!(
        restructure::select(&proposal, Some(&["9".into()])),
        Err(RestructureError::Invalid(_))
    ));
    assert!(matches!(
        restructure::select(&proposal, Some(&["3".into()])),
        Err(RestructureError::Invalid(_))
    ));
}
