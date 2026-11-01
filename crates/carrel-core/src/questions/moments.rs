//! Questions at natural moments (`docs/design/librarian.md`, "Questions"): beside an item
//! while it's browsed, now and then on a wander through the stacks, by the librarian when an
//! item comes up in conversation, and a few new ones a day on Status. Never as pop-ups.
//!
//! This module decides which items to ask about and how often; the questions themselves come
//! from [`super::about::item`], and answers go through the corrections path as any item
//! question's do. A question is only added to the queue ([`ask`]) when the owner is about to
//! answer it or when Status names it, so looking around doesn't fill the queue.
//!
//! - **Browsing:** every question about the item that's open, first those already in the
//!   queue ([`about_item`]).
//! - **Wandering:** a question about a step's item at most every [`WANDER_EVERY`] steps
//!   ([`wander_asks`]).
//! - **Conversation:** at most one question an answer, never after two answers in a row, and
//!   never twice about the same item in one conversation ([`Conversing`]).
//! - **Status:** at most [`DAILY`] new questions a day, the same all day, kept on this device
//!   ([`Daily`]).
//!
//! A question that was skipped rests, and one that was declined is never asked again, wherever
//! it would have come up.

use std::path::{Path, PathBuf};

use jiff::civil::Date;
use serde::{Deserialize, Serialize};

use crate::actions::ActionError;
use crate::archive::{Archive, ItemRef};
use crate::card::{Card, Id};
use crate::card_file::CardFile;
use crate::discover::{Rng, day_seed};
use crate::ingest::card::Glossary;

use super::rank::same;
use super::{QuestionEntry, QuestionQueue};

/// How many new questions Status names in a day, at most: "a few".
pub const DAILY: usize = 3;

/// How many steps of a wander go by between questions, at least.
pub const WANDER_EVERY: usize = 3;

/// How many cards are read, at most, looking for the day's questions, so a large archive
/// doesn't hold Status up.
pub const DAILY_LOOKS: usize = 400;

/// The open questions about `item`, whose card is `card`, given the queue as it is: those
/// already in the queue, then those [`super::about::item`] raises that the queue doesn't have
/// in any state. A skipped question resting at `now` and a declined one aren't open.
#[must_use]
pub fn open_about(
    queue: &[QuestionEntry],
    item: &ItemRef,
    card: &Card,
    glossary: &Glossary,
    today: Date,
    (now, skip_days): (jiff::Timestamp, u32),
) -> Vec<QuestionEntry> {
    let mut open: Vec<QuestionEntry> = queue
        .iter()
        .filter(|q| q.about.contains(&item.id) || q.items.contains(&item.id))
        .filter(|q| q.kind == super::QuestionKind::Item)
        .filter(|q| q.declined.is_none() && !q.is_resting(now, skip_days))
        .cloned()
        .collect();
    for fresh in super::about::item(item, card, glossary, today) {
        if !queue.iter().any(|q| same(q, &fresh)) {
            open.push(fresh);
        }
    }
    open
}

/// The queue as it is, every question in it.
fn queue_of(archive: &Archive) -> Result<Vec<QuestionEntry>, ActionError> {
    Ok(QuestionQueue::of(archive).list()?.0)
}

/// `item`'s card, read from the archive.
fn card_of(archive: &Archive, item: &ItemRef) -> Result<Card, ActionError> {
    let path = archive.root().join(&item.card);
    let text = std::fs::read_to_string(&path).map_err(|source| ActionError::Io {
        path: path.clone(),
        source,
    })?;
    CardFile::parse(&text)
        .map(|file| file.card().clone())
        .map_err(|e| ActionError::Invalid(format!("{}: {e}", item.card)))
}

/// The open questions about the filed `item` on `today`, as [`open_about`] finds them, for
/// showing beside it.
///
/// # Errors
///
/// [`ActionError`] if the queue or the item's card can't be read.
pub fn about_item(
    archive: &Archive,
    item: &ItemRef,
    today: Date,
) -> Result<Vec<QuestionEntry>, ActionError> {
    let card = card_of(archive, item)?;
    Ok(open_about(
        &queue_of(archive)?,
        item,
        &card,
        &Glossary::of(archive),
        today,
        (
            jiff::Timestamp::now(),
            archive.settings().questions.skip_days,
        ),
    ))
}

/// Puts `question` in the queue, if it isn't there already, and returns it as the queue
/// keeps it: merged into one that says the same, or saved as it is.
///
/// # Errors
///
/// [`ActionError`] if the queue can't be read or written, or the question was declined.
pub fn ask(archive: &Archive, question: &QuestionEntry) -> Result<QuestionEntry, ActionError> {
    let saved = super::answer::raise(archive, vec![question.clone()])?;
    if let Some(saved) = saved.into_iter().next() {
        return Ok(saved);
    }
    // Nothing new to save: the queue has it already, as it is.
    queue_of(archive)?
        .into_iter()
        .find(|q| same(q, question) && q.declined.is_none())
        .ok_or_else(|| ActionError::Invalid("that question was declined".into()))
}

/// Whether to ask about the item at step `step` of a wander, when the last question was asked
/// at `last` (none yet on this walk): at most every [`WANDER_EVERY`] steps, and not on the
/// first, which is for looking around.
#[must_use]
pub fn wander_asks(step: usize, last: Option<usize>) -> bool {
    match last {
        Some(last) => step >= last + WANDER_EVERY,
        None => step >= 2,
    }
}

