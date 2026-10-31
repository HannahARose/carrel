//! The word index: words found in cards, item text, collection documents and the glossary,
//! ranked by BM25 (`docs/design/search.md`). Exact names, numbers and codes such as
//! "invoice 2057-4038" or "AS1115" are found here, where an embedding would blur them.
//!
//! A word with punctuation inside, such as `2057-4038` or `v1.2`, is indexed whole and as its
//! parts, so the whole matches best and each part still matches. Plural endings are dropped, so
//! "receipts" finds "receipt", and a few very common words are left out.

use std::collections::HashMap;
use std::ops::Range;

/// A word as it's indexed, and where it is in the text it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// The word, lowercased and without a plural ending.
    pub term: String,
    /// Where it is, in bytes.
    pub span: Range<usize>,
}

/// Words too common to search by.
const STOP: [&str; 24] = [
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "in", "into", "is", "it",
    "its", "of", "on", "or", "that", "the", "this", "to", "was", "with",
];

/// `word` lowercased, with a plural ending dropped: "Receipts" is "receipt", "Copies" is
/// "copy". Words with digits, and short ones, are kept as they are.
#[must_use]
pub fn normalize(word: &str) -> String {
    let lower = word.to_lowercase();
    if lower.chars().count() <= 3 || !lower.chars().all(char::is_alphabetic) {
        return lower;
    }
    if let Some(stem) = lower.strip_suffix("ies") {
        return format!("{stem}y");
    }
    if let Some(stem) = lower.strip_suffix("sses") {
        return format!("{stem}ss");
    }
    if lower.ends_with('s') && !["ss", "us", "is"].iter().any(|e| lower.ends_with(e)) {
        return lower[..lower.len() - 1].to_owned();
    }
    lower
}

fn keep(term: &str) -> bool {
    let mut chars = term.chars();
    let single_letter = chars.next().is_some_and(char::is_alphabetic) && chars.next().is_none();
    !term.is_empty() && !single_letter && !STOP.contains(&term)
}

/// The words of `text`, in order. A word with punctuation inside comes first whole, then as
/// each of its parts.
#[must_use]
pub fn tokens(text: &str) -> Vec<Token> {
    let mut found = Vec::new();
    let mut offset = 0;
    for raw in text.split_inclusive(char::is_whitespace) {
        let start = offset;
        offset += raw.len();
        let trimmed_start = raw.trim_start_matches(|c: char| !c.is_alphanumeric());
        let word = trimmed_start.trim_end_matches(|c: char| !c.is_alphanumeric());
        if word.is_empty() {
            continue;
        }
        let begin = start + (raw.len() - trimmed_start.len());
        let parts: Vec<(usize, &str)> = word
            .split(|c: char| !c.is_alphanumeric())
            .scan(0, |at, part| {
                let here = *at;
                *at += part.len() + 1;
                Some((here, part))
            })
            .filter(|(_, part)| !part.is_empty())
            .collect();
        let mut push = |piece: &str, from: usize| {
            let term = normalize(piece);
            if keep(&term) {
                found.push(Token {
                    term,
                    span: begin + from..begin + from + piece.len(),
                });
            }
        };
        if parts.len() > 1 {
            push(word, 0);
        }
        for (at, part) in parts {
            push(part, at);
        }
    }
    found
}

/// One field of a document, ranked on its own and weighted by [`Field::weight`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Field {
    /// A card's title, a collection's title, or a glossary term.
    Title,
    /// A card's summary, or a collection's description.
    Summary,
    /// A card's tags.
    Tags,
    /// The collection an item is in.
    Collection,
    /// A card's dates.
    Date,
    /// The owner's text on a card, outside its managed sections.
    Notes,
    /// An item's own text, or a collection document's or glossary fact's text.
    Text,
}

/// How many fields there are.
pub const FIELDS: usize = 7;

impl Field {
    /// Every field, in order.
    pub const ALL: [Self; FIELDS] = [
        Self::Title,
        Self::Summary,
        Self::Tags,
        Self::Collection,
        Self::Date,
        Self::Notes,
        Self::Text,
    ];

