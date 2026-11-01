//! The interactive mode's state, and what keys and clicks do to it. Nothing here draws or
//! touches the terminal, so it's tested directly.

use std::collections::BTreeSet;

use carrel_core::archive::{Archive, Collection};
use carrel_core::card::Id;
use carrel_core::history::{Entry, History, HistoryError};
use carrel_core::questions::QuestionEntry;
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
    /// What's checked out and on the desk, and their pending history entries.
    Active,
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
    /// A conversation with the librarian, and the item it's discussing.
    Librarian,
    /// Discovery: on this day, wander, the timeline, the map and activity.
    Discover,
}

impl Tab {
    /// Every tab, in order.
    pub const ALL: [Self; 11] = [
        Self::Status,
        Self::Review,
        Self::Active,
        Self::Questions,
        Self::Browse,
        Self::History,
        Self::Ingest,
        Self::Settings,
        Self::Backups,
        Self::Librarian,
        Self::Discover,
    ];

    /// The tab's name.
    pub fn title(self) -> &'static str {
        match self {
            Self::Status => "Status",
            Self::Review => "Review",
            Self::Active => "Active",
            Self::Questions => "Questions",
            Self::Browse => "Browse",
            Self::History => "History",
            Self::Ingest => "Ingest",
            Self::Settings => "Settings",
            Self::Backups => "Backups",
            Self::Librarian => "Librarian",
            Self::Discover => "Discover",
        }
    }

    /// The key that shows it: its number, or for the librarian and discovery, `L` and `D`.
    pub fn key(self) -> char {
        match self {
            Self::Librarian => 'L',
            Self::Discover => 'D',
            tab => char::from(b'1' + u8::try_from(tab.index()).unwrap_or(0)),
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

/// Where what the interactive mode learns about the owner, such as the folder they pick
/// from, is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preferences {
    /// In this device's preferences, for next time.
    Saved,
    /// For this session only.
    InMemory,
}

/// What the file picker is choosing for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Picking {
    /// Files and folders to ingest.
    Ingest,
    /// The folder for a backup repository.
    BackupFolder,
    /// The folder to check the item in `App::checking_out` out to.
    CheckoutFolder,
    /// A file or folder to put on the desk.
    DeskItem,
}

/// Something that waits for the owner to say yes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirm {
    /// Installing or uninstalling the background agent.
    Agent(super::agent::AgentChange),
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
    /// Returning this checkout, replacing the archive copy.
    Return {
        /// The item.
        id: Id,
        /// Its title.
        title: String,
        /// What changed and the history entry it adds, as `carrel return` shows them.
        changes: String,
        /// Where the working copy is now, when it moved, as `carrel return --from` says.
        from: Option<std::path::PathBuf>,
        /// Whether backups are set up, so `t` can forget the checkout's session snapshots.
        snapshots: bool,
    },
    /// Cancelling this checkout, leaving the working copy's changes out.
    CancelCheckout {
        /// The item.
        id: Id,
        /// Its title.
        title: String,
    },
    /// Answering a sender question by gathering its items in a collection, and whether to
    /// add a routing rule for future ones too.
    Rule {
        /// The question.
        id: Id,
        /// The answer: a suggestion's number, yes, or a collection's path.
        answer: String,
        /// The sender or project.
        subject: String,
        /// The collection.
        path: String,
    },
    /// Adding a fact a correction stated to the glossary.
    Learn {
        /// The word or name it's about.
        about: String,
        /// The fact.
        fact: String,
    },
    /// Checking this item out to a folder, as `carrel checkout ITEM --to --due` does.
    Checkout {
        /// The item's card path.
        card: String,
        /// Its title.
        title: String,
        /// The folder the working copy goes in.
        to: std::path::PathBuf,
        /// When it's due back, as YYYY-MM-DD.
        due: Option<String>,
    },
    /// Putting this file or folder on the desk, as `carrel desk add` does.
    DeskAdd(super::option_lines::DeskCard),
    /// Taking this desk item off the desk: `y` keeps its card, `d` deletes it too.
    DeskRemove {
        /// The desk item.
        path: std::path::PathBuf,
        /// Its title.
        title: String,
    },
    /// Queueing this desk item for ingest and taking it off the desk.
    DeskArchive {
        /// The desk item.
        path: std::path::PathBuf,
        /// Its title.
        title: String,
    },
    /// Creating a collection with its design document, as `carrel collection new` does.
    NewCollection(super::actions::NewCollection),
    /// Renaming an item and its card together.
    Rename {
        /// The item's card path.
        card: String,
        /// Its name now.
        from: String,
        /// The new name.
        to: String,
    },
    /// Expanding a filed compressed file.
    Expand {
        /// The item's card path.
        card: String,
        /// Its title.
        title: String,
        /// Why it's kept compressed, when it's expanded all the same.
        anyway: Option<String>,
    },
    /// Undoing all of the librarian's changes this week, this many.
    UndoLibrarian(usize),
}

