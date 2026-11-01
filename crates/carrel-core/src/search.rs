//! Search (`docs/design/search.md`): finds items by the words in their cards and text, and the
//! collection documents and glossary facts that mention them, narrowed by filters, with the
//! passage that matched and why each result came back.
//!
//! [`Search::build`] reads every card, collection document and glossary fact, and each item's
//! text from the [`texts`] cache, into a [`words::WordIndex`]; build it once and run as many
//! queries as needed, as the interactive mode does while a query is typed. A second ranking,
//! by meaning from the embedding index, is added with [`Search::with_meaning`]: the two are
//! merged ([`merge`]): items with the query's exact names and numbers first, then by weighted
//! reciprocal rank fusion, so either ranking can bring an item back.
//!
//! Restricted items are found by their card only: their text is never cached, indexed or
//! shown.
//!
//! ```
//! use carrel_core::archive::{Archive, InitOptions};
//! use carrel_core::search::{Search, query::Query};
//!
//! let dir = tempfile::tempdir()?;
//! let archive = Archive::init(&dir.path().join("Archive"), &InitOptions::new("My archive"))?;
//! let search = Search::build(&archive)?;
//! let results = search.search(&Query::parse("invoice 2057-4038 kind:file")?, 20);
//! assert!(results.hits.is_empty());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod passage;
pub mod query;
pub mod run;
pub mod texts;
pub mod words;

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

use serde::{Deserialize, Serialize};

use crate::archive::{Archive, ArchiveError};
use crate::card::{Card, Checksum, Id, Kind, Sensitivity};
use crate::card_file::CardFile;
use crate::ingest::card::Glossary;
use passage::Passage;
use query::{Filters, Query};
use texts::TextCache;
use words::{Field, WordIndex, tokens};

/// What a result is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Found {
    /// A filed item.
    Item(FoundItem),
    /// A collection, by its design document.
    Collection {
        /// Its folder, relative to the archive root.
        path: String,
        /// Its title.
        title: String,
    },
    /// A glossary fact.
    Glossary {
        /// The term it's about.
        term: String,
    },
}

/// A filed item, as a result shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoundItem {
    /// Its identifier.
    pub id: Id,
    /// Its card's title.
    pub title: String,
    /// Its card, relative to the archive root.
    pub card: String,
    /// Its contents, relative to the archive root; card-only items have none.
    pub item: Option<String>,
    /// The collection it's in.
    pub collection: String,
    /// Its kind.
    pub kind: Kind,
    /// How sensitive it is.
    pub sensitivity: Sensitivity,
    /// Its date: the document's date, else when it was made, else when it was added.
    pub date: Option<String>,
    /// Its tags.
    pub tags: Vec<String>,
    /// Whether it's checked out.
    pub checked_out: bool,
}

/// Why a result came back: where the words matched, or how else it was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Why {
    /// The title, or a glossary term.
    Title,
    /// The summary, or a collection's description.
    Summary,
    /// The tags.
    Tags,
    /// The collection's name.
    Collection,
    /// The dates.
    Date,
    /// The owner's text on the card.
    Notes,
    /// The item's own text, or a collection document's or glossary fact's.
    Text,
    /// Its meaning is near the query's, by the embedding index.
    Meaning,
    /// Only the filters: the query had no words.
    Filters,
}

impl Why {
    fn of(field: Field) -> Self {
        match field {
            Field::Title => Self::Title,
            Field::Summary => Self::Summary,
            Field::Tags => Self::Tags,
            Field::Collection => Self::Collection,
            Field::Date => Self::Date,
            Field::Notes => Self::Notes,
            Field::Text => Self::Text,
        }
    }
}

impl std::fmt::Display for Why {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Title => "title",
            Self::Summary => "summary",
            Self::Tags => "tags",
            Self::Collection => "collection",
            Self::Date => "date",
            Self::Notes => "notes",
            Self::Text => "text",
            Self::Meaning => "meaning",
            Self::Filters => "filters",
        })
    }
}

/// One result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hit {
    /// What was found.
    pub found: Found,
    /// Its score: higher is better. Only the order means anything.
    pub score: f64,
    /// Why it came back, the strongest reason first.
    pub why: Vec<Why>,
    /// The passage that matched, with the query's words marked.
    pub passage: Option<Passage>,
    /// Where the passage is from.
    pub passage_from: Option<Why>,
}

