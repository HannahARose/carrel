//! Readings: when the drafting model isn't sure what an item is, the two or three things it
//! might be, most likely first. The first fills the card; the proposal keeps them all, so the
//! owner can pick another in Review or by answering the item's question.
//!
//! A file name, folder or text that one reading accounts for and another doesn't counts
//! against the other ([`weigh`]): the readings are reordered, and the proposal's confidence
//! drops, so the alternatives show.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::card::Tag;
use crate::correct::{Changes, Field, TagEdit, Worked};
use crate::questions::{QuestionEntry, QuestionKind, Suggestion};
use crate::review::{Confidence, ReviewEntry};

use super::card::{GENERIC_WORDS, machine_made};

/// The most readings a draft keeps: the card's and two alternatives.
pub const MAX_READINGS: usize = 3;

/// The longest reason, in characters.
pub const MAX_REASON: usize = 200;

/// The key a question's suggestion carries its reading's number under.
const READING_KEY: &str = "reading";

/// Folder names that say nothing about what's in them.
const PLAIN_FOLDERS: [&str; 10] = [
    "desktop",
    "documents",
    "downloads",
    "home",
    "inbox",
    "tmp",
    "temp",
    "users",
    "volumes",
    "private",
];

/// Short words that say nothing about what an item is.
const STOP_WORDS: [&str; 16] = [
    "and", "are", "for", "from", "has", "have", "its", "not", "our", "that", "the", "this",
    "was", "with", "you", "your",
];

/// One reading of what an item is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reading {
    /// The title it gives the card.
    pub title: String,
    /// The summary it gives the card.
    pub summary: String,
    /// The tags it gives the card.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
    /// Why the model read the item this way.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    /// What about the item doesn't fit this reading, such as "the file name
    /// (“keychain plate”)".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub against: Vec<String>,
}

/// Something about the item a reading should account for, and the words in it.
struct Clue {
    /// What it is, for people.
    label: String,
    /// Its words, lowercased.
    words: Vec<String>,
    /// Whether a reading accounts for it by mentioning one of its words (a name), rather
    /// than by its title's words appearing in it (the text).
    named: bool,
}

