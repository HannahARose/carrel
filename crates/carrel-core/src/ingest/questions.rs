//! Stage 10: questions for the owner (`docs/design/ingest.md`). When the same unknown term
//! recurs in a batch's item names, one question replaces a guess on each item. When a known
//! term's items still can't be placed, one question asks where they go.
//!
//! Terms come from names, without a model, so the same batch always asks the same questions:
//! words that look like identifiers (`scope-ds`, `ABX`, `Cog2`, `CellTrack`) appearing in at
//! least two names, that the glossary, vocabularies and collection names don't already explain.

use std::collections::BTreeMap;

use crate::archive::{Archive, Collection};
use crate::card::{DateOrTime, Extra, Id};
use crate::ingest::card::{Glossary, Vocabulary};
use crate::questions::{QuestionEntry, QuestionKind, QuestionMarker};
use crate::review::{Confidence, ReviewEntry};

/// Words that recur in names without naming anything.
const COMMON: [&str; 48] = [
    "a",
    "an",
    "and",
    "the",
    "of",
    "for",
    "to",
    "in",
    "on",
    "at",
    "by",
    "with",
    "copy",
    "final",
    "draft",
    "new",
    "old",
    "scan",
    "scans",
    "notes",
    "note",
    "untitled",
    "document",
    "doc",
    "file",
    "files",
    "edit",
    "edited",
    "backup",
    "export",
    "img",
    "image",
    "photo",
    "screenshot",
    "pdf",
    "jpg",
    "jpeg",
    "png",
    "dsc",
    "readme",
    "todo",
    "id",
    "ok",
    "am",
    "pm",
    "us",
    "uk",
    "eu",
];

/// A part of a name that's a number, a date or a version, and names nothing on its own.
fn is_numberish(piece: &str) -> bool {
    let letters = piece.chars().filter(char::is_ascii_alphabetic).count();
    letters == 0
        || (piece.len() <= 4
            && piece.starts_with(['v', 'V', 'q', 'Q'])
            && piece[1..].chars().all(|c| c.is_ascii_digit()))
}

/// Whether one word looks like an identifier rather than an ordinary word: an acronym, a
/// mix of letters and digits, or `CamelCase`.
fn looks_like_identifier(word: &str) -> bool {
    let letters = word.chars().filter(char::is_ascii_alphabetic).count();
    let digits = word.chars().filter(char::is_ascii_digit).count();
    let all_caps = letters >= 2
        && word
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    let camel = word.chars().skip(1).any(|c| c.is_ascii_uppercase())
        && word.chars().any(|c| c.is_ascii_lowercase());
    (all_caps && word.len() <= 6) || (letters >= 2 && digits >= 1) || camel
}

