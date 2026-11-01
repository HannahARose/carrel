//! Questions at natural moments, for the command line and the interactive mode
//! (`docs/design/librarian.md`, "Questions"; [`carrel_core::questions::moments`]): the day's
//! few questions on Status, a question on a wander step, and the librarian's question in a
//! conversation. Also this device's records of discovery and of the day's questions, which
//! the interactive mode keeps in memory in tests so they never write the real configuration.

use std::fmt::Write as _;
use std::path::PathBuf;

use carrel_core::archive::Archive;
use carrel_core::discover::visits::Visits;
use carrel_core::questions::QuestionEntry;
use carrel_core::questions::moments::{self, Daily};
use carrel_core::review::short_id;
use serde_json::{Value, json};

/// This device's records for one archive: which collections were opened and the day's
/// questions. Saved to Carrel's folder on this device, or kept in memory only.
#[derive(Debug, Clone, Default)]
pub struct Records {
    /// Where they're saved: Carrel's folder on this device, or nowhere.
    config: Option<PathBuf>,
    /// The archive they're for.
    archive_id: String,
    /// Which collections were opened, what was resurfaced, and the last walk.
    pub visits: Visits,
    /// The day's questions.
    pub daily: Daily,
}

impl Records {
    /// The records saved on this device for `archive`.
    pub fn here(archive: &Archive) -> Self {
        let archive_id = archive.info().id.to_string();
        let config = carrel_core::local::config_dir();
        let visits = config
            .as_deref()
            .map(|c| Visits::load(&Visits::path_in(c, &archive_id)))
            .unwrap_or_default();
        let daily = config
            .as_deref()
            .map(|c| Daily::load(&Daily::path_in(c, &archive_id)))
            .unwrap_or_default();
        Self {
            config,
            archive_id,
            visits,
            daily,
        }
    }

    /// Records for `archive` kept in memory only, never saved.
    pub fn in_memory(archive: &Archive) -> Self {
        Self {
            archive_id: archive.info().id.to_string(),
            ..Self::default()
        }
    }

    /// Saves which collections were opened. Discovery's record is a convenience: a device
    /// that can't write it still discovers.
    pub fn save_visits(&self) {
        if let Some(config) = &self.config {
            let _ = self.visits.save(&Visits::path_in(config, &self.archive_id));
        }
    }

    /// Saves the day's questions, as [`Self::save_visits`] does.
    pub fn save_daily(&self) {
        if let Some(config) = &self.config {
            let _ = self.daily.save(&Daily::path_in(config, &self.archive_id));
        }
    }

    /// Records that `collection` was opened today, and saves it if that's news.
    pub fn opened(&mut self, collection: &str) {
        if self.visits.open(collection, crate::discover::today()) {
            self.save_visits();
        }
    }
}

/// A question and its suggested answers, numbered, for the command line, with the command
/// that answers it.
pub fn question_text(question: &QuestionEntry, indent: &str) -> String {
    let mut text = format!("{indent}{}  {}\n", short_id(question.id), question.question);
    for (i, suggestion) in question.suggestions.iter().enumerate() {
        writeln!(
            text,
            "{indent}          {}",
            crate::questions::suggestion_line(i + 1, suggestion)
        )
        .unwrap();
    }
    text
}

/// The part of `carrel status` that names the day's few questions, choosing them if they
/// haven't been chosen today, and the same for programs. Empty when there are none.
pub fn daily_section(archive: &Archive, records: &mut Records) -> (String, Value) {
    let Ok((questions, changed)) = records.daily.today(archive, crate::discover::today()) else {
        return (String::new(), json!([]));
    };
    if changed {
        records.save_daily();
    }
    let value = json!(
        questions
            .iter()
            .map(|q| json!({
                "id": q.id,
                "question": q.question,
                "subject": q.subject,
            }))
            .collect::<Vec<_>>()
    );
    if questions.is_empty() {
        return (String::new(), value);
    }
    let mut text =
        String::from("\nToday's questions (answer with `carrel questions answer ID`):\n");
    for question in &questions {
        writeln!(
            text,
            "  {}  {} ({})",
            short_id(question.id),
            question.question,
            question.subject
        )
        .unwrap();
    }
    (text, value)
}

/// A question about the item whose card is at `card`, for a wander step or the librarian,
/// added to the queue so it can be answered by its identifier; none when there's none open.
pub fn question_on(archive: &Archive, card: &str) -> Option<QuestionEntry> {
    let item = archive.items().ok()?.into_iter().find(|i| i.card == card)?;
    let question = moments::about_item(archive, &item, crate::discover::today())
        .ok()?
        .into_iter()
        .next()?;
    moments::ask(archive, &question).ok()
}

/// A wander step's question, under the step.
pub fn wander_question_text(question: &QuestionEntry) -> String {
    format!(
        "   A question about it:\n{}   Answer with `carrel questions answer {}`.\n",
        question_text(question, "   "),
        short_id(question.id)
    )
}

/// The part of `carrel show` that says what Carrel would ask about the item whose card is at
/// `card`, without adding anything to the queue, and the same for programs.
pub fn show_section(archive: &Archive, card: &str) -> (String, Value) {
    let Some(item) = archive
        .items()
        .ok()
        .and_then(|items| items.into_iter().find(|i| i.card == card))
    else {
        return (String::new(), json!([]));
    };
    let open = moments::about_item(archive, &item, crate::discover::today()).unwrap_or_default();
    let value = json!(open.iter().map(|q| q.question.clone()).collect::<Vec<_>>());
    if open.is_empty() {
        return (String::new(), json!([]));
    }
    let mut text = String::from("\nCarrel asks:\n");
    for question in &open {
        writeln!(text, "  {}", question.question).unwrap();
    }
    writeln!(
        text,
        "  `carrel questions about \"{card}\"` asks them, with likely answers"
    )
    .unwrap();
    (text, value)
}
