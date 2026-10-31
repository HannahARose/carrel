//! The embedding index (`docs/design/search.md`, FORMAT.md §7.11): each item's card and the
//! passages of its text as vectors from the archive's embedding model, so search can find
//! items by what they mean.
//!
//! The index is plain files in `_System/index/<model>/`: an `index.json` saying which model
//! and how many numbers each vector has, and the items in 256 shards by the end of their
//! identifier, so a change rewrites one small file. A shard is one line of JSON saying what
//! each vector belongs to (the item, its card or which passage) and the checksums each item
//! was embedded from, then the vectors as 16-bit floats. It's regenerable, so it's left out
//! of history and backups.
//!
//! [`plan`] compares the index with the archive's cards to find what's behind; [`Index::embed`]
//! catches it up through the `embed` provider capability, with the archive's model and never
//! another; [`Index::nearest`] compares a query with every vector (brute force), behind the
//! [`Neighbours`] interface an approximate index could provide instead; [`Index::near_item`]
//! finds the items nearest one already in the index, by its card's vector, for browsing; and
//! [`ByMeaning`] is the ranking search merges with its word ranking.
//!
//! Restricted items are embedded by their card only, so nothing in their text can be found,
//! or reconstructed, from the index.

pub mod chunk;
pub mod half;
pub mod switch;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::archive::{Archive, ArchiveError};
use crate::card::{Card, Checksum, Id, Sensitivity};
use crate::card_file::CardFile;
use crate::providers::{EmbeddingRequest, ProviderError, Purpose, Registry};
use crate::search::passage::{self, Passage};
use crate::search::texts::{TextCache, fingerprint};
use crate::search::words::tokens;
use crate::search::{Ranked, Ranking};

/// The index's folder, relative to the archive root; each model's index is a folder in it.
pub const INDEX: &str = "_System/index";

/// The version of the index's files this reads and writes. An index in a later version is
/// left alone and built again beside it.
pub const FORMAT: u32 = 1;

/// How many items are embedded between saves, so an interrupted run loses little.
const SAVE_EVERY: usize = 16;

/// Why the index couldn't be read, written or brought up to date.
#[derive(Debug)]
pub enum IndexError {
    /// Listing the archive failed.
    Archive(ArchiveError),
    /// Reading or writing a file failed.
    Io {
        /// The file or folder concerned.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The embedding model couldn't embed.
    Provider(ProviderError),
    /// The model's vectors aren't as long as the index's.
    Dimensions {
        /// How many numbers the index's vectors have.
        index: usize,
        /// How many the model gave.
        model: usize,
    },
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Archive(e) => e.fmt(f),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Provider(e) => e.fmt(f),
            Self::Dimensions { index, model } => write!(
                f,
                "the index's vectors have {index} numbers, but the model's have {model}; \
                 rebuild it with `carrel index rebuild`"
            ),
        }
    }
}

impl std::error::Error for IndexError {}

impl From<ArchiveError> for IndexError {
    fn from(e: ArchiveError) -> Self {
        Self::Archive(e)
    }
}

impl From<ProviderError> for IndexError {
    fn from(e: ProviderError) -> Self {
        Self::Provider(e)
    }
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> IndexError + '_ {
    move |source| IndexError::Io {
        path: path.to_owned(),
        source,
    }
}

/// What a vector is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "of", rename_all = "kebab-case")]
pub enum Part {
    /// The item's card, as [`chunk::card_text`] makes it.
    Card,
    /// A passage of the item's text, in bytes of the text drafting read.
    Passage {
        /// Where it starts.
        start: usize,
        /// Where it ends.
        end: usize,
    },
}

/// An item in the index: what it was embedded from, and what each of its vectors is of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The item.
    pub id: Id,
    /// The checksum of its card's text when it was embedded.
    pub card: Checksum,
    /// The checksum of the contents whose text was cut into passages, if any were.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<Checksum>,
    /// What each vector is of, in order: the card first.
    pub parts: Vec<Part>,
}

/// An entry with its vectors, each [`Index::dimensions`] 16-bit floats, one after another.
#[derive(Debug, Clone)]
struct Stored {
    entry: Entry,
    vectors: Vec<u16>,
}

/// The first line of a shard.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ShardHead {
    items: Vec<Entry>,
}

