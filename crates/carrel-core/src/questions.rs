//! The question queue and the glossary (FORMAT.md §7.3 and §7.9, `docs/design/ingest.md` and
//! the "Questions" section of `docs/design/librarian.md`).
//!
//! When several items depend on the same unknown, Carrel asks the owner one question rather
//! than guessing on each. Questions come in five kinds: what a term means, which collection a
//! term's items belong in, what an item is ([`QuestionKind::Item`]), where a group of items
//! goes ([`QuestionKind::Group`]), and whether a sender or project deserves its own collection
//! ([`QuestionKind::Sender`]). There's one queue for all of them, ranked by what an answer
//! would settle ([`rank`]), each question with likely answers to pick from ([`suggest`]), and
//! a skipped question rests for a while before it's asked again.
//!
//! Answers about terms become glossary facts, which every model reads; answers about where
//! items go redirect their proposals or refile them; answers about an item go through the
//! corrections path ([`crate::correct`]). Nothing becomes a glossary fact or a routing rule
//! without the owner's yes ([`answer`]).

pub mod about;
pub mod answer;
pub mod moments;
pub mod rank;
pub mod suggest;

use serde::{Deserialize, Serialize};

use crate::FormatVersion;
use crate::archive::{Archive, Collection};
use crate::card::{DateOrTime, Extra, Id};
use crate::review::{Confidence, Destination, Queue, QueueEntry, ReviewEntry, State};

/// Where question entries live, relative to the archive root.
pub const QUESTIONS_DIR: &str = "_System/queue/questions";

/// The glossary, relative to the archive root.
pub const GLOSSARY: &str = "_System/glossary.md";

/// The document type of a question entry; always `question`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionMarker {
    /// The only value.
    Question,
}

/// What a question asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionKind {
    /// What a recurring term means. The answer becomes a glossary fact.
    Term,
    /// Which collection items about a term belong in. The answer is a collection path.
    Collection,
    /// Something about one item: what a photo shows, who someone named in it is, what a
    /// project was for, whether it still matters. The answer is a correction of its card.
    Item,
    /// Where a group of items that belong together goes: a collection they're split across,
    /// or a new one. The answer is a collection path.
    Group,
    /// Whether a sender or project that recurs deserves its own collection. The answer is yes,
    /// with the collection's path, or no.
    Sender,
}

impl QuestionKind {
    /// Its name in question files.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Term => "term",
            Self::Collection => "collection",
            Self::Item => "item",
            Self::Group => "group",
            Self::Sender => "sender",
        }
    }

    /// Whether its answer is a collection's path.
    #[must_use]
    pub fn answers_with_collection(self) -> bool {
        matches!(self, Self::Collection | Self::Group | Self::Sender)
    }
}

