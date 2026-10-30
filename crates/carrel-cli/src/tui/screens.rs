//! The Questions and Browse screens, and the line the owner types into for answers and
//! searches.

use carrel_core::card::Id;
use carrel_core::questions::{QuestionEntry, QuestionKind};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::{App, Effect, step};
use super::review::{Handled, Picker, Purpose};

/// A line being typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    /// What it's for.
    pub purpose: InputPurpose,
    /// What's been typed.
    pub text: String,
}

/// What a typed line is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputPurpose {
    /// The answer to a term question: what the term means.
    Answer {
        /// The question.
        id: Id,
        /// The question in words, shown above the line.
        question: String,
    },
    /// Words to find in titles, tags and summaries.
    Search,
    /// Files and folders to ingest.
    Ingest,
}

impl Input {
    /// What to type, for the help line.
    pub fn prompt(&self) -> &'static str {
        match self.purpose {
            InputPurpose::Answer { .. } => "what it is",
            InputPurpose::Search => "words to find",
            InputPurpose::Ingest => "paths to ingest (or drop files here)",
        }
    }
}

impl App {
    /// Keeps the questions, most items first, and the choice among them.
    pub(super) fn load_questions(&mut self, mut questions: Vec<QuestionEntry>) {
        questions.sort_by_key(|q| std::cmp::Reverse(q.items.len()));
        self.asked = questions;
        self.asked_chosen = self.asked_chosen.min(self.asked.len().saturating_sub(1));
    }

    /// The titles of the proposals a question names, for showing beside it.
    pub fn waiting_titles(&self, question: &QuestionEntry) -> Vec<String> {
        question
            .items
            .iter()
            .map(|id| {
                self.queue.iter().find(|e| e.id == *id).map_or_else(
                    || carrel_core::review::short_id(*id),
                    |e| e.card.title.clone(),
                )
            })
            .collect()
    }

    /// Handles a key on the Questions screen.
    pub(super) fn questions_key(&mut self, key: KeyEvent) -> Handled {
        if !matches!(key.code, KeyCode::Enter | KeyCode::Char('d')) {
            return Handled::No;
        }
        if let Some(busy) = &self.busy {
            self.message = Some(format!("Still busy: {busy}"));
            return Handled::Yes(None);
        }
        let Some(question) = self.asked.get(self.asked_chosen) else {
            self.message = Some("No questions".into());
            return Handled::Yes(None);
        };
        Handled::Yes(match (key.code, question.kind) {
            (KeyCode::Char('d'), _) => Some(Effect::Dismiss(question.id)),
            (_, QuestionKind::Term) => {
                self.input = Some(Input {
                    purpose: InputPurpose::Answer {
                        id: question.id,
                        question: question.question.clone(),
                    },
                    text: String::new(),
                });
                None
            }
            (_, QuestionKind::Collection) => {
                self.picker = Some(Picker {
                    collections: question.choices.clone(),
                    filter: String::new(),
                    chosen: 0,
                    purpose: Purpose::Answer(question.id),
                });
                None
            }
        })
    }

    /// Handles a key while a line is being typed.
    pub(super) fn input_key(&mut self, key: KeyEvent) -> Option<Effect> {
        let input = self.input.as_mut()?;
        match key.code {
            KeyCode::Esc => {
                if input.purpose == InputPurpose::Search {
                    self.search = None;
                    self.load_items();
                }
                self.input = None;
            }
            KeyCode::Backspace => {
                input.text.pop();
            }
            KeyCode::Char(c) => input.text.push(c),
            KeyCode::Enter => {
                let input = self.input.take()?;
                let text = input.text.trim().to_owned();
                match input.purpose {
                    InputPurpose::Answer { id, .. } if !text.is_empty() => {
                        return Some(Effect::Answer { id, answer: text });
                    }
                    InputPurpose::Answer { .. } => {
                        self.message = Some("No answer given; the question still waits".into());
                    }
                    InputPurpose::Ingest => {
                        let paths = super::intake::split_paths(&text);
                        if paths.is_empty() {
                            self.message = Some("Nothing to ingest".into());
                        } else {
                            self.plan(paths, carrel_core::ingest::IntakeOptions::default());
                        }
                    }
                    InputPurpose::Search => {
                        self.search = (!text.is_empty()).then_some(text);
                        self.browse_in_items = self.search.is_some();
                        self.browse_item = 0;
                        self.load_items();
                    }
                }
            }
            _ => {}
        }
        None
    }