/// The index's description, in `index.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Head {
    format: u32,
    model: String,
    dimensions: usize,
    /// Set while `carrel index rebuild` hasn't finished, so running it again carries on.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    rebuilding: bool,
}

/// An item near a query, as [`Neighbours::nearest`] finds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Near {
    /// The item.
    pub id: Id,
    /// Its vector nearest the query.
    pub part: Part,
    /// The contents whose text its passages are from, if any.
    pub text: Option<Checksum>,
    /// The cosine similarity: 1 is the same direction.
    pub score: f32,
}

/// Finds the items nearest a query vector. The index does it by brute force; an approximate
/// index can do it instead, behind this interface, if archives outgrow that.
pub trait Neighbours {
    /// Up to `limit` items whose vectors are nearest `query`, nearest first, each with its
    /// nearest vector, leaving out those `allowed` rejects.
    fn nearest(&self, query: &[f32], allowed: &dyn Fn(Id) -> bool, limit: usize) -> Vec<Near>;
}

/// One model's embedding index of an archive.
#[derive(Debug, Clone)]
pub struct Index {
    dir: PathBuf,
    model: String,
    dimensions: usize,
    rebuilding: bool,
    shards: BTreeMap<u8, Vec<Stored>>,
    dirty: BTreeSet<u8>,
}

/// The folder name for `model`'s index: its identifier, with anything that isn't safe in a
/// file name replaced.
fn folder_name(model: &str) -> String {
    let name: String = model
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    if name.starts_with('.') || name.is_empty() {
        format!("_{name}")
    } else {
        name
    }
}

/// The shard an item is kept in: the last byte of its identifier, which is random.
fn shard_of(id: Id) -> u8 {
    id.0.as_bytes()[15]
}

fn shard_path(dir: &Path, shard: u8) -> PathBuf {
    dir.join(format!("{shard:02x}.shard"))
}

/// Writes `bytes` to `path` through a file beside it, so a reader never sees half of it.
fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), IndexError> {
    let partial = path.with_extension(format!("{}.partial", std::process::id()));
    std::fs::write(&partial, bytes).map_err(io(&partial))?;
    std::fs::rename(&partial, path).map_err(io(path))
}

/// A shard's entries and vectors, or nothing if it isn't whole: its items are then embedded
/// again.
fn read_shard(path: &Path, dimensions: usize) -> Option<Vec<Stored>> {
    let bytes = std::fs::read(path).ok()?;
    let newline = bytes.iter().position(|b| *b == b'\n')?;
    let head: ShardHead = serde_json::from_slice(&bytes[..newline]).ok()?;
    let data = &bytes[newline + 1..];
    let count: usize = head.items.iter().map(|e| e.parts.len()).sum();
    if data.len() != count * dimensions * 2 {
        return None;
    }
    let mut numbers = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair));
    Some(
        head.items
            .into_iter()
            .map(|entry| {
                let vectors = numbers
                    .by_ref()
                    .take(entry.parts.len() * dimensions)
                    .collect();
                Stored { entry, vectors }
            })
            .collect(),
    )
}

impl Index {
    /// The folder `model`'s index of `archive` is in.
    #[must_use]
    pub fn dir_of(archive: &Archive, model: &str) -> PathBuf {
        archive.root().join(INDEX).join(folder_name(model))
    }

