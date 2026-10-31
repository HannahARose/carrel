//! Search and an item's view (`docs/design/search.md`): `carrel search` with its filters,
//! results with why they came back and the passage that matched, and an item's card, links,
//! history and the items nearest it by meaning, as `carrel show` and Browse show them.

use std::sync::Arc;

use carrel_core::card_file::CardFile;
use carrel_core::history::History;
use carrel_core::search::passage::Passage;
use carrel_core::search::query::Query;
use carrel_core::search::{Found, Hit, Meaning};

use crate::history::Change;
use crate::records::{CardFields, CardHistoryEntry, Sensitivity};
use crate::{ArchiveHandle, CarrelError, ItemKind, ProgressListener};

/// What a result is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FoundKind {
    /// A filed item.
    Item,
    /// A collection, by its design document.
    Collection,
    /// A glossary fact.
    Glossary,
}

/// A run of a matched passage: the query's words are highlighted.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PassagePart {
    /// The text.
    pub text: String,
    /// Whether it's one of the query's words.
    pub highlighted: bool,
}

fn parts(passage: &Passage) -> Vec<PassagePart> {
    let mut parts = Vec::new();
    let mut at = 0;
    for range in &passage.highlights {
        if range.start > at {
            parts.push(PassagePart {
                text: passage.text[at..range.start].to_owned(),
                highlighted: false,
            });
        }
        parts.push(PassagePart {
            text: passage.text[range.clone()].to_owned(),
            highlighted: true,
        });
        at = range.end;
    }
    if at < passage.text.len() {
        parts.push(PassagePart {
            text: passage.text[at..].to_owned(),
            highlighted: false,
        });
    }
    parts
}

/// One search result.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SearchHit {
    /// What it is.
    pub found: FoundKind,
    /// For an item, its identifier.
    pub item_id: Option<String>,
    /// Its title; for a glossary fact, the term.
    pub title: String,
    /// For an item, its card's path; for a collection, its folder.
    pub path: String,
    /// For an item, its contents' path, if it has any.
    pub item: Option<String>,
    /// For an item, the collection it's in.
    pub collection: Option<String>,
    /// For an item, its kind.
    pub kind: Option<ItemKind>,
    /// For an item, how sensitive it is.
    pub sensitivity: Option<Sensitivity>,
    /// For an item, its date.
    pub date: Option<String>,
    /// For an item, its tags.
    pub tags: Vec<String>,
    /// For an item, whether it's checked out.
    pub checked_out: bool,
    /// Why it came back, the strongest reason first: `title`, `summary`, `tags`,
    /// `collection`, `date`, `notes`, `text`, `meaning` or `filters`.
    pub why: Vec<String>,
    /// The passage that matched, in runs.
    pub passage: Vec<PassagePart>,
    /// Where the passage is from.
    pub passage_from: Option<String>,
}

impl From<&Hit> for SearchHit {
    fn from(hit: &Hit) -> Self {
        let why = hit.why.iter().map(ToString::to_string).collect();
        let passage = hit.passage.as_ref().map(parts).unwrap_or_default();
        let passage_from = hit.passage_from.map(|w| w.to_string());
        let empty = |found, title: String, path: String| Self {
            found,
            item_id: None,
            title,
            path,
            item: None,
            collection: None,
            kind: None,
            sensitivity: None,
            date: None,
            tags: Vec::new(),
            checked_out: false,
            why: Vec::new(),
            passage: Vec::new(),
            passage_from: None,
        };
        let hit_record = match &hit.found {
            Found::Item(item) => Self {
                found: FoundKind::Item,
                item_id: Some(item.id.to_string()),
                title: item.title.clone(),
                path: item.card.clone(),
                item: item.item.clone(),
                collection: Some(item.collection.clone()),
                kind: Some(item.kind.into()),
                sensitivity: Some(item.sensitivity.into()),
                date: item.date.clone(),
                tags: item.tags.clone(),
                checked_out: item.checked_out,
                ..empty(FoundKind::Item, String::new(), String::new())
            },
            Found::Collection { path, title } => {
                empty(FoundKind::Collection, title.clone(), path.clone())
            }
            Found::Glossary { term } => empty(FoundKind::Glossary, term.clone(), String::new()),
        };
        Self {
            why,
            passage,
            passage_from,
            ..hit_record
        }
    }
}

