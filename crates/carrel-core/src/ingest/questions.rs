//! Stage 10: questions for the owner (`docs/design/ingest.md`). When the same unknown term
//! recurs in a batch's item names, one question replaces a guess on each item. When a known
//! term's items still can't be placed, one question asks where they go.
//!
//! Terms come from names, without a model, so the same batch always asks the same questions:
//! words that look like identifiers (`scope-ds`, `ABX`, `Cog2`, `CellTrack`) appearing in at
//! least two names, that the glossary, vocabularies and collection names don't already explain.

use std::collections::BTreeMap;

use crate::archive::{Archive, Collection};
use crate::card::Id;
use crate::ingest::card::{Glossary, Vocabulary};
use crate::questions::rank::unsureness;
use crate::questions::{QuestionEntry, QuestionKind, suggest};
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

/// The questions a batch raises, the most worth answering first: those whose answer would
/// settle the most items Carrel is least sure of. `entries` are the batch's proposals.
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

    let batch = entries.first().map(|e| e.batch);
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
        let mut entry = QuestionEntry::new(kind, written.clone(), question);
        entry.batch = batch;
        entry.items = waiting.iter().map(|e| e.id).collect();
        if kind == QuestionKind::Collection {
            entry.choices.clone_from(&choices);
            entry.suggestions = suggest::destinations(&waiting);
        }
        questions.push(entry);
    }
    questions.extend(groups(entries, &known, &questions, &choices, batch));
    let value = |q: &QuestionEntry| -> f64 {
        q.items
            .iter()
            .filter_map(|id| entries.iter().find(|e| e.id == *id))
            .map(|e| unsureness(e.destination.confidence))
            .sum()
    };
    questions.sort_by(|a, b| {
        value(b)
            .total_cmp(&value(a))
            .then(b.items.len().cmp(&a.items.len()))
    });
    questions
}

/// The least items a group question gathers.
pub const GROUP_MIN: usize = 3;

/// Group questions: at least [`GROUP_MIN`] proposals whose names share a known term or a name
/// (`Dave's Hot Chicken` receipts, `Northbridge Bank` statements), split between collections or
/// not surely placed, that no other question already covers. Each offers where they were
/// going, and a new collection named for them.
fn groups(
    entries: &[ReviewEntry],
    known: &[String],
    asked: &[QuestionEntry],
    choices: &[String],
    batch: Option<Id>,
) -> Vec<QuestionEntry> {
    let mut by_name: BTreeMap<String, (String, Vec<&ReviewEntry>)> = BTreeMap::new();
    for entry in entries {
        let Some(name) = entry.source.path.file_name() else {
            continue;
        };
        let name = name.to_string_lossy();
        let mut found = crate::questions::about::names_in(&name);
        found.extend(
            terms_in(&name)
                .into_iter()
                .filter(|t| known.contains(&t.to_lowercase())),
        );
        for subject in found {
            let slot = by_name
                .entry(subject.to_lowercase())
                .or_insert_with(|| (subject.clone(), Vec::new()));
            if !slot.1.iter().any(|e| e.id == entry.id) {
                slot.1.push(entry);
            }
        }
    }
    let mut questions: Vec<QuestionEntry> = Vec::new();
    for (written, items) in by_name.into_values() {
        if items.len() < GROUP_MIN {
            continue;
        }
        let mut places: Vec<&str> = items.iter().map(|e| e.destination.path.as_str()).collect();
        places.sort_unstable();
        places.dedup();
        let unsure = items.iter().any(|e| {
            e.destination.confidence != Confidence::High || e.destination.path == "Holding"
        });
        if places.len() < 2 && !unsure {
            continue;
        }
        let mut suggestions = suggest::destinations(&items);
        let main = suggestions
            .first()
            .map_or_else(String::new, |s| format!("{}/", s.answer));
        let new_path = format!("{main}{written}");
        let new_path_known = choices.iter().any(|c| c.eq_ignore_ascii_case(&new_path));
        if !new_path_known && crate::names::problems(&written).is_empty() {
            suggestions.push(suggest::new_collection(
                &new_path,
                format!("a collection of their own, for all {}", items.len()),
            ));
        }
        let options: Vec<String> = suggestions
            .iter()
            .map(|s| {
                if s.new_collection {
                    format!("a new {} collection", s.answer)
                } else {
                    s.answer.clone()
                }
            })
            .collect();
        let question = format!(
            "These {} items about “{written}”: {}?",
            items.len(),
            match options.as_slice() {
                [] => "where do they go".to_owned(),
                [one] => one.clone(),
                [rest @ .., last] => format!("{}, or {last}", rest.join(", ")),
            }
        );
        let mut entry = QuestionEntry::new(QuestionKind::Group, written, question);
        entry.batch = batch;
        entry.items = items.iter().map(|e| e.id).collect();
        entry.choices = choices.to_vec();
        entry.suggestions = suggestions;
        if asked
            .iter()
            .chain(&questions)
            .any(|q| crate::questions::rank::same(q, &entry) || q.items == entry.items)
        {
            continue;
        }
        questions.push(entry);
    }
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
