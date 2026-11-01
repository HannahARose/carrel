//! The Librarian screen (`docs/design/librarian.md`, "In each interface"): the conversation on
//! the left, the item it's discussing on the right.
//!
//! `Enter` asks a question; the core's librarian answers in the background, on the model
//! `carrel ask` would use, and its answer comes with the items it found, the passages it read,
//! its suggestions and the model that gave it. `s` saves the conversation to the Journal, and
//! `n` starts another. With `librarian.changes` off, `y` makes the chosen suggestion through
//! the flows that already exist: a correction shown before it's saved, or a rename or a new
//! collection asked about first. With it on, the librarian's changes this week are listed,
//! `u` undoes the chosen one and `U` all of them. When an item comes up that Carrel has a
//! question about, the librarian asks it, now and then, and `a` answers it.

use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
use carrel_core::history::Entry;
use carrel_core::librarian::changes::{Change, Outcome, Suggestion};
use carrel_core::librarian::{Answer, Conversation, ItemLink, Librarian, journal};
use carrel_core::providers::Registry;
use carrel_core::providers::device::AvailableModel;
use carrel_core::questions::QuestionEntry;
use carrel_core::questions::moments::Conversing;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::{App, Confirm, Effect, Step, Tab};
use super::item::ItemView;
use super::review::Handled;
use super::screens::{Correcting, Input, InputPurpose};
use crate::Failure;

/// How far back the screen lists the librarian's changes: a week, as `carrel status` does.
pub const CHANGES_DAYS: i64 = 7;

/// The conversation on the Librarian screen.
#[derive(Debug, Default)]
pub struct Talk {
    /// The answers so far.
    pub conversation: Conversation,
    /// The chosen row: an item, a suggestion or one of the librarian's changes.
    pub chosen: usize,
    /// The question being answered, while the librarian works on it.
    pub waiting: Option<String>,
    /// Which items the librarian has asked about in this conversation.
    pub conversing: Conversing,
    /// The question it asked after its last answer, if it did.
    pub question: Option<QuestionEntry>,
    /// The chosen item, for the right side.
    pub item: ItemView,
}