    /// `model`'s index of `archive`, or `None` if there isn't one this version reads.
    ///
    /// # Errors
    ///
    /// An error reading its description, other than its not being there.
    pub fn open(archive: &Archive, model: &str) -> Result<Option<Self>, IndexError> {
        let dir = Self::dir_of(archive, model);
        let path = dir.join("index.json");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(io(&path)(e)),
        };
        let Ok(head) = serde_json::from_str::<Head>(&text) else {
            return Ok(None);
        };
        if head.format != FORMAT || head.model != model {
            return Ok(None);
        }
        let mut shards = BTreeMap::new();
        if head.dimensions > 0 {
            for shard in 0..=u8::MAX {
                if let Some(stored) = read_shard(&shard_path(&dir, shard), head.dimensions) {
                    shards.insert(shard, stored);
                }
            }
        }
        Ok(Some(Self {
            dir,
            model: head.model,
            dimensions: head.dimensions,
            rebuilding: head.rebuilding,
            shards,
            dirty: BTreeSet::new(),
        }))
    }

    /// An empty index of `archive` for `model`, written when it's [saved](Index::save).
    #[must_use]
    pub fn new(archive: &Archive, model: &str) -> Self {
        Self {
            dir: Self::dir_of(archive, model),
            model: model.to_owned(),
            dimensions: 0,
            rebuilding: false,
            shards: BTreeMap::new(),
            dirty: BTreeSet::new(),
        }
    }

    /// `model`'s index of `archive`, or a new, empty one.
    ///
    /// # Errors
    ///
    /// As for [`Index::open`].
    pub fn open_or_new(archive: &Archive, model: &str) -> Result<Self, IndexError> {
        Ok(Self::open(archive, model)?.unwrap_or_else(|| Self::new(archive, model)))
    }

    /// The embedding model.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// How many numbers each vector has; 0 until the first is added.
    #[must_use]
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// How many items are in the index.
    #[must_use]
    pub fn len(&self) -> usize {
        self.shards.values().map(Vec::len).sum()
    }

    /// Whether no item is.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many vectors are in the index.
    #[must_use]
    pub fn vectors(&self) -> usize {
        self.shards
            .values()
            .flatten()
            .map(|s| s.entry.parts.len())
            .sum()
    }

    /// Whether a rebuild was started and hasn't finished.
    #[must_use]
    pub fn rebuilding(&self) -> bool {
        self.rebuilding
    }

    /// The entry for `id`, if it's in the index.
    #[must_use]
    pub fn get(&self, id: Id) -> Option<&Entry> {
        self.shards
            .get(&shard_of(id))?
            .iter()
            .find(|s| s.entry.id == id)
            .map(|s| &s.entry)
    }

    /// Empties the index and marks it as being rebuilt, until an [`Index::embed`] of
    /// everything [`plan`] finds finishes. Run again after an interruption, the rebuild
    /// carries on from what was saved.
    pub fn start_rebuild(&mut self) {
        self.dirty.extend(self.shards.keys().copied());
        self.shards.clear();
        self.rebuilding = true;
    }

    /// Marks the index as being built, keeping what's in it, until an [`Index::embed`] of
    /// everything [`plan`] finds finishes: a new model's index while the archive
    /// [switches](switch) to it.
    pub fn start_building(&mut self) {
        self.rebuilding = true;
    }

    /// Adds `entry` with its `vectors`, one for each of its parts, replacing what was there
    /// for the item.
    ///
    /// # Errors
    ///
    /// [`IndexError::Dimensions`] if the vectors aren't as long as the index's.
    pub fn put(&mut self, entry: Entry, vectors: &[Vec<f32>]) -> Result<(), IndexError> {
        let model = vectors.first().map_or(self.dimensions, Vec::len);
        if self.dimensions == 0 {
            self.dimensions = model;
        }
        if model != self.dimensions || vectors.iter().any(|v| v.len() != self.dimensions) {
            return Err(IndexError::Dimensions {
                index: self.dimensions,
                model,
            });
        }
        debug_assert_eq!(entry.parts.len(), vectors.len());
        let shard = shard_of(entry.id);
        self.remove(entry.id);
        let vectors = vectors
            .iter()
            .flatten()
            .map(|n| half::to_bits(*n))
            .collect();
        self.shards
            .entry(shard)
            .or_default()
            .push(Stored { entry, vectors });
        self.dirty.insert(shard);
        Ok(())
    }

    /// Removes `id` from the index, if it's there.
    pub fn remove(&mut self, id: Id) {
        let shard = shard_of(id);
        if let Some(stored) = self.shards.get_mut(&shard) {
            let before = stored.len();
            stored.retain(|s| s.entry.id != id);
            if stored.len() != before {
                self.dirty.insert(shard);
            }
        }
    }

    /// Writes the index's description and the shards that changed.
    ///
    /// # Errors
    ///
    /// An error creating the folder or writing a file.
    pub fn save(&mut self) -> Result<(), IndexError> {
        std::fs::create_dir_all(&self.dir).map_err(io(&self.dir))?;
        let head = Head {
            format: FORMAT,
            model: self.model.clone(),
            dimensions: self.dimensions,
            rebuilding: self.rebuilding,
        };
        let mut text = serde_json::to_string_pretty(&head).unwrap_or_default();
        text.push('\n');
        write_whole(&self.dir.join("index.json"), text.as_bytes())?;
        for shard in std::mem::take(&mut self.dirty) {
            let path = shard_path(&self.dir, shard);
            let stored = self.shards.get(&shard).filter(|s| !s.is_empty());
            let Some(stored) = stored else {
                match std::fs::remove_file(&path) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io(&path)(e)),
                    _ => continue,
                }
            };
            let head = ShardHead {
                items: stored.iter().map(|s| s.entry.clone()).collect(),
            };
            let mut bytes = serde_json::to_vec(&head).unwrap_or_default();
            bytes.push(b'\n');
            for number in stored.iter().flat_map(|s| &s.vectors) {
                bytes.extend_from_slice(&number.to_le_bytes());
            }
            write_whole(&path, &bytes)?;
        }
        Ok(())
    }
}