/// The candidate terms in an item's name: hyphenated runs of two or more parts (`scope-ds` in
/// `scope-ds-plan.pdf`), and single words that look like identifiers.
#[must_use]
pub fn terms_in(name: &str) -> Vec<String> {
    let stem = match name.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && ext.len() <= 5
                && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            stem
        }
        _ => name,
    };
    let mut found = Vec::new();
    for word in stem.split(|c: char| c.is_whitespace() || "_.,()[]{}+&".contains(c)) {
        let pieces: Vec<&str> = word.split('-').filter(|p| !p.is_empty()).collect();
        for (i, piece) in pieces.iter().enumerate() {
            if looks_like_identifier(piece)
                && !is_numberish(piece)
                && !COMMON.contains(&piece.to_lowercase().as_str())
            {
                found.push((*piece).to_owned());
            }
            for end in i + 2..=pieces.len() {
                let run = &pieces[i..end];
                if run.iter().any(|p| is_numberish(p)) {
                    break;
                }
                if run
                    .iter()
                    .all(|p| COMMON.contains(&p.to_lowercase().as_str()))
                {
                    continue;
                }
                found.push(run.join("-"));
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// What the archive already explains: glossary terms, tags, collection folder names and the
/// archive's name, lowercased.
fn known_terms(archive: &Archive, collections: &[Collection], glossary: &Glossary) -> Vec<String> {
    let mut known: Vec<String> = glossary
        .facts
        .iter()
        .map(|(t, _)| t.to_lowercase())
        .collect();
    known.extend(
        Vocabulary::of(archive, collections)
            .tags
            .iter()
            .map(|(t, _)| t.0.to_lowercase()),
    );
    for collection in collections {
        known.extend(collection.path.split('/').map(str::to_lowercase));
        known.push(collection.doc.title.to_lowercase());
    }
    known.push(archive.doc().name.to_lowercase());
    known
}

/// The questions a batch raises, most items first. `entries` are the batch's proposals.
#[must_use]
pub fn ask(
    archive: &Archive,
    collections: &[Collection],
    glossary: &Glossary,
    entries: &[ReviewEntry],
) -> Vec<QuestionEntry> {
    let in_glossary: Vec<String> = glossary
        .facts
        .iter()
        .map(|(t, _)| t.to_lowercase())
        .collect();
    let known = known_terms(archive, collections, glossary);

    // Each term (lowercased), the way it was first written, and the proposals using it.
    let mut by_term: BTreeMap<String, (String, Vec<&ReviewEntry>)> = BTreeMap::new();
    for entry in entries {
        let Some(name) = entry.source.path.file_name() else {
            continue;
        };
        for term in terms_in(&name.to_string_lossy()) {
            let slot = by_term
                .entry(term.to_lowercase())
                .or_insert_with(|| (term.clone(), Vec::new()));
            if !slot.1.iter().any(|e| e.id == entry.id) {
                slot.1.push(entry);
            }
        }
    }
    by_term.retain(|_, (_, items)| items.len() >= 2);
    // A shorter term that always appears inside a longer one adds nothing.
    let sets: Vec<(String, Vec<Id>)> = by_term
        .iter()
        .map(|(k, (_, items))| (k.clone(), items.iter().map(|e| e.id).collect()))
        .collect();
    by_term.retain(|term, (_, items)| {
        let ids: Vec<Id> = items.iter().map(|e| e.id).collect();
        !sets.iter().any(|(other, other_ids)| {
            other != term && other.contains(term.as_str()) && *other_ids == ids
        })
    });

    let batch = entries.first().map_or_else(Id::new_v7, |e| e.batch);
    let now = DateOrTime(
        jiff::Zoned::now()
            .strftime("%Y-%m-%dT%H:%M:%S%:z")
            .to_string(),
    );
    let choices: Vec<String> = collections
        .iter()
        .filter(|c| c.path != "Holding")
        .map(|c| c.path.clone())
        .collect();
    let mut questions = Vec::new();
    for (term, (written, items)) in &by_term {
        let (kind, question, waiting) = if !known.contains(term) {
            (
                QuestionKind::Term,
                format!("What is “{written}”?"),
                items.clone(),
            )
        } else if in_glossary.contains(term) {
            let unplaced: Vec<&ReviewEntry> = items
                .iter()
                .copied()
                .filter(|e| e.destination.confidence == Confidence::Low)
                .collect();
            if unplaced.len() < 2 || choices.is_empty() {
                continue;
            }
            (
                QuestionKind::Collection,
                format!("Which collection do items about “{written}” belong in?"),
                unplaced,
            )
        } else {
            continue;
        };
        questions.push(QuestionEntry {
            carrel: QuestionMarker::Question,
            format: crate::FORMAT_VERSION,
            id: Id::new_v7(),
            batch,
            created: now.clone(),
            kind,
            subject: written.clone(),
            question,
            items: waiting.iter().map(|e| e.id).collect(),
            choices: if kind == QuestionKind::Collection {
                choices.clone()
            } else {
                Vec::new()
            },
            extra: Extra::new(),
        });
    }
    questions.sort_by_key(|q| std::cmp::Reverse(q.items.len()));
    questions
}

#[cfg(test)]
mod tests {
    use super::terms_in;

    #[test]
    fn terms_are_identifier_like_parts_of_names() {
        assert_eq!(
            terms_in("scope-ds-plan.pdf"),
            ["ds-plan", "scope-ds", "scope-ds-plan"]
        );
        assert_eq!(terms_in("ABX results 2024.xlsx"), ["ABX"]);
        assert_eq!(terms_in("CellTrack_v2.zip"), ["CellTrack"]);
        assert_eq!(terms_in("report-2024-q1.pdf"), Vec::<String>::new());
        assert_eq!(terms_in("final copy.docx"), Vec::<String>::new());
        assert_eq!(terms_in("IMG_2041.jpg"), Vec::<String>::new());
        assert_eq!(terms_in("lamp notes.txt"), Vec::<String>::new());
    }
}
