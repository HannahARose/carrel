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
//!
//! `L` from Browse, Questions or Discover carries what's in view there, the item or
//! collection (or the search, or the Discover view), shown above the conversation, so "this"
//! means it; `x` asks about the whole archive again.

use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
use carrel_core::history::Entry;
use carrel_core::librarian::changes::{Change, Outcome, Suggestion};
use carrel_core::librarian::{About, Answer, Conversation, ItemLink, Librarian, journal};
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
    /// What the questions are about, with its title, when `L` brought it from another screen.
    pub about: Option<(About, String)>,
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

/// Asks `question` about `about` in `conversation` with `model` from `registry`, as
/// `carrel ask` does, letting the librarian make the changes it's sure of for `holder` when
/// `librarian.changes` is on. Returns the conversation with the answer added.
///
/// # Errors
///
/// Why the librarian couldn't answer, in words.
pub fn ask_with(
    root: &std::path::Path,
    (registry, model): (&Registry, AvailableModel),
    mut conversation: Conversation,
    (question, about): (&str, Option<&About>),
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
        .ask_about(&mut conversation, question, about)
        .map_err(|e| e.to_string())?;
    Ok(conversation)
}

/// Why the librarian on this device didn't answer.
#[derive(Debug)]
pub enum LibrarianUnanswered {
    /// It has no model to run on, and this is what would let it.
    NoModel(Box<carrel_core::librarian::model::NoModel>),
    /// Something else went wrong, in words.
    Failed(String),
}

