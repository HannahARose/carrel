//! The interactive mode's state, and what keys and clicks do to it. Nothing here draws or
//! touches the terminal, so it's tested directly.

use std::collections::BTreeSet;

use carrel_core::archive::{Archive, Collection};
use carrel_core::card::Id;
use carrel_core::history::{Entry, History, HistoryError};
use carrel_core::questions::{QuestionEntry, QuestionQueue};
use carrel_core::review::{ReviewEntry, ReviewQueue};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use super::review::Handled;
pub use super::review::Picker;
pub use super::screens::Input;
use crate::Failure;

/// The screens, in the order of their number keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// What `carrel status` shows.
    Status,
    /// The review queue.
    Review,
    /// The question queue.
    Questions,
    /// Collections and their items.
    Browse,
    /// Recorded changes.
    History,
}

impl Tab {
    /// Every tab, in order.
    pub const ALL: [Self; 5] = [
        Self::Status,
        Self::Review,
        Self::Questions,
        Self::Browse,
        Self::History,
    ];

    /// The tab's name.
    pub fn title(self) -> &'static str {
        match self {
            Self::Status => "Status",
            Self::Review => "Review",
            Self::Questions => "Questions",
            Self::Browse => "Browse",
            Self::History => "History",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

/// Something that waits for the owner to say yes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirm {
    /// Undoing the change with this identifier and summary.
    Undo {
        /// The commit.
        id: String,
        /// What it did, for people.
        summary: String,
    },
    /// Rejecting these proposals, which history can't undo.
    Reject(Vec<Id>),
}

/// Work the run loop does for the app: it needs the writer lease, which the app doesn't hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Undo the change with this identifier.
    Undo(String),
    /// File these proposals where proposed, or in Holding.
    Decide {
        /// The proposals.
        ids: Vec<Id>,
        /// Whether they go to Holding.
        holding: bool,
    },
    /// Approve every high-confidence proposal that isn't sensitive.
    ApproveAllHigh,
    /// Reject these proposals.
    Reject(Vec<Id>),
    /// Propose another collection for these proposals.
    Move {
        /// The proposals.
        ids: Vec<Id>,
        /// The collection's path.
        to: String,
    },
    /// Edit this proposal's card in the owner's editor.
    Edit(Id),
    /// Answer this question.
    Answer {
        /// The question.
        id: Id,
        /// A sentence for a term, or a collection's path.
        answer: String,
    },
    /// Dismiss this question.
    Dismiss(Id),
}

/// The interactive mode's state.
pub struct App {
    /// The archive shown.
    pub archive: Archive,
    /// The current screen.
    pub tab: Tab,
    /// What `carrel status` shows.
    pub status: Vec<String>,
    /// How far each report screen is scrolled.
    pub scroll: [u16; 5],
    /// The history, newest first.
    pub history: Vec<Entry>,
    /// The selected change.
    pub selected: usize,
    /// Proposals and questions waiting for the owner.
    pub waiting: (usize, usize),
    /// The review queue: everything not yet rejected or filed, by destination.
    pub queue: Vec<ReviewEntry>,
    /// The chosen proposal.
    pub chosen: usize,
    /// Proposals marked for a decision together.
    pub marked: BTreeSet<Id>,
    /// The chosen proposal in full, as `carrel review show` gives it.
    pub detail: Vec<String>,
    /// The collection picker, while it's open.
    pub picker: Option<Picker>,
    /// Work running in the background, described for the owner.
    pub busy: Option<String>,
    /// The questions, most items first.
    pub asked: Vec<QuestionEntry>,
    /// The chosen question.
    pub asked_chosen: usize,
    /// The collections, for Browse.
    pub collections: Vec<Collection>,
    /// Browse's chosen collection.
    pub browse_collection: usize,
    /// The chosen collection's cards, or what a search found, as card paths and titles.
    pub browse_items: Vec<(String, String)>,
    /// Browse's chosen item.
    pub browse_item: usize,
    /// Whether Browse's keys move among items rather than collections.
    pub browse_in_items: bool,
    /// The search shown in Browse, if any.
    pub search: Option<String>,
    /// The chosen item's card in full, as `carrel show` gives it.
    pub card: Vec<String>,
    /// A line being typed, while it's open.
    pub input: Option<Input>,
    /// A line for the owner about the last thing that happened.
    pub message: Option<String>,
    /// A question waiting for yes or no.
    pub confirm: Option<Confirm>,
    /// Whether every key is shown.
    pub help: bool,
    /// Whether to leave.
    pub quit: bool,
}

fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

impl App {
    /// The app for `archive`, with everything loaded.
    ///
    /// # Errors
    ///
    /// [`Failure`] if the archive can't be read.
    pub fn new(archive: Archive) -> Result<Self, Failure> {
        let mut app = Self {
            archive,
            tab: Tab::Status,
            status: Vec::new(),
            scroll: [0; 5],
            history: Vec::new(),
            selected: 0,
            waiting: (0, 0),
            queue: Vec::new(),
            chosen: 0,
            marked: BTreeSet::new(),
            detail: Vec::new(),
            picker: None,
            busy: None,
            asked: Vec::new(),
            asked_chosen: 0,
            collections: Vec::new(),
            browse_collection: 0,
            browse_items: Vec::new(),
            browse_item: 0,
            browse_in_items: false,
            search: None,
            card: Vec::new(),
            input: None,
            message: None,
            confirm: None,
            help: false,
            quit: false,
        };
        app.reload()?;
        Ok(app)
    }

    /// Reads everything again.
    ///
    /// # Errors
    ///
    /// [`Failure`] if the archive can't be read.
    pub fn reload(&mut self) -> Result<(), Failure> {
        self.archive = Archive::open(self.archive.root())?;
        let root_path = self.archive.root().to_owned();
        let root = Some(root_path.as_path());
        self.status = lines(&crate::commands::status(root)?.text);
        self.history = match History::open(&self.archive) {
            Ok(history) => history.log()?,
            Err(HistoryError::NotInitialized) => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        self.selected = self.selected.min(self.history.len().saturating_sub(1));
        let (entries, _) = ReviewQueue::of(&self.archive).list()?;
        let (questions, _) = QuestionQueue::of(&self.archive).list()?;
        self.waiting = (
            entries.iter().filter(|e| e.state.is_open()).count(),
            questions.len(),
        );
        self.load_queue(entries);
        self.load_questions(questions);
        self.load_browse()?;
        Ok(())
    }

    /// The lines of the current screen, for the screens that show a report.
    pub fn report(&self) -> &[String] {
        match self.tab {
            Tab::Status => &self.status,
            _ => &[],
        }
    }

    /// The keys the current screen answers to, for the help line.
    pub fn keys(&self) -> Vec<(&'static str, &'static str)> {
        if self.confirm.is_some() {
            return vec![("y", "yes"), ("n", "no")];
        }
        if self.tab == Tab::Review {
            // The decisions need the room; the screen keys are in ? help.
            return vec![
                ("a", "approve"),
                ("h", "hold"),
                ("m", "move"),
                ("e", "edit"),
                ("x", "reject"),
                ("Space", "mark"),
                ("A", "all high"),
                ("?", "help"),
                ("q", "quit"),
            ];
        }
        let mut keys = vec![("1-5", "screens")];
        if let Some(input) = &self.input {
            return vec![
                ("type", input.prompt()),
                ("Enter", "done"),
                ("Esc", "cancel"),
            ];
        }
        if self.picker.is_some() {
            return vec![
                ("↑↓", "choose"),
                ("type", "filter"),
                ("Enter", "choose it"),
                ("Esc", "cancel"),
            ];
        }
        match self.tab {
            Tab::History => keys.extend([("↑↓", "choose"), ("u", "undo")]),
            Tab::Questions => {
                keys.extend([("↑↓", "choose"), ("Enter", "answer"), ("d", "dismiss")]);
            }
            Tab::Browse => keys.extend([
                ("↑↓", "choose"),
                ("←→", "collections/items"),
                ("/", "search"),
            ]),
            _ => keys.push(("↑↓", "scroll")),
        }
        keys.extend([("^R", "refresh"), ("?", "help"), ("q", "quit")]);
        keys
    }

    pub(super) fn move_by(&mut self, delta: i32) {
        if self.tab == Tab::Review {
            self.choose(delta);
        } else if self.tab == Tab::Questions {
            self.asked_chosen = step(self.asked_chosen, delta, self.asked.len());
        } else if self.tab == Tab::Browse {
            self.browse_move(delta);
        } else if self.tab == Tab::History {
            let last = self.history.len().saturating_sub(1);
            let next = i64::try_from(self.selected).unwrap_or(0) + i64::from(delta);
            self.selected = usize::try_from(next.max(0)).unwrap_or(0).min(last);
        } else {
            let lines = u16::try_from(self.report().len()).unwrap_or(u16::MAX);
            let scroll = &mut self.scroll[self.tab.index()];
            let next = i32::from(*scroll) + delta;
            *scroll = u16::try_from(next.max(0))
                .unwrap_or(0)
                .min(lines.saturating_sub(1));
        }
    }

    /// Handles a key, returning work for the run loop when there is some.
    pub fn key(&mut self, key: KeyEvent) -> Option<Effect> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return None;
        }
        if let Some(confirm) = self.confirm.take() {
            if matches!(key.code, KeyCode::Char('y' | 'Y')) {
                return Some(match confirm {
                    Confirm::Undo { id, .. } => Effect::Undo(id),
                    Confirm::Reject(ids) => Effect::Reject(ids),
                });
            }
            self.message = Some("Nothing changed".into());
            return None;
        }
        if self.help {
            self.help = false;
            return None;
        }
        if self.input.is_some() {
            return self.input_key(key);
        }
        if self.picker.is_some() {
            return self.picker_key(key);
        }
        self.message = None;
        let handled = match self.tab {
            Tab::Review => self.review_key(key),
            Tab::Questions => self.questions_key(key),
            Tab::Browse => self.browse_key(key),
            _ => Handled::No,
        };
        if let Handled::Yes(effect) = handled {
            return effect;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.message = Some(match self.reload() {
                    Ok(()) => "Refreshed".into(),
                    Err(Failure(e)) => e,
                });
            }
            KeyCode::Char(c @ '1'..='5') => {
                self.tab = Tab::ALL[usize::from(c as u8 - b'1')];
            }
            KeyCode::Tab | KeyCode::Right => {
                self.tab = Tab::ALL[(self.tab.index() + 1) % Tab::ALL.len()];
            }
            KeyCode::BackTab | KeyCode::Left => {
                self.tab = Tab::ALL[(self.tab.index() + Tab::ALL.len() - 1) % Tab::ALL.len()];
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::Home => self.move_by(i32::MIN / 2),
            KeyCode::End => self.move_by(i32::MAX / 2),
            KeyCode::Char('u') if self.tab == Tab::History => {
                if let Some(entry) = self.history.get(self.selected) {
                    self.confirm = Some(Confirm::Undo {
                        id: entry.id.clone(),
                        summary: entry.summary.clone(),
                    });
                }
            }
            _ => {}
        }
        None
    }

    /// Handles the mouse: the wheel scrolls, and a click on the top line picks a screen.
    pub fn mouse(&mut self, event: MouseEvent, tab_starts: &[u16]) {
        match event.kind {
            MouseEventKind::ScrollDown => self.move_by(3),
            MouseEventKind::ScrollUp => self.move_by(-3),
            MouseEventKind::Down(_) if event.row == 0 && self.confirm.is_none() => {
                if let Some(i) = tab_starts.iter().rposition(|start| event.column >= *start) {
                    self.tab = Tab::ALL[i];
                }
            }
            _ => {}
        }
    }
}

/// `index` moved by `delta` within `len` items.
pub(super) fn step(index: usize, delta: i32, len: usize) -> usize {
    let next = i64::try_from(index).unwrap_or(0) + i64::from(delta);
    usize::try_from(next.max(0))
        .unwrap_or(0)
        .min(len.saturating_sub(1))
}
