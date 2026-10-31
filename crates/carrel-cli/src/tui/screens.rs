//! The Questions and Browse screens, and the line the owner types into for answers and
//! searches.

use carrel_core::card::Id;
use carrel_core::questions::{QuestionEntry, QuestionKind};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::{App, Effect, step};
use super::item::Pane;
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
    /// What to search for: words, and filters such as `tag:receipts`.
    Search,
    /// Files and folders to ingest.
    Ingest,
    /// A new value for this setting.
    Setting(String),
    /// The folder for a backup repository, new or existing.
    BackupRepository,
    /// The repository's password; for a new one, typed twice. Shown as dots.
    BackupPassword {
        /// The repository.
        repository: std::path::PathBuf,
        /// Whether it's being created, rather than connected to.
        new: bool,
        /// The first typing, when this is the second.
        first: Option<super::backups::Secret>,
    },
    /// The item to restore as of this change's snapshot.
    RestoreItem {
        /// The history commit.
        at: String,
    },
    /// What's wrong with this card, in words or `field: value` lines.
    Correct(Correcting),
    /// The path of an archive to open, or of any folder in it.
    OpenArchive,
    /// The folder for a new archive.
    CreateArchive,
}

/// What a correction is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Correcting {
    /// A proposal in Review.
    Proposal(Id),
    /// A filed item, by its card's path.
    Item(String),
}

impl Input {
    /// What to type, for the help line.
    pub fn prompt(&self) -> &'static str {
        match &self.purpose {
            InputPurpose::Answer { .. } => "what it is",
            InputPurpose::Search => "words and filters",
            InputPurpose::Ingest => "paths to ingest (or drop files here)",
            InputPurpose::Setting(_) => "the new value",
            InputPurpose::BackupRepository => "the backup folder (drop it here)",
            InputPurpose::BackupPassword { .. } => "the password",
            InputPurpose::RestoreItem { .. } => "the item to restore",
            InputPurpose::OpenArchive => "the archive's folder (drop it here)",
            InputPurpose::CreateArchive => "the new archive's folder; it's named after it",
            InputPurpose::Correct(_) => {
                "what's wrong (\"it's from 2019\"), or field: value lines such as created: 2019-03-31"
            }
        }
    }

    /// Whether what's typed is shown as dots.
    pub fn masked(&self) -> bool {
        matches!(self.purpose, InputPurpose::BackupPassword { .. })
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
                    InputPurpose::Setting(key) => {
                        return Some(super::settings::set(&key, &text));
                    }
                    InputPurpose::Ingest => {
                        let paths = crate::paths::split_paths(&text);
                        if paths.is_empty() {
                            self.message = Some("Nothing to ingest".into());
                        } else {
                            self.plan(paths, carrel_core::ingest::IntakeOptions::default());
                        }
                    }
                    InputPurpose::Search => {
                        self.search = (!text.is_empty()).then_some(text);
                        self.browse_pane = if self.search.is_some() {
                            Pane::Items
                        } else {
                            Pane::Collections
                        };
                        self.browse_item = 0;
                        self.load_items();
                    }
                    InputPurpose::Correct(_) if text.is_empty() => {
                        self.message = Some("Nothing to correct".into());
                    }
                    InputPurpose::Correct(target) => {
                        return Some(Effect::Correction(super::app::Step::Work {
                            target,
                            words: text,
                        }));
                    }
                    InputPurpose::BackupPassword { .. } => {
                        // A password is taken as typed, spaces and all.
                        return self.backup_input(input.purpose, input.text);
                    }
                    purpose @ (InputPurpose::OpenArchive | InputPurpose::CreateArchive) => {
                        let Some(path) = super::archives::typed_path(&text) else {
                            self.message = Some("No folder given; nothing changed".into());
                            return None;
                        };
                        return Some(if purpose == InputPurpose::OpenArchive {
                            Effect::OpenArchive(path)
                        } else {
                            Effect::CreateArchive(path)
                        });
                    }
                    purpose @ (InputPurpose::BackupRepository
                    | InputPurpose::RestoreItem { .. }) => {
                        return self.backup_input(purpose, text);
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
        self.browse_items = if let Some(typed) = self.search.clone() {
            self.find(&typed)
        } else {
            self.searched = super::search::Searched::default();
            self.collection_items()
        };
        self.browse_item = self
            .browse_item
            .min(self.browse_items.len().saturating_sub(1));
        self.load_card();
    }

    /// The chosen collection's cards, as card paths and titles.
    fn collection_items(&self) -> Vec<(String, String)> {
        self.collections
            .get(self.browse_collection)
            .and_then(|c| {
                self.archive.cards_in(c).ok().map(|cards| {
                    cards
                        .into_iter()
                        .map(|(name, card)| (format!("{}/{name}", c.path), card.title))
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    /// Moves the choice among collections, items, or the chosen item's rows.
    pub(super) fn browse_move(&mut self, delta: i32) {
        match self.browse_pane {
            Pane::Item => self.item_move(delta),
            Pane::Items => {
                self.browse_item = step(self.browse_item, delta, self.browse_items.len());
                self.load_card();
            }
            Pane::Collections => {
                self.browse_collection =
                    step(self.browse_collection, delta, self.collections.len());
                self.browse_item = 0;
                self.load_items();
            }
        }
    }

    /// Handles a key on the Browse screen: ←→ move between collections, items and the chosen
    /// item's view, `e` and `c` edit or correct the chosen item, `/` searches, and Esc clears a
    /// search.
    pub(super) fn browse_key(&mut self, key: KeyEvent) -> Handled {
        if self.browse_pane == Pane::Item
            && let Handled::Yes(effect) = self.item_key(key)
        {
            return Handled::Yes(effect);
        }
        let on_item = self.browse_pane != Pane::Collections;
        match key.code {
            // Here ←→ move between the panes; Tab still switches screens.
            KeyCode::Right | KeyCode::Enter | KeyCode::Char('l') => match self.browse_pane {
                Pane::Collections if !self.browse_items.is_empty() => {
                    self.browse_pane = Pane::Items;
                    self.load_card();
                }
                Pane::Items if self.item.card.is_some() => self.browse_pane = Pane::Item,
                _ => {}
            },
            KeyCode::Left | KeyCode::Char('h') => {
                if self.search.is_none() {
                    self.browse_pane = Pane::Collections;
                }
            }
            KeyCode::Char('e') if on_item => {
                if let Some((card, _)) = self.browse_items.get(self.browse_item) {
                    return Handled::Yes(Some(Effect::EditItem(card.clone())));
                }
            }
            KeyCode::Char('c') if on_item => {
                if let Some((card, _)) = self.browse_items.get(self.browse_item) {
                    self.input = Some(Input {
                        purpose: InputPurpose::Correct(Correcting::Item(card.clone())),
                        text: String::new(),
                    });
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
                self.browse_pane = Pane::Collections;
                self.load_items();
            }
            _ => return Handled::No,
        }
        Handled::Yes(None)
    }
}