impl Neighbours for Index {
    fn nearest(&self, query: &[f32], allowed: &dyn Fn(Id) -> bool, limit: usize) -> Vec<Near> {
        if query.len() != self.dimensions || self.dimensions == 0 {
            return Vec::new();
        }
        let length = query.iter().map(|n| n * n).sum::<f32>().sqrt();
        if length == 0.0 {
            return Vec::new();
        }
        let query: Vec<f32> = query.iter().map(|n| n / length).collect();
        let table = half::table();
        let mut found: Vec<Near> = Vec::new();
        for stored in self.shards.values().flatten() {
            if !allowed(stored.entry.id) {
                continue;
            }
            let best = stored
                .vectors
                .chunks_exact(self.dimensions)
                .zip(&stored.entry.parts)
                .map(|(vector, part)| {
                    // Stored vectors are of unit length, so this is the cosine.
                    let dot: f32 = vector
                        .iter()
                        .zip(&query)
                        .map(|(v, q)| table[usize::from(*v)] * q)
                        .sum();
                    (dot, *part)
                })
                .max_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((score, part)) = best {
                found.push(Near {
                    id: stored.entry.id,
                    part,
                    text: stored.entry.text.clone(),
                    score,
                });
            }
        }
        found.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.id.cmp(&b.id)));
        found.truncate(limit);
        found
    }
}

impl Index {
    /// The vector of `id`'s card, if the item is in the index.
    #[must_use]
    pub fn card_vector(&self, id: Id) -> Option<Vec<f32>> {
        let stored = self
            .shards
            .get(&shard_of(id))?
            .iter()
            .find(|s| s.entry.id == id)?;
        let at = stored.entry.parts.iter().position(|p| *p == Part::Card)?;
        let table = half::table();
        let bits = stored
            .vectors
            .get(at * self.dimensions..(at + 1) * self.dimensions)?;
        Some(bits.iter().map(|b| table[usize::from(*b)]).collect())
    }

    /// Whether the index is up to date for `card`, filed in `collection` of `archive`: it's in
    /// the index, embedded from what the card says now, as [`plan`] would find it.
    #[must_use]
    pub fn is_current(&self, archive: &Archive, card: &Card, collection: &str) -> bool {
        let (_, checksum, contents) = embedded_from(card, collection);
        self.get(card.id).is_some_and(|entry| {
            !stale(entry, &checksum, contents.as_ref(), &TextCache::of(archive))
        })
    }

    /// Up to `limit` items nearest the item `id`, nearest first, leaving out the item itself,
    /// passages of its own text included, and those `allowed` rejects; `None` if it isn't in
    /// the index. The item is compared by its card's vector, which is all a restricted item
    /// has, so nothing is embedded: the item's vector is already there.
    #[must_use]
    pub fn near_item(
        &self,
        id: Id,
        allowed: &dyn Fn(Id) -> bool,
        limit: usize,
    ) -> Option<Vec<Near>> {
        let vector = self.card_vector(id)?;
        Some(self.nearest(&vector, &|other| other != id && allowed(other), limit))
    }
}

/// An item whose entry in the index is missing or out of date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// The item.
    pub id: Id,
    /// Its card, relative to the archive root.
    pub card: String,
    /// Its card as the text that's embedded.
    pub card_text: String,
    /// The contents whose cached text is cut into passages: `None` for restricted and
    /// card-only items.
    pub contents: Option<Checksum>,
}

