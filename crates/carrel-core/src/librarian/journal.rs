//! Saving a conversation with the librarian to the month's Journal (`docs/design/librarian.md`,
//! decision 4; FORMAT.md §9.3): as Markdown, with the questions, the answers and links to the
//! items, readable without Carrel. Nothing about a conversation is written unless the owner
//! saves it.

use std::fmt::Write as _;

use crate::active::{CONVERSATIONS, conversations_in, month_heading};
use crate::archive::Archive;
use crate::coordinator::WriterLease;
use crate::history::{Change, History, HistoryError, Operation};
use crate::listing::{link_destination, relative};

use super::Conversation;

/// A conversation saved to the Journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    /// The month's Journal, relative to the archive root.
    pub path: String,
    /// The history change that records it.
    pub commit: Option<String>,
}

/// Why a conversation couldn't be saved.
#[derive(Debug)]
pub enum SaveError {
    /// There's nothing in it to save.
    Empty,
    /// The Journal couldn't be read or written.
    Io(std::io::Error),
    /// The change couldn't be recorded.
    History(HistoryError),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("there's no conversation to save yet"),
            Self::Io(e) => write!(f, "the Journal: {e}"),
            Self::History(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SaveError {}

/// Square brackets and bars escaped, so a title is safe as a link's text.
fn escape(text: &str) -> String {
    text.replace('[', "\\[")
        .replace(']', "\\]")
        .replace('|', "\\|")
}

/// One line, for a heading.
fn one_line(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > 80 {
        line.chars().take(80).collect::<String>() + "…"
    } else {
        line
    }
}

/// `conversation` as the Journal keeps it, saved at `at`, with links relative to `folder`.
#[must_use]
pub fn markdown(conversation: &Conversation, folder: &str, at: &jiff::Zoned) -> String {
    let first = conversation
        .answers
        .first()
        .map(|a| one_line(&a.question))
        .unwrap_or_default();
    let mut text = format!("### {} · {first}\n\n", at.strftime("%Y-%m-%d %H:%M"));
    let models: Vec<&str> = {
        let mut models: Vec<&str> = conversation
            .answers
            .iter()
            .map(|a| a.model.as_str())
            .collect();
        models.dedup();
        models
    };
    writeln!(text, "With the librarian, on {}.\n", models.join(", ")).unwrap();
    for answer in &conversation.answers {
        match &answer.about {
            Some(about) => writeln!(
                text,
                "**You asked,** about {}: {}\n",
                escape(about),
                answer.question.trim()
            ),
            None => writeln!(text, "**You asked:** {}\n", answer.question.trim()),
        }
        .unwrap();
        writeln!(text, "**The librarian:** {}\n", answer.text.trim()).unwrap();
        if !answer.items.is_empty() {
            text.push_str("Items:\n\n");
            for item in &answer.items {
                writeln!(
                    text,
                    "- [{}]({})",
                    escape(&item.title),
                    link_destination(&relative(folder, &item.card))
                )
                .unwrap();
            }
            text.push('\n');
        }
        for quote in &answer.passages {
            writeln!(
                text,
                "> {}\n>\n> — [{}]({})\n",
                one_paragraph(&quote.text),
                escape(&quote.item.title),
                link_destination(&relative(folder, &quote.item.card))
            )
            .unwrap();
        }
    }
    text
}

fn one_paragraph(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Saves `conversation` to the Journal of the month of `at`, just below the conversations
/// heading, newest first, as one history change, holding `lease`.
///
/// # Errors
///
/// [`SaveError`] if it's empty, or the Journal can't be written or the change recorded.
pub fn save(
    archive: &Archive,
    conversation: &Conversation,
    at: &jiff::Zoned,
    lease: &WriterLease,
) -> Result<Saved, SaveError> {
    if conversation.answers.is_empty() {
        return Err(SaveError::Empty);
    }
    let month = at.strftime("%Y-%m").to_string();
    let folder = format!("Journal/{}", at.strftime("%Y"));
    let path = format!("{folder}/{month}.md");
    let file = archive.root().join(&path);
    let current = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            month_heading(&month).unwrap_or_default()
        }
        Err(e) => return Err(SaveError::Io(e)),
    };
    let entry = markdown(conversation, &folder, at);
    let text = if let Some(kept) = conversations_in(&current) {
        let before = &current[..current.len() - kept.len()];
        let rest = kept[CONVERSATIONS.len()..].trim_start_matches('\n');
        let (intro, older) = if rest.starts_with("### ") {
            ("", rest.strip_prefix("### ").unwrap_or(rest))
        } else {
            rest.split_once("\n### ").unwrap_or((rest, ""))
        };
        let older = if older.is_empty() {
            String::new()
        } else {
            format!("### {older}")
        };
        format!(
            "{before}{CONVERSATIONS}\n\n{}\n\n{entry}\n{older}",
            intro.trim_end()
        )
    } else {
        let mut text = current;
        if !text.ends_with("\n\n") {
            text.push('\n');
        }
        write!(
            text,
            "{CONVERSATIONS}\n\nSaved from conversations with the librarian, newest first.\n\n{entry}"
        )
        .unwrap();
        text
    };
    let text = text.trim_end().to_owned() + "\n";
    std::fs::create_dir_all(archive.root().join(&folder)).map_err(SaveError::Io)?;
    std::fs::write(&file, text).map_err(SaveError::Io)?;
    let change = Change::new(
        Operation::Edit,
        format!(
            "Save a conversation with the librarian to the Journal: {}",
            one_line(&conversation.answers[0].question)
        ),
        lease.info().person,
        lease.info().device,
    );
    let commit = History::open(archive)
        .and_then(|h| h.commit(archive, &change, lease))
        .map_err(SaveError::History)?;
    Ok(Saved { path, commit })
}