/// A step of a correction, which the run loop does itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Work out what these words change in this card, in the background.
    Work {
        /// The proposal or filed item.
        target: super::screens::Correcting,
        /// What the owner typed.
        words: String,
        /// The item question it answers, which leaves the queue once it's saved.
        answering: Option<Id>,
    },
    /// Save the correction shown in the pop-up.
    Save,
    /// Add this fact from a correction to the glossary.
    Learn {
        /// The word or name it's about.
        about: String,
        /// The fact.
        fact: String,
    },
}

/// Work the run loop does for the app: it needs the writer lease, which the app doesn't hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Install or uninstall the background agent, as `carrel agent` does.
    Agent(super::agent::AgentChange),
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
    /// Expand these compressed files when they're approved, or keep them compressed.
    Compressed {
        /// The proposals.
        ids: Vec<Id>,
        /// Expand or keep.
        choice: carrel_core::compressed::plan::Choice,
    },
    /// Propose another collection for these proposals.
    Move {
        /// The proposals.
        ids: Vec<Id>,
        /// The collection's path.
        to: String,
    },
    /// Edit this proposal's card in the owner's editor.
    Edit(Id),
    /// Edit this filed item's card, by its card's path, in the owner's editor.
    EditItem(String),
    /// Edit this filed item's card body, by its card's path, in the owner's editor, as
    /// `carrel edit --body` does.
    EditItemBody(String),
    /// Edit an active item's pending history entry in the owner's editor, as
    /// `carrel sessions edit` does.
    EditEntry {
        /// The item, as `carrel sessions` names it.
        item: String,
        /// The entry's number.
        number: usize,
    },
    /// A step of correcting a card in words.
    Correction(Step),
    /// Answer this question.
    Answer {
        /// The question.
        id: Id,
        /// A suggestion's number, a sentence for a term, a collection's path, or yes or no.
        answer: String,
        /// For a sender, whether the owner said yes to a routing rule for future items.
        learn: bool,
    },
    /// Skip this question for now.
    Skip(Id),
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
    /// Return this checkout, as `carrel return --yes [--from] [--trim-snapshots]` does.
    Return(super::active::Returning),
    /// Cancel this checkout, as `carrel return --discard --yes` does.
    CancelCheckout(Id),
    /// Open this working copy or desk item in the system's own app for it.
    Open(std::path::PathBuf),
    /// Decide an active item's pending history entries, as `carrel sessions` does.
    Entries {
        /// The item, as `carrel sessions` names it.
        item: String,
        /// The entries' numbers; none means all of them.
        numbers: Vec<usize>,
        /// Approve, combine or drop them.
        decision: crate::sessions::Decision,
    },
    /// Check this item out, as `carrel checkout ITEM --to FOLDER --due DATE` does.
    Checkout {
        /// The item's card path.
        item: String,
        /// The folder the working copy goes in.
        to: std::path::PathBuf,
        /// When it's due back, as YYYY-MM-DD.
        due: Option<String>,
    },
    /// Put this file or folder on the desk, as `carrel desk add [--title] [--summary]` does.
    DeskAdd(super::option_lines::DeskCard),
    /// Take this desk item off the desk, as `carrel desk remove [--delete-card]` does.
    DeskRemove {
        /// The desk item.
        path: std::path::PathBuf,
        /// Whether its card goes too.
        delete_card: bool,
    },
    /// Archive this desk item, as `carrel desk archive` does.
    DeskArchive(std::path::PathBuf),
    /// Create a collection, as `carrel collection new` does.
    NewCollection(super::actions::NewCollection),
    /// Rename an item and its card, as `carrel rename` does.
    Rename {
        /// The item's card path.
        item: String,
        /// The new name.
        new_name: String,
    },
    /// Expand a filed compressed file, as `carrel expand [--anyway]` does.
    Expand {
        /// The item's card path.
        item: String,
        /// Whether to expand it even though there's a reason to keep it compressed.
        anyway: bool,
    },
    /// Take a backup snapshot now.
    BackupNow,
    /// Check the backups, reading back this percentage of the files.
    BackupVerify(u8),
    /// Restore an item as of a change's snapshot, beside the archive or in a chosen folder.
    BackupRestore {
        /// The item: its identifier, or its card's or contents' path.
        item: String,
        /// The history commit whose snapshot to restore from.
        at: String,
        /// The folder to restore into, as `--to` says; beside the archive when none.
        to: Option<std::path::PathBuf>,
    },
    /// Create or connect to a backup repository.
    BackupSetup {
        /// Where it is or will be.
        repository: std::path::PathBuf,
        /// Its password, handed to Kopia, which keeps it in the keychain.
        password: super::backups::Secret,
        /// The repository's name, as `--description` gives it; the default when none.
        description: Option<String>,
    },
    /// Rebuild the embedding index, or switch the archive's model, as `carrel index` does.
    Index(super::embedding::Embedding),
    /// Show the archive at this path instead, as `carrel open` does.
    OpenArchive(std::path::PathBuf),
    /// Create an archive, as `carrel init --embedding-model` does, and show it.
    CreateArchive(super::archives::NewArchive),
    /// Start history for the existing archive at this path, as `carrel init` does there.
    StartHistory(std::path::PathBuf),
    /// Ask Carrel for its questions about the item at this card, as `carrel questions about`.
    QuestionsAbout(String),
    /// Ask the librarian this, in the conversation on the Librarian screen.
    Ask(String),
    /// Save the conversation to the Journal, as `carrel ask --save` does.
    SaveConversation,
    /// Undo the librarian's changes this week, as `carrel undo --librarian 7d` does.
    UndoLibrarian,
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
    pub scroll: [u16; Tab::ALL.len()],
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
    /// What's checked out, then what's on this device's desk.
    pub active: Vec<super::active::ActiveItem>,
    /// The chosen row of the Active list: an item or one of its pending entries.
    pub active_chosen: usize,
    /// Pending entries marked for a decision together.
    pub active_marked: super::active::Marks,
    /// The questions, the most worth answering first.
    pub asked: Vec<QuestionEntry>,
    /// How many items an answer to each question would settle.
    pub asked_settles: Vec<usize>,
    /// How many questions are resting after a skip.
    pub resting: usize,
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
    /// Where Browse's arrows move: collections, items, or the chosen item's view.
    pub browse_pane: super::item::Pane,
    /// The search shown in Browse, if any.
    pub search: Option<String>,
    /// What it found, and why.
    pub searched: super::search::Searched,
    /// The chosen item: its card, its changes and related items.
    pub item: super::item::ItemView,
    /// The archive's embedding index, which the item's view finds the items nearest it in,
    /// read again only when it changes on disk.
    pub index: crate::related::Cached,
    /// A line being typed, while it's open.
    pub input: Option<Input>,
    /// A line for the owner about the last thing that happened.
    pub message: Option<String>,
    /// A question waiting for yes or no.
    pub confirm: Option<Confirm>,
    /// A correction worked out and shown in a pop-up, waiting for `y` or `n`.
    pub correction: Option<crate::correct::Prepared>,
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
    /// What the providers on this device offer, found when Settings is first shown.
    pub here: Option<super::embedding::Here>,
    /// What the providers offer, while it's being found in the background.
    pub finding_here: Option<std::sync::mpsc::Receiver<super::embedding::Here>>,
    /// How far work in the background has got, when it says.
    pub progress: super::embedding::Shared,
    /// The archives opened or created on this device, for the archive switcher.
    pub known_archives: Vec<std::path::PathBuf>,
    /// The chosen snapshot on the Backups screen.
    pub backup_chosen: usize,
    /// The built-in file browser, while it's open.
    pub browser: Option<super::browser::Browser>,
    /// The `picker` preference.
    pub picker_preference: Option<String>,
    /// Where picking files starts: the folder last picked from, or the home folder.
    pub start_folder: std::path::PathBuf,
    /// What the file picker is choosing for.
    pub picking: Picking,
    /// The item being checked out, as its card's path and title, while its folder is chosen.
    pub checking_out: Option<(String, String)>,
    /// Whether the folder picking ends in is saved to this device's preferences, or only kept
    /// for this session, as in tests, which must never write the real configuration.
    pub preferences: Preferences,
    /// Where the background agent's service is read and installed: nowhere in tests, which
    /// must never write the real home folder.
    pub service: super::agent::Service,
    /// What the last change to the agent did, line by line.
    pub agent_done: Vec<String>,
    /// Item previews in Browse, on terminals that draw images.
    pub previews: super::preview::Previews,
    /// This device's records of discovery and of the day's questions: saved, or in tests
    /// kept in memory, so they never write the real configuration.
    pub records: crate::asking::Records,
    /// Where Status looks for active work: the person's home, or in tests none, so they
    /// never look in the person's own folders.
    pub active_work_home: Option<std::path::PathBuf>,
    /// The open questions about the item Browse shows.
    pub item_questions: Vec<QuestionEntry>,
    /// The conversation on the Librarian screen.
    pub talk: super::librarian::Talk,
    /// The librarian answering in the background.
    pub asking: super::librarian::Asking,
    /// The librarian's changes this week, newest first.
    pub librarian_changes: Vec<Entry>,
    /// The Discover screen.
    pub stacks: super::discover::Stacks,
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
        let archive_for_records = archive.clone();
        let mut app = Self {
            archive,
            tab: Tab::Status,
            status: Vec::new(),
            scroll: [0; Tab::ALL.len()],
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
            correction: None,
            busy: None,
            active: Vec::new(),
            active_chosen: 0,
            active_marked: BTreeSet::new(),
            asked: Vec::new(),
            asked_settles: Vec::new(),
            resting: 0,
            asked_chosen: 0,
            collections: Vec::new(),
            browse_collection: 0,
            browse_items: Vec::new(),
            browse_item: 0,
            browse_pane: super::item::Pane::Collections,
            search: None,
            searched: super::search::Searched::default(),
            item: super::item::ItemView::default(),
            index: crate::related::Cached::default(),
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
            here: None,
            finding_here: None,
            progress: super::embedding::Shared::default(),
            known_archives: Vec::new(),
            backup_chosen: 0,
            browser: None,
            picker_preference: None,
            start_folder: std::path::PathBuf::from("."),
            picking: Picking::Ingest,
            checking_out: None,
            preferences: if cfg!(test) {
                Preferences::InMemory
            } else {
                Preferences::Saved
            },
            service: if cfg!(test) {
                super::agent::Service::default()
            } else {
                super::agent::Service::here()
            },
            agent_done: Vec::new(),
            previews: super::preview::Previews::default(),
            records: if cfg!(test) {
                crate::asking::Records::in_memory(&archive_for_records)
            } else {
                crate::asking::Records::here(&archive_for_records)
            },
            active_work_home: if cfg!(test) {
                None
            } else {
                carrel_core::local::home_dir()
            },
            item_questions: Vec::new(),
            talk: super::librarian::Talk::default(),
            asking: super::librarian::Asking::default(),
            librarian_changes: Vec::new(),
            stacks: super::discover::Stacks::default(),
            help: false,
            quit: false,
        };
        let prefs = crate::config::Prefs::load().unwrap_or_default();
        app.known_archives = prefs.known_archives();
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
        self.status = lines(
            &crate::commands::status_on(
                &self.archive,
                &mut self.records,
                self.active_work_home.clone(),
            )?
            .text,
        );
        self.status.extend(self.index_status());
        self.status
            .extend(crate::usage_report::status_lines(&self.archive));
        self.history = match History::open(&self.archive) {
            Ok(history) => history.log()?,
            Err(HistoryError::NotInitialized) => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        self.selected = self.selected.min(self.history.len().saturating_sub(1));
        self.librarian_changes = super::librarian::recent_changes(&self.history);
        let ranked = carrel_core::questions::answer::tidy(&self.archive)?;
        let (entries, _) = ReviewQueue::of(&self.archive).list()?;
        self.waiting = (
            entries.iter().filter(|e| e.state.is_open()).count(),
            ranked.open.len(),
        );
        self.load_queue(entries);
        self.load_questions(ranked);
        self.load_browse()?;
        self.load_intake();
        self.load_settings();
        self.load_active();
        if !self.stacks.lines.is_empty() {
            self.load_discover();
        }
        Ok(())
    }

    /// The lines of the current screen, for the screens that show a report.
    pub fn report(&self) -> &[String] {
        match self.tab {
            Tab::Status => &self.status,
            _ => &[],
        }
    }

    /// Review's keys: a duplicate's choices when one is shown, otherwise the decisions.
    fn review_keys(&self) -> Vec<(&'static str, &'static str)> {
        if !self.comparisons.is_empty() {
            let mut keys = vec![("v", "file as version"), ("b", "keep both"), ("s", "skip")];
            if self.comparisons.len() > 1 {
                keys.push(("[ ]", "other match"));
            }
            keys.extend([
                ("a", "approve"),
                ("h", "hold"),
                ("e", "edit"),
                ("c", "correct"),
                ("?", "help"),
            ]);
            return keys;
        }
        // The decisions need the room; the screen keys are in ? help.
        let mut keys = vec![
            ("a", "approve"),
            ("h", "hold"),
            ("m", "move"),
            ("e", "edit"),
            ("c", "correct"),
            ("x", "reject"),
            ("Space", "mark"),
            ("A", "all high"),
            ("?", "help"),
            ("q", "quit"),
        ];
        if let Some(plan) = self
            .queue
            .get(self.chosen)
            .and_then(|e| e.compressed.as_ref())
        {
            let label = if plan.action == carrel_core::compressed::plan::Choice::Expand {
                "keep compressed"
            } else {
                "expand"
            };
            keys.insert(3, ("z", label));
        }
        keys
    }

    /// The keys the current screen answers to, for the help line.
    pub fn keys(&self) -> Vec<(&'static str, &'static str)> {
        if self.correction.is_some() {
            return vec![("y", "save"), ("n", "discard")];
        }
        if let Some(confirm) = &self.confirm {
            return confirm_keys(confirm);
        }
        // Typing comes before any screen's keys: they don't apply while a line is open.
        if let Some(input) = &self.input {
            return vec![
                ("type", input.prompt()),
                ("Enter", "done"),
                ("Esc", "cancel"),
            ];
        }
        if self.tab == Tab::Review {
            return self.review_keys();
        }
        if self.tab == Tab::Librarian && self.picker.is_none() && self.browser.is_none() {
            return self.talk_keys();
        }
        if self.tab == Tab::Discover && self.picker.is_none() && self.browser.is_none() {
            return self.discover_keys();
        }
        let mut keys = vec![("1-9", "screens")];
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
                (
                    "i",
                    match self.picking {
                        Picking::BackupFolder => "choose the folder",
                        Picking::CheckoutFolder => "check out here",
                        Picking::DeskItem => "put on the desk",
                        Picking::Ingest => "add",
                    },
                ),
                (".", "hidden"),
                ("/", "type a path"),
                ("Esc", "cancel"),
            ];
        }
        // After the pop-ups that open on any screen, whose keys come first.
        if self.tab == Tab::Active {
            return self.active_keys();
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
            Tab::Settings => {
                keys.extend([("↑↓", "choose"), ("Enter", "change"), ("^O", "archives")]);
            }
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
            Tab::Browse => return self.browse_keys(),
            _ => keys.push(("↑↓", "scroll")),
        }
        keys.extend([("^R", "refresh"), ("?", "help"), ("q", "quit")]);
        keys
    }

    pub(super) fn move_by(&mut self, delta: i32) {
        if self.tab == Tab::Review {
            self.choose(delta);
        } else if self.tab == Tab::Active {
            self.active_move(delta);
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
        } else if self.tab == Tab::Librarian {
            self.talk_move(delta);
        } else if self.tab == Tab::Discover {
            self.discover_move(delta);
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
        let effect = self.key_here(key);
        self.settings_shown();
        self.screen_shown();
        effect
    }

    fn key_here(&mut self, key: KeyEvent) -> Option<Effect> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return None;
        }
        if self.correction.is_some() {
            if matches!(key.code, KeyCode::Char('y' | 'Y')) {
                return Some(Effect::Correction(Step::Save));
            }
            self.correction = None;
            self.message = Some("Correction discarded; nothing changed".into());
            return None;
        }
        if let Some(confirm) = self.confirm.take() {
            let effect = confirmed(confirm, key.code);
            if effect.is_none() {
                self.message = Some("Nothing changed".into());
            }
            return effect;
        }
        if self.help {
            self.help = false;
            return None;
        }
        if self.input.is_some() {
            return self.input_key(key);
        }
        if self.browser.is_some() {
            self.browser_key(key);
            return None;
        }
        if self.picker.is_some() {
            return self.picker_key(key);
        }
        self.message = None;
        if key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.archive_picker();
            return None;
        }
        let handled = match self.tab {
            Tab::Review => self.review_key(key),
            Tab::Active => self.active_key(key),
            Tab::Questions => self.questions_key(key),
            Tab::Browse => self.browse_key(key),
            Tab::Ingest => self.intake_key(key),
            Tab::Settings => self.settings_key(key),
            Tab::Backups => self.backups_key(key),
            Tab::Librarian => self.talk_key(key),
            Tab::Discover => self.discover_key(key),
            _ => Handled::No,
        };
        if let Handled::Yes(effect) = handled {
            return effect;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Providers may have come or gone too.
                if self.here.is_some() {
                    carrel_core::providers::device::forget_providers();
                    self.find_here();
                }
                self.message = Some(match self.reload() {
                    Ok(()) => "Refreshed".into(),
                    Err(Failure(e)) => e,
                });
            }
            KeyCode::Char('i') => return self.start_ingest(),
            KeyCode::Char(c) if Tab::ALL.iter().any(|t| t.key() == c) => {
                if let Some(tab) = Tab::ALL.iter().find(|t| t.key() == c) {
                    self.tab = *tab;
                }
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

    /// Handles a key while the built-in browser is open.
    fn browser_key(&mut self, key: KeyEvent) {
        let Some(browser) = self.browser.as_mut() else {
            return;
        };
        let outcome = browser.key(key);
        if outcome != super::browser::Outcome::Browsing {
            // Picking starts here next time, wherever the browser was left.
            let dir = browser.dir.clone();
            self.remember_folder(dir);
        }
        match outcome {
            super::browser::Outcome::Browsing => {}
            super::browser::Outcome::Add(paths) => {
                self.browser = None;
                self.picked(paths);
            }
            super::browser::Outcome::Type => {
                self.browser = None;
                match self.picking {
                    Picking::BackupFolder => {
                        self.picking = Picking::Ingest;
                        self.ask_for_backup_folder();
                    }
                    Picking::CheckoutFolder | Picking::DeskItem => self.ask_for_path(),
                    Picking::Ingest => self.ask_for_paths(),
                }
            }
            super::browser::Outcome::Cancel => {
                self.browser = None;
                self.picking = Picking::Ingest;
                self.checking_out = None;
            }
        }
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
        self.settings_shown();
        self.screen_shown();
    }

    /// Readies the screen shown, the first time it's shown and whenever it comes back: the
    /// preview follows the item the screen shows, and Discover draws its first view.
    pub(super) fn screen_shown(&mut self) {
        match self.tab {
            Tab::Browse => self.previews.want(&self.archive, &self.item),
            Tab::Discover => {
                if self.stacks.view == super::discover::View::Wander {
                    self.previews.want(&self.archive, &self.stacks.item);
                } else if self.stacks.lines.is_empty() {
                    self.load_discover();
                }
            }
            _ => {}
        }
    }
}