/// What a search found.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SearchResults {
    /// The best results, best first.
    pub hits: Vec<SearchHit>,
    /// How many matched in all, before the limit.
    pub total: u64,
    /// Whether meaning was used.
    pub by_meaning: bool,
    /// How many items aren't in the meaning index yet; they're found by their words.
    pub behind: u64,
    /// Why it was by words only, when the query had words and meaning couldn't be used.
    pub words_only: Option<String>,
}

/// An item another links to.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LinkedItem {
    /// How it's linked: `version-of`, `related`, `part-of`, and so on.
    pub link_type: String,
    /// The linked item.
    pub id: String,
    /// Its card's path, if it's in the archive.
    pub card: Option<String>,
    /// Its title, if its card could be read.
    pub title: Option<String>,
}

/// A filed item, as its view shows it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ItemDetail {
    /// Its identifier.
    pub id: String,
    /// Its kind.
    pub kind: ItemKind,
    /// Its card's path in the archive.
    pub card: String,
    /// Its contents' path in the archive, if it has any.
    pub item: Option<String>,
    /// The collection it's in: its card's folder.
    pub collection: String,
    /// The fields the owner edits.
    pub fields: CardFields,
    /// Where a physical item is.
    pub location: Option<String>,
    /// A web page's address.
    pub url: Option<String>,
    /// Its size in bytes.
    pub size: Option<u64>,
    /// Where it's checked out to, if it is.
    pub checked_out_to: Option<String>,
    /// What's wrong with its card's fields, in words.
    pub problems: Vec<String>,
    /// The items it links to.
    pub links: Vec<LinkedItem>,
    /// Its card's history section: work sessions, returns and the owner's own entries.
    pub entries: Vec<CardHistoryEntry>,
}

/// An item near another by meaning.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RelatedItem {
    /// The item.
    pub id: String,
    /// Its card's path.
    pub card: String,
    /// Its title.
    pub title: String,
    /// How sensitive it is, so a restricted or sensitive one is marked.
    pub sensitivity: Sensitivity,
    /// How near: 1 is the same direction.
    pub score: f32,
}

/// The items nearest one by meaning, or why there are none to list.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Related {
    /// Nearest first.
    pub items: Vec<RelatedItem>,
    /// Why only its links are listed, when the meaning index can't say.
    pub why_none: Option<String>,
}

#[uniffi::export]
impl ArchiveHandle {
    /// Searches for `query`: words, and filters such as `tag:receipts`, `collection:Finances`,
    /// `kind:folder`, `after:2025-01`, `before:2026`, `sensitivity:sensitive` and
    /// `checked-out`. Returns the best `limit`. Items whose text hasn't been read yet are read
    /// first, reported to `progress`.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for a filter that can't be used or a query with nothing in
    /// it, or [`CarrelError::Failed`] if the archive or its index can't be read.
    pub fn search(
        &self,
        query: String,
        limit: u32,
        progress: Option<Arc<dyn ProgressListener>>,
    ) -> Result<SearchResults, CarrelError> {
        let query = Query::parse(&query).map_err(CarrelError::invalid)?;
        if query.text.trim().is_empty() && query.filters.is_empty() {
            return Err(CarrelError::invalid(
                "search for some words, a filter such as tag:receipts, or both",
            ));
        }
        let archive = self.fresh()?;
        let searched = carrel_core::search::run::search(
            &archive,
            &query,
            limit as usize,
            || self.registry(),
            &mut |done, total, item| {
                if let Some(progress) = &progress {
                    progress.update(done as u64, total as u64, item.map(str::to_owned));
                }
            },
        )?;
        let results = searched.results;
        let (by_meaning, behind) = match results.meaning {
            Meaning::Off => (false, 0),
            Meaning::Current => (true, 0),
            Meaning::Behind { items } => (true, items as u64),
        };
        Ok(SearchResults {
            hits: results.hits.iter().map(Into::into).collect(),
            total: results.total as u64,
            by_meaning,
            behind,
            words_only: searched.words_only,
        })
    }