/// How the index compares with the archive, as [`plan`] finds it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// How many items the archive has.
    pub items: usize,
    /// Those whose entries are missing or out of date.
    pub pending: Vec<Pending>,
    /// Items in the index that aren't in the archive any more.
    pub gone: Vec<Id>,
}

impl Plan {
    /// How many items are embedded and up to date.
    #[must_use]
    pub fn current(&self) -> usize {
        self.items - self.pending.len()
    }
}

/// Whether `entry` is out of date for an item whose card text has `card` as its checksum
/// and whose passages should be from `contents`. Text this device hasn't read can't be cut
/// into passages, so an entry without them is only behind once the text is in the cache.
fn stale(entry: &Entry, card: &Checksum, contents: Option<&Checksum>, cache: &TextCache) -> bool {
    entry.card != *card
        || match (&entry.text, contents) {
            (Some(had), wanted) => Some(had) != wanted,
            (None, Some(wanted)) => cache.has(wanted),
            (None, None) => false,
        }
}

/// What `card`, filed in `collection`, is embedded from: its card as one text, that text's
/// checksum, and the contents whose text is cut into passages, if any are.
fn embedded_from(card: &Card, collection: &str) -> (String, Checksum, Option<Checksum>) {
    let card_text = chunk::card_text(card, collection);
    let checksum = Checksum::of_bytes(card_text.as_bytes());
    let contents = (card.sensitivity != Sensitivity::Restricted && !card.kind.is_card_only())
        .then(|| fingerprint(card).cloned())
        .flatten();
    (card_text, checksum, contents)
}

/// Compares `index`, if there is one, with `archive`'s cards: which items are missing or out
/// of date, and which are in the index but gone from the archive. Cards that don't parse are
/// left out.
///
/// # Errors
///
/// An error listing the archive.
pub fn plan(archive: &Archive, index: Option<&Index>) -> Result<Plan, IndexError> {
    let cache = TextCache::of(archive);
    let mut plan = Plan::default();
    let mut seen = HashSet::new();
    for item in archive.items()? {
        let Ok(text) = std::fs::read_to_string(archive.root().join(&item.card)) else {
            continue;
        };
        let Ok(file) = CardFile::parse(&text) else {
            continue;
        };
        let card = file.card();
        plan.items += 1;
        seen.insert(card.id);
        let collection = item.card.rsplit_once('/').map_or("", |(c, _)| c);
        let (card_text, checksum, contents) = embedded_from(card, collection);
        let current = index
            .and_then(|i| i.get(card.id))
            .is_some_and(|e| !stale(e, &checksum, contents.as_ref(), &cache));
        if !current {
            plan.pending.push(Pending {
                id: card.id,
                card: item.card.clone(),
                card_text,
                contents,
            });
        }
    }
    if let Some(index) = index {
        plan.gone = index
            .shards
            .values()
            .flatten()
            .map(|s| s.entry.id)
            .filter(|id| !seen.contains(id))
            .collect();
    }
    Ok(plan)
}

impl Index {
    /// Embeds `pending`'s items with the index's model through `registry`, and removes
    /// `gone`'s, saving as it goes so an interrupted run keeps what it did. Each item's card is
    /// one text, and its cached text, if any, is cut into [passages](chunk::passages). Calls
    /// `progress` with how many are done, how many there are, and the item being embedded, if
    /// any. Returns how many vectors were added. A rebuild is finished once it succeeds.
    ///
    /// # Errors
    ///
    /// An error from the model or writing the index; what was embedded before it is saved.
    pub fn embed(
        &mut self,
        archive: &Archive,
        registry: &Registry,
        plan: Plan,
        progress: &mut dyn FnMut(usize, usize, Option<&str>),
    ) -> Result<usize, IndexError> {
        for id in &plan.gone {
            self.remove(*id);
        }
        let cache = TextCache::of(archive);
        let total = plan.pending.len();
        let mut added = 0;
        for (done, pending) in plan.pending.into_iter().enumerate() {
            progress(done, total, Some(&pending.card));
            if done > 0 && done % SAVE_EVERY == 0 {
                self.save()?;
            }
            match self.embed_one(registry, &cache, &pending) {
                Ok(count) => added += count,
                Err(e) => {
                    self.save()?;
                    return Err(e);
                }
            }
        }
        self.rebuilding = false;
        self.save()?;
        progress(total, total, None);
        Ok(added)
    }