/// Asks `question` about `about` on this device's librarian, in the background.
///
/// # Errors
///
/// [`LibrarianUnanswered`]: there's no model, or why it couldn't answer.
pub fn ask_here(
    root: &std::path::Path,
    conversation: Conversation,
    (question, about): (&str, Option<&About>),
    progress: &super::embedding::Shared,
) -> Result<Conversation, LibrarianUnanswered> {
    let (registry, _) = crate::providers::registry();
    let model = crate::ask::explain_the_model(&registry)
        .map_err(|Failure(e)| LibrarianUnanswered::Failed(e))?
        .map_err(LibrarianUnanswered::NoModel)?;
    let identity = crate::config::Identity::load_or_create()
        .map_err(|e| LibrarianUnanswered::Failed(e.to_string()))?;
    ask_with(
        root,
        (&registry, model),
        conversation,
        (question, about),
        identity.holder(),
        progress,
    )
    .map_err(LibrarianUnanswered::Failed)
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
    /// Shows the screen whose key is `c`, taking what's in view along to the Librarian
    /// screen ([`Self::carry_to_librarian`]).
    pub(super) fn switch_tab_carrying_view(&mut self, c: char) {
        if c == Tab::Librarian.key() {
            self.carry_to_librarian();
        }
        if let Some(tab) = Tab::ALL.iter().find(|t| t.key() == c) {
            self.tab = *tab;
        }
    }

    /// Takes what's in view to the Librarian screen, as `L` does from Browse, Questions or
    /// Discover: Browse's chosen item, or its collection while choosing among them, or the
    /// search it shows; the filed item the chosen question is about; or the item a wander is at, else the
    /// Discover view. From other screens, what the questions were about stays.
    pub(super) fn carry_to_librarian(&mut self) {
        let about = match self.tab {
            Tab::Browse if self.browse_pane == super::item::Pane::Collections => self
                .collections
                .get(self.browse_collection)
                .map(|c| About::Collection(c.path.clone())),
            Tab::Browse => match self.browse_items.get(self.browse_item) {
                Some((card, _)) => About::find(&self.archive, card).ok(),
                None => self.search.clone().map(About::Search),
            },
            Tab::Questions => self
                .asked
                .get(self.asked_chosen)
                .and_then(|q| q.about.first())
                .map(|id| About::Item(*id)),
            Tab::Discover => match &self.stacks.item.card {
                Some(card) if self.stacks.view == super::discover::View::Wander => {
                    About::find(&self.archive, card).ok()
                }
                _ => super::discover::View::ALL
                    .iter()
                    .find(|(view, ..)| *view == self.stacks.view)
                    .map(|(.., name)| About::Discover((*name).to_owned())),
            },
            _ => return,
        };
        self.talk.about = about.map(|about| {
            let title = about.title(&self.archive);
            (about, title)
        });
    }

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

    /// Says why the librarian has no model and what to do: the setting to clear, or, when
    /// there's no text model at all, the model that suits this device, asking to download
    /// it, as Settings does.
    pub(super) fn librarian_has_no_model(
        &mut self,
        missing: &carrel_core::librarian::model::NoModel,
    ) {
        match missing.download {
            Some(download) => {
                self.answered(Err(missing.to_string()));
                self.confirm_download(download.name);
            }
            None => self.answered(Err(crate::ask::what_to_do(missing))),
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
                    Some(question) => self.answer_moment(&question),
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
                self.talk = Talk {
                    about: self.talk.about.take(),
                    ..Talk::default()
                };
                self.message = Some("A new conversation".into());
                None
            }
            (KeyCode::Char('x'), _) if self.talk.about.is_some() => {
                self.talk.about = None;
                self.message = Some("Asking about the whole archive".into());
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
            (KeyCode::Char('o') | KeyCode::Right, _) if self.talk.item.card.is_some() => {
                let card = self.talk.item.card.clone().unwrap_or_default();
                self.tab = Tab::Browse;
                self.go_to(&card);
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
        if self.talk.about.is_some() {
            keys.push(("x", "about the whole archive"));
        }
        keys.extend([("s", "save"), ("n", "new"), ("?", "help")]);
        keys
    }
}

/// What asking to undo all of the librarian's changes this week, `count` of them, shows: its
/// title, the question, what saying yes does, and the word for yes.
pub fn confirm_text(count: usize) -> (&'static str, String, String, &'static str) {
    (
        " Undo ",
        format!(
            "Undo {} the librarian made this week?",
            crate::commands::plural(count, "change")
        ),
        "Each is reversed by a change of your own, newest first, as `carrel undo --librarian \
         7d` does. They stay in History."
            .to_owned(),
        " undo them   ",
    )
}

/// Does the Librarian screen's work: asking, in the background, and saving or undoing at once.
pub fn librarian_work(app: &mut App, effect: Effect) {
    match effect {
        Effect::Ask(_) if app.asking.busy() => {
            app.message = Some("The librarian is still answering".into());
        }
        Effect::Ask(question) => {
            let mut asking = std::mem::take(&mut app.asking);
            asking.start(app, question);
            app.asking = asking;
        }
        Effect::SaveConversation => {
            let result = save_here(app);
            super::finished(app, result);
        }
        Effect::UndoLibrarian => {
            let since = format!("{CHANGES_DAYS}d");
            let result = crate::ask::undo(Some(app.archive.root()), &since);
            super::finished(app, result.map(|r| r.text).map_err(|Failure(e)| e));
        }
        _ => {}
    }
}

/// The librarian answering in the background, for the run loop.
#[derive(Default)]
pub struct Asking {
    answer: Option<std::sync::mpsc::Receiver<Result<Conversation, LibrarianUnanswered>>>,
}

impl Asking {
    /// Whether the librarian is answering.
    pub fn busy(&self) -> bool {
        self.answer.is_some()
    }

    /// Asks `question` in the conversation on the Librarian screen, about what it shows the
    /// questions are about, in the background.
    pub fn start(&mut self, app: &mut App, question: String) {
        let root = app.archive.root().to_owned();
        let conversation = app.talk.conversation.clone();
        let about = app.talk.about.as_ref().map(|(about, _)| about.clone());
        let progress = app.progress.clone();
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(ask_here(
                &root,
                conversation,
                (&question, about.as_ref()),
                &progress,
            ));
        });
        app.busy = Some("Asking the librarian…".into());
        self.answer = Some(receive);
    }

    /// Shows the answer once it's come.
    pub fn poll(app: &mut App) {
        let mut asking = std::mem::take(&mut app.asking);
        asking.poll_into(app);
        app.asking = asking;
    }

    fn poll_into(&mut self, app: &mut App) {
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
        match result {
            Ok(conversation) => app.answered(Ok(conversation)),
            Err(LibrarianUnanswered::Failed(e)) => app.answered(Err(e)),
            Err(LibrarianUnanswered::NoModel(missing)) => app.librarian_has_no_model(&missing),
        }
    }
}

