//! The items nearest an item in the embedding index, which `carrel show`, Browse and the
//! apps list under Related beside its links (`docs/design/search.md`, Browsing).
//!
//! The item is compared by its card's vector, already in the index, so nothing is embedded
//! and no provider is started: showing an item never waits for a model. When there's no
//! index, or it hasn't caught up with the item, only the links are listed, with a line
//! saying why.

use std::collections::{HashMap, HashSet};

use crate::archive::{Archive, ItemRef};
use crate::card::{Card, Id, Kind, Sensitivity};
use crate::card_file::CardFile;
use crate::index::Index;

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

/// The archive's embedding model and the folder its index is in, or why there's none.
///
/// # Errors
///
/// Why only its links are listed: the archive has no embedding model.
pub fn model_of(archive: &Archive) -> Result<(String, std::path::PathBuf), String> {
    let model = archive.settings().embedding_model().map(str::to_owned).ok_or_else(|| {
        "Only its links are listed: this archive has no embedding model yet; see `carrel index status`."
            .to_owned()
    })?;
    let dir = Index::dir_of(archive, &model);
    Ok((model, dir))
}

/// The archive's index, read from disk, or why there's none to compare with.
///
/// # Errors
///
/// Why only its links are listed.
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

/// The items nearest the item `id`, whose card `card` is at `card_path`, in `index`, nearest
/// first, leaving out the item itself, those it links to, and tombstones; or why there are
/// none, when the index hasn't caught up with it. `items` are the archive's items.
///
/// # Errors
///
/// Why only its links can be listed.
pub fn nearest(
    archive: &Archive,
    index: &Index,
    (id, card, card_path): (Id, &Card, &str),
    items: &[ItemRef],
) -> Result<Vec<Neighbour>, String> {
    let collection = card_path.rsplit_once('/').map_or("", |(c, _)| c);
    if !index.is_current(archive, card, collection) {
        return Err(
            "Only its links are listed: the meaning index hasn't caught up with this item yet; \
             `carrel index status` shows what's behind."
                .into(),
        );
    }
    let linked: HashSet<Id> = card.links.iter().map(|l| l.id).collect();
    let listed: HashMap<Id, &ItemRef> = items
        .iter()
        .filter(|i| i.kind != Kind::Tombstone && !linked.contains(&i.id))
        .map(|i| (i.id, i))
        .collect();
    let near = index
        .near_item(id, &|id| listed.contains_key(&id), NEAREST)
        .unwrap_or_default();
    Ok(near
        .into_iter()
        .filter_map(|near| {
            let item = *listed.get(&near.id)?;
            let text = std::fs::read_to_string(archive.root().join(&item.card)).ok()?;
            let card = CardFile::parse(&text).ok()?.card().clone();
            Some(Neighbour {
                id: near.id,
                card: item.card.clone(),
                title: card.title,
                sensitivity: card.sensitivity,
                score: near.score,
            })
        })
        .collect())
}