/// A row to choose on the Librarian screen.
#[derive(Debug, Clone, Copy)]
pub enum TalkRow<'a> {
    /// An item the last answer is about.
    Item(&'a ItemLink),
    /// A change the last answer suggested.
    Suggestion(&'a Suggestion),
    /// A change the librarian made on its own this week.
    Change(&'a Entry),
}

/// The librarian's changes still standing in `history`, from the last week.
pub fn recent_changes(history: &[Entry]) -> Vec<Entry> {
    let since = jiff::Timestamp::now().as_second() - CHANGES_DAYS * 86_400;
    carrel_core::librarian::changes::recent(history, since)
}

/// Asks `question` in `conversation` with `model` from `registry`, as `carrel ask` does,
/// letting the librarian make the changes it's sure of for `holder` when `librarian.changes`
/// is on. Returns the conversation with the answer added.
///
/// # Errors
///
/// Why the librarian couldn't answer, in words.
pub fn ask_with(
    root: &std::path::Path,
    (registry, model): (&Registry, AvailableModel),
    mut conversation: Conversation,
    question: &str,
    holder: Holder,
    progress: &super::embedding::Shared,
) -> Result<Conversation, String> {
    let archive = carrel_core::archive::Archive::open(root).map_err(|e| e.to_string())?;
    let providers = || crate::providers::registry().0;
    let label = model.label.clone();
    let tell = |p: carrel_core::librarian::Progress<'_>| {
        if let Ok(mut shared) = progress.lock() {
            *shared = Some(super::embedding::Progress {
                what: format!("Asking the librarian ({label})"),
                done: p.step.saturating_sub(1),
                total: p.steps,
                item: p.did.map(str::to_owned),
            });
        }
    };
    let librarian = Librarian::new(&archive, registry, model, &providers)
        .map_err(|e| e.to_string())?
        .with_changes_for(holder)
        .with_progress(&tell);
    librarian
        .ask(&mut conversation, question)
        .map_err(|e| e.to_string())?;
    Ok(conversation)
}

/// Asks `question` on this device's librarian, in the background.
pub fn ask_here(
    root: &std::path::Path,
    conversation: Conversation,
    question: &str,
    progress: &super::embedding::Shared,
) -> Result<Conversation, String> {
    let (registry, _) = crate::providers::registry();
    let model = crate::ask::the_model(&registry).map_err(|Failure(e)| e)?;
    let identity = crate::config::Identity::load_or_create().map_err(|e| e.to_string())?;
    ask_with(
        root,
        (&registry, model),
        conversation,
        question,
        identity.holder(),
        progress,
    )
}

/// Saves `conversation` to this month's Journal, as `carrel ask --save` does.
///
/// # Errors
///
/// Why it couldn't be saved, in words.
pub fn save(
    archive: &carrel_core::archive::Archive,
    conversation: &Conversation,
    holder: Holder,
) -> Result<String, String> {
    let lease = DiskLock::for_archive(archive)
        .acquire(holder, "librarian", DEFAULT_LEASE)
        .map_err(|e| e.to_string())?;
    let saved = journal::save(archive, conversation, &jiff::Zoned::now(), &lease);
    let _ = lease.release();
    saved
        .map(|s| format!("Saved the conversation to {}", s.path))
        .map_err(|e| e.to_string())
}

/// An answer as the lines of the conversation: the question, the answer, the passages, and
/// the model that gave it. Items and suggestions are rows below.
pub fn answer_lines(answer: &Answer) -> Vec<String> {
    let mut lines = vec![format!("You: {}", answer.question)];
    lines.extend(answer.text.trim().lines().map(str::to_owned));
    for quote in &answer.passages {
        lines.push(format!("  “{}” ({})", quote.text, quote.item.title));
    }
    let stopped = match answer.stopped {
        Some(carrel_core::librarian::Stopped::Steps) => "; out of steps",
        Some(carrel_core::librarian::Stopped::Tokens) => "; out of room",
        None => "",
    };
    lines.push(format!(
        "({} · {}{stopped})",
        answer.model,
        crate::commands::plural(answer.steps.len(), "step")
    ));
    lines.push(String::new());
    lines
}

/// A suggestion as a row: the change, how sure, and what became of it.
pub fn suggestion_row(suggestion: &Suggestion) -> String {
    let became = match &suggestion.outcome {
        Outcome::Made { .. } => "made by the librarian".to_owned(),
        Outcome::Proposed { .. } => "y to make it".to_owned(),
        Outcome::Refused { reason } => format!("not made: {reason}"),
        Outcome::Failed { reason } => format!("couldn't be made: {reason}"),
    };
    format!(
        "{} ({:.0}% sure; {became})",
        suggestion.change.describe(),
        suggestion.confidence * 100.0
    )
}

impl App {
    /// The rows of the Librarian screen: the last answer's items and suggestions, then the
    /// librarian's changes this week.
    pub fn talk_rows(&self) -> Vec<TalkRow<'_>> {
        let mut rows = Vec::new();
        if let Some(answer) = self.talk.conversation.answers.last() {
            rows.extend(answer.items.iter().map(TalkRow::Item));
            rows.extend(answer.suggestions.iter().map(TalkRow::Suggestion));
        }
        rows.extend(self.librarian_changes.iter().map(TalkRow::Change));
        rows
    }

    /// Moves the choice among the rows, and shows the chosen item.
    pub(super) fn talk_move(&mut self, delta: i32) {
        self.talk.chosen = super::app::step(self.talk.chosen, delta, self.talk_rows().len());
        self.load_talk_item();
    }

    /// Reads the chosen row's item, for the right side.
    pub(super) fn load_talk_item(&mut self) {
        let card = match self.talk_rows().get(self.talk.chosen) {
            Some(TalkRow::Item(item)) => Some(item.card.clone()),
            Some(TalkRow::Suggestion(s)) => s.change.item().map(|i| i.card.clone()),
            _ => None,
        };
        self.talk.item = match card {
            Some(card) if self.talk.item.card.as_deref() == Some(card.as_str()) => {
                return;
            }
            Some(card) => ItemView::load(&self.archive, &card, &self.history, &mut self.index),
            None => ItemView::default(),
        };
    }

    /// Takes the librarian's answer: the conversation with it, and the question it asks about
    /// an item that came up, now and then.
    pub(super) fn answered(&mut self, result: Result<Conversation, String>) {
        self.talk.waiting = None;
        match result {
            Ok(conversation) => {
                self.talk.conversation = conversation;
                let items: Vec<_> = self
                    .talk
                    .conversation
                    .answers
                    .last()
                    .map(|a| a.items.iter().map(|i| i.id).collect())
                    .unwrap_or_default();
                self.talk.question = self
                    .talk
                    .conversing
                    .after(&self.archive, &items, crate::discover::today())
                    .ok()
                    .flatten();
                self.talk.chosen = 0;
                self.talk.item = ItemView::default();
                // A change it made shows in History and in its list here.
                if let Err(Failure(e)) = self.reload() {
                    self.message = Some(e);
                }
                self.load_talk_item();
            }
            Err(e) => self.message = Some(e),
        }
    }

    /// Makes the suggestion `suggestion` through the flow the owner would use for it.
    fn make(&mut self, suggestion: &Suggestion) -> Option<Effect> {
        if !matches!(suggestion.outcome, Outcome::Proposed { .. }) {
            self.message = Some("That suggestion isn't waiting for you".into());
            return None;
        }
        match &suggestion.change {
            Change::Refile { item, to } => Some(Effect::Correction(Step::Work {
                target: Correcting::Item(item.card.clone()),
                words: format!("collection: {to}"),
                answering: None,
            })),
            Change::Correct { item, field, value } => Some(Effect::Correction(Step::Work {
                target: Correcting::Item(item.card.clone()),
                words: format!("{field}: {value}"),
                answering: None,
            })),
            Change::Rename { item, name } => {
                let from = item.card.rsplit('/').next().unwrap_or(&item.card);
                self.confirm = Some(Confirm::Rename {
                    card: item.card.clone(),
                    from: from.strip_suffix(".md").unwrap_or(from).to_owned(),
                    to: name.clone(),
                });
                None
            }
            Change::Collection { path, description } => {
                self.confirm = Some(Confirm::NewCollection(super::actions::NewCollection {
                    path: path.clone(),
                    description: Some(description.clone()),
                    tags: Vec::new(),
                }));
                None
            }
        }
    }

    /// Handles a key on the Librarian screen.
    pub(super) fn talk_key(&mut self, key: KeyEvent) -> Handled {
        let row = self.talk_rows().get(self.talk.chosen).copied();
        let effect = match (key.code, row) {
            (KeyCode::Enter, _) if self.talk.waiting.is_some() => {
                self.message = Some("The librarian is still answering".into());
                None
            }
            (KeyCode::Enter, _) => {
                self.input = Some(Input {
                    purpose: InputPurpose::Ask,
                    text: String::new(),
                });
                None
            }
            (KeyCode::Char('a'), _) => {
                match self.talk.question.take() {
                    Some(question) => self.answer_moment(question),
                    None => self.message = Some("The librarian hasn't asked anything".into()),
                }
                None
            }
            (KeyCode::Char('s'), _) if self.talk.conversation.answers.is_empty() => {
                self.message = Some("There's nothing to save yet".into());
                None
            }
            (KeyCode::Char('s'), _) => Some(Effect::SaveConversation),
            (KeyCode::Char('n'), _) => {
                self.talk = Talk::default();
                self.message = Some("A new conversation".into());
                None
            }
            (KeyCode::Char('y'), Some(TalkRow::Suggestion(s))) => {
                let s = s.clone();
                self.make(&s)
            }
            (KeyCode::Char('u'), Some(TalkRow::Change(entry))) => {
                self.confirm = Some(Confirm::Undo {
                    id: entry.id.clone(),
                    summary: entry.summary.clone(),
                });
                None
            }
            (KeyCode::Char('U'), _) if !self.librarian_changes.is_empty() => {
                self.confirm = Some(Confirm::UndoLibrarian(self.librarian_changes.len()));
                None
            }
            (KeyCode::Char('o') | KeyCode::Right, _) => {
                if let Some(card) = self.talk.item.card.clone() {
                    self.tab = Tab::Browse;
                    self.go_to(&card);
                }
                None
            }
            _ => return Handled::No,
        };
        Handled::Yes(effect)
    }

    /// The Librarian screen's keys, for the help line.
    pub(super) fn talk_keys(&self) -> Vec<(&'static str, &'static str)> {
        let mut keys = vec![("Enter", "ask"), ("↑↓", "choose")];
        match self.talk_rows().get(self.talk.chosen) {
            Some(TalkRow::Suggestion(s)) if matches!(s.outcome, Outcome::Proposed { .. }) => {
                keys.push(("y", "make it"));
            }
            Some(TalkRow::Change(_)) => keys.push(("u", "undo")),
            _ => {}
        }
        if self.talk.item.card.is_some() {
            keys.push(("o", "open in Browse"));
        }
        if self.talk.question.is_some() {
            keys.push(("a", "answer its question"));
        }
        if !self.librarian_changes.is_empty() {
            keys.push(("U", "undo all its changes"));
        }
        keys.extend([("s", "save"), ("n", "new"), ("?", "help")]);
        keys
    }
}