    /// The filed item `id`: its card, links and history section.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if there's no such item, or [`CarrelError::Failed`] if its
    /// card can't be read.
    pub fn item_detail(&self, id: String) -> Result<ItemDetail, CarrelError> {
        let archive = self.fresh()?;
        let items = archive.items()?;
        let item = Self::item(&archive, &id)?;
        let file = CardFile::parse(&std::fs::read_to_string(archive.root().join(&item.card))?)?;
        let card = file.card();
        let title_of = |card_path: &str| {
            std::fs::read_to_string(archive.root().join(card_path))
                .ok()
                .and_then(|t| CardFile::parse(&t).ok())
                .map(|f| f.card().title.clone())
        };
        let links = card
            .links
            .iter()
            .map(|link| {
                let target = items.iter().find(|i| i.id == link.id);
                LinkedItem {
                    link_type: link.link_type.0.clone(),
                    id: link.id.to_string(),
                    card: target.map(|i| i.card.clone()),
                    title: target.and_then(|i| title_of(&i.card)),
                }
            })
            .collect();
        let entries = file
            .history()
            .map(|log| log.entries.iter().map(Into::into).collect())
            .unwrap_or_default();
        Ok(ItemDetail {
            id: item.id.to_string(),
            kind: item.kind.into(),
            collection: item.card.rsplit_once('/').map_or("", |(c, _)| c).to_owned(),
            card: item.card.clone(),
            item: item.item.clone(),
            fields: (&carrel_core::actions::card_fields(card)).into(),
            location: card.location.clone(),
            url: card.url.clone(),
            size: card.size,
            checked_out_to: card.checked_out.as_ref().map(|c| c.to.clone()),
            problems: card.problems().iter().map(ToString::to_string).collect(),
            links,
            entries,
        })
    }

    /// The changes in the history that concern the item `id`, newest first.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if there's no such item, or [`CarrelError::Failed`] if the
    /// history can't be read.
    pub fn item_history(&self, id: String) -> Result<Vec<Change>, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &id)?;
        Ok(History::open(&archive)?
            .item_log(item.id)?
            .iter()
            .map(Into::into)
            .collect())
    }

    /// The items nearest the item `id` by meaning, from the embedding index; nothing is
    /// embedded, so no model is started.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if there's no such item, or [`CarrelError::Failed`] if its
    /// card can't be read.
    pub fn related(&self, id: String) -> Result<Related, CarrelError> {
        let archive = self.fresh()?;
        let items = archive.items()?;
        let item = Self::item(&archive, &id)?;
        let file = CardFile::parse(&std::fs::read_to_string(archive.root().join(&item.card))?)?;
        let near = carrel_core::index::related::open(&archive).and_then(|index| {
            carrel_core::index::related::nearest(
                &archive,
                &index,
                (item.id, file.card(), &item.card),
                &items,
            )
        });
        Ok(match near {
            Ok(near) => Related {
                items: near
                    .into_iter()
                    .map(|n| RelatedItem {
                        id: n.id.to_string(),
                        card: n.card,
                        title: n.title,
                        sensitivity: n.sensitivity.into(),
                        score: n.score,
                    })
                    .collect(),
                why_none: None,
            },
            Err(why) => Related {
                items: Vec::new(),
                why_none: Some(why),
            },
        })
    }
}
