//! The question queue and the glossary (FORMAT.md §7.3 and §7.9, and `docs/design/ingest.md`).
//!
//! When several items in a batch depend on the same unknown, ingest asks the owner one
//! question rather than guessing on each. Answers about terms become glossary facts, which
//! every model reads; answers about where items go redirect their proposals.

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionKind {
    /// What a recurring term means. The answer becomes a glossary fact.
    Term,
    /// Which collection items about a term belong in. The answer is a collection path.
    Collection,
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
    /// The ingest batch that raised it.
    pub batch: Id,
    /// When it was asked.
    pub created: DateOrTime,
    /// What it asks.
    pub kind: QuestionKind,
    /// The term it's about, as it appears in the items' names.
    pub subject: String,
    /// The question, in words.
    pub question: String,
    /// The proposals waiting on the answer.
    pub items: Vec<Id>,
    /// For a collection question, the collections to choose from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
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
        Ok(())
    }
}

/// The question queue.
pub type QuestionQueue = Queue<QuestionEntry>;

/// The stage name of the problem that marks a proposal waiting on a question.
pub const QUESTION_STAGE: &str = "question";

/// Marks `entry` as waiting on any of `questions` that name it: a `question` problem each,
/// and `needs-owner`, so it isn't approved in bulk before the answer.
pub fn mark_waiting(entry: &mut ReviewEntry, questions: &[QuestionEntry]) {
    for question in questions.iter().filter(|q| q.items.contains(&entry.id)) {
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