    /// How much a match in this field counts.
    #[must_use]
    pub fn weight(self) -> f64 {
        match self {
            Self::Title => 3.0,
            Self::Tags => 2.0,
            Self::Summary => 1.5,
            Self::Collection | Self::Date | Self::Notes | Self::Text => 1.0,
        }
    }
}

/// One appearance of a term: in which document and field, and how often.
#[derive(Debug, Clone, Copy)]
struct Posting {
    doc: u32,
    field: Field,
    count: u32,
}

/// A document's score for a query, and which of the query's terms each field matched.
#[derive(Debug, Clone, PartialEq)]
pub struct Scored {
    /// The document, by the order it was added in.
    pub doc: usize,
    /// Its BM25 score, weighted by field and by how many of the query's terms it has.
    pub score: f64,
    /// The fields that matched, each with the number of the query's terms it has.
    pub fields: Vec<(Field, usize)>,
    /// The query's terms it has, each once.
    pub terms: Vec<String>,
}

/// An inverted index of documents' fields, ranked by BM25.
#[derive(Debug, Clone, Default)]
pub struct WordIndex {
    postings: HashMap<String, Vec<Posting>>,
    lengths: Vec<[u32; FIELDS]>,
    totals: [u64; FIELDS],
}

const K1: f64 = 1.2;
const B: f64 = 0.75;

impl WordIndex {
    /// An empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a document made of `fields`, returning its number. A field may appear more than
    /// once, as a card's several dates do.
    pub fn add(&mut self, fields: &[(Field, &str)]) -> usize {
        let doc = self.lengths.len();
        let number = u32::try_from(doc).unwrap_or(u32::MAX);
        let mut lengths = [0u32; FIELDS];
        let mut counts: HashMap<(String, Field), u32> = HashMap::new();
        for (field, text) in fields {
            for token in tokens(text) {
                lengths[*field as usize] += 1;
                *counts.entry((token.term, *field)).or_default() += 1;
            }
        }
        for ((term, field), count) in counts {
            self.postings.entry(term).or_default().push(Posting {
                doc: number,
                field,
                count,
            });
        }
        for (total, length) in self.totals.iter_mut().zip(lengths) {
            *total += u64::from(length);
        }
        self.lengths.push(lengths);
        doc
    }

