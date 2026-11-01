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
    /// The answer to a question, in the owner's words: what a term means, what an item is,
    /// or for a sender, yes, no or a collection.
    Answer {
        /// The question.
        id: Id,
        /// The question in words, shown above the line.
        question: String,
        /// What it asks.
        kind: QuestionKind,
    },
    /// What to search for: words, and filters such as `tag:receipts`.
    Search,
    /// A question for the librarian.
    Ask,
    /// Files and folders to ingest, or a web address.
    Ingest,
    /// A note about what's planned for ingest, for the drafts and the cards.
    IngestNote,
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
    /// The folder to check the item being checked out to.
    CheckoutFolder,
    /// When the item being checked out is due back, if ever.
    Due {
        /// The item's card path.
        card: String,
        /// Its title.
        title: String,
        /// The folder chosen for it.
        to: std::path::PathBuf,
    },
    /// The file or folder to put on the desk.
    DeskPath,
    /// A new collection's path.
    CollectionPath,
    /// What belongs in the new collection at this path.
    CollectionDescription {
        /// The collection's path.
        path: String,
    },
    /// The new collection's tags.
    CollectionTags {
        /// The collection's path.
        path: String,
        /// What belongs there.
        description: Option<String>,
    },
    /// A new name for this item.
    Rename {
        /// The item's card path.
        card: String,
        /// Its name now.
        from: String,
    },
    /// One of the commands' options, asked for along the way.
    Option(super::option_lines::OptionLine),
    /// What needs work on an item or a collection being flagged.
    FlagNote {
        /// The item's card or the collection's folder.
        target: String,
        /// Its title.
        title: String,
    },
    /// Something about the gaps in Carrel (`super::gaps`).
    Gap(super::gaps::GapInput),
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
            InputPurpose::Answer {
                kind: QuestionKind::Sender,
                ..
            } => "yes, no, or the collection's path",
            InputPurpose::Answer {
                kind: QuestionKind::Item,
                ..
            } => "your answer; you see what it changes before it's saved",
            InputPurpose::Answer { .. } => "what it is",
            InputPurpose::Search => "words and filters",
            InputPurpose::Ask => "your question for the librarian",
            InputPurpose::Ingest => "paths to ingest (or drop files here), or a web address",
            InputPurpose::IngestNote => "a note about them, for their cards, or nothing",
            InputPurpose::Setting(_) => "the new value",
            InputPurpose::BackupRepository => "the backup folder (drop it here)",
            InputPurpose::BackupPassword { .. } => "the password",
            InputPurpose::RestoreItem { .. } => "the item to restore",
            InputPurpose::OpenArchive => "the archive's folder (drop it here)",
            InputPurpose::CreateArchive => "the new archive's folder; it's named after it",
            InputPurpose::Correct(_) => {
                "what's wrong (\"it's from 2019\"), or field: value lines such as created: 2019-03-31"
            }
            InputPurpose::CheckoutFolder => "the folder (drop it here)",
            InputPurpose::Due { .. } => "YYYY-MM-DD, or nothing",
            InputPurpose::DeskPath => "the file or folder (drop it here)",
            InputPurpose::CollectionPath => "the collection's path",
            InputPurpose::CollectionDescription { .. } => "what belongs there, or nothing",
            InputPurpose::CollectionTags { .. } => "name=meaning; …, or nothing",
            InputPurpose::Rename { .. } => "the new name",
            InputPurpose::Option(line) => line.prompt(),
            InputPurpose::FlagNote { .. } => "what needs work, or nothing",
            InputPurpose::Gap(gap) => gap.prompt(),
        }
    }

    /// Whether what's typed is shown as dots.
    pub fn masked(&self) -> bool {
        matches!(self.purpose, InputPurpose::BackupPassword { .. })
    }
}

impl App {
    /// Keeps the questions, ranked, and the choice among them.
    pub(super) fn load_questions(&mut self, ranked: carrel_core::questions::rank::Ranked) {
        self.asked_settles = ranked.open.iter().map(|w| w.settles).collect();
        self.asked = ranked.open.into_iter().map(|w| w.question).collect();
        self.resting = ranked.resting.len();
        self.asked_chosen = self.asked_chosen.min(self.asked.len().saturating_sub(1));
        if self.tab == super::app::Tab::Questions {
            self.question_shown();
        }
    }

