//! The items nearest an item in the embedding index, which `carrel show` and Browse list
//! under Related beside its links (`docs/design/search.md`, Browsing).
//!
//! The item is compared by its card's vector, already in the index, so nothing is embedded
//! and no provider is started: showing an item never waits for a model. When there's no
//! index, or it hasn't caught up with the item, only the links are listed, with a line
//! saying why.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;

use carrel_core::archive::{Archive, ItemRef};
use carrel_core::card::{Id, Kind, Sensitivity};
use carrel_core::index::Index;
use serde_json::{Value, json};

use crate::commands::{Shown, load_card};

/// How many of the nearest items are listed.
pub const NEAREST: usize = 5;

/// An item near the one shown, by meaning.
#[derive(Debug, Clone, PartialEq)]
pub struct Neighbour {
    /// The item.
    pub id: Id,
    /// Its card's path.
    pub card: String,
    /// Its title.
    pub title: String,
    /// How sensitive it is, so a restricted or sensitive one is marked.
    pub sensitivity: Sensitivity,
    /// The cosine similarity of its nearest vector to the item's card: 1 is the same direction.
    pub score: f32,
}

impl Neighbour {
    /// The neighbour for programs.
    pub fn json(&self) -> Value {
        json!({
            "id": self.id,
            "card": self.card,
            "title": self.title,
            "sensitivity": self.sensitivity,
            "score": self.score,
        })
    }
}

/// How a neighbour is marked when it's sensitive or restricted, as search marks results.
pub fn mark(sensitivity: Sensitivity) -> &'static str {
    match sensitivity {
        Sensitivity::Restricted => "  [restricted]",
        Sensitivity::Sensitive => "  [sensitive]",
        _ => "",
    }
}

/// The archive's embedding model and the folder its index is in, or why there's none.
fn model_of(archive: &Archive) -> Result<(String, PathBuf), String> {
    let model = archive.settings().embedding_model().map(str::to_owned).ok_or_else(|| {
        "Only its links are listed: this archive has no embedding model yet; see `carrel index status`."
            .to_owned()
    })?;
    let dir = Index::dir_of(archive, &model);
    Ok((model, dir))
}

/// The archive's index, read from disk, or why there's none to compare with.
pub fn open(archive: &Archive) -> Result<Index, String> {
    let (model, _) = model_of(archive)?;
    match Index::open(archive, &model) {
        Ok(Some(index)) if !index.is_empty() => Ok(index),
        Ok(_) => Err(format!(
            "Only its links are listed: there's no meaning index for {model} yet; build it with `carrel index rebuild`."
        )),
        Err(e) => Err(format!(
            "Only its links are listed: the meaning index couldn't be read ({e})."
        )),
    }
}

/// Where the archive's index is and when it last changed: saving it replaces `index.json`
/// through a rename, so this changes whenever anything in it does.
type Stamp = Option<(PathBuf, Option<SystemTime>)>;

/// The archive's index as it was last read, kept until it changes on disk, so moving among
/// items in Browse doesn't read every shard again each time.
#[derive(Debug, Default)]
pub struct Cached {
    /// When it was read, and what was.
    read: Option<(Stamp, Result<Index, String>)>,
}

impl Cached {
    /// The archive's index, read again if it changed since it was last read, or why there's
    /// none.
    pub fn get(&mut self, archive: &Archive) -> Result<&Index, String> {
        let stamp: Stamp = model_of(archive).ok().map(|(_, dir)| {
            let changed = std::fs::metadata(dir.join("index.json"))
                .and_then(|m| m.modified())
                .ok();
            (dir, changed)
        });
        if self.read.as_ref().is_none_or(|(had, _)| *had != stamp) {
            self.read = Some((stamp, open(archive)));
        }
        match &self.read {
            Some((_, Ok(index))) => Ok(index),
            Some((_, Err(why))) => Err(why.clone()),
            None => Err(String::new()),
        }
    }
}

/// The items nearest `shown` in `index`, nearest first, leaving out the item itself, those
/// it links to, and tombstones; or why there are none, when the index hasn't caught up with
/// it.
///
/// # Errors
///
/// Why only its links can be listed.
pub fn nearest(archive: &Archive, index: &Index, shown: &Shown) -> Result<Vec<Neighbour>, String> {
    let collection = shown.card_path.rsplit_once('/').map_or("", |(c, _)| c);
    if !index.is_current(archive, &shown.card, collection) {
        return Err(
            "Only its links are listed: the meaning index hasn't caught up with this item yet; \
             `carrel index status` shows what's behind."
                .into(),
        );
    }
    let linked: HashSet<Id> = shown.links.iter().map(|l| l.id).collect();
    let listed: HashMap<Id, &ItemRef> = shown
        .items
        .iter()
        .filter(|i| i.kind != Kind::Tombstone && !linked.contains(&i.id))
        .map(|i| (i.id, i))
        .collect();
    let near = index
        .near_item(shown.id, &|id| listed.contains_key(&id), NEAREST)
        .unwrap_or_default();
    Ok(near
        .into_iter()
        .filter_map(|near| {
            let item = *listed.get(&near.id)?;
            let loaded = load_card(archive, item).ok()?;
            Some(Neighbour {
                id: near.id,
                card: item.card.clone(),
                title: loaded.card.title,
                sensitivity: loaded.card.sensitivity,
                score: near.score,
            })
        })
        .collect())
}