    fn embed_one(
        &mut self,
        registry: &Registry,
        cache: &TextCache,
        pending: &Pending,
    ) -> Result<usize, IndexError> {
        let mut parts = vec![Part::Card];
        let mut texts = vec![pending.card_text.clone()];
        let read = pending
            .contents
            .as_ref()
            .and_then(|c| cache.get(c).map(|t| (c.clone(), t)));
        if let Some((_, text)) = &read {
            for range in chunk::passages(text) {
                texts.push(text[range.clone()].to_owned());
                parts.push(Part::Passage {
                    start: range.start,
                    end: range.end,
                });
            }
        }
        let request = EmbeddingRequest {
            texts,
            purpose: Purpose::Document,
        };
        let (embeddings, _, _) = registry.embed(&self.model, &request)?;
        let entry = Entry {
            id: pending.id,
            card: Checksum::of_bytes(pending.card_text.as_bytes()),
            text: read.map(|(c, _)| c),
            parts,
        };
        let count = embeddings.vectors.len();
        self.put(entry, &embeddings.vectors)?;
        Ok(count)
    }
}

/// Embeds a search `query` with `model` through `registry`.
///
/// # Errors
///
/// The model's error.
pub fn embed_query(registry: &Registry, model: &str, query: &str) -> Result<Vec<f32>, IndexError> {
    let request = EmbeddingRequest {
        texts: vec![query.to_owned()],
        purpose: Purpose::Query,
    };
    let (embeddings, _, _) = registry.embed(model, &request)?;
    Ok(embeddings.vectors.into_iter().next().unwrap_or_default())
}

/// A way to embed a search query, or `None` when it can't be.
pub type Embedder = Box<dyn Fn(&str) -> Option<Vec<f32>>>;

/// The ranking by meaning that search merges with its word ranking: the items nearest the
/// query, each with the passage of its text that was nearest, if that wasn't its card.
pub struct ByMeaning {
    neighbours: Box<dyn Neighbours>,
    texts: TextCache,
    behind: usize,
    embed: Embedder,
}

impl std::fmt::Debug for ByMeaning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ByMeaning")
            .field("behind", &self.behind)
            .finish_non_exhaustive()
    }
}

impl ByMeaning {
    /// A ranking from `neighbours`, such as an [`Index`], with `behind` items not in it yet,
    /// embedding each query with `embed`. Passages are read from `archive`'s text cache.
    #[must_use]
    pub fn new(
        archive: &Archive,
        neighbours: Box<dyn Neighbours>,
        behind: usize,
        embed: Embedder,
    ) -> Self {
        Self {
            neighbours,
            texts: TextCache::of(archive),
            behind,
            embed,
        }
    }

    /// The passage `near` is of, with `terms` marked, if it's a passage whose text is cached.
    fn passage(&self, near: &Near, terms: &HashSet<String>) -> Option<Passage> {
        let Part::Passage { start, end } = near.part else {
            return None;
        };
        let text = self.texts.get(near.text.as_ref()?)?;
        passage::best(text.get(start..end)?, terms)
    }
}

impl Ranking for ByMeaning {
    fn rank(&self, query: &str, allowed: &dyn Fn(Id) -> bool, limit: usize) -> Vec<Ranked> {
        let Some(vector) = (self.embed)(query) else {
            return Vec::new();
        };
        let terms: HashSet<String> = tokens(query).into_iter().map(|t| t.term).collect();
        self.neighbours
            .nearest(&vector, allowed, limit)
            .into_iter()
            .map(|near| Ranked {
                id: near.id,
                passage: self.passage(&near, &terms),
                score: f64::from(near.score),
            })
            .collect()
    }

    fn behind(&self) -> usize {
        self.behind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_are_safe() {
        assert_eq!(folder_name("embeddinggemma-300m"), "embeddinggemma-300m");
        assert_eq!(folder_name("org/model:v2"), "org_model_v2");
        assert_eq!(folder_name(".."), "_..");
        assert_eq!(folder_name(""), "_");
    }
}