/// How far the meaning ranking can be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "state")]
pub enum Meaning {
    /// There's no meaning ranking: results are by words only.
    Off,
    /// Every item is in the embedding index.
    Current,
    /// Some items aren't embedded yet; they're found by their words until they are.
    Behind {
        /// How many.
        items: usize,
    },
}

/// What a search found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Results {
    /// The best results, best first.
    pub hits: Vec<Hit>,
    /// How many matched in all, before the limit.
    pub total: usize,
    /// Whether meaning was used, and whether it's behind.
    pub meaning: Meaning,
}

/// An item a second ranking found, such as by meaning.
#[derive(Debug, Clone, PartialEq)]
pub struct Ranked {
    /// The item.
    pub id: Id,
    /// The passage that matched, if the ranking knows one.
    pub passage: Option<Passage>,
    /// How near it is, such as the cosine similarity: higher is nearer. Kept so a benchmark
    /// can replay how rankings are merged.
    pub score: f64,
}

/// A second ranking of items, merged with the word ranking by reciprocal rank fusion. The
/// embedding index provides one; it must never rank a restricted item by its text.
pub trait Ranking {
    /// Up to `limit` items for `query`, best first, leaving out those `allowed` rejects.
    fn rank(&self, query: &str, allowed: &dyn Fn(Id) -> bool, limit: usize) -> Vec<Ranked>;

    /// How many items aren't in this ranking yet.
    fn behind(&self) -> usize;
}

/// The constant in reciprocal rank fusion: how much the first places count over later ones.
pub const RRF_K: f64 = 60.0;

/// How many documents an exact name or number may be in and still put them first: one in
/// more, such as a year in many cards' dates, doesn't single anything out.
pub const RARE: usize = 3;

/// Whether `term`, a word as the word index keeps it, is an exact name or number: a code
/// with letters and digits ("as1115", "k7q2px", "ql-700"), or a number of three digits or
/// more ("2057-4038", "1040", "07:10"). These are what an embedding blurs.
#[must_use]
pub fn is_exact(term: &str) -> bool {
    let digits = term.chars().filter(char::is_ascii_digit).count();
    let letters = term.chars().filter(|c| c.is_alphabetic()).count();
    digits > 0 && (letters > 0 || digits >= 3)
}

/// The exact names and numbers in `query`, as the word index keeps them: those [`is_exact`]
/// accepts, and words of three or more capitals such as "HMRC" or "CSV".
#[must_use]
pub fn exact_terms(query: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for token in tokens(query) {
        let original = &query[token.span.clone()];
        let capitals = original.chars().filter(|c| c.is_alphabetic()).count() >= 3
            && original
                .chars()
                .filter(|c| c.is_alphabetic())
                .all(char::is_uppercase);
        if (is_exact(&token.term) || capitals) && !found.contains(&token.term) {
            found.push(token.term);
        }
    }
    found
}

/// How the word ranking and the meaning ranking are merged: weighted reciprocal rank fusion,
/// after the items with the query's exact names and numbers.
///
/// Each document's score is the number of the query's rare exact terms it has ([`RARE`],
/// [`exact_terms`]), so those come first, plus `words / (RRF_K + place)` for its place among
/// the words and `meaning / (RRF_K + place)` for its place by meaning. A document the meaning
/// ranking can't have, such as a collection, or an item it hasn't embedded yet, is judged by
/// its words in place of its meaning.
///
/// The weights come from the findability benchmark (`docs/bench/`, 2026-10-31): plain fusion
/// gave a weak word match as much weight as a strong meaning match and ranked below meaning
/// alone, so with a [strong](crate::models::Strength::Strong) model the words only break ties
/// and bring back what meaning misses, and with a weak one meaning does that for the words.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Merge {
    /// How much a place among the words counts.
    pub words: f64,
    /// How much a place by meaning counts.
    pub meaning: f64,
}

impl Merge {
    /// For a strong model: meaning ranks, words promote exact names and numbers.
    pub const STRONG: Self = Self {
        words: 0.05,
        meaning: 1.0,
    };

    /// For a weak model: words rank, meaning breaks near-ties.
    pub const WEAK: Self = Self {
        words: 1.0,
        meaning: 0.05,
    };

