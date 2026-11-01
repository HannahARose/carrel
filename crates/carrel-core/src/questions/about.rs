//! The questions Carrel has about items already filed (`docs/design/librarian.md`,
//! "Questions"): about one item, while browsing it or when the librarian comes across it ("what
//! does this photo show?", "who is J. Okafor?", "what was this project for?", "does this still
//! matter?"), and about a sender or project that recurs across the archive ("should Northbridge
//! Bank have its own collection?").
//!
//! Like ingest's questions, these come from names, titles and dates, without a model, so the
//! same archive always raises the same questions. A model may add suggested answers
//! ([`super::suggest::with_model`]).

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::archive::ItemRef;
use crate::card::{Card, Id, Kind};
use crate::ingest::card::Glossary;
use crate::review::ReviewEntry;

use super::rank::Context;
use super::suggest::{new_collection, no_change};
use super::{QuestionEntry, QuestionKind};

/// How many items a sender or project needs before Carrel asks whether it deserves its own
/// collection.
pub const SENDER_MIN: usize = 5;
/// How many years an item can go untouched before Carrel asks whether it still matters.
pub const STALE_YEARS: i16 = 3;

/// Extensions of photos.
const PHOTOS: [&str; 8] = ["jpg", "jpeg", "png", "heic", "heif", "tif", "tiff", "webp"];

/// Words that say what kind of document something is, rather than who it's from.
const GENERIC: [&str; 44] = [
    "statement",
    "statements",
    "receipt",
    "receipts",
    "invoice",
    "invoices",
    "letter",
    "letters",
    "report",
    "reports",
    "notes",
    "note",
    "plan",
    "scan",
    "scans",
    "copy",
    "final",
    "draft",
    "photo",
    "photos",
    "document",
    "documents",
    "form",
    "forms",
    "contract",
    "bill",
    "bills",
    "summary",
    "minutes",
    "agenda",
    "slides",
    "budget",
    "results",
    "application",
    "certificate",
    "ticket",
    "order",
    "confirmation",
    "quote",
    "estimate",
    "untitled",
    "screenshot",
    "image",
    "new",
];

/// Months and days, which are capitalized without naming anyone.
const CALENDAR: [&str; 31] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
    "jan",
    "feb",
    "mar",
    "apr",
    "jun",
    "jul",
    "aug",
    "sep",
    "sept",
    "oct",
    "nov",
    "dec",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

static CAMERA_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(img|dsc|dscf|dscn|pxl|dcim|photo|screenshot|screen shot|image)[ _-]?\d")
        .expect("a valid pattern")
});

static PERSON: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(?:(?:[A-Z]\.\s?)+[A-Z][a-z]+(?:-[A-Z][a-z]+)?|(?:Dr|Mr|Mrs|Ms|Prof)\.?\s[A-Z][a-z]+)\b",
    )
    .expect("a valid pattern")
});

/// The capitalized names in an item's name or title, such as "Northbridge Bank" in
/// `Northbridge Bank statement 2024-03.pdf`: runs of one to three capitalized words, without
/// words that say what kind of document it is, months and days.
#[must_use]
pub fn names_in(name: &str) -> Vec<String> {
    let stem = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.len() <= 5 => stem,
        _ => name,
    };
    let mut found = Vec::new();
    let mut run: Vec<&str> = Vec::new();
    let close = |run: &mut Vec<&str>, found: &mut Vec<String>| {
        if !run.is_empty() && run.len() <= 3 {
            found.push(run.join(" "));
        }
        run.clear();
    };
    for word in stem.split(|c: char| c.is_whitespace() || "_-.,()[]{}+&".contains(c)) {
        let lower = word.to_lowercase();
        let named = word.chars().next().is_some_and(char::is_uppercase)
            && word.chars().count() >= 3
            && word.chars().skip(1).all(|c| c.is_lowercase() || c == '\'')
            && !GENERIC.contains(&lower.as_str())
            && !CALENDAR.contains(&lower.as_str())
            && !lower.ends_with("'s");
        if named {
            run.push(word);
        } else {
            close(&mut run, &mut found);
        }
    }
    close(&mut run, &mut found);
    found.sort();
    found.dedup();
    found
}