/// The words of `text` worth matching: lowercased, three letters or more, and not words
/// devices and programs put in file names.
fn words(text: &str) -> Vec<String> {
    let mut found: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| {
            w.chars().count() >= 3
                && w.chars().any(char::is_alphabetic)
                && !GENERIC_WORDS.contains(&w.as_str())
                && !STOP_WORDS.contains(&w.as_str())
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Whether two words are the same, allowing for endings: `plate` and `plates`.
fn same_word(a: &str, b: &str) -> bool {
    a == b || (a.len().min(b.len()) >= 4 && (a.starts_with(b) || b.starts_with(a)))
}

/// What a reading of the item at `path`, whose text is `text`, should account for: its file
/// name and folder when a person chose them, and its text.
fn clues(path: &Path, text: &str) -> Vec<Clue> {
    let mut clues = Vec::new();
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !machine_made(&stem) {
        clues.push(Clue {
            label: format!("the file name (“{stem}”)"),
            words: words(&stem),
            named: true,
        });
    }
    if let Some(folder) = chosen_folder(path) {
        clues.push(Clue {
            label: format!("the folder “{folder}”"),
            words: words(&folder),
            named: true,
        });
    }
    let text: String = text.chars().take(8_000).collect();
    if !text.trim().is_empty() {
        clues.push(Clue {
            label: "the item's text".to_owned(),
            words: words(&text),
            named: false,
        });
    }
    clues.retain(|c| !c.words.is_empty());
    clues
}

/// The name of the folder the item at `path` is in, when a person chose it to say something
/// about what's in it: not `Downloads` or a name a program made.
#[must_use]
pub(crate) fn chosen_folder(path: &Path) -> Option<String> {
    path.parent()
        .and_then(Path::file_name)
        .map(|f| f.to_string_lossy().into_owned())
        .filter(|f| !machine_made(f) && !PLAIN_FOLDERS.contains(&f.to_lowercase().as_str()))
}

/// Whether `reading` accounts for `clue`.
fn accounts_for(reading: &Reading, clue: &Clue) -> bool {
    if clue.named {
        let own = words(&format!(
            "{} {} {}",
            reading.title,
            reading.summary,
            reading
                .tags
                .iter()
                .map(|t| t.0.replace('-', " "))
                .collect::<Vec<_>>()
                .join(" ")
        ));
        clue.words
            .iter()
            .any(|w| own.iter().any(|o| same_word(w, o)))
    } else {
        words(&reading.title)
            .iter()
            .any(|w| clue.words.iter().any(|o| same_word(w, o)))
    }
}

/// Whether a card titled `title`, summarised as `summary`, says nothing of what the name of
/// the file at `path` says, when a person chose the name: worth asking the model whether it's
/// sure.
#[must_use]
pub fn ignores_name(path: &Path, title: &str, summary: &str) -> Option<String> {
    let reading = Reading {
        title: title.to_owned(),
        summary: summary.to_owned(),
        tags: Vec::new(),
        reason: String::new(),
        against: Vec::new(),
    };
    clues(path, "")
        .into_iter()
        .find(|c| c.label.starts_with("the file name") && !accounts_for(&reading, c))
        .map(|c| c.label)
}

/// Weighs `readings` of the item at `path`, whose text is `text`, against what the item
/// shows: a clue some readings account for counts against those that don't. The readings are
/// then ordered with the fewest against first, keeping the model's order otherwise. Returns
/// whether the first reading changed.
pub fn weigh(readings: &mut [Reading], path: &Path, text: &str) -> bool {
    if readings.len() < 2 {
        return false;
    }
    for reading in readings.iter_mut() {
        reading.against.clear();
    }
    for clue in clues(path, text) {
        let fits: Vec<bool> = readings.iter().map(|r| accounts_for(r, &clue)).collect();
        if fits.iter().any(|f| *f) {
            for (reading, fits) in readings.iter_mut().zip(fits) {
                if !fits {
                    reading.against.push(clue.label.clone());
                }
            }
        }
    }
    let first = readings[0].title.clone();
    readings.sort_by_key(|r| r.against.len());
    readings[0].title != first
}

/// Whether anything about the item counts against one of `readings`, so what the item is
/// isn't settled.
#[must_use]
pub fn doubted(readings: &[Reading]) -> bool {
    readings.iter().any(|r| !r.against.is_empty())
}

/// Lowers `entry`'s confidence when its readings are in doubt, unless a routing rule decided
/// where it goes, and says why.
pub fn lower_confidence(entry: &mut ReviewEntry) {
    if !doubted(&entry.readings) || entry.destination.confidence == Confidence::High {
        return;
    }
    entry.destination.confidence = Confidence::Low;
    entry.destination.reasons.push(format!(
        "what the item is isn't clear: {} readings, and {}",
        entry.readings.len(),
        doubts(&entry.readings)
    ));
}

/// What counts against the readings, in words, such as "the file name (“…”) doesn't fit “…”".
fn doubts(readings: &[Reading]) -> String {
    readings
        .iter()
        .filter(|r| !r.against.is_empty())
        .map(|r| format!("{} doesn't fit “{}”", r.against.join(" or "), r.title))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Which of `entry`'s readings its card shows now, counting from 0: the one whose title and
/// summary the card has, if any does.
#[must_use]
pub fn on_card(entry: &ReviewEntry) -> Option<usize> {
    entry.readings.iter().position(|r| {
        r.title == entry.card.title && Some(&r.summary) == entry.card.summary.as_ref()
    })
}

/// The number of the reading `words` pick, counting from 1, when they're a pick: `reading: 2`
/// or `reading 2`.
#[must_use]
pub fn picked(words: &str) -> Option<usize> {
    let rest = words.trim().strip_prefix(READING_KEY)?;
    rest.trim_start_matches([':', ' ']).trim().parse().ok()
}

/// The words for picking reading `n`: what [`picked`] reads.
#[must_use]
pub fn pick_words(n: usize) -> String {
    format!("{READING_KEY}: {n}")
}

/// The correction picking reading `n` (counting from 1) of `entry` makes: its title, summary
/// and tags, as the owner's, and routing again since what the item is changed. Also returns
/// the words it's kept as.
///
/// # Errors
///
/// A message when `entry` has no reading `n`.
pub fn pick(entry: &ReviewEntry, n: usize) -> Result<(Worked, String), String> {
    let Some(reading) = n.checked_sub(1).and_then(|i| entry.readings.get(i)) else {
        return Err(if entry.readings.is_empty() {
            "the proposal has no other readings".to_owned()
        } else {
            format!(
                "there's no reading {n}; pick 1 to {}",
                entry.readings.len()
            )
        });
    };
    let owner = |value| Field {
        value,
        dictated: true,
    };
    let remove = entry
        .card
        .tags
        .iter()
        .filter(|t| !reading.tags.contains(t))
        .cloned()
        .collect();
    let changes = Changes {
        title: (reading.title != entry.card.title).then(|| owner(reading.title.clone())),
        summary: (Some(&reading.summary) != entry.card.summary.as_ref())
            .then(|| owner(reading.summary.clone())),
        tags: (reading.tags != entry.card.tags).then(|| Field {
            value: TagEdit {
                add: reading.tags.clone(),
                remove,
                new: None,
            },
            dictated: true,
        }),
        reroute: true,
        ..Changes::default()
    };
    let words = format!("Picked reading {n}: “{}”", reading.title);
    Ok((
        Worked {
            changes,
            model: None,
            attempts: Vec::new(),
        },
        words,
    ))
}

/// The readings of `entry` as suggested answers to a question about it, each carrying its
/// number, so answering with one picks it ([`answer_words`]).
#[must_use]
pub fn suggestions(entry: &ReviewEntry) -> Vec<Suggestion> {
    entry
        .readings
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let mut suggestion = Suggestion::new(
                format!("{}: {}", r.title, r.summary),
                (!r.reason.is_empty()).then(|| r.reason.clone()),
            );
            suggestion
                .extra
                .insert(READING_KEY.into(), serde_json::json!(i + 1));
            suggestion
        })
        .collect()
}

/// The pick words for `answer` to `question`, when the answer is one of its readings.
#[must_use]
pub fn answer_words(question: &QuestionEntry, answer: &str) -> Option<String> {
    let suggestion = question.picked(answer)?;
    let n = suggestion.extra.get(READING_KEY)?.as_u64()?;
    Some(pick_words(usize::try_from(n).ok()?))
}

/// Takes the questions asking what the proposal `id` is off the queue, once the owner has
/// picked one of its readings.
///
/// # Errors
///
/// A queue error.
pub fn settle_questions(archive: &crate::archive::Archive, id: crate::card::Id) -> Result<(), crate::actions::ActionError> {
    let (asked, _) = crate::questions::QuestionQueue::of(archive).list()?;
    for question in asked.iter().filter(|q| {
        q.kind == QuestionKind::Item
            && q.items == [id]
            && q.suggestions.iter().any(|s| s.extra.contains_key(READING_KEY))
    }) {
        crate::questions::answer::settled(archive, question)?;
    }
    Ok(())
}

/// A question asking what `entry` is, when its readings are in doubt, with the readings to
/// pick from.
#[must_use]
pub fn question(entry: &ReviewEntry) -> Option<QuestionEntry> {
    if !doubted(&entry.readings) {
        return None;
    }
    let name = entry
        .source
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut asked = QuestionEntry::new(
        QuestionKind::Item,
        entry.card.title.clone(),
        format!("What is “{name}”?"),
    );
    asked.batch = Some(entry.batch);
    asked.items = vec![entry.id];
    asked.suggestions = suggestions(entry);
    Some(asked)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(title: &str, summary: &str) -> Reading {
        Reading {
            title: title.into(),
            summary: summary.into(),
            tags: Vec::new(),
            reason: String::new(),
            against: Vec::new(),
        }
    }

    #[test]
    fn a_file_name_that_fits_another_reading_puts_that_one_first() {
        let mut readings = vec![
            reading("3D-printed fish model", "A fish-shaped model with text."),
            reading(
                "Keychain plate",
                "A keychain plate with the Cognitogram lettering.",
            ),
        ];
        let path = Path::new("/Users/me/Downloads/cognitogram_keychain_plate copy.stl");
        assert!(weigh(&mut readings, path, ""));
        assert_eq!(readings[0].title, "Keychain plate");
        assert!(readings[0].against.is_empty());
        assert_eq!(
            readings[1].against,
            ["the file name (“cognitogram_keychain_plate copy”)"]
        );
        assert!(doubted(&readings));
    }

    #[test]
    fn readings_nothing_tells_apart_keep_their_order() {
        let mut readings = vec![reading("A fish", "A fish."), reading("A boat", "A boat.")];
        assert!(!weigh(
            &mut readings,
            Path::new("/Users/me/Downloads/IMG_2041.stl"),
            ""
        ));
        assert_eq!(readings[0].title, "A fish");
        assert!(!doubted(&readings));
    }

    #[test]
    fn the_text_counts_against_a_title_it_never_mentions() {
        let mut readings = vec![
            reading("Boiler service invoice", "x"),
            reading("Kitchen tap quote", "y"),
        ];
        weigh(
            &mut readings,
            Path::new("/tmp/scan0001.pdf"),
            "Quote for replacing the kitchen tap",
        );
        assert_eq!(readings[0].title, "Kitchen tap quote");
        assert_eq!(readings[1].against, ["the item's text"]);
    }

    #[test]
    fn a_card_that_ignores_a_chosen_name_is_noticed() {
        let path = Path::new("/a/cognitogram_keychain_plate copy.stl");
        assert!(ignores_name(path, "3D-printed fish model", "A fish.").is_some());
        assert!(ignores_name(path, "Keychain plates", "x").is_none());
        assert!(ignores_name(Path::new("/a/IMG_2041.jpg"), "A dog", "x").is_none());
    }

    #[test]
    fn pick_words_round_trip() {
        assert_eq!(picked(&pick_words(2)), Some(2));
        assert_eq!(picked("reading 3"), Some(3));
        assert_eq!(picked("it's a reading lamp"), None);
    }
}