    /// Plain reciprocal rank fusion, both counting the same: how search merged before the
    /// benchmark, kept to compare against.
    pub const EQUAL: Self = Self {
        words: 1.0,
        meaning: 1.0,
    };

    /// The merge for an embedding model, by its [strength](crate::models::Strength) in the
    /// catalog; a model the catalog doesn't know is taken to be strong.
    #[must_use]
    pub fn for_model(model: &str) -> Self {
        let strength = crate::models::find(model)
            .and_then(|m| m.embedding)
            .map_or(crate::models::Strength::Strong, |e| e.strength);
        match strength {
            crate::models::Strength::Strong => Self::STRONG,
            crate::models::Strength::Weak => Self::WEAK,
        }
    }
}

impl Default for Merge {
    fn default() -> Self {
        Self::STRONG
    }
}

/// A document the word ranking found, as [`merge`] takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worded<K> {
    /// The document.
    pub key: K,
    /// How many of the query's rare exact names and numbers it has.
    pub exact: usize,
}

/// Merges `words`, best first, with `meaning`, best first, by `weights` ([`Merge`]). Without
/// a meaning ranking it's the words alone, after the exact matches. `by_words` says which
/// documents missing from `meaning` are judged by their words in its place. Best first; ties
/// keep the words' order, then the meaning ranking's.
#[must_use]
pub fn merge<K: Eq + Hash + Clone>(
    words: &[Worded<K>],
    meaning: Option<&[K]>,
    by_words: &dyn Fn(&K) -> bool,
    weights: Merge,
) -> Vec<(K, f64)> {
    #[allow(clippy::cast_precision_loss)] // places and counts far below 2^52
    let part = |weight: f64, place: usize| weight / (RRF_K + place as f64 + 1.0);
    let mut scores: HashMap<K, (f64, usize)> = HashMap::new();
    let mut order = 0;
    let mut add = |key: &K, score: f64| {
        let entry = scores.entry(key.clone()).or_insert_with(|| {
            order += 1;
            (0.0, order)
        });
        entry.0 += score;
    };
    let ranked: HashSet<&K> = meaning.unwrap_or_default().iter().collect();
    for (place, worded) in words.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let mut score = worded.exact as f64 + part(weights.words, place);
        if meaning.is_some() && !ranked.contains(&worded.key) && by_words(&worded.key) {
            score += part(weights.meaning, place);
        }
        add(&worded.key, score);
    }
    for (place, key) in meaning.unwrap_or_default().iter().enumerate() {
        add(key, part(weights.meaning, place));
    }
    let mut merged: Vec<(K, f64, usize)> =
        scores.into_iter().map(|(k, (s, o))| (k, s, o)).collect();
    merged.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.2.cmp(&b.2)));
    merged.into_iter().map(|(k, s, _)| (k, s)).collect()
}

/// A document in the index, and what's needed to filter it and show its passage.
#[derive(Debug, Clone)]
struct Doc {
    found: Found,
    /// The fields kept to show passages from; an item's text is read from the cache instead.
    fields: Vec<(Field, String)>,
    /// The cached text's checksum, for an item whose text is searched.
    text: Option<Checksum>,
}

/// The archive's search index: build it once, then search it as often as needed.
pub struct Search {
    docs: Vec<Doc>,
    words: WordIndex,
    by_id: HashMap<Id, usize>,
    texts: TextCache,
    meaning: Option<Box<dyn Ranking>>,
    merge: Merge,
}

impl std::fmt::Debug for Search {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Search")
            .field("docs", &self.docs.len())
            .field("meaning", &self.meaning.is_some())
            .field("merge", &self.merge)
            .finish_non_exhaustive()
    }
}

/// `body` without its managed sections: the owner's own text.
fn owner_text(body: &str) -> String {
    let Ok(sections) = crate::sections::find_all(body) else {
        return body.to_owned();
    };
    let mut text = String::new();
    let mut at = 0;
    for section in sections {
        text.push_str(&body[at..section.whole.start]);
        at = section.whole.end;
    }
    text.push_str(&body[at..]);
    text.trim().to_owned()
}

/// An item's date for filters and results: the document's date, else when it was made, else
/// when it was added.
fn date_of(card: &Card) -> Option<String> {
    let dates = card.dates.as_ref()?;
    [&dates.document, &dates.created, &dates.added]
        .into_iter()
        .find_map(|d| d.as_ref().map(|d| d.0.clone()))
}