/// A question about the filed `item`.
fn about_item(item: &ItemRef, card: &Card, question: String) -> QuestionEntry {
    let mut entry = QuestionEntry::new(QuestionKind::Item, card.title.clone(), question);
    entry.about = vec![item.id];
    entry
}

/// The questions Carrel has about the filed `item`, whose card is `card`, on `today`: what a
/// photo with no description shows, who a person named in it is when the glossary doesn't say,
/// what a project with no description was for, and whether something untouched for years
/// still matters.
#[must_use]
pub fn item(
    item: &ItemRef,
    card: &Card,
    glossary: &Glossary,
    today: jiff::civil::Date,
) -> Vec<QuestionEntry> {
    let mut questions = Vec::new();
    let name = item
        .item
        .as_deref()
        .unwrap_or(&item.card)
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let undescribed = card.summary.as_deref().is_none_or(|s| s.trim().len() < 40);
    let photo = name
        .rsplit_once('.')
        .is_some_and(|(_, ext)| PHOTOS.contains(&ext.to_lowercase().as_str()));
    if photo && (undescribed || CAMERA_NAME.is_match(&card.title)) {
        questions.push(about_item(item, card, "What does this photo show?".into()));
    }

    let known: Vec<String> = glossary
        .facts
        .iter()
        .map(|(t, _)| t.to_lowercase())
        .collect();
    let text = format!(
        "{} {}",
        card.title,
        card.summary.as_deref().unwrap_or_default()
    );
    let mut people: Vec<&str> = PERSON.find_iter(&text).map(|m| m.as_str()).collect();
    people.dedup();
    for person in people
        .into_iter()
        .filter(|p| !known.contains(&p.to_lowercase()))
    {
        let mut question = about_item(item, card, format!("Who is {person}?"));
        person.clone_into(&mut question.subject);
        questions.push(question);
    }

    let in_projects = item
        .card
        .split('/')
        .any(|part| part.eq_ignore_ascii_case("projects"));
    if in_projects && matches!(item.kind, Kind::Folder | Kind::Bundle) && undescribed {
        questions.push(about_item(item, card, "What was this project for?".into()));
    }

    let last = card.dates.as_ref().and_then(|d| {
        [&d.worked, &d.modified, &d.created, &d.added]
            .into_iter()
            .flatten()
            .filter_map(|t| t.0.get(..10)?.parse::<jiff::civil::Date>().ok())
            .max()
    });
    let stale = last.is_some_and(|last| {
        last.checked_add(jiff::Span::new().years(STALE_YEARS))
            .is_ok_and(|due| due < today)
    });
    if stale && card.checked_out.is_none() {
        let mut question = about_item(item, card, "Does this still matter?".into());
        question.suggestions = vec![
            no_change(
                "Yes, it still matters",
                Some("nothing changes; Carrel won't ask again".into()),
            ),
            super::Suggestion::new(
                "No, it's only kept for the record",
                Some("the card says so, through a correction you confirm".into()),
            ),
        ];
        questions.push(question);
    }
    questions
}

/// Where a sender's or project's items are, gathered.
#[derive(Default)]
struct Seen {
    /// The name as first written.
    written: String,
    /// Filed items, with their collections.
    filed: Vec<(Id, String)>,
    /// Open proposals, with where they're going.
    proposals: Vec<(Id, String)>,
}

/// The collection a filed item is in: its card's folder.
fn folder_of(card: &str) -> String {
    card.rsplit_once('/')
        .map_or_else(String::new, |(f, _)| f.to_owned())
}

