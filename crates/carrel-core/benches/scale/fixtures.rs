//! Generated fixtures: cards, manifests, documents, vectors and history, at any size, the same
//! on every run.

use std::fmt::Write as _;
use std::path::Path;

use carrel_core::archive::{Archive, InitOptions};
use carrel_core::card::{Checksum, DateOrTime, Id};
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
use carrel_core::history::History;
use carrel_core::index::{Entry as IndexEntry, Index, Part};
use carrel_core::manifest::{Entry as ManifestEntry, Manifest};
use carrel_core::search::words::{Field, WordIndex};

use crate::harness::Rng;

/// How many numbers each generated vector has: a common size for small embedding models.
pub const DIMENSIONS: usize = 384;

/// How many vectors each generated item has in the embedding index: its card and three
/// passages.
pub const PARTS: usize = 4;

/// A vocabulary of `n` made-up words, so text has realistic repetition without a word list.
pub fn vocabulary(n: usize) -> Vec<String> {
    const LETTERS: &[u8] = b"etaoinshrdlucmfwypvbgkqjxz";
    (0..n)
        .map(|mut i| {
            let mut word = String::new();
            loop {
                word.push(char::from(LETTERS[i % LETTERS.len()]));
                i /= LETTERS.len();
                if i == 0 {
                    break;
                }
            }
            // Short words are rare in real text's vocabulary; pad to a plausible length.
            while word.len() < 4 {
                word.push('a');
            }
            word
        })
        .collect()
}

/// `count` words of `vocabulary`, favouring its first ones.
pub fn words(rng: &mut Rng, vocabulary: &[String], count: usize) -> String {
    let mut out = String::new();
    for i in 0..count {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&vocabulary[rng.skewed(vocabulary.len())]);
    }
    out
}

/// A random SHA-256 checksum.
fn checksum(rng: &mut Rng) -> Checksum {
    let mut text = String::from("sha256:");
    for _ in 0..4 {
        let _ = write!(text, "{:016x}", rng.next_u64());
    }
    Checksum(text)
}

/// `n` card files, as text, like the sample archive's: a full frontmatter, a note and a
/// history section.
pub fn card_texts(n: usize) -> Vec<String> {
    let mut rng = Rng::new(1);
    let vocab = vocabulary(5_000);
    (0..n)
        .map(|i| {
            let title = words(&mut rng, &vocab, 4);
            let summary = words(&mut rng, &vocab, 18);
            let tags = [words(&mut rng, &vocab, 1), words(&mut rng, &vocab, 1)];
            let note = words(&mut rng, &vocab, 30);
            let id = Id::new_v7();
            let manifest = checksum(&mut rng);
            format!(
                "---\ncarrel: card\nformat: \"1.0\"\nid: {id}\nkind: folder\ntitle: \"{title}\"\n\
                 item: \"item-{i}\"\nsummary: \"{summary}\"\ntags: [{}, {}]\n\
                 links:\n  - type: related\n    id: {}\ndates:\n  created: \"2026-09-01\"\n  \
                 added: \"2026-10-24\"\n  worked: \"2026-10-28\"\nsensitivity: personal\n\
                 size: {}\nmanifest:\n  checksum: \"{}\"\n  path: \"_System/manifests/{id}.tsv\"\n\
                 ---\n\n{note}\n\n<!-- carrel:history -->\n## History\n\n### 2026-10-27 · 45m\n\
                 {note}\n<!-- /carrel:history -->\n",
                tags[0],
                tags[1],
                Id::new_v7(),
                rng.below(1 << 30),
                manifest.0
            )
        })
        .collect()
}

/// A manifest of `n` files in nested folders.
pub fn manifest(n: usize) -> Manifest {
    let mut rng = Rng::new(2);
    let mut entries: Vec<ManifestEntry> = (0..n)
        .map(|i| ManifestEntry {
            path: format!("folder-{:03}/part-{:03}/file-{i:07}.dat", i % 997, i % 101),
            checksum: checksum(&mut rng),
            size: rng.next_u64() % (1 << 24),
            modified: DateOrTime("2026-10-24T09:12:00Z".to_owned()),
        })
        .collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Manifest {
        item: Id::new_v7(),
        entries,
        built: None,
    }
}

/// `older` with a few changes, a percent each: files changed, renamed, removed and added.
pub fn changed_manifest(older: &Manifest) -> Manifest {
    let mut rng = Rng::new(3);
    let mut newer = older.clone();
    let n = newer.entries.len();
    let step = 100;
    for i in (0..n).step_by(step) {
        newer.entries[i].checksum = checksum(&mut rng);
    }
    for i in (step / 4..n).step_by(step) {
        newer.entries[i].path.push_str(".moved");
    }
    let mut i = step / 2;
    while i < newer.entries.len() {
        newer.entries.remove(i);
        i += step - 1;
    }
    for i in 0..n / step {
        newer.entries.push(ManifestEntry {
            path: format!("added/file-{i:07}.dat"),
            checksum: checksum(&mut rng),
            size: 1,
            modified: DateOrTime("2026-10-25T09:12:00Z".to_owned()),
        });
    }
    newer.entries.sort_by(|a, b| a.path.cmp(&b.path));
    newer
}