fn item_doc(card: &Card, card_path: &str, item: Option<&String>, body: &str) -> Doc {
    let collection = card_path.rsplit_once('/').map_or("", |(c, _)| c).to_owned();
    let restricted = card.sensitivity == Sensitivity::Restricted;
    let mut fields = vec![
        (Field::Title, card.title.clone()),
        (Field::Collection, collection.replace('/', " ")),
    ];
    if let Some(summary) = &card.summary {
        fields.push((Field::Summary, summary.clone()));
    }
    let tags: Vec<String> = card.tags.iter().map(|t| t.0.clone()).collect();
    if !tags.is_empty() {
        fields.push((Field::Tags, tags.join(" ")));
    }
    if let Some(dates) = &card.dates {
        let all = [
            &dates.document,
            &dates.created,
            &dates.modified,
            &dates.added,
            &dates.worked,
        ];
        let days: Vec<&str> = all
            .iter()
            .filter_map(|d| d.as_ref())
            .map(|d| d.0.as_str())
            .collect();
        fields.push((Field::Date, days.join(" ")));
    }
    let notes = owner_text(body);
    if !notes.is_empty() {
        fields.push((Field::Notes, notes));
    }
    Doc {
        found: Found::Item(FoundItem {
            id: card.id,
            title: card.title.clone(),
            card: card_path.to_owned(),
            item: item.cloned(),
            collection,
            kind: card.kind,
            sensitivity: card.sensitivity,
            date: date_of(card),
            tags,
            checked_out: card.checked_out.is_some(),
        }),
        fields,
        text: (!restricted)
            .then(|| texts::fingerprint(card).cloned())
            .flatten(),
    }
}

fn collection_doc(archive: &Archive, collection: &crate::archive::Collection) -> Doc {
    let doc = &collection.doc;
    let mut fields = vec![(Field::Title, doc.title.clone())];
    if let Some(description) = &doc.description {
        fields.push((Field::Summary, description.clone()));
    }
    let tags: Vec<String> = doc
        .tags
        .iter()
        .map(|t| format!("{} {}", t.name.0, t.description))
        .collect();
    if !tags.is_empty() {
        fields.push((Field::Tags, tags.join(" ")));
    }
    let path = archive.root().join(&collection.path).join("_collection.md");
    if let Ok(text) = std::fs::read_to_string(path)
        && let Ok(split) = crate::frontmatter::split(&text)
    {
        fields.push((Field::Text, owner_text(split.body)));
    }
    Doc {
        found: Found::Collection {
            path: collection.path.clone(),
            title: doc.title.clone(),
        },
        fields,
        text: None,
    }
}

/// `filters` applied to `item`. Tombstones are left out unless `kind:tombstone` asks for them.
pub(crate) fn passes(filters: &Filters, item: &FoundItem) -> bool {
    let date = item.date.as_deref();
    let in_collection = |wanted: &String| {
        format!("/{}/", item.collection.to_lowercase())
            .contains(&format!("/{}/", wanted.trim_matches('/').to_lowercase()))
    };
    (filters.kinds.is_empty() && item.kind != Kind::Tombstone || filters.kinds.contains(&item.kind))
        && (filters.collections.is_empty() || filters.collections.iter().any(in_collection))
        && filters.tags.iter().all(|t| item.tags.contains(t))
        && (filters.sensitivity.is_empty() || filters.sensitivity.contains(&item.sensitivity))
        && (!filters.checked_out || item.checked_out)
        && filters
            .after
            .as_ref()
            .is_none_or(|p| date.is_some_and(|d| p.starts_by(d)))
        && filters
            .before
            .as_ref()
            .is_none_or(|p| date.is_some_and(|d| p.is_after(d)))
}