    /// How many documents there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lengths.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lengths.is_empty()
    }

    /// How many documents have `term`, in any field.
    #[must_use]
    pub fn documents_with(&self, term: &str) -> usize {
        self.postings.get(term).map_or(0, |postings| {
            let mut docs: Vec<u32> = postings.iter().map(|p| p.doc).collect();
            docs.dedup();
            docs.len()
        })
    }

    /// Every document with any of `terms`, best first, leaving out those `allowed` rejects. A
    /// document's score is scaled by the share of the terms it has, so one with all of them
    /// comes before one that repeats a single term.
    #[must_use]
    pub fn search(&self, terms: &[String], allowed: &dyn Fn(usize) -> bool) -> Vec<Scored> {
        #[allow(clippy::cast_precision_loss)] // counts far below 2^52
        let docs = self.len() as f64;
        let mut terms: Vec<&String> = terms.iter().collect();
        terms.sort();
        terms.dedup();
        let mut scores: HashMap<usize, (f64, [usize; FIELDS], Vec<usize>)> = HashMap::new();
        for (index, term) in terms.iter().enumerate() {
            let Some(postings) = self.postings.get(*term) else {
                continue;
            };
            let mut with: Vec<u32> = postings.iter().map(|p| p.doc).collect();
            with.dedup();
            #[allow(clippy::cast_precision_loss)]
            let df = with.len() as f64;
            let idf = ((docs - df + 0.5) / (df + 0.5)).ln_1p();
            for posting in postings {
                let doc = posting.doc as usize;
                if !allowed(doc) {
                    continue;
                }
                let field = posting.field as usize;
                #[allow(clippy::cast_precision_loss)]
                let average = (self.totals[field] as f64 / docs).max(1.0);
                let length = f64::from(self.lengths[doc][field]);
                let tf = f64::from(posting.count);
                let part = idf * tf * (K1 + 1.0) / (tf + K1 * (1.0 - B + B * length / average));
                let entry = scores.entry(doc).or_default();
                entry.0 += part * posting.field.weight();
                entry.1[field] += 1;
                if entry.2.last() != Some(&index) {
                    entry.2.push(index);
                }
            }
        }
        let mut ranked: Vec<Scored> = scores
            .into_iter()
            .map(|(doc, (score, fields, matched))| {
                #[allow(clippy::cast_precision_loss)]
                let share = matched.len() as f64 / terms.len() as f64;
                Scored {
                    doc,
                    score: score * share * share,
                    fields: Field::ALL
                        .into_iter()
                        .zip(fields)
                        .filter(|(_, n)| *n > 0)
                        .collect(),
                    terms: matched.iter().map(|&i| terms[i].clone()).collect(),
                }
            })
            .collect();
        ranked.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.doc.cmp(&b.doc)));
        ranked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(text: &str) -> Vec<String> {
        tokens(text).into_iter().map(|t| t.term).collect()
    }

    #[test]
    fn words_are_lowercased_and_plurals_dropped() {
        assert_eq!(
            terms("The Receipts for two Copies, and a lens."),
            ["receipt", "two", "copy", "len"]
        );
        assert_eq!(
            terms("glass address status"),
            ["glass", "address", "status"]
        );
    }

    #[test]
    fn words_with_punctuation_inside_are_kept_whole_and_in_parts() {
        let found = tokens("(invoice 2057-4038), AS1115.");
        let pieces: Vec<(&str, &str)> = found
            .iter()
            .map(|t| {
                (
                    t.term.as_str(),
                    &"(invoice 2057-4038), AS1115."[t.span.clone()],
                )
            })
            .collect();
        assert_eq!(
            pieces,
            [
                ("invoice", "invoice"),
                ("2057-4038", "2057-4038"),
                ("2057", "2057"),
                ("4038", "4038"),
                ("as1115", "AS1115"),
            ]
        );
        assert_eq!(terms("owner's"), ["owner's", "owner"]);
        assert_eq!(terms("café-crème"), ["café-crème", "café", "crème"]);
    }

    #[test]
    fn documents_with_every_term_and_rare_terms_rank_first() {
        let mut index = WordIndex::new();
        let lease = index.add(&[
            (Field::Title, "Lease for the flat"),
            (Field::Text, "lease lease lease"),
        ]);
        let invoice = index.add(&[
            (Field::Title, "Invoice"),
            (Field::Text, "Invoice 2057-4038 for the desk chair"),
        ]);
        let other = index.add(&[
            (Field::Title, "Invoice"),
            (Field::Text, "Invoice 2057-1111"),
        ]);
        let all = |_| true;
        let found = index.search(&terms("invoice 2057-4038"), &all);
        assert_eq!(found[0].doc, invoice);
        assert_eq!(found[1].doc, other);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].fields, [(Field::Title, 1), (Field::Text, 4)]);
        assert_eq!(found[0].terms, ["2057", "2057-4038", "4038", "invoice"]);
        assert_eq!(found[1].terms, ["2057", "invoice"]);
        assert_eq!(index.documents_with("2057-4038"), 1);
        assert_eq!(index.documents_with("invoice"), 2);
        assert_eq!(index.documents_with("nothing"), 0);

        let found = index.search(&terms("flat lease"), &all);
        assert_eq!(found[0].doc, lease);
        let found = index.search(&terms("invoice"), &|doc| doc != invoice);
        assert_eq!(
            found.iter().map(|s| s.doc).collect::<Vec<_>>(),
            [other],
            "filtered"
        );
        assert_eq!(index.search(&terms("nothing"), &all).len(), 0);
    }
}