/// A document's fields, as search indexes an item: its card's title, summary and tags, and for
/// one item in four, a passage of its text.
pub struct Document {
    title: String,
    summary: String,
    tags: String,
    text: Option<String>,
}

impl Document {
    /// The fields, to add to a [`WordIndex`].
    pub fn fields(&self) -> Vec<(Field, &str)> {
        let mut fields = vec![
            (Field::Title, self.title.as_str()),
            (Field::Summary, self.summary.as_str()),
            (Field::Tags, self.tags.as_str()),
        ];
        if let Some(text) = &self.text {
            fields.push((Field::Text, text.as_str()));
        }
        fields
    }
}

/// `n` documents over a vocabulary of 50,000 words.
pub fn documents(n: usize) -> Vec<Document> {
    let mut rng = Rng::new(4);
    let vocab = vocabulary(50_000);
    (0..n)
        .map(|i| Document {
            title: words(&mut rng, &vocab, 4),
            summary: words(&mut rng, &vocab, 18),
            tags: words(&mut rng, &vocab, 2),
            text: (i % 4 == 0).then(|| words(&mut rng, &vocab, 120)),
        })
        .collect()
}

/// A word index of `documents`.
pub fn word_index(documents: &[Document]) -> WordIndex {
    let mut index = WordIndex::new();
    for document in documents {
        index.add(&document.fields());
    }
    index
}

/// A query of a common word and a rarer one, as people search.
pub fn word_query() -> Vec<String> {
    let vocab = vocabulary(50_000);
    vec![vocab[3].clone(), vocab[700].clone()]
}

/// A new, empty archive in `dir`.
pub fn archive(dir: &Path) -> Archive {
    Archive::init(dir, &InitOptions::new("Benchmarks")).expect("an archive in a new folder")
}

/// A random unit-length vector.
pub fn vector(rng: &mut Rng) -> Vec<f32> {
    let v: Vec<f32> = (0..DIMENSIONS).map(|_| rng.unit()).collect();
    let length = v.iter().map(|n| n * n).sum::<f32>().sqrt();
    v.into_iter().map(|n| n / length).collect()
}

/// An embedding index of `archive` with `vectors` vectors, [`PARTS`] to an item.
pub fn embedding_index(archive: &Archive, vectors: usize) -> Index {
    let mut rng = Rng::new(5);
    let mut index = Index::new(archive, "bench-model");
    for _ in 0..vectors.div_ceil(PARTS) {
        let parts = std::iter::once(Part::Card)
            .chain((1..PARTS).map(|p| Part::Passage {
                start: p * 1000,
                end: p * 1000 + 900,
            }))
            .collect();
        let entry = IndexEntry {
            id: Id::new_v7(),
            card: checksum(&mut rng),
            text: Some(checksum(&mut rng)),
            parts,
        };
        let vectors: Vec<Vec<f32>> = (0..PARTS).map(|_| vector(&mut rng)).collect();
        index.put(entry, &vectors).expect("vectors of one length");
    }
    index
}

/// An archive's history with `commits` commits, the first its creation, each with the
/// trailers Carrel writes. They share one tree: reading the log reads only the commits.
pub fn history(archive: &Archive, commits: usize) -> History {
    let holder = Holder {
        person: Id::new_v7(),
        device: Id::new_v7(),
    };
    let lease = DiskLock::for_archive(archive)
        .acquire(holder, "bench", DEFAULT_LEASE)
        .expect("the writer lock of a new archive");
    History::init(archive, &lease).expect("a new history");
    let repo = git2::Repository::open(archive.root().join("_System/history.git"))
        .expect("the history repository");
    let mut parent = repo
        .head()
        .expect("a first commit")
        .peel_to_commit()
        .expect("a commit");
    let tree = parent.tree().expect("a tree");
    let signature = git2::Signature::now("Carrel", "carrel@localhost").expect("a signature");
    for i in 1..commits {
        let message = format!(
            "Correct item {i}\n\nCarrel-Operation: correct\nCarrel-Person: {}\n\
             Carrel-Device: {}\nCarrel-Items: {}\n",
            holder.person,
            holder.device,
            Id::new_v7()
        );
        let oid = repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                &message,
                &tree,
                &[&parent],
            )
            .expect("a commit");
        parent = repo.find_commit(oid).expect("the new commit");
    }
    History::open(archive).expect("the history")
}