impl Search {
    /// Reads `archive`'s cards, collection documents and glossary, and the cached text of each
    /// item that isn't restricted, into a new index. Cards that don't parse are left out.
    ///
    /// # Errors
    ///
    /// An error listing the archive's collections or items.
    pub fn build(archive: &Archive) -> Result<Self, ArchiveError> {
        let texts = TextCache::of(archive);
        let mut docs = Vec::new();
        for item in archive.items()? {
            let Ok(text) = std::fs::read_to_string(archive.root().join(&item.card)) else {
                continue;
            };
            let Ok(file) = CardFile::parse(&text) else {
                continue;
            };
            docs.push(item_doc(
                file.card(),
                &item.card,
                item.item.as_ref(),
                file.body(),
            ));
        }
        for collection in archive.collections()? {
            docs.push(collection_doc(archive, &collection));
        }
        for (term, fact) in Glossary::of(archive).facts {
            docs.push(Doc {
                found: Found::Glossary { term: term.clone() },
                fields: vec![(Field::Title, term), (Field::Text, fact)],
                text: None,
            });
        }
        let mut words = WordIndex::new();
        let mut by_id = HashMap::new();
        for (number, doc) in docs.iter().enumerate() {
            let cached = doc.text.as_ref().and_then(|c| texts.get(c));
            let mut fields: Vec<(Field, &str)> =
                doc.fields.iter().map(|(f, t)| (*f, t.as_str())).collect();
            if let Some(text) = &cached {
                fields.push((Field::Text, text));
            }
            words.add(&fields);
            if let Found::Item(item) = &doc.found {
                by_id.insert(item.id, number);
            }
        }
        Ok(Self {
            docs,
            words,
            by_id,
            texts,
            meaning: None,
            merge: Merge::default(),
        })
    }

    /// Adds a ranking by meaning, merged with the word ranking from now on.
    #[must_use]
    pub fn with_meaning(mut self, ranking: Box<dyn Ranking>) -> Self {
        self.meaning = Some(ranking);
        self
    }

    /// Merges the rankings with `weights` from now on, such as [`Merge::for_model`] for the
    /// meaning ranking's model; [`Merge::STRONG`] until then.
    #[must_use]
    pub fn with_merge(mut self, weights: Merge) -> Self {
        self.merge = weights;
        self
    }