/// The work a confirmation's answer starts: `y` says yes, and `d` takes a desk item off the
/// desk with its card, and `t` returns a checkout and trims its session snapshots. Any other
/// key changes nothing.
fn confirmed(confirm: Confirm, key: KeyCode) -> Option<Effect> {
    if let (Confirm::DeskRemove { path, .. }, KeyCode::Char('d' | 'D')) = (&confirm, key) {
        return Some(Effect::DeskRemove {
            path: path.clone(),
            delete_card: true,
        });
    }
    if let (
        Confirm::Return {
            id,
            from,
            snapshots: true,
            ..
        },
        KeyCode::Char('t' | 'T'),
    ) = (&confirm, key)
    {
        return Some(Effect::Return(super::active::Returning {
            id: *id,
            from: from.clone(),
            trim: true,
        }));
    }
    if let (Confirm::Rule { id, answer, .. }, KeyCode::Char('n' | 'N')) = (&confirm, key) {
        return Some(Effect::Answer {
            id: *id,
            answer: answer.clone(),
            learn: false,
        });
    }
    if !matches!(key, KeyCode::Char('y' | 'Y')) {
        return None;
    }
    Some(match confirm {
        Confirm::Rule { id, answer, .. } => Effect::Answer {
            id,
            answer,
            learn: true,
        },
        Confirm::Undo { id, .. } => Effect::Undo(id),
        Confirm::Reject(ids) => Effect::Reject(ids),
        Confirm::Download { name, .. } => Effect::Download(name),
        Confirm::Agent(change) => Effect::Agent(change),
        Confirm::Return { id, from, .. } => Effect::Return(super::active::Returning {
            id,
            from,
            trim: false,
        }),
        Confirm::CancelCheckout { id, .. } => Effect::CancelCheckout(id),
        Confirm::Learn { about, fact } => Effect::Correction(Step::Learn { about, fact }),
        Confirm::Checkout { card, to, due, .. } => Effect::Checkout {
            item: card,
            to,
            due,
        },
        Confirm::DeskAdd(card) => Effect::DeskAdd(card),
        Confirm::DeskRemove { path, .. } => Effect::DeskRemove {
            path,
            delete_card: false,
        },
        Confirm::DeskArchive { path, .. } => Effect::DeskArchive(path),
        Confirm::NewCollection(new) => Effect::NewCollection(new),
        Confirm::Rename { card, to, .. } => Effect::Rename {
            item: card,
            new_name: to,
        },
        Confirm::Expand { card, anyway, .. } => Effect::Expand {
            item: card,
            anyway: anyway.is_some(),
        },
        Confirm::UndoLibrarian(_) => Effect::UndoLibrarian,
    })
}

/// The keys a confirmation answers to, for the help line.
fn confirm_keys(confirm: &Confirm) -> Vec<(&'static str, &'static str)> {
    match confirm {
        Confirm::DeskRemove { .. } => vec![
            ("y", "keep its card"),
            ("d", "delete its card"),
            ("n", "no"),
        ],
        Confirm::Return {
            snapshots: true, ..
        } => vec![("y", "yes"), ("t", "and trim snapshots"), ("n", "no")],
        _ => vec![("y", "yes"), ("n", "no")],
    }
}

/// `index` moved by `delta` within `len` items.
pub(super) fn step(index: usize, delta: i32, len: usize) -> usize {
    let next = i64::try_from(index).unwrap_or(0) + i64::from(delta);
    usize::try_from(next.max(0))
        .unwrap_or(0)
        .min(len.saturating_sub(1))
}