    /// Reads the collections, and the chosen one's items.
    ///
    /// # Errors
    ///
    /// [`crate::Failure`] if the archive can't be read.
    pub(super) fn load_browse(&mut self) -> Result<(), crate::Failure> {
        self.collections = self.archive.collections()?;
        self.browse_collection = self
            .browse_collection
            .min(self.collections.len().saturating_sub(1));
        self.load_items();
        Ok(())
    }

    /// The items Browse lists: the chosen collection's, or those the search finds anywhere.
    pub(super) fn load_items(&mut self) {
        self.browse_items = match &self.search {
            Some(words) => self.find(words),
            None => self
                .collections
                .get(self.browse_collection)
                .and_then(|c| {
                    self.archive.cards_in(c).ok().map(|cards| {
                        cards
                            .into_iter()
                            .map(|(name, card)| (format!("{}/{name}", c.path), card.title))
                            .collect()
                    })
                })
                .unwrap_or_default(),
        };
        self.browse_item = self
            .browse_item
            .min(self.browse_items.len().saturating_sub(1));
        self.load_card();
    }

    /// The cards whose title, tags or summary contain every word, ignoring case.
    fn find(&self, words: &str) -> Vec<(String, String)> {
        let words: Vec<String> = words.split_whitespace().map(str::to_lowercase).collect();
        let mut found = Vec::new();
        for collection in &self.collections {
            let Ok(cards) = self.archive.cards_in(collection) else {
                continue;
            };
            for (name, card) in cards {
                let text = format!(
                    "{} {} {}",
                    card.title,
                    card.summary.as_deref().unwrap_or(""),
                    card.tags
                        .iter()
                        .map(|t| t.0.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                )
                .to_lowercase();
                if words.iter().all(|w| text.contains(w)) {
                    found.push((format!("{}/{name}", collection.path), card.title));
                }
            }
        }
        found
    }

    /// Reads the chosen item's card in full.
    pub(super) fn load_card(&mut self) {
        self.card = match self.browse_items.get(self.browse_item) {
            Some((path, _)) => match crate::commands::show(Some(self.archive.root()), path) {
                Ok(report) => report.text.lines().map(str::to_owned).collect(),
                Err(crate::Failure(e)) => vec![e],
            },
            None => Vec::new(),
        };
    }

    /// Moves the choice among collections or items.
    pub(super) fn browse_move(&mut self, delta: i32) {
        if self.browse_in_items {
            self.browse_item = step(self.browse_item, delta, self.browse_items.len());
            self.load_card();
        } else {
            self.browse_collection = step(self.browse_collection, delta, self.collections.len());
            self.browse_item = 0;
            self.load_items();
        }
    }

    /// Handles a key on the Browse screen: ←→ move between collections and items, `/`
    /// searches, and Esc clears a search.
    pub(super) fn browse_key(&mut self, key: KeyEvent) -> Handled {
        match key.code {
            // Here ←→ move between the panes; Tab still switches screens.
            KeyCode::Right | KeyCode::Enter | KeyCode::Char('l') => {
                if !self.browse_in_items && !self.browse_items.is_empty() {
                    self.browse_in_items = true;
                    self.load_card();
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if self.search.is_none() {
                    self.browse_in_items = false;
                }
            }
            KeyCode::Char('/') => {
                self.input = Some(Input {
                    purpose: InputPurpose::Search,
                    text: self.search.clone().unwrap_or_default(),
                });
            }
            KeyCode::Esc if self.search.is_some() => {
                self.search = None;
                self.browse_in_items = false;
                self.load_items();
            }
            _ => return Handled::No,
        }
        Handled::Yes(None)
    }
}