    /// How many documents are indexed: items, collections and glossary facts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// Whether nothing is indexed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    fn allowed(&self, filters: &Filters, doc: usize) -> bool {
        match &self.docs[doc].found {
            Found::Item(item) => passes(filters, item),
            _ => filters.is_empty(),
        }
    }

    /// The best `limit` results for `query`. A query with only filters lists the items they
    /// match, newest first; an empty query finds nothing.
    #[must_use]
    pub fn search(&self, query: &Query, limit: usize) -> Results {
        let terms: Vec<String> = tokens(&query.text).into_iter().map(|t| t.term).collect();
        let allowed = |doc: usize| self.allowed(&query.filters, doc);
        let meaning = self
            .meaning
            .as_ref()
            .map_or(Meaning::Off, |m| match m.behind() {
                0 => Meaning::Current,
                items => Meaning::Behind { items },
            });
        if terms.is_empty() {
            return self.filtered(&query.filters, limit, meaning);
        }
        let scored = self.words.search(&terms, &allowed);
        let mut found: HashMap<usize, (Vec<Why>, Option<Passage>)> = scored
            .iter()
            .map(|s| {
                let mut fields = s.fields.clone();
                fields.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.weight().total_cmp(&a.0.weight())));
                (
                    s.doc,
                    (fields.into_iter().map(|(f, _)| Why::of(f)).collect(), None),
                )
            })
            .collect();
        let worded = self.worded(&query.text, &scored);
        let is_item = |doc: &usize| matches!(self.docs[*doc].found, Found::Item(_));
        let fused: Vec<(usize, f64)> = match &self.meaning {
            None => merge(&worded, None, &|_| true, self.merge),
            Some(ranking) => {
                let allowed_id = |id: Id| self.by_id.get(&id).is_some_and(|&d| allowed(d));
                let wanted = limit.max(50);
                let mut meaning_order = Vec::new();
                let ranked = ranking.rank(&query.text, &allowed_id, wanted);
                // Fewer than asked for, or some not embedded yet: an item missing may be one
                // the index doesn't have, so it's judged by its words.
                let partial = ranked.len() < wanted || ranking.behind() > 0;
                for ranked in ranked {
                    let Some(&doc) = self.by_id.get(&ranked.id) else {
                        continue;
                    };
                    let entry = found.entry(doc).or_default();
                    entry.0.push(Why::Meaning);
                    entry.1 = ranked.passage;
                    meaning_order.push(doc);
                }
                let by_words = |doc: &usize| partial || !is_item(doc);
                merge(&worded, Some(&meaning_order), &by_words, self.merge)
            }
        };
        let total = fused.len();
        let wanted: HashSet<String> = terms.into_iter().collect();
        let hits = fused
            .into_iter()
            .take(limit)
            .map(|(doc, score)| {
                let (why, meaning_passage) = found.remove(&doc).unwrap_or_default();
                let (passage, passage_from) = self.passage(doc, &why, &wanted).map_or(
                    (
                        meaning_passage.clone(),
                        meaning_passage.map(|_| Why::Meaning),
                    ),
                    |(p, w)| (Some(p), Some(w)),
                );
                Hit {
                    found: self.docs[doc].found.clone(),
                    score,
                    why,
                    passage,
                    passage_from,
                }
            })
            .collect();
        Results {
            hits,
            total,
            meaning,
        }
    }

    /// The word ranking `scored` for `text`, with how many of its rare exact names and numbers
    /// each document has.
    fn worded(&self, text: &str, scored: &[words::Scored]) -> Vec<Worded<usize>> {
        let exact: HashSet<String> = exact_terms(text)
            .into_iter()
            .filter(|t| self.words.documents_with(t) <= RARE)
            .collect();
        scored
            .iter()
            .map(|s| Worded {
                key: s.doc,
                exact: s.terms.iter().filter(|t| exact.contains(*t)).count(),
            })
            .collect()
    }

    /// The word ranking alone for `query`, best first, before it's merged with anything: each
    /// document found, its BM25 score, and how many of the query's rare exact names and
    /// numbers it has. The findability benchmark saves it to replay merges with.
    #[must_use]
    pub fn by_words(&self, query: &Query) -> Vec<(Found, f64, usize)> {
        let terms: Vec<String> = tokens(&query.text).into_iter().map(|t| t.term).collect();
        let scored = self
            .words
            .search(&terms, &|doc| self.allowed(&query.filters, doc));
        self.worded(&query.text, &scored)
            .into_iter()
            .zip(&scored)
            .map(|(w, s)| (self.docs[w.key].found.clone(), s.score, w.exact))
            .collect()
    }

    /// Every item `filters` allow, newest first.
    fn filtered(&self, filters: &Filters, limit: usize, meaning: Meaning) -> Results {
        if filters.is_empty() {
            return Results {
                hits: Vec::new(),
                total: 0,
                meaning,
            };
        }
        let mut items: Vec<&FoundItem> = self
            .docs
            .iter()
            .filter_map(|d| match &d.found {
                Found::Item(item) if passes(filters, item) => Some(item),
                _ => None,
            })
            .collect();
        items.sort_by(|a, b| b.date.cmp(&a.date).then(a.title.cmp(&b.title)));
        Results {
            total: items.len(),
            hits: items
                .into_iter()
                .take(limit)
                .map(|item| Hit {
                    found: Found::Item(item.clone()),
                    score: 0.0,
                    why: vec![Why::Filters],
                    passage: None,
                    passage_from: None,
                })
                .collect(),
            meaning,
        }
    }

    /// The passage to show for `doc`: from the field among its summary, notes and text that
    /// has the most of the query's words, else its summary.
    fn passage(&self, doc: usize, why: &[Why], wanted: &HashSet<String>) -> Option<(Passage, Why)> {
        let doc = &self.docs[doc];
        let text_of = |field: Field| -> Option<String> {
            if field == Field::Text
                && let Some(checksum) = &doc.text
            {
                return self.texts.get(checksum);
            }
            doc.fields
                .iter()
                .find(|(f, _)| *f == field)
                .map(|(_, t)| t.clone())
        };
        let mut best: Option<(usize, Passage, Why)> = None;
        for field in [Field::Summary, Field::Notes, Field::Text] {
            if !why.contains(&Why::of(field)) {
                continue;
            }
            let Some(passage) = text_of(field).and_then(|t| passage::best(&t, wanted)) else {
                continue;
            };
            let count = passage.highlights.len();
            if best.as_ref().is_none_or(|(n, ..)| count > *n) {
                best = Some((count, passage, Why::of(field)));
            }
        }
        best.map(|(_, p, w)| (p, w)).or_else(|| {
            let summary = text_of(Field::Summary)?;
            Some((passage::best(&summary, wanted)?, Why::Summary))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worded(keys: &[&'static str]) -> Vec<Worded<&'static str>> {
        keys.iter().map(|&key| Worded { key, exact: 0 }).collect()
    }

    fn keys<K: Clone>(merged: &[(K, f64)]) -> Vec<K> {
        merged.iter().map(|(k, _)| k.clone()).collect()
    }

    #[test]
    fn equal_fusion_rewards_agreement_and_keeps_the_words_order_on_ties() {
        let words = worded(&["a", "b", "c"]);
        let meaning = ["c", "b", "d"];
        let merged = merge(&words, Some(&meaning), &|_| false, Merge::EQUAL);
        assert_eq!(keys(&merged), ["c", "b", "a", "d"]);
        let alone = merge(&worded(&["x", "y"]), None, &|_| false, Merge::STRONG);
        assert_eq!(keys(&alone), ["x", "y"]);
        assert!(alone[0].1 > alone[1].1);
    }

    #[test]
    fn a_strong_model_ranks_and_the_words_break_ties() {
        // Every item is ranked by meaning; a word match moves an item up a few places, not to
        // the top.
        let words = worded(&["j"]);
        let meaning = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
        let merged = merge(&words, Some(&meaning), &|_| false, Merge::STRONG);
        assert_eq!(
            keys(&merged),
            ["a", "b", "c", "d", "e", "f", "j", "g", "h", "i"]
        );
        // A weak model is the other way round.
        let merged = merge(&words, Some(&meaning), &|_| false, Merge::WEAK);
        assert_eq!(keys(&merged)[..2], ["j", "a"]);
    }

    #[test]
    fn exact_matches_come_first_most_exact_terms_first() {
        let words = vec![
            Worded {
                key: "lease",
                exact: 0,
            },
            Worded {
                key: "other invoice",
                exact: 1,
            },
            Worded {
                key: "invoice",
                exact: 3,
            },
        ];
        let meaning = ["lease", "chair", "other invoice", "invoice"];
        for weights in [Merge::STRONG, Merge::WEAK, Merge::EQUAL] {
            let merged = merge(&words, Some(&meaning), &|_| false, weights);
            assert_eq!(
                keys(&merged)[..3],
                ["invoice", "other invoice", "lease"],
                "{weights:?}"
            );
        }
        let alone = merge(&words, None, &|_| false, Merge::STRONG);
        assert_eq!(keys(&alone), ["invoice", "other invoice", "lease"]);
    }

    #[test]
    fn what_meaning_cant_rank_is_judged_by_its_words() {
        // "new" isn't embedded yet and "Receipts" is a collection: neither is buried below
        // everything the meaning ranking has.
        let words = worded(&["new", "Receipts", "b"]);
        let meaning = ["a", "b", "c"];
        let by_words = |k: &&str| ["new", "Receipts"].contains(k);
        let merged = merge(&words, Some(&meaning), &by_words, Merge::STRONG);
        assert_eq!(keys(&merged), ["new", "Receipts", "b", "a", "c"]);
        let merged = merge(&words, Some(&meaning), &|_| false, Merge::STRONG);
        assert_eq!(keys(&merged), ["b", "a", "c", "new", "Receipts"]);
    }

    #[test]
    fn exact_terms_are_codes_numbers_and_capitals() {
        assert_eq!(
            exact_terms("invoice 2057-4038"),
            ["2057-4038", "2057", "4038"]
        );
        assert_eq!(exact_terms("the AS1115 datasheet"), ["as1115"]);
        assert_eq!(exact_terms("HMRC letter about my code"), ["hmrc"]);
        assert_eq!(
            exact_terms("the QL-700 driver, 62 mm tape"),
            ["ql-700", "700"]
        );
        assert_eq!(exact_terms("Gatwick at 07:10, seat 61"), ["07:10"]);
        assert_eq!(exact_terms("the GP's letter"), Vec::<String>::new());
        assert_eq!(
            exact_terms("my résumé from 4 Elm Court"),
            Vec::<String>::new()
        );
        assert!(is_exact("k7q2px") && is_exact("1040") && !is_exact("15") && !is_exact("lease"));
    }

    #[test]
    fn managed_sections_are_not_the_owners_text() {
        let body = "\nMy note.\n\n<!-- carrel:history -->\n## History\n- filed\n<!-- /carrel:history -->\n";
        assert_eq!(owner_text(body), "My note.");
    }
}