/// The librarian answering in the background, for the run loop.
#[derive(Default)]
pub struct Asking {
    answer: Option<std::sync::mpsc::Receiver<Result<Conversation, String>>>,
}

impl Asking {
    /// Whether the librarian is answering.
    pub fn busy(&self) -> bool {
        self.answer.is_some()
    }

    /// Asks `question` in the conversation on the Librarian screen, in the background.
    pub fn start(&mut self, app: &mut App, question: String) {
        let root = app.archive.root().to_owned();
        let conversation = app.talk.conversation.clone();
        let progress = app.progress.clone();
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(ask_here(&root, conversation, &question, &progress));
        });
        app.busy = Some("Asking the librarian…".into());
        self.answer = Some(receive);
    }

    /// Shows the answer once it's come.
    pub fn poll(&mut self, app: &mut App) {
        let Some(answer) = &self.answer else {
            return;
        };
        let Ok(result) = answer.try_recv() else {
            return;
        };
        self.answer = None;
        app.busy = None;
        if let Ok(mut progress) = app.progress.lock() {
            *progress = None;
        }
        app.answered(result);
    }
}

/// Saves the Librarian screen's conversation to the Journal, as the owner on this device.
pub fn save_here(app: &App) -> Result<String, String> {
    let identity = crate::config::Identity::load_or_create().map_err(|e| e.to_string())?;
    save(&app.archive, &app.talk.conversation, identity.holder())
}