/// Saves the Librarian screen's conversation to the Journal, as the owner on this device.
pub fn save_here(app: &App) -> Result<String, String> {
    let identity = crate::config::Identity::load_or_create().map_err(|e| e.to_string())?;
    save(&app.archive, &app.talk.conversation, identity.holder())
}

#[cfg(test)]
pub(super) mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use carrel_core::card::Id;
    use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
    use carrel_core::librarian::Conversation;
    use carrel_core::providers::device::AvailableModel;
    use carrel_core::providers::{
        Capability, Completion, CompletionRequest, Offer, Provider, ProviderError, Registry,
    };
    use ratatui::crossterm::event::KeyCode;
    use serde_json::{Value, json};

    use super::super::app::{App, Confirm, Effect, Step, Tab};
    use super::super::screens::{Correcting, InputPurpose};
    use super::super::tests::{key, sample, screen};

    const PARTS_LIST: &str = "Projects/Hardware/parts-list.csv.md";

    /// A text model that gives scripted answers, one a request.
    struct Scripted {
        offers: Vec<Offer>,
        answers: Mutex<VecDeque<Value>>,
    }

    impl Provider for Scripted {
        fn name(&self) -> &'static str {
            "scripted"
        }
        fn offers(&self) -> &[Offer] {
            &self.offers
        }
        fn complete(&self, _: &str, _: &CompletionRequest) -> Result<Completion, ProviderError> {
            let answer = self
                .answers
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| ProviderError::Failed("out of answers".into()))?;
            Ok(Completion {
                text: answer.to_string(),
                json: Some(answer),
                model: "scripted/m".into(),
                usage: None,
            })
        }
    }

    /// A registry with a scripted model giving `answers`, and the model to ask.
    pub(in super::super) fn scripted(answers: Vec<Value>) -> (Registry, AvailableModel) {
        let mut extra = serde_json::Map::new();
        extra.insert("context_tokens".into(), json!(8_000));
        let model = Scripted {
            offers: vec![Offer {
                capability: Capability::TextModel,
                id: "m".into(),
                label: None,
                priority: 10,
                available: true,
                reason: None,
                media_types: Vec::new(),
                extra,
            }],
            answers: Mutex::new(answers.into()),
        };
        let mut registry = Registry::with_built_ins();
        registry.add(Box::new(model));
        (
            registry,
            AvailableModel {
                provider: "scripted".into(),
                offer: "m".into(),
                label: "scripted/m".into(),
            },
        )
    }

    fn tool(action: &str, input: &str) -> Value {
        json!({"action": action, "input": input, "answer": "", "items": [], "passages": [], "suggestions": []})
    }

    fn answer(text: &str, items: &[&str], confidence: f64) -> Value {
        json!({"action": "answer", "input": "", "answer": text, "items": items, "passages": [],
               "suggestions": [{"change": "refile", "item": PARTS_LIST, "field": "", "value": "Reading",
                                "why": "It's reference material.", "confidence": confidence}]})
    }

    fn holder() -> Holder {
        Holder {
            person: Id::new_v7(),
            device: Id::new_v7(),
        }
    }

    /// Asks `question` on the screen and answers it with `script`, as the run loop would.
    fn ask(app: &mut App, question: &str, script: Vec<Value>) {
        app.key(key(KeyCode::Enter));
        for c in question.chars() {
            app.key(key(KeyCode::Char(c)));
        }
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::Ask(question.to_owned()))
        );
        assert!(screen(app, 120, 30).contains("The librarian is looking…"));
        let (registry, model) = scripted(script);
        let result = super::ask_with(
            app.archive.root(),
            (&registry, model),
            app.talk.conversation.clone(),
            (question, app.talk.about.as_ref().map(|(about, _)| about)),
            holder(),
            &app.progress,
        );
        app.answered(result);
    }

    #[test]
    fn the_librarian_answers_beside_the_item_and_its_suggestions_go_through_the_flows() {
        let (_dir, mut app) = sample(true);
        app.key(key(KeyCode::Char('L')));
        assert_eq!(app.tab, Tab::Librarian);
        assert!(screen(&app, 160, 30).contains("9 Backups   L Librarian   D Discover"));
        assert!(screen(&app, 120, 30).contains("Ask the librarian about your archive"));
        ask(
            &mut app,
            "What about the lamp?",
            vec![
                tool("search", "lamp"),
                answer(
                    "Your desk lamp build has a parts list.",
                    &[PARTS_LIST],
                    0.95,
                ),
            ],
        );
        let text = screen(&app, 140, 34);
        assert!(text.contains("You: What about the lamp?"), "{text}");
        assert!(text.contains("Your desk lamp build has a parts list."));
        assert!(
            text.contains("(scripted/m · 1 step)"),
            "each answer names its model"
        );
        assert!(
            text.contains("Desk lamp parts list  Projects/Hardware"),
            "{text}"
        );
        assert!(
            text.contains("refile “Desk lamp parts list” to Reading (95% sure; y to make it)"),
            "with librarian.changes off, it's for the owner: {text}"
        );
        assert_eq!(
            app.talk.item.card.as_deref(),
            Some(PARTS_LIST),
            "the item it's discussing"
        );
        let first = app.talk.item.text.first().unwrap().trim().to_owned();
        assert!(
            text.contains(&first),
            "the card on the right, {first}: {text}"
        );

        // Making the suggestion is a correction, shown before it's saved.
        app.key(key(KeyCode::Down));
        assert!(app.keys().contains(&("y", "make it")));
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::Correction(Step::Work {
                target: Correcting::Item(PARTS_LIST.into()),
                words: "collection: Reading".into(),
                answering: None,
            }))
        );
        assert!(
            app.archive.root().join(PARTS_LIST).exists(),
            "nothing changed yet"
        );

        // Saving keeps the conversation in the Journal.
        assert_eq!(
            app.key(key(KeyCode::Char('s'))),
            Some(Effect::SaveConversation)
        );
        let saved = super::save(&app.archive, &app.talk.conversation, holder()).unwrap();
        assert!(
            saved.starts_with("Saved the conversation to Journal/"),
            "{saved}"
        );

        // `o` opens the item in Browse; `n` starts again.
        app.key(key(KeyCode::Char('L')));
        app.key(key(KeyCode::Char('o')));
        assert_eq!(app.tab, Tab::Browse);
        assert_eq!(app.item.card.as_deref(), Some(PARTS_LIST));
        app.key(key(KeyCode::Char('L')));
        app.key(key(KeyCode::Char('n')));
        assert_eq!(app.talk.conversation, Conversation::default());
    }

    #[test]
    fn l_carries_what_is_in_view_and_x_takes_it_away() {
        use carrel_core::librarian::About;
        let (_dir, mut app) = sample(true);

        // A collection, while choosing among them in Browse.
        app.tab = Tab::Browse;
        app.browse_pane = super::super::item::Pane::Collections;
        app.browse_collection = app
            .collections
            .iter()
            .position(|c| c.path == "Projects/Hardware")
            .unwrap();
        app.key(key(KeyCode::Char('L')));
        assert_eq!(app.tab, Tab::Librarian);
        assert_eq!(
            app.talk.about,
            Some((
                About::Collection("Projects/Hardware".into()),
                "Hardware".into()
            ))
        );
        assert!(screen(&app, 120, 30).contains("About: Hardware  (x for the whole archive)"));

        // The chosen item, which the question is then about.
        app.tab = Tab::Browse;
        app.go_to(PARTS_LIST);
        app.key(key(KeyCode::Char('L')));
        let parts = About::find(&app.archive, PARTS_LIST).unwrap();
        assert_eq!(app.talk.about.as_ref().map(|(a, _)| a), Some(&parts));
        assert!(screen(&app, 120, 30).contains("About: Desk lamp parts list"));
        let About::Item(id) = parts else {
            panic!("an item")
        };
        ask(
            &mut app,
            "Is this in the right place?",
            vec![
                tool("show", &id.to_string()),
                answer("Yes: Hardware is for builds.", &[PARTS_LIST], 0.2),
            ],
        );
        let answered = app.talk.conversation.answers.last().unwrap();
        assert_eq!(
            answered.about.as_deref(),
            Some("the item “Desk lamp parts list” (Projects/Hardware/parts-list.csv.md)")
        );
        assert!(
            answered.steps.is_empty(),
            "its card was loaded up front, so showing it again isn't a step"
        );

        // `n` keeps it; `x` takes it away; other screens leave it as it is.
        app.key(key(KeyCode::Char('n')));
        assert!(app.talk.about.is_some());
        assert!(app.keys().contains(&("x", "about the whole archive")));
        app.key(key(KeyCode::Char('x')));
        assert_eq!(app.talk.about, None);
        assert!(!screen(&app, 120, 30).contains("About:"));

        // Discover's view, when no item is in view there.
        app.key(key(KeyCode::Char('D')));
        app.key(key(KeyCode::Char('L')));
        assert_eq!(
            app.talk.about,
            Some((
                About::Discover("on this day".into()),
                "Discover: on this day".into()
            ))
        );
        app.key(key(KeyCode::Char('1')));
        app.key(key(KeyCode::Char('L')));
        assert!(app.talk.about.is_some(), "Status has nothing to carry");

        // The chosen question's item.
        let (card, question) = super::super::discover::tests::asked_item(&app);
        app.asked = vec![question];
        app.asked_chosen = 0;
        app.tab = Tab::Questions;
        app.key(key(KeyCode::Char('L')));
        assert_eq!(
            app.talk.about.as_ref().map(|(a, _)| a),
            Some(&About::find(&app.archive, &card).unwrap())
        );
    }

    #[test]
    fn with_changes_on_its_changes_are_listed_and_undone_one_or_all() {
        let (_dir, mut app) = sample(true);
        let lease = DiskLock::for_archive(&app.archive)
            .acquire(holder(), "test", DEFAULT_LEASE)
            .unwrap();
        carrel_core::settings::change(
            &app.archive,
            carrel_core::settings::Key::LibrarianChanges,
            "on",
            &lease,
        )
        .unwrap();
        lease.release().unwrap();
        app.reload().unwrap();
        app.key(key(KeyCode::Char('L')));
        ask(&mut app, "Tidy?", vec![answer("Done.", &[], 0.95)]);
        assert!(
            app.archive.root().join("Reading/parts-list.csv").exists(),
            "it made it"
        );
        assert_eq!(app.librarian_changes.len(), 1);
        let text = screen(&app, 140, 34);
        assert!(text.contains("made by the librarian"), "{text}");
        assert!(text.contains("The librarian's changes this week"), "{text}");
        // The change is the row after the suggestion.
        app.key(key(KeyCode::Down));
        assert!(app.keys().contains(&("u", "undo")));
        app.key(key(KeyCode::Char('u')));
        assert!(matches!(app.confirm, Some(Confirm::Undo { .. })));
        app.key(key(KeyCode::Char('n')));
        app.key(key(KeyCode::Char('U')));
        assert_eq!(app.confirm, Some(Confirm::UndoLibrarian(1)));
        assert!(screen(&app, 120, 30).contains("Undo 1 change the librarian made this week?"));
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::UndoLibrarian)
        );
    }

    #[test]
    fn the_librarian_asks_about_an_item_that_comes_up_and_the_answer_is_a_correction() {
        let (_dir, mut app) = sample(true);
        let (card, _) = super::super::discover::tests::asked_item(&app);
        app.key(key(KeyCode::Char('L')));
        ask(
            &mut app,
            "Anything?",
            vec![answer("This one.", &[&card], 0.5)],
        );
        let asked = app
            .talk
            .question
            .clone()
            .expect("a question about the item");
        assert!(screen(&app, 140, 34).contains("While we're here"));
        app.key(key(KeyCode::Char('a')));
        assert!(matches!(
            &app.input.as_ref().unwrap().purpose,
            InputPurpose::Answer { question, .. } if *question == asked.question
        ));
        for c in "It's from the lab".chars() {
            app.key(key(KeyCode::Char(c)));
        }
        let Some(Effect::Correction(Step::Work {
            target, answering, ..
        })) = app.key(key(KeyCode::Enter))
        else {
            panic!("the answer goes through the corrections path");
        };
        let id = super::super::asking::item_at(&app.archive, &card)
            .unwrap()
            .id;
        assert_eq!(target, Correcting::Item(id.to_string()));
        assert!(
            answering.is_some(),
            "the question leaves the queue once it's saved"
        );

        // Not after the next answer too.
        ask(
            &mut app,
            "More?",
            vec![answer("Still this one.", &[&card], 0.5)],
        );
        assert!(app.talk.question.is_none());
    }

    #[test]
    fn with_no_model_the_librarian_says_what_to_do_and_asks_to_download_one() {
        use carrel_core::librarian::model::NoModel;
        let (_dir, mut app) = sample(false);
        app.key(key(KeyCode::Char('L')));
        let qwen = carrel_core::models::find("qwen3.5-4b").unwrap();
        app.librarian_has_no_model(&NoModel {
            gone: None,
            available: Vec::new(),
            instead: None,
            download: Some(qwen),
            downloaded: Vec::new(),
            memory_gb: Some(8),
        });
        let message = app.message.clone().unwrap();
        assert!(message.contains("needs a text model"), "{message}");
        assert!(message.contains("(qwen3.5-4b, 2.7 GB)"), "{message}");
        assert!(
            matches!(&app.confirm, Some(Confirm::Download { name, .. }) if name == "qwen3.5-4b")
        );
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::Download("qwen3.5-4b".into()))
        );
    }

    #[test]
    fn a_librarian_model_thats_gone_is_named_with_those_installed() {
        use carrel_core::librarian::model::NoModel;
        let (_dir, mut app) = sample(false);
        let installed = AvailableModel {
            provider: "llama".into(),
            offer: "gemma-4-e4b".into(),
            label: "llama/Gemma 4 E4B".into(),
        };
        app.librarian_has_no_model(&NoModel {
            gone: Some("llama/gone-7b".into()),
            available: vec![installed.clone()],
            instead: Some(installed),
            download: None,
            downloaded: Vec::new(),
            memory_gb: Some(16),
        });
        let message = app.message.clone().unwrap();
        assert!(
            message.contains("librarian.model names llama/gone-7b"),
            "{message}"
        );
        assert!(message.contains("runs on llama/Gemma 4 E4B"), "{message}");
        assert!(
            message.contains("`carrel settings set librarian.model llama/gemma-4-e4b`"),
            "{message}"
        );
        assert!(app.confirm.is_none());
    }
}