/// A likely answer to a question, offered to pick from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suggestion {
    /// The answer, as the owner would give it: a collection's path, a sentence, or words for
    /// a correction.
    pub answer: String,
    /// Why it's likely, for people, such as "where 9 of them were going".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// For a collection answer, whether the collection doesn't exist yet and picking this
    /// creates it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub new_collection: bool,
    /// Whether picking it settles the question without changing anything, such as "it still
    /// matters" or "no, it doesn't need its own collection".
    #[serde(default, skip_serializing_if = "is_false")]
    pub no_change: bool,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Suggestion {
    /// A plain suggestion: `answer`, likely because of `why`.
    #[must_use]
    pub fn new(answer: impl Into<String>, why: Option<String>) -> Self {
        Self {
            answer: answer.into(),
            why,
            new_collection: false,
            no_change: false,
            extra: Extra::new(),
        }
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip_serializing_if passes a reference
fn is_false(value: &bool) -> bool {
    !*value
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip_serializing_if passes a reference
fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// A question for the owner: `_System/queue/questions/<id>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionEntry {
    /// Document type; always `question`.
    pub carrel: QuestionMarker,
    /// The format version it was written with.
    pub format: FormatVersion,
    /// The question's identifier.
    pub id: Id,
    /// The ingest batch that raised it; none for questions raised outside ingest, such as
    /// about an item while browsing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch: Option<Id>,
    /// When it was asked.
    pub created: DateOrTime,
    /// What it asks.
    pub kind: QuestionKind,
    /// What it's about: the term as it appears in the items' names, the sender or project, or
    /// for an item question, the item's title.
    pub subject: String,
    /// The question, in words.
    pub question: String,
    /// The proposals waiting on the answer.
    #[serde(default)]
    pub items: Vec<Id>,
    /// Filed items the question is about, by their identifiers: an item question's item, or
    /// a group's or sender's items already in the archive.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub about: Vec<Id>,
    /// For a collection, group or sender question, the collections to choose from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    /// Likely answers to pick from, most likely first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<Suggestion>,
    /// When the owner last skipped it. It rests, out of the queue, for the archive's
    /// `questions.skip_days` after that.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped: Option<DateOrTime>,
    /// How many times it's been skipped.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub skips: u32,
    /// When the owner answered no to it, for a question whose "no" is worth remembering (a
    /// sender that doesn't need its own collection). It's never asked again, and isn't shown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declined: Option<DateOrTime>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl QueueEntry for QuestionEntry {
    const DIR: &'static str = QUESTIONS_DIR;
    const NOUN: &'static str = "question";

    fn id(&self) -> Id {
        self.id
    }

    fn check(&self) -> Result<(), String> {
        if self.subject.trim().is_empty() {
            return Err("the subject is empty".into());
        }
        if self.kind == QuestionKind::Item && self.items.len() + self.about.len() != 1 {
            return Err("an item question is about exactly one item".into());
        }
        Ok(())
    }
}

impl QuestionEntry {
    /// A new question of `kind` about `subject`, asked now, waiting on nothing yet.
    #[must_use]
    pub fn new(
        kind: QuestionKind,
        subject: impl Into<String>,
        question: impl Into<String>,
    ) -> Self {
        Self {
            carrel: QuestionMarker::Question,
            format: crate::FORMAT_VERSION,
            id: Id::new_v7(),
            batch: None,
            created: now(),
            kind,
            subject: subject.into(),
            question: question.into(),
            items: Vec::new(),
            about: Vec::new(),
            choices: Vec::new(),
            suggestions: Vec::new(),
            skipped: None,
            skips: 0,
            declined: None,
            extra: Extra::new(),
        }
    }

    /// When it can be asked again after a skip, `days` after it was skipped; none when it
    /// wasn't skipped or the date can't be read.
    #[must_use]
    pub fn rests_until(&self, days: u32) -> Option<jiff::Timestamp> {
        let skipped = timestamp_of(self.skipped.as_ref()?)?;
        skipped
            .checked_add(jiff::SignedDuration::from_hours(i64::from(days) * 24))
            .ok()
    }

    /// Whether it's resting after a skip at `now`, for an archive whose skipped questions rest
    /// `days`.
    #[must_use]
    pub fn is_resting(&self, now: jiff::Timestamp, days: u32) -> bool {
        self.rests_until(days).is_some_and(|until| now < until)
    }

    /// The suggestion `answer` picks: a number counting from 1, or a suggestion's own words.
    #[must_use]
    pub fn picked(&self, answer: &str) -> Option<&Suggestion> {
        let answer = answer.trim();
        if let Ok(n) = answer.parse::<usize>() {
            return n.checked_sub(1).and_then(|i| self.suggestions.get(i));
        }
        self.suggestions
            .iter()
            .find(|s| s.answer.eq_ignore_ascii_case(answer))
    }
}

/// `time` as a moment: a timestamp as it is, a date at its start in UTC.
#[must_use]
pub fn timestamp_of(time: &DateOrTime) -> Option<jiff::Timestamp> {
    if time.has_time() {
        return time.0.parse().ok();
    }
    let date: jiff::civil::Date = time.0.parse().ok()?;
    date.to_zoned(jiff::tz::TimeZone::UTC)
        .ok()
        .map(|z| z.timestamp())
}

/// Now, as question files write times.
#[must_use]
pub fn now() -> DateOrTime {
    DateOrTime(
        jiff::Zoned::now()
            .strftime("%Y-%m-%dT%H:%M:%S%:z")
            .to_string(),
    )
}

/// The question queue.
pub type QuestionQueue = Queue<QuestionEntry>;

/// The stage name of the problem that marks a proposal waiting on a question.
pub const QUESTION_STAGE: &str = "question";

/// Marks `entry` as waiting on any of `questions` that name it: a `question` problem each,
/// and `needs-owner`, so it isn't approved in bulk before the answer.
pub fn mark_waiting(entry: &mut ReviewEntry, questions: &[QuestionEntry]) {
    for question in questions.iter().filter(|q| q.items.contains(&entry.id)) {
        let tag = crate::review::short_id(question.id);
        if entry
            .problems
            .iter()
            .any(|p| p.stage == QUESTION_STAGE && p.message.contains(&tag))
        {
            continue;
        }
        entry.problems.push(crate::review::Problem {
            stage: QUESTION_STAGE.into(),
            message: format!(
                "waits for an answer to {} ({})",
                crate::review::short_id(question.id),
                question.question
            ),
        });
        entry.state = State::NeedsOwner;
    }
}

/// Clears what `question` put on `entry`, so it's back to `proposed` if nothing else needs
/// the owner.
pub fn clear_waiting(entry: &mut ReviewEntry, question: &QuestionEntry) {
    let tag = crate::review::short_id(question.id);
    entry
        .problems
        .retain(|p| !(p.stage == QUESTION_STAGE && p.message.contains(&tag)));
    if entry.state == State::NeedsOwner && entry.problems.is_empty() {
        entry.state = State::Proposed;
    }
}

/// Adds a fact to the glossary, newest first, creating the glossary if there is none.
/// Returns the fact's identifier.
///
/// # Errors
///
/// A file error.
pub fn add_fact(archive: &Archive, term: &str, fact: &str) -> std::io::Result<Id> {
    let path = archive.root().join(GLOSSARY);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "# Glossary\n".into(),
        Err(e) => return Err(e),
    };
    let id = Id::new_v7();
    let entry = format!(
        "### {}\n{}\n<!-- id: {id}  added: {} -->\n",
        term.trim(),
        fact.split_whitespace().collect::<Vec<_>>().join(" "),
        jiff::Zoned::now().strftime("%Y-%m-%d")
    );
    // Before the newest fact, or at the end when there are none.
    let at = text.find("\n### ").map_or(text.len(), |i| i + 1);
    let (head, rest) = text.split_at(at);
    let mut updated = head.trim_end_matches('\n').to_owned();
    updated.push_str("\n\n");
    updated.push_str(&entry);
    if !rest.is_empty() {
        updated.push('\n');
        updated.push_str(rest);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension(format!("tmp-{id}"));
    std::fs::write(&temp, updated)?;
    std::fs::rename(&temp, &path)?;
    Ok(id)
}

/// Points `entry` at `collection`, chosen by the owner: the destination becomes certain, and
/// tags that don't apply there are left off. Returns the tags left off.
pub fn redirect(
    archive: &Archive,
    collections: &[Collection],
    entry: &mut ReviewEntry,
    collection: &Collection,
    reason: impl Into<String>,
) -> Vec<String> {
    let tags =
        crate::ingest::route::tags_for(archive, collections, &collection.path, &entry.card.tags);
    let dropped = entry
        .card
        .tags
        .iter()
        .filter(|t| !tags.contains(t))
        .map(|t| t.0.clone())
        .collect();
    entry.card.tags = tags;
    entry.destination = Destination {
        collection: collection.doc.id,
        path: collection.path.clone(),
        confidence: Confidence::High,
        reasons: vec![reason.into()],
        rule: None,
        extra: Extra::new(),
    };
    entry.problems.retain(|p| p.stage != "route");
    if entry.state == State::NeedsOwner && entry.problems.is_empty() {
        entry.state = State::Proposed;
    }
    dropped
}
