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
    /// Items waiting to be drafted, and adding more.
    Ingest,
    /// This device's and the archive's settings.
    Settings,
    /// Backups: snapshots, and taking, checking and restoring from them.
    Backups,
}

impl Tab {
    /// Every tab, in order.
    pub const ALL: [Self; 8] = [
        Self::Status,
        Self::Review,
        Self::Questions,
        Self::Browse,
        Self::History,
        Self::Ingest,
        Self::Settings,
        Self::Backups,
    ];

    /// The tab's name.
    pub fn title(self) -> &'static str {
        match self {
            Self::Status => "Status",
            Self::Review => "Review",
            Self::Questions => "Questions",
            Self::Browse => "Browse",
            Self::History => "History",
            Self::Ingest => "Ingest",
            Self::Settings => "Settings",
            Self::Backups => "Backups",
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
    /// Downloading this catalog model.
    Download {
        /// The model's name.
        name: String,
        /// What it is, its size and license, and where it goes.
        about: String,
    },
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
    /// File this proposal linked to an archived item it duplicates.
    Keep {
        /// The proposal.
        id: Id,
        /// As a new version, or beside it.
        kept: carrel_core::actions::Kept,
        /// The archived item.
        target: Id,
    },
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
    /// Add these paths to the intake queue, and start a worker.
    Queue {
        /// The files and folders.
        paths: Vec<std::path::PathBuf>,
        /// How folders among them are read.
        options: carrel_core::ingest::IntakeOptions,
    },
    /// Take this item off the intake queue.
    Unqueue(Id),
    /// Move this item to the front of the intake queue.
    Front(Id),
    /// Try this failed item again.
    Retry(Id),
    /// Run the owner's file picker, with the terminal to itself, and plan what it chose.
    Pick(Vec<String>),
    /// Set a setting, as `carrel settings set` does.
    SetSetting {
        /// The setting.
        key: String,
        /// Its new value.
        value: String,
    },
    /// Download this catalog model.
    Download(String),
    /// Take a backup snapshot now.
    BackupNow,
    /// Check the backups, reading back a share of the files.
    BackupVerify,
    /// Restore an item as of a change's snapshot, beside the archive.
    BackupRestore {
        /// The item: its identifier, or its card's or contents' path.
        item: String,
        /// The history commit whose snapshot to restore from.
        at: String,
    },
    /// Create or connect to a backup repository.
    BackupSetup {
        /// Where it is or will be.
        repository: std::path::PathBuf,
        /// Its password, handed to Kopia, which keeps it in the keychain.
        password: super::backups::Secret,
    },
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
    pub scroll: [u16; 7],
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
    /// The archived items the chosen proposal duplicates, side by side with it.
    pub comparisons: Vec<carrel_core::actions::Comparison>,
    /// The comparison shown.
    pub compared: usize,
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
    /// This device, whose intake queue is shown.
    pub device: Option<Id>,
    /// This device's intake queue: waiting, then failed.
    pub intake: Vec<carrel_core::intake::IntakeEntry>,
    /// The chosen queued item.
    pub intake_chosen: usize,
    /// What would be added, while it's shown before queueing.
    pub planned: Option<super::intake::Plan>,
    /// What the worker was doing when last looked at.
    pub worker: super::intake::Worker,
    /// The Settings screen's rows.
    pub settings: Vec<super::settings::Shown>,
    /// The chosen setting.
    pub settings_chosen: usize,
    /// The chosen snapshot on the Backups screen.
    pub backup_chosen: usize,
    /// The built-in file browser, while it's open.
    pub browser: Option<super::browser::Browser>,
    /// The `picker` preference.
    pub picker_preference: Option<String>,
    /// Where picking files starts: the folder last picked from, or the home folder.
    pub start_folder: std::path::PathBuf,
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
            scroll: [0; 7],
            history: Vec::new(),
            selected: 0,
            waiting: (0, 0),
            queue: Vec::new(),
            chosen: 0,
            marked: BTreeSet::new(),
            detail: Vec::new(),
            comparisons: Vec::new(),
            compared: 0,
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
            device: crate::config::Identity::load()
                .ok()
                .flatten()
                .map(|i| i.device),
            intake: Vec::new(),
            intake_chosen: 0,
            planned: None,
            worker: super::intake::Worker::default(),
            settings: Vec::new(),
            settings_chosen: 0,
            backup_chosen: 0,
            browser: None,
            picker_preference: None,
            start_folder: std::path::PathBuf::from("."),
            help: false,
            quit: false,
        };
        let prefs = crate::config::Prefs::load().unwrap_or_default();
        app.picker_preference = prefs.picker;
        app.start_folder = prefs
            .last_folder
            .filter(|f| f.is_dir())
            .or_else(carrel_core::local::home_dir)
            .unwrap_or_else(|| std::path::PathBuf::from("."));
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
        self.load_intake();
        self.load_settings();
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
        if self.tab == Tab::Review && !self.comparisons.is_empty() {
            let mut keys = vec![("v", "file as version"), ("b", "keep both"), ("s", "skip")];
            if self.comparisons.len() > 1 {
                keys.push(("[ ]", "other match"));
            }
            keys.extend([
                ("a", "approve"),
                ("h", "hold"),
                ("e", "edit"),
                ("?", "help"),
            ]);
            return keys;
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
        let mut keys = vec![("1-7", "screens")];
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
        if self.browser.is_some() {
            return vec![
                ("↑↓", "choose"),
                ("Enter ←", "open, up"),
                ("Space", "mark"),
                ("i", "add"),
                (".", "hidden"),
                ("/", "type a path"),
                ("Esc", "cancel"),
            ];
        }
        if self.tab == Tab::Ingest && self.planned.is_some() {
            return vec![
                ("Enter", "queue them"),
                ("e", "each entry"),
                ("w", "whole folder"),
                ("Esc", "cancel"),
            ];
        }
        match self.tab {
            Tab::Settings => keys.extend([("↑↓", "choose"), ("Enter", "change")]),
            Tab::Ingest => keys.extend([
                ("i", "add"),
                ("↑↓", "choose"),
                ("x", "remove"),
                ("t", "to front"),
                ("r", "retry"),
            ]),
            Tab::History => keys.extend([("↑↓", "choose"), ("u", "undo")]),
            Tab::Backups => keys.extend([
                ("↑↓", "choose"),
                ("n", "snapshot now"),
                ("v", "verify"),
                ("r", "restore"),
                ("s", "set up"),
            ]),
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
        } else if self.tab == Tab::Ingest {
            self.intake_move(delta);
        } else if self.tab == Tab::Settings {
            self.settings_move(delta);
        } else if self.tab == Tab::Backups {
            self.backups_move(delta);
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
                    Confirm::Download { name, .. } => Effect::Download(name),
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
        if let Some(browser) = &mut self.browser {
            match browser.key(key) {
                super::browser::Outcome::Browsing => {}
                super::browser::Outcome::Add(paths) => {
                    self.browser = None;
                    self.plan(paths, carrel_core::ingest::IntakeOptions::default());
                }
                super::browser::Outcome::Type => {
                    self.browser = None;
                    self.ask_for_paths();
                }
                super::browser::Outcome::Cancel => self.browser = None,
            }
            return None;
        }
        if self.picker.is_some() {
            return self.picker_key(key);
        }
        self.message = None;
        let handled = match self.tab {
            Tab::Review => self.review_key(key),
            Tab::Questions => self.questions_key(key),
            Tab::Browse => self.browse_key(key),
            Tab::Ingest => self.intake_key(key),
            Tab::Settings => self.settings_key(key),
            Tab::Backups => self.backups_key(key),
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
            KeyCode::Char('i') => return self.start_ingest(),
            KeyCode::Char(c @ '1'..='8') => {
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
