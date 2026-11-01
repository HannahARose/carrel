//! Questions at natural moments in the interactive mode (`docs/design/librarian.md`,
//! "Questions"; [`carrel_core::questions::moments`]): beside the item in Browse, on a wander
//! step in Discover, and from the librarian when an item comes up. `a` answers one: it joins
//! the queue then, and the answer goes through the corrections pop-up as any item question's
//! does, shown before it's saved, with the fields the owner dictated kept.

use carrel_core::archive::{Archive, ItemRef};
use carrel_core::questions::QuestionEntry;
use carrel_core::questions::moments;

use super::app::App;
use super::screens::{Input, InputPurpose};

/// The filed item whose card is at `card`, if there is one.
pub fn item_at(archive: &Archive, card: &str) -> Option<ItemRef> {
    archive.items().ok()?.into_iter().find(|i| i.card == card)
}

/// The open questions about the item whose card is at `card`; none when it can't be read.
pub fn questions_about(archive: &Archive, card: &str) -> Vec<QuestionEntry> {
    item_at(archive, card)
        .and_then(|item| moments::about_item(archive, &item, crate::discover::today()).ok())
        .unwrap_or_default()
}

/// A question as the lines shown beside an item: the question, its suggestions numbered,
/// and how to answer.
pub fn question_lines(question: &QuestionEntry) -> Vec<String> {
    let mut lines = vec![format!("Carrel asks: {}", question.question)];
    for (i, suggestion) in question.suggestions.iter().enumerate() {
        lines.push(format!("  {}. {}", i + 1, suggestion.answer));
    }
    lines.push("  a to answer; you see what it changes before it's saved".into());
    lines
}

impl App {
    /// Reads the questions about the item Browse shows.
    pub(super) fn load_item_questions(&mut self) {
        self.item_questions = self
            .item
            .card
            .as_deref()
            .map(|card| questions_about(&self.archive, card))
            .unwrap_or_default();
    }

    /// Starts answering `question`, asked at a moment: it joins the queue, and the line for
    /// the answer opens. A suggestion's number picks it.
    pub(super) fn answer_moment(&mut self, question: &QuestionEntry) {
        let asked = match moments::ask(&self.archive, question) {
            Ok(asked) => asked,
            Err(e) => {
                self.message = Some(format!("The question couldn't be asked: {e}"));
                return;
            }
        };
        if let Some(known) = self.asked.iter_mut().find(|q| q.id == asked.id) {
            *known = asked.clone();
        } else {
            self.asked.push(asked.clone());
            self.asked_settles.push(1);
        }
        self.input = Some(Input {
            purpose: InputPurpose::Answer {
                id: asked.id,
                question: asked.question.clone(),
                kind: asked.kind,
            },
            text: String::new(),
        });
    }
}
