//! The items nearest an item in the embedding index, which `carrel show` and Browse list
//! under Related beside its links (`docs/design/search.md`, Browsing).
//!
//! The item is compared by its card's vector, already in the index, so nothing is embedded
//! and no provider is started: showing an item never waits for a model. When there's no
//! index, or it hasn't caught up with the item, only the links are listed, with a line
//! saying why.

use std::path::PathBuf;
use std::time::SystemTime;

use carrel_core::archive::Archive;
use carrel_core::card::Sensitivity;
use carrel_core::index::Index;
use serde_json::{Value, json};

use crate::commands::Shown;

pub use carrel_core::index::related::Neighbour;

/// The neighbour for programs.
pub fn json(neighbour: &Neighbour) -> Value {
    json!({
        "id": neighbour.id,
        "card": neighbour.card,
        "title": neighbour.title,
        "sensitivity": neighbour.sensitivity,
        "score": neighbour.score,
    })
}

/// How a neighbour is marked when it's sensitive or restricted, as search marks results.
pub fn mark(sensitivity: Sensitivity) -> &'static str {
    match sensitivity {
        Sensitivity::Restricted => "  [restricted]",
        Sensitivity::Sensitive => "  [sensitive]",
        _ => "",
    }
}

use carrel_core::index::related::{model_of, open};

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

/// The items nearest `shown` in `index` ([`carrel_core::index::related::nearest`]).
///
/// # Errors
///
/// Why only its links can be listed.
pub fn nearest(archive: &Archive, index: &Index, shown: &Shown) -> Result<Vec<Neighbour>, String> {
    carrel_core::index::related::nearest(
        archive,
        index,
        (shown.id, &shown.card, &shown.card_path),
        &shown.items,
    )
}