/// The questions about senders and projects that recur across the archive: a name shared by at
/// least [`SENDER_MIN`] items, filed (`items`, with their names) and waiting (`proposals`),
/// mostly in one collection, that has no collection of its own and no rule filing it.
#[must_use]
pub fn senders(
    items: &[(ItemRef, String)],
    proposals: &[ReviewEntry],
    context: &Context,
) -> Vec<QuestionEntry> {
    let mut by_name: BTreeMap<String, Seen> = BTreeMap::new();
    let mut note = |name: &str, add: &mut dyn FnMut(&mut Seen)| {
        for found in names_in(name) {
            let seen = by_name.entry(found.to_lowercase()).or_default();
            if seen.written.is_empty() {
                seen.written.clone_from(&found);
            }
            add(seen);
        }
    };
    for (item, name) in items {
        note(name, &mut |seen| {
            seen.filed.push((item.id, folder_of(&item.card)));
        });
    }
    for entry in proposals.iter().filter(|e| e.state.is_open()) {
        let Some(name) = entry.source.path.file_name() else {
            continue;
        };
        note(&name.to_string_lossy(), &mut |seen| {
            seen.proposals
                .push((entry.id, entry.destination.path.clone()));
        });
    }

    let mut questions = Vec::new();
    for seen in by_name.into_values() {
        let total = seen.filed.len() + seen.proposals.len();
        if total < SENDER_MIN {
            continue;
        }
        let mut places: Vec<(&str, usize)> = Vec::new();
        for (_, place) in seen.filed.iter().chain(&seen.proposals) {
            match places.iter_mut().find(|(p, _)| p == place) {
                Some((_, n)) => *n += 1,
                None => places.push((place, 1)),
            }
        }
        places.sort_by_key(|p| std::cmp::Reverse(p.1));
        let Some(&(main, most)) = places.first() else {
            continue;
        };
        // Spread across collections, it's a group to place, not a sender to gather.
        if main.is_empty() || main == "Holding" || most * 5 < total * 3 {
            continue;
        }
        let subject = seen.written.clone();
        let mut question = QuestionEntry::new(
            QuestionKind::Sender,
            subject.clone(),
            format!("Should “{subject}” have its own collection?"),
        );
        question.items = seen.proposals.iter().map(|(id, _)| *id).collect();
        question.about = seen.filed.iter().map(|(id, _)| *id).collect();
        question.choices = vec![format!("{main}/{subject}")];
        question.suggestions = vec![
            new_collection(
                &format!("{main}/{subject}"),
                format!("{most} of its {total} items are in {main}"),
            ),
            no_change(
                &format!("No, keep them in {main}"),
                Some("Carrel won't ask again".into()),
            ),
        ];
        if context.answered(&question).is_none() {
            questions.push(question);
        }
    }
    questions
}

/// The sender and project questions across `archive`, its filed items and the open
/// proposals among `proposals`.
///
/// # Errors
///
/// [`ArchiveError`] if the collections or items can't be read.
pub fn senders_in(
    archive: &crate::archive::Archive,
    proposals: &[ReviewEntry],
) -> Result<Vec<QuestionEntry>, crate::archive::ArchiveError> {
    let collections = archive.collections()?;
    let items = archive.items()?;
    let context = Context::new(
        archive,
        proposals,
        items.iter().map(|i| i.id).collect(),
        collections,
    );
    let named: Vec<(ItemRef, String)> = items
        .into_iter()
        .map(|i| {
            let name = i
                .item
                .as_deref()
                .unwrap_or_else(|| i.card.trim_end_matches(".md"))
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned();
            (i, name)
        })
        .collect();
    Ok(senders(&named, proposals, &context))
}

#[cfg(test)]
mod tests {
    use super::names_in;

    #[test]
    fn names_are_capitalized_runs_without_document_words() {
        assert_eq!(
            names_in("Northbridge Bank statement 2024-03.pdf"),
            ["Northbridge Bank"]
        );
        assert_eq!(names_in("Receipt March 2024.pdf"), Vec::<String>::new());
        assert_eq!(names_in("scan_0001.jpg"), Vec::<String>::new());
        assert_eq!(
            names_in("Okafor lab notes - Cowley Road.txt"),
            ["Cowley Road", "Okafor"]
        );
    }
}