    /// Handles a key on the Questions screen: Enter answers in words (or picks a collection),
    /// a number picks a suggestion, `s` skips and `d` dismisses.
    pub(super) fn questions_key(&mut self, key: KeyEvent) -> Handled {
        let digit = match key.code {
            KeyCode::Char(c @ '1'..='9') => c.to_digit(10).map(|d| d as usize),
            KeyCode::Enter | KeyCode::Char('d' | 's') => None,
            _ => return Handled::No,
        };
        if let Some(busy) = &self.busy {
            self.message = Some(format!("Still busy: {busy}"));
            return Handled::Yes(None);
        }
        let Some(question) = self.asked.get(self.asked_chosen).cloned() else {
            self.message = Some("No questions".into());
            return Handled::Yes(None);
        };
        if let Some(n) = digit {
            return Handled::Yes(self.pick(&question, n));
        }
        Handled::Yes(match (key.code, question.kind) {
            (KeyCode::Char('d'), _) => Some(Effect::Dismiss(question.id)),
            (KeyCode::Char('s'), _) => Some(Effect::Skip(question.id)),
            (_, QuestionKind::Collection | QuestionKind::Group) => {
                self.picker = Some(Picker {
                    collections: question.choices.clone(),
                    filter: String::new(),
                    chosen: 0,
                    purpose: Purpose::Answer(question.id),
                });
                None
            }
            (_, kind) => {
                self.input = Some(Input {
                    purpose: InputPurpose::Answer {
                        id: question.id,
                        question: question.question.clone(),
                        kind,
                    },
                    text: String::new(),
                });
                None
            }
        })
    }

    /// Picks `question`'s suggestion `n`, counting from 1.
    fn pick(&mut self, question: &QuestionEntry, n: usize) -> Option<Effect> {
        let Some(suggestion) = n.checked_sub(1).and_then(|i| question.suggestions.get(i)) else {
            self.message = Some(format!("There's no suggestion {n}"));
            return None;
        };
        if suggestion.no_change {
            return Some(Effect::Answer {
                id: question.id,
                answer: n.to_string(),
                learn: false,
            });
        }
        match question.kind {
            QuestionKind::Item => Self::answer_item(question, &suggestion.answer),
            QuestionKind::Sender => {
                self.confirm = Some(super::app::Confirm::Rule {
                    id: question.id,
                    answer: n.to_string(),
                    subject: question.subject.clone(),
                    path: suggestion.answer.clone(),
                });
                None
            }
            _ => Some(Effect::Answer {
                id: question.id,
                answer: n.to_string(),
                learn: false,
            }),
        }
    }

    /// Answers the item `question` with `answer`, through the corrections pop-up.
    fn answer_item(question: &QuestionEntry, answer: &str) -> Option<Effect> {
        use carrel_core::questions::answer::{Target, item_words, target};
        let target = match target(question)? {
            Target::Proposal(id) => Correcting::Proposal(id),
            Target::Filed(id) => Correcting::Item(id.to_string()),
        };
        Some(Effect::Correction(super::app::Step::Work {
            target,
            words: item_words(question, answer),
            answering: Some(question.id),
        }))
    }