/// The librarian's questions in one conversation: which items it has asked about, and
/// whether it asked after the last answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Conversing {
    /// The items asked about so far.
    pub asked: Vec<Id>,
    /// Whether the last answer came with a question.
    pub just_asked: bool,
}

impl Conversing {
    /// The question to ask after an answer about `items`, from `open` (each item's open
    /// questions): the first about an item not yet asked about, unless the last answer came
    /// with one. Records what it chose.
    pub fn next(&mut self, open: &[(Id, Vec<QuestionEntry>)]) -> Option<QuestionEntry> {
        if self.just_asked {
            self.just_asked = false;
            return None;
        }
        let (item, question) = open
            .iter()
            .filter(|(item, _)| !self.asked.contains(item))
            .find_map(|(item, questions)| questions.first().map(|q| (*item, q.clone())))?;
        self.asked.push(item);
        self.just_asked = true;
        Some(question)
    }

    /// The question to ask after an answer about `items` in `archive`, as [`Self::next`]
    /// chooses it. Items whose cards can't be read are passed over.
    ///
    /// # Errors
    ///
    /// [`ActionError`] if the queue can't be read.
    pub fn after(
        &mut self,
        archive: &Archive,
        items: &[Id],
        today: Date,
    ) -> Result<Option<QuestionEntry>, ActionError> {
        if self.just_asked || items.iter().all(|id| self.asked.contains(id)) {
            return Ok(self.next(&[]));
        }
        let all = archive.items()?;
        let queue = queue_of(archive)?;
        let glossary = Glossary::of(archive);
        let now = (
            jiff::Timestamp::now(),
            archive.settings().questions.skip_days,
        );
        let open: Vec<(Id, Vec<QuestionEntry>)> = items
            .iter()
            .filter(|id| !self.asked.contains(id))
            .filter_map(|id| all.iter().find(|i| i.id == *id))
            .filter_map(|item| {
                let card = card_of(archive, item).ok()?;
                Some((
                    item.id,
                    open_about(&queue, item, &card, &glossary, today, now),
                ))
            })
            .collect();
        Ok(self.next(&open))
    }
}

/// The day's questions for Status, kept on this device, never in the archive, so the same
/// few are named all day.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Daily {
    /// The day they were chosen, as `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<String>,
    /// The questions chosen that day.
    #[serde(default)]
    pub questions: Vec<Id>,
}

impl Daily {
    /// The file it's kept in for the archive `archive_id`, in Carrel's folder on this device.
    #[must_use]
    pub fn path(archive_id: &str) -> Option<PathBuf> {
        crate::local::config_dir().map(|d| Self::path_in(&d, archive_id))
    }

    /// The file it's kept in for the archive `archive_id`, in the configuration folder
    /// `config`.
    #[must_use]
    pub fn path_in(config: &Path, archive_id: &str) -> PathBuf {
        config.join("questions").join(format!("{archive_id}.json"))
    }

    /// The saved record, or an empty one.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Saves it.
    ///
    /// # Errors
    ///
    /// An I/O error if it can't be written.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, text + "\n")
    }

    /// The day's questions on `today`, choosing them if they haven't been chosen today: up to
    /// [`DAILY`] new questions, one an item, about filed items in an order that's the same all
    /// day, each added to the queue. Those answered or skipped since drop out. Returns them,
    /// and whether the record changed.
    ///
    /// # Errors
    ///
    /// [`ActionError`] if the archive or the queue can't be read, or the queue written.
    pub fn today(
        &mut self,
        archive: &Archive,
        today: Date,
    ) -> Result<(Vec<QuestionEntry>, bool), ActionError> {
        let day = today.to_string();
        let mut changed = false;
        if self.day.as_deref() != Some(day.as_str()) {
            let chosen = choose(archive, today)?;
            let mut ids = Vec::new();
            for question in chosen {
                if let Ok(asked) = ask(archive, &question) {
                    ids.push(asked.id);
                }
            }
            self.day = Some(day);
            self.questions = ids;
            changed = true;
        }
        let now = jiff::Timestamp::now();
        let skip_days = archive.settings().questions.skip_days;
        let queue = queue_of(archive)?;
        let open = self
            .questions
            .iter()
            .filter_map(|id| queue.iter().find(|q| q.id == *id))
            .filter(|q| q.declined.is_none() && !q.is_resting(now, skip_days))
            .cloned()
            .collect();
        Ok((open, changed))
    }
}

/// Up to [`DAILY`] new questions for `today`: filed items in an order seeded by the day, each
/// giving its first question the queue doesn't already have in any state.
fn choose(archive: &Archive, today: Date) -> Result<Vec<QuestionEntry>, ActionError> {
    let mut items = archive.items()?;
    let queue = queue_of(archive)?;
    let glossary = Glossary::of(archive);
    let mut rng = Rng::new(day_seed(today) ^ archive.info().id.0.as_u64_pair().1);
    // A shuffle, so the day's items come from all over the archive.
    for i in (1..items.len()).rev() {
        items.swap(i, rng.below(i + 1));
    }
    let mut chosen: Vec<QuestionEntry> = Vec::new();
    for item in items.iter().take(DAILY_LOOKS) {
        if chosen.len() >= DAILY {
            break;
        }
        let Ok(card) = card_of(archive, item) else {
            continue;
        };
        let fresh = super::about::item(item, &card, &glossary, today)
            .into_iter()
            .find(|q| !queue.iter().any(|e| same(e, q)) && !chosen.iter().any(|c| same(c, q)));
        chosen.extend(fresh);
    }
    Ok(chosen)
}