    /// Answers the question `id` with what the owner typed.
    fn typed_answer(&mut self, id: Id, text: String) -> Option<Effect> {
        let question = self.asked.iter().find(|q| q.id == id)?.clone();
        // A suggestion's number picks it, as the number keys do on Questions.
        if let Ok(n) = text.parse::<usize>()
            && (1..=question.suggestions.len()).contains(&n)
        {
            return self.pick(&question, n);
        }
        match question.kind {
            QuestionKind::Item => Self::answer_item(&question, &text),
            QuestionKind::Sender => {
                use carrel_core::questions::answer::{SenderAnswer, sender_answer};
                match sender_answer(&question, &text, None, false) {
                    Ok(SenderAnswer::Place { path, .. }) => {
                        self.confirm = Some(super::app::Confirm::Rule {
                            id,
                            answer: text,
                            subject: question.subject.clone(),
                            path,
                        });
                        None
                    }
                    // No settles it, and a yes with no collection to name says so.
                    Ok(SenderAnswer::Decline) | Err(_) => Some(Effect::Answer {
                        id,
                        answer: text,
                        learn: false,
                    }),
                }
            }
            _ => Some(Effect::Answer {
                id,
                answer: text,
                learn: false,
            }),
        }
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
                        return self.typed_answer(id, text);
                    }
                    InputPurpose::Answer { .. } => {
                        self.message = Some("No answer given; the question still waits".into());
                    }
                    InputPurpose::Ask if text.is_empty() => {}
                    InputPurpose::Ask => {
                        self.talk.waiting = Some(text.clone());
                        return Some(Effect::Ask(text));
                    }
                    InputPurpose::Setting(key) => {
                        return Some(super::settings::set(&key, &text));
                    }
                    InputPurpose::IngestNote => self.noted(&text),
                    InputPurpose::Ingest if super::intake::is_web_address(&text) => {
                        self.plan_url(text.trim());
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
                            answering: None,
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
                        if purpose == InputPurpose::OpenArchive {
                            return Some(Effect::OpenArchive(path));
                        }
                        self.ask_option(super::option_lines::OptionLine::EmbeddingModel { path });
                    }
                    purpose @ (InputPurpose::BackupRepository
                    | InputPurpose::RestoreItem { .. }) => {
                        return self.backup_input(purpose, text);
                    }
                    InputPurpose::Option(line) => return self.option_entered(line, text),
                    InputPurpose::Gap(gap) => self.gap_typed(gap, &text),
                    purpose => return self.action_input(purpose, text),
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
        self.browse_flagged = self.flagged_cards();
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
    /// item's view, `e` and `c` edit or correct the chosen item, `o`, `r` and `z` check it out,
    /// rename or expand it, `a` answers Carrel's question about it and `A` asks for them, `w`
    /// wanders from it, `n` creates a collection, `f` flags an item or a collection and `F`
    /// lists the flagged items, `/` searches, and Esc clears a search.
    pub(super) fn browse_key(&mut self, key: KeyEvent) -> Handled {
        if self.browse_pane == Pane::Item
            && let Handled::Yes(effect) = self.item_key(key)
        {
            return Handled::Yes(effect);
        }
        let on_item = self.browse_pane != Pane::Collections;
        if !on_item && let Some(handled) = self.collection_restructure_key(key) {
            return handled;
        }
        if let Some(handled) = self.flag_key(key, on_item) {
            return handled;
        }
        if let Handled::Yes(effect) = self.browse_action_key(key, on_item) {
            return Handled::Yes(effect);
        }
        match key.code {
            // Here ←→ move between the panes; Tab still switches screens.
            KeyCode::Right | KeyCode::Enter | KeyCode::Char('l') => match self.browse_pane {
                Pane::Collections if !self.browse_items.is_empty() => {
                    self.browse_pane = Pane::Items;
                    self.load_card();
                }
                Pane::Items if self.item.card.is_some() => {
                    self.browse_pane = Pane::Item;
                    self.note_search_result_opened();
                }
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
            KeyCode::Char('E') if on_item => {
                if let Some((card, _)) = self.browse_items.get(self.browse_item) {
                    return Handled::Yes(Some(Effect::EditItemBody(card.clone())));
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
            KeyCode::Char('a') if on_item => match self.item_questions.first().cloned() {
                Some(question) => self.answer_moment(&question),
                None => self.message = Some("Carrel has no question about this item".into()),
            },
            KeyCode::Char('A') if on_item => {
                if let Some((card, _)) = self.browse_items.get(self.browse_item) {
                    return Handled::Yes(Some(Effect::QuestionsAbout(card.clone())));
                }
            }
            KeyCode::Char('M') if on_item => {
                if let Some((card, _)) = self.browse_items.get(self.browse_item) {
                    self.gap_input(super::gaps::GapInput::Missing {
                        card: Some(card.clone()),
                    });
                }
            }
            KeyCode::Char('w') if on_item => {
                if let Some((card, _)) = self.browse_items.get(self.browse_item).cloned() {
                    self.wander_from(&card);
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
