//! The interactive mode (`docs/design/interactive.md`): `carrel` in a terminal, or
//! `carrel tui`. Everything it does is also a command.

mod actions;
mod active;
mod agent;
mod app;
mod archives;
mod asking;
mod backups;
mod browser;
mod discover;
mod embedding;
mod intake;
mod item;
mod librarian;
mod option_lines;
mod preview;
mod review;
mod review_crawler;
mod review_restructure;
mod screens;
mod search;
mod settings;
mod ui;
mod ui_discovery;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
};
use ratatui::crossterm::execute;
use serde_json::Value;

use crate::commands::locate;
use crate::{Failure, Report};

use crate::sessions::Decision;
pub use app::App;
use app::{Confirm, Effect, Step};
use carrel_core::actions::Kept;
use carrel_core::archive::Archive;
use carrel_core::card::Id;

/// Work run in the background, with what it reports.
type Work = Box<dyn FnOnce() -> Result<Report, Failure> + Send>;

/// `text`'s lines, as a report screen shows them.
fn lines_of(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

/// Opens the interactive mode on the archive commands would use, and returns when the owner
/// quits. The terminal is restored however it ends.
pub fn run(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    // Noted among the recent archives, so the switcher can come back to it.
    if let Ok(root) = std::fs::canonicalize(archive.root()) {
        let mut prefs = crate::config::Prefs::load()?;
        prefs.note_archive(&root);
        prefs.save()?;
    }
    let mut app = App::new(archive)?;
    app.previews = preview::Previews::detected();
    // The item shown first gets its preview too.
    app.load_card();
    let theme = ui::Theme::from_env();
    let mut terminal = enter();
    let result = event_loop(&mut terminal, &mut app, theme);
    leave();
    result.map(|()| Report::new(String::new(), Value::Null))
}

/// Takes over the terminal. `ratatui::init` enters raw mode and the alternate screen, and
/// restores both on a panic.
fn enter() -> ratatui::DefaultTerminal {
    let terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture);
    terminal
}

/// Gives the terminal back as it was.
fn leave() {
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
}

/// Background work's outcome, as a line for the owner.
type Done = Receiver<Result<String, String>>;

/// Shows what looking for work sessions found, and reloads when it wrote something.
fn recorded_sessions(app: &mut App, result: Option<Result<String, Failure>>) {
    match result {
        Some(Ok(text)) if !text.is_empty() => {
            let message = text.lines().collect::<Vec<_>>().join("; ");
            if let Err(Failure(e)) = app.reload() {
                app.message = Some(e);
            } else {
                app.message = Some(message);
            }
        }
        Some(Err(Failure(e))) => app.message = Some(format!("Work sessions weren't recorded: {e}")),
        _ => {}
    }
}

/// Work sessions, looked for in a background thread about once a minute; `record_if_due`
/// keeps the looking itself to every few minutes.
#[derive(Default)]
struct SessionLook {
    looking: Option<Receiver<Option<Result<String, Failure>>>>,
    last: Option<std::time::Instant>,
}

impl SessionLook {
    /// Starts a look when one is due, and shows what a finished one found.
    fn tick(&mut self, app: &mut App) {
        if self.looking.is_none()
            && self
                .last
                .is_none_or(|t| t.elapsed() >= Duration::from_mins(1))
        {
            self.last = Some(std::time::Instant::now());
            let (send, receive) = std::sync::mpsc::channel();
            let root = app.archive.root().to_owned();
            std::thread::spawn(move || {
                let _ = send.send(crate::sessions::record_if_due(Some(&root)));
            });
            self.looking = Some(receive);
        }
        if let Some(done) = &self.looking
            && let Ok(result) = done.try_recv()
        {
            self.looking = None;
            recorded_sessions(app, result);
        }
    }
}

/// Takes the terminal back after a program that needed it to itself, such as the editor,
/// which leaves no picture behind.
fn reenter(terminal: &mut ratatui::DefaultTerminal, painter: &mut preview::Painter) {
    *terminal = enter();
    *painter = preview::Painter::default();
}

/// Draws the screen, then the chosen item's picture, if it has one.
fn draw(
    terminal: &mut ratatui::DefaultTerminal,
    app: &App,
    theme: ui::Theme,
    painter: &mut preview::Painter,
) -> Result<(), Failure> {
    terminal
        .draw(|frame| ui::draw(frame, app, theme))
        .map_err(|e| Failure::new(format!("couldn't draw: {e}")))?;
    if painter.paint(app) {
        // A picture was taken away: everything is drawn again, and the next one put in.
        let _ = terminal.clear();
        terminal
            .draw(|frame| ui::draw(frame, app, theme))
            .map_err(|e| Failure::new(format!("couldn't draw: {e}")))?;
        painter.paint(app);
    }
    Ok(())
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    theme: ui::Theme,
) -> Result<(), Failure> {
    let starts = ui::tab_starts();
    let mut running: Option<Done> = None;
    let mut preparing: Option<Preparing> = None;
    let mut checked = std::time::Instant::now();
    let mut sessions = SessionLook::default();
    let mut painter = preview::Painter::default();
    while !app.quit {
        librarian::Asking::poll(app);
        sessions.tick(app);
        app.previews.poll();
        app.poll_here();
        // The worker runs in another process; see what it has done about once a second.
        if checked.elapsed() >= Duration::from_secs(1) {
            checked = std::time::Instant::now();
            if app.poll_worker() {
                let message = app.message.take();
                if let Err(Failure(e)) = app.reload() {
                    app.message = Some(e);
                } else {
                    app.message = message;
                }
            }
        }
        if let Some(done) = &preparing
            && let Ok(result) = done.try_recv()
        {
            preparing = None;
            prepared(app, result);
        }
        if let Some(done) = &running
            && let Ok(result) = done.try_recv()
        {
            running = None;
            app.busy = None;
            if let Ok(mut progress) = app.progress.lock() {
                *progress = None;
            }
            finished(app, result);
            // After a download, Settings finds what the providers offer again.
            app.settings_shown();
        }
        draw(terminal, app, theme, &mut painter)?;
        if !event::poll(Duration::from_millis(100))
            .map_err(|e| Failure::new(format!("couldn't read the terminal: {e}")))?
        {
            continue;
        }
        let effect = match event::read()
            .map_err(|e| Failure::new(format!("couldn't read the terminal: {e}")))?
        {
            Event::Key(key) if key.kind == KeyEventKind::Press => app.key(key),
            Event::Mouse(mouse) => {
                app.mouse(mouse, &starts);
                None
            }
            _ => None,
        };
        match effect {
            Some(Effect::Edit(id)) => {
                // The editor needs the terminal to itself.
                leave();
                let result = crate::review::edit(Some(app.archive.root()), &id.to_string());
                reenter(terminal, &mut painter);
                finished(app, result.map(|r| r.text).map_err(|Failure(e)| e));
            }
            Some(Effect::Open(path)) => {
                // Opening needs no lease, so it doesn't wait for other work.
                app.message = Some(match open(&path) {
                    Ok(()) => format!("Opened {}", path.display()),
                    Err(Failure(e)) => e,
                });
            }
            Some(Effect::Correction(step)) if preparing.is_none() => {
                correction(app, step, &mut preparing);
            }
            Some(Effect::EditItem { card, body }) => {
                // The editor needs the terminal to itself.
                leave();
                let result = crate::edit::edit(Some(app.archive.root()), &card, body);
                reenter(terminal, &mut painter);
                changed(app.archive.root(), &result);
                finished(app, result.map(|r| r.text).map_err(|Failure(e)| e));
            }
            Some(Effect::EditEntry { item, number }) => {
                // The editor needs the terminal to itself.
                leave();
                let result = crate::sessions::edit(Some(app.archive.root()), &item, number);
                reenter(terminal, &mut painter);
                finished(app, result.map(|r| r.text).map_err(|Failure(e)| e));
            }
            Some(Effect::Pick(words)) => {
                // The picker needs the terminal to itself, as the editor does.
                leave();
                let picked = crate::picker::run(&words, &app.start_folder);
                reenter(terminal, &mut painter);
                picked_files(app, picked);
            }
            Some(effect) if running.is_none() => {
                if let Some(work) = background(app, effect) {
                    running = Some(work);
                }
            }
            Some(_) => app.message = Some("Still busy; try again when it's done".into()),
            None => {}
        }
    }
    Ok(())
}

/// Skips or dismisses a question, as `effect` says.
fn settle_question(root: &Path, effect: &Effect) -> Result<String, String> {
    let result = match effect {
        Effect::Skip(id) => crate::questions::skip(Some(root), &id.to_string()),
        Effect::Dismiss(id) => crate::questions::dismiss(Some(root), &id.to_string()),
        _ => return Ok(String::new()),
    };
    result.map(|r| r.text).map_err(|Failure(e)| e)
}

/// Answering a question, in the background: a term's answer queues its items to be drafted
/// again by the worker, and a sender's adds a routing rule if the owner said yes (`learn`).
fn answer_work(root: PathBuf, id: Id, answer: String, learn: bool) -> (String, Work) {
    let id = id.to_string();
    let options = crate::questions::Options {
        learn,
        ..crate::questions::Options::default()
    };
    (
        "Answering…".into(),
        Box::new(move || crate::questions::answer(Some(&root), &id, &[answer], options)),
    )
}

/// Shows how work went and reads everything again.
fn finished(app: &mut App, result: Result<String, String>) {
    let text = match result {
        Ok(text) | Err(text) => text,
    };
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    app.message = Some(match lines.as_slice() {
        [] => "Done".into(),
        [one] => (*one).to_owned(),
        [first, rest @ ..] => format!("{first} (+{} more)", rest.len()),
    });
    app.marked.clear();
    app.active_marked.clear();
    if let Err(Failure(message)) = app.reload() {
        app.message = Some(message);
    }
}

/// A correction being worked out in the background.
type Preparing = Receiver<Result<crate::correct::Prepared, String>>;

/// Handles a correction's steps: working it out in the background, saving it from the
/// pop-up, and adding the fact it stated to the glossary.
fn correction(app: &mut App, step: Step, preparing: &mut Option<Preparing>) {
    match step {
        Step::Work {
            target,
            words,
            answering,
        } => {
            // A model takes seconds, so it works in the background; the pop-up follows.
            let root = app.archive.root().to_owned();
            let (send, receive) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let result = Archive::open(&root)
                    .map_err(|e| Failure::new(e.to_string()))
                    .and_then(|archive| match &target {
                        screens::Correcting::Proposal(id) => {
                            crate::correct::prepare_review(&archive, &id.to_string(), &words)
                        }
                        screens::Correcting::Item(card) => {
                            crate::correct::prepare_filed(&archive, card, &words)
                        }
                    })
                    .map(|mut prepared| {
                        prepared.answering = answering;
                        prepared
                    });
                let _ = send.send(result.map_err(|Failure(e)| e));
            });
            app.busy = Some("Working out the correction…".into());
            *preparing = Some(receive);
        }
        Step::Save => {
            if let Some(prepared) = app.correction.take() {
                let fact = prepared.fact.clone();
                let result = crate::correct::save(&app.archive, prepared);
                let saved = result.is_ok();
                if saved {
                    crate::worker::after_change(Some(app.archive.root()));
                }
                finished(app, result.map(|s| s.message).map_err(|Failure(e)| e));
                if let (true, Some(fact)) = (saved, fact) {
                    app.confirm = Some(Confirm::Learn {
                        about: fact.about,
                        fact: fact.fact,
                    });
                }
            }
        }
        Step::Learn { about, fact } => {
            let fact = carrel_core::correct::Fact { about, fact };
            finished(
                app,
                crate::correct::learn(&app.archive, &fact).map_err(|Failure(e)| e),
            );
        }
    }
}

/// Shows a correction once it's worked out: the pop-up, or why there's none.
fn prepared(app: &mut App, result: Result<crate::correct::Prepared, String>) {
    app.busy = None;
    match result {
        Ok(prepared) if prepared.is_empty() => {
            app.message = Some("That changes nothing".into());
        }
        Ok(prepared) => app.correction = Some(prepared),
        Err(e) => app.message = Some(e),
    }
}

/// Starts work that takes the writer lease, or does it at once when it's quick. The
/// commands do the work, so the interactive mode reports exactly what they report.
/// Changes proposals without filing anything: their destination, or whether a compressed file
/// is expanded.
fn change_proposals(root: &Path, effect: Effect) -> Result<String, String> {
    match effect {
        Effect::Compressed {
            ids: chosen,
            choice,
        } => crate::review::choose_compressed(Some(root), &ids(&chosen), choice)
            .map(|r| r.text)
            .map_err(|Failure(e)| e),
        Effect::Move { ids: chosen, to } => {
            let mut text = String::new();
            for id in ids(&chosen) {
                match crate::review::move_to(Some(root), &id, &to) {
                    Ok(report) => text.push_str(&report.text),
                    Err(Failure(e)) => writeln!(text, "{e}").unwrap(),
                }
            }
            Ok(text)
        }
        _ => Ok(String::new()),
    }
}

/// The identifiers `ids`, as the commands take them.
fn ids(ids: &[carrel_core::card::Id]) -> Vec<String> {
    ids.iter().map(ToString::to_string).collect()
}

fn background(app: &mut App, effect: Effect) -> Option<Done> {
    let root: PathBuf = app.archive.root().to_owned();
    let (busy, work): (String, Work) = match effect {
        Effect::Undo(id) => {
            let result = crate::commands::undo(Some(&root), &id);
            changed(&root, &result);
            finished(app, result.map(|r| r.text).map_err(|Failure(e)| e));
            return None;
        }
        Effect::Reject(chosen) => {
            let result = crate::review::reject(Some(&root), &ids(&chosen));
            finished(app, result.map(|r| r.text).map_err(|Failure(e)| e));
            return None;
        }
        effect @ (Effect::Compressed { .. } | Effect::Move { .. }) => {
            finished(app, change_proposals(&root, effect));
            return None;
        }
        // The run loop does these itself, on the terminal or in the pop-up.
        Effect::Edit(_)
        | Effect::EditItem { .. }
        | Effect::EditEntry { .. }
        | Effect::Pick(_)
        | Effect::Correction(_)
        | Effect::Open(_) => return None,
        effect @ (Effect::Ask(_) | Effect::SaveConversation | Effect::UndoLibrarian) => {
            librarian::librarian_work(app, effect);
            return None;
        }
        effect @ (Effect::Dismiss(_) | Effect::Skip(_)) => {
            finished(app, settle_question(&root, &effect));
            return None;
        }
        Effect::Agent(change) => {
            let result = app.change_agent(&change);
            finished(app, result);
            return None;
        }
        Effect::SetSetting { key, value } => {
            let result = crate::settings::set(Some(&root), &key, &value);
            finished(app, result.map(|r| r.text).map_err(|Failure(e)| e));
            return None;
        }
        effect @ (Effect::Return(_) | Effect::CancelCheckout(_) | Effect::Entries { .. }) => {
            active_work(root, effect)
        }
        effect @ (Effect::Download(_)
        | Effect::Restructure(_)
        | Effect::Index(_)
        | Effect::OpenArchive(_)
        | Effect::CreateArchive(_)
        | Effect::StartHistory(_)) => switch_or_index_work(app, root, effect)?,
        Effect::QuestionsAbout(card) => option_lines::questions_about_work(root, card),
        Effect::Queue { paths, options } => {
            let result = queue(&root, &paths, options);
            finished(app, result.map_err(|Failure(e)| e));
            return None;
        }
        Effect::Unqueue(id) | Effect::Front(id) | Effect::Retry(id) => {
            let change = match effect {
                Effect::Unqueue(_) => crate::ingest::Change::Remove,
                Effect::Front(_) => crate::ingest::Change::First,
                _ => crate::ingest::Change::Retry,
            };
            let result = crate::ingest::change_queued(Some(&root), &id.to_string(), change);
            finished(app, result.map(|r| r.text).map_err(|Failure(e)| e));
            return None;
        }
        Effect::Answer { id, answer, learn } => answer_work(root, id, answer, learn),
        Effect::Decide {
            ids: chosen,
            holding,
        } => decide(root, ids(&chosen), holding),
        effect @ (Effect::BackupNow
        | Effect::BackupVerify(_)
        | Effect::BackupRestore { .. }
        | Effect::BackupSetup { .. }) => backup_work(root, effect),
        Effect::ApproveAllHigh => (
            "Filing every high-confidence proposal…".into(),
            Box::new(move || {
                let result = crate::review::approve(Some(&root), &[], true, None, None);
                changed(&root, &result);
                result
            }),
        ),
        Effect::Keep { id, kept, target } => keep(root, id, kept, target),
        effect @ (Effect::Checkout { .. }
        | Effect::DeskAdd(_)
        | Effect::DeskRemove { .. }
        | Effect::DeskArchive(_)
        | Effect::NewCollection(_)
        | Effect::Rename { .. }
        | Effect::Expand { .. }) => archive_work(root, effect),
    };
    app.busy = Some(busy);
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = work().map(|r| r.text).map_err(|Failure(e)| e);
        let _ = send.send(result);
    });
    Some(receive)
}

/// The work for restructuring and the crawler, downloading a model, the index and switching
/// archives: all in the background but switching, which happens at once, since the app is
/// replaced.
fn switch_or_index_work(app: &mut App, root: PathBuf, effect: Effect) -> Option<(String, Work)> {
    match effect {
        Effect::Restructure(action) => Some(review_restructure::restructure_work(root, action)),
        Effect::Download(name) => {
            // What the providers offer changes once it's here.
            app.here = None;
            Some((
                format!("Downloading {name}…"),
                Box::new(move || crate::models::download_quietly(&name)),
            ))
        }
        Effect::Index(work) => Some(embedding::index_work(
            root,
            work,
            app.progress.clone(),
            Box::new(|| crate::providers::registry().0),
            Box::new(|| {
                Ok(crate::commands::holder(
                    &crate::config::Identity::load_or_create()?,
                ))
            }),
        )),
        Effect::OpenArchive(path) => {
            let result = app.switch_archive(&path);
            app.message = Some(result.unwrap_or_else(|Failure(e)| e));
            None
        }
        Effect::CreateArchive(new) => {
            let result = app.create_archive(&new);
            app.message = Some(result.unwrap_or_else(|Failure(e)| e));
            None
        }
        Effect::StartHistory(path) => {
            let result = app.start_history(&path);
            app.message = Some(result.unwrap_or_else(|Failure(e)| e));
            None
        }
        _ => None,
    }
}

/// Plans what the picker chose; if the picker couldn't run, says why and opens the built-in
/// browser instead. Returns the folder picked from, to start in next time.
fn picked_files(app: &mut App, picked: Result<Vec<PathBuf>, Failure>) {
    match picked {
        Ok(paths) if paths.is_empty() => {
            app.message = Some("Nothing chosen".into());
            app.picking = app::Picking::Ingest;
            app.checking_out = None;
        }
        Ok(paths) => {
            if let Some(folder) = paths[0].parent().filter(|f| f.is_dir()) {
                app.remember_folder(folder.to_owned());
            }
            app.picked(paths);
        }
        Err(Failure(e)) => {
            app.message = Some(format!("{e}; using the built-in browser"));
            app.open_browser();
        }
    }
}

/// Adds `paths` to the intake queue and starts a worker, saying what happened in a line.
fn queue(
    root: &Path,
    paths: &[PathBuf],
    options: carrel_core::ingest::IntakeOptions,
) -> Result<String, Failure> {
    let archive = carrel_core::archive::Archive::open(root)?;
    let identity = crate::config::Identity::load_or_create()?;
    let queued = carrel_core::intake::add(&archive, paths, options, identity.device)?;
    let started = crate::worker::start(&archive)?;
    let n = queued.added.len();
    Ok(match (n, started || crate::worker::running(&archive)?) {
        (0, _) => "Nothing new to queue".into(),
        (n, true) => format!(
            "Queued {n} item{}; the worker drafts them in the background",
            if n == 1 { "" } else { "s" }
        ),
        (n, false) => format!(
            "Queued {n} item{}; no worker started (CARREL_WORKER is off)",
            if n == 1 { "" } else { "s" }
        ),
    })
}

/// The work for the Backups screen's effects, which `carrel backup` does.
fn backup_work(root: PathBuf, effect: Effect) -> (String, Work) {
    match effect {
        Effect::BackupNow => (
            "Taking a snapshot…".into(),
            Box::new(move || crate::backup::now(Some(&root))),
        ),
        Effect::BackupVerify(percent) => (
            "Checking the backups…".into(),
            Box::new(move || crate::backup::verify(Some(&root), percent)),
        ),
        Effect::BackupRestore { item, at, to } => (
            format!("Restoring {item}…"),
            Box::new(move || crate::backup::restore(Some(&root), &item, Some(&at), to.as_deref())),
        ),
        Effect::BackupSetup {
            repository,
            password,
            description,
        } => (
            "Setting up backups…".into(),
            Box::new(move || {
                crate::backup::setup(
                    Some(&root),
                    &repository,
                    description.as_deref(),
                    Some(&password.0),
                )
            }),
        ),
        _ => unreachable!("only backup effects come here"),
    }
}

/// The work for the Active screen's effects, which `carrel return` and `carrel sessions` do.
fn active_work(root: PathBuf, effect: Effect) -> (String, Work) {
    match effect {
        Effect::Return(returning) => (
            "Returning it…".into(),
            Box::new(move || {
                crate::checkout::return_now(
                    Some(&root),
                    &returning.id.to_string(),
                    returning.from.as_deref(),
                    returning.trim,
                )
            }),
        ),
        Effect::CancelCheckout(id) => (
            "Cancelling the checkout…".into(),
            Box::new(move || crate::checkout::cancel_now(Some(&root), &id.to_string())),
        ),
        Effect::Entries {
            item,
            numbers,
            decision,
        } => (
            match decision {
                Decision::Approve => "Approving…",
                Decision::Combine => "Combining…",
                Decision::Drop => "Dropping…",
            }
            .into(),
            Box::new(move || crate::sessions::decide(Some(&root), &item, &numbers, decision)),
        ),
        _ => unreachable!("only the Active screen's effects come here"),
    }
}

/// Work on the archive at the path it's given.
type ArchiveWork = Box<dyn FnOnce(&Path) -> Result<Report, Failure> + Send>;

/// The work for checking out, the desk, and Browse's actions, which `carrel checkout`,
/// `carrel desk`, `carrel collection new`, `carrel rename` and `carrel expand` do. Each changes
/// the archive, so a worker brings the search index up to date after it.
fn archive_work(root: PathBuf, effect: Effect) -> (String, Work) {
    let (busy, work): (&str, ArchiveWork) = match effect {
        Effect::Checkout { item, to, due } => (
            "Checking it out…",
            Box::new(move |root| {
                crate::checkout::checkout(Some(root), &item, Some(&to), due.as_deref())
            }),
        ),
        Effect::DeskAdd(card) => (
            if card.title.is_some() && card.summary.is_some() {
                "Putting it on the desk…"
            } else {
                "Putting it on the desk and drafting its card…"
            },
            Box::new(move |root| {
                crate::desk::add_item(
                    Some(root),
                    &card.path,
                    card.title.as_deref(),
                    card.summary.as_deref(),
                )
            }),
        ),
        Effect::DeskRemove { path, delete_card } => (
            "Taking it off the desk…",
            Box::new(move |root| crate::desk::remove_item(Some(root), &path, delete_card)),
        ),
        Effect::DeskArchive(path) => (
            "Queueing it for the archive…",
            Box::new(move |root| crate::desk::archive_desk_item(Some(root), &path)),
        ),
        Effect::NewCollection(new) => (
            "Creating the collection…",
            Box::new(move |root| {
                crate::commands::collection_new(Some(root), &new.path, new.description, &new.tags)
            }),
        ),
        Effect::Rename { item, new_name } => (
            "Renaming it…",
            Box::new(move |root| crate::commands::rename(Some(root), &item, Some(&new_name))),
        ),
        Effect::Expand { item, anyway } => (
            "Expanding it…",
            Box::new(move |root| crate::expand::expand(Some(root), &item, anyway)),
        ),
        _ => unreachable!("only the archive's actions come here"),
    };
    (
        busy.into(),
        Box::new(move || {
            let result = work(&root);
            changed(&root, &result);
            result
        }),
    )
}

/// Opens `path` in the system's own app for it (Finder, a file manager, or what the file's
/// type opens with), leaving the interactive mode where it is.
fn open(path: &Path) -> Result<(), Failure> {
    if !path.exists() {
        return Err(Failure::new(format!("{} isn't there", path.display())));
    }
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|mut child| {
            // Waited for elsewhere, so it doesn't linger once it's done.
            std::thread::spawn(move || child.wait());
        })
        .map_err(|e| Failure::new(format!("couldn't run {program}: {e}")))
}

/// The work for [`Effect::Decide`]: filing proposals where proposed, or in Holding.
fn decide(root: PathBuf, chosen: Vec<String>, holding: bool) -> (String, Work) {
    let count = chosen.len();
    let busy = format!(
        "{} {count} proposal{}…",
        if holding { "Holding" } else { "Filing" },
        if count == 1 { "" } else { "s" }
    );
    let work: Work = if holding {
        Box::new(move || crate::review::hold(Some(&root), &chosen))
    } else {
        Box::new(move || {
            let result = crate::review::approve(Some(&root), &chosen, false, None, None);
            changed(&root, &result);
            result
        })
    };
    (busy, work)
}

/// Starts a worker to embed what changed for search, when `result` says filed items changed
/// in the archive at `root`.
fn changed<T>(root: &Path, result: &Result<T, Failure>) {
    if result.is_ok() {
        crate::worker::after_change(Some(root));
    }
}

/// The work for [`Effect::Keep`]: filing a proposal linked to the archived item it duplicates.
fn keep(root: PathBuf, id: Id, kept: Kept, target: Id) -> (String, Work) {
    let busy = match kept {
        Kept::AsVersion => "Filing it as a new version…",
        Kept::Both => "Filing it beside the archived item…",
    };
    let target = target.to_string();
    let work: Work = Box::new(move || {
        let (version, related) = match kept {
            Kept::AsVersion => (Some(target.as_str()), None),
            Kept::Both => (None, Some(target.as_str())),
        };
        let result =
            crate::review::approve(Some(&root), &[id.to_string()], false, version, related);
        changed(&root, &result);
        result
    });
    (busy.into(), work)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use carrel_core::archive::Archive;
    use carrel_core::card::Id;
    use carrel_core::coordinator::{Coordinator, DEFAULT_LEASE, DiskLock, Holder};
    use carrel_core::history::History;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };

    use super::app::{App, Confirm, Effect, Tab};
    use super::item::Pane;
    use super::ui;

    pub(super) fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    pub(super) fn sample(with_history: bool) -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("archive");
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
            &root,
        );
        let archive = Archive::open(&root).unwrap();
        if with_history {
            let holder = Holder {
                person: Id::new_v7(),
                device: Id::new_v7(),
            };
            let lease = DiskLock::for_archive(&archive)
                .acquire(holder, "test", DEFAULT_LEASE)
                .unwrap();
            History::start(&archive, &lease).unwrap();
            lease.release().unwrap();
        }
        (dir, App::new(archive).unwrap())
    }

    pub(super) fn screen(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| ui::draw(frame, app, ui::Theme::plain()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(super) fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn status_shows_the_archive_what_waits_and_the_keys() {
        let (_dir, app) = sample(false);
        let text = screen(&app, 100, 30);
        assert!(
            text.starts_with(
                "  1 Status   2 Review   3 Active   4 Questions   5 Browse   6 History"
            ),
            "{text}"
        );
        assert!(text.contains(" Sample archive "), "{text}");
        assert!(
            text.contains("Waiting for you: 1 proposal (2 to review), 1 question (3 to answer)."),
            "{text}"
        );
        assert!(text.contains("items in"), "the status report");
        assert!(
            text.contains("Recent:") && text.contains("Worked on “oscillator-sim” for 45m"),
            "what was worked on lately: {text}"
        );
        let help = text.lines().last().unwrap();
        assert!(help.contains("? help  q quit"), "{help}");
    }

    #[test]
    fn a_small_terminal_says_how_big_it_needs_to_be() {
        let (_dir, app) = sample(false);
        let text = screen(&app, 60, 20);
        let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            joined.contains("at least 80×24; this one is 60×20."),
            "{text}"
        );
    }

    #[test]
    fn keys_and_clicks_switch_screens_help_and_quit() {
        let (_dir, mut app) = sample(false);
        app.key(key(KeyCode::Char('4')));
        assert_eq!(app.tab, Tab::Questions);
        app.key(key(KeyCode::Tab));
        assert_eq!(app.tab, Tab::Browse);
        let browse = screen(&app, 100, 30);
        assert!(
            browse.contains("│  Hardware"),
            "the tree, indented:\n{browse}"
        );
        app.key(key(KeyCode::BackTab));
        app.key(key(KeyCode::BackTab));
        assert_eq!(app.tab, Tab::Active);
        app.key(key(KeyCode::BackTab));
        assert_eq!(app.tab, Tab::Review);
        assert!(screen(&app, 100, 30).contains("Desk lamp diffuser (print file)"));

        let starts = ui::tab_starts();
        let click = |column| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        app.mouse(click(starts[5] + 2), &starts);
        assert_eq!(app.tab, Tab::History);
        app.mouse(click(starts[0]), &starts);
        assert_eq!(app.tab, Tab::Status);

        app.key(key(KeyCode::Char('?')));
        assert!(screen(&app, 100, 30).contains("Keys (any key closes)"));
        app.key(key(KeyCode::Char('x')));
        assert!(!app.help);
        assert!(!app.quit, "closing help doesn't quit");
        app.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.quit);
    }

    #[test]
    fn undo_from_history_asks_first() {
        let (_dir, mut app) = sample(true);
        app.key(key(KeyCode::Char('6')));
        let text = screen(&app, 100, 30);
        assert!(text.contains("Start history of “Sample archive”"), "{text}");
        app.key(key(KeyCode::Char('u')));
        assert!(matches!(app.confirm, Some(Confirm::Undo { .. })));
        assert!(screen(&app, 100, 30).contains("Undo “Start history of “Sample archive””?"));
        assert_eq!(app.key(key(KeyCode::Char('n'))), None);
        assert_eq!(app.message.as_deref(), Some("Nothing changed"));
        app.key(key(KeyCode::Char('u')));
        let id = app.history[0].id.clone();
        assert_eq!(app.key(key(KeyCode::Char('y'))), Some(Effect::Undo(id)));
    }

    #[test]
    fn no_color_draws_without_color() {
        let (_dir, app) = sample(false);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| ui::draw(frame, &app, ui::Theme::plain()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(
            buffer
                .content
                .iter()
                .all(|c| c.fg == ratatui::style::Color::Reset)
        );
        assert!(
            buffer
                .content
                .iter()
                .any(|c| c.modifier.contains(ratatui::style::Modifier::REVERSED)),
            "the current screen is still marked"
        );
    }

    /// The sample archive with three proposals: the fixture's, one for Reading and one for
    /// Holding.
    fn review_sample() -> (tempfile::TempDir, App) {
        let (dir, app) = sample(false);
        let queue = carrel_core::review::ReviewQueue::of(&app.archive);
        let (entries, _) = queue.list().unwrap();
        let collections = app.archive.collections().unwrap();
        for (title, path) in [("A paper", "Reading"), ("A scan", "Holding")] {
            let mut entry = entries[0].clone();
            entry.id = Id::new_v7();
            entry.card.title = title.into();
            let collection = collections.iter().find(|c| c.path == path).unwrap();
            entry.destination.collection = collection.doc.id;
            entry.destination.path = path.into();
            queue.save(&entry).unwrap();
        }
        let archive = app.archive.clone();
        (dir, App::new(archive).unwrap())
    }

    #[test]
    fn a_likely_duplicate_is_shown_beside_the_archived_item() {
        use carrel_core::ingest::likely::{Likely, Why};
        let (_dir, app) = sample(false);
        let queue = carrel_core::review::ReviewQueue::of(&app.archive);
        let (entries, _) = queue.list().unwrap();
        let archived = app
            .archive
            .items()
            .unwrap()
            .into_iter()
            .find(|i| i.card == "Projects/Hardware/parts-list.csv.md")
            .unwrap();
        let mut entry = entries[0].clone();
        entry.likely_duplicates = vec![Likely {
            id: archived.id,
            card: archived.card.clone(),
            why: vec![Why::NewerVersion],
        }];
        queue.save(&entry).unwrap();
        let mut app = App::new(app.archive.clone()).unwrap();
        app.key(key(KeyCode::Char('2')));
        let chosen = app.queue.iter().position(|e| e.id == entry.id).unwrap();
        app.chosen = chosen;
        app.load_detail();

        let text = screen(&app, 140, 40);
        assert!(text.contains("likely duplicate"), "{text}");
        assert!(
            text.contains("Likely duplicate (looks like a newer version)"),
            "{text}"
        );
        assert!(text.contains("parts-list.csv"), "{text}");
        assert!(text.contains("file as version"), "the keys change: {text}");
        assert_eq!(
            app.key(key(KeyCode::Char('v'))),
            Some(Effect::Keep {
                id: entry.id,
                kept: carrel_core::actions::Kept::AsVersion,
                target: archived.id,
            })
        );
        assert_eq!(app.key(key(KeyCode::Char('s'))), None);
        assert!(screen(&app, 140, 40).contains("Reject 1 proposal?"));
    }

    #[test]
    fn backups_are_set_up_with_a_masked_password_typed_twice() {
        let (_dir, mut app) = sample(false);
        app.picker_preference = Some("builtin".into());
        app.key(key(KeyCode::Char('9')));
        assert_eq!(app.tab, Tab::Backups);
        let text = screen(&app, 120, 30);
        assert!(text.contains("No backups set up"), "{text}");
        assert!(text.contains("s to set them up"), "{text}");
        assert_eq!(app.key(key(KeyCode::Char('n'))), None);
        assert!(app.message.as_deref().unwrap().contains("aren't set up"));

        let repository = app.archive.root().parent().unwrap().join("backups");
        app.key(key(KeyCode::Char('s')));
        // `/` in the browser types a path instead.
        app.key(key(KeyCode::Char('/')));
        for c in repository.to_str().unwrap().chars() {
            app.key(key(KeyCode::Char(c)));
        }
        app.key(key(KeyCode::Enter));
        for c in "hunter 2".chars() {
            app.key(key(KeyCode::Char(c)));
        }
        let text = screen(&app, 120, 30);
        assert!(text.contains("New password"), "{text}");
        assert!(
            text.contains("••••••••"),
            "the password is shown as dots: {text}"
        );
        assert!(!text.contains("hunter"), "{text}");
        assert!(text.contains("Keep it in a password"), "{text}");
        app.key(key(KeyCode::Enter));
        for c in "hunter 3".chars() {
            app.key(key(KeyCode::Char(c)));
        }
        assert_eq!(app.key(key(KeyCode::Enter)), None, "they don't match");
        assert!(app.message.as_deref().unwrap().contains("don't match"));

        app.key(key(KeyCode::Char('s')));
        // `/` in the browser types a path instead.
        app.key(key(KeyCode::Char('/')));
        for c in repository.to_str().unwrap().chars() {
            app.key(key(KeyCode::Char(c)));
        }
        app.key(key(KeyCode::Enter));
        for c in "pw".chars() {
            app.key(key(KeyCode::Char(c)));
        }
        app.key(key(KeyCode::Enter));
        for c in "pw".chars() {
            app.key(key(KeyCode::Char(c)));
        }
        assert_eq!(app.key(key(KeyCode::Enter)), None, "then its name");
        assert!(screen(&app, 220, 30).contains("A name for the backups"));
        for c in "Drive".chars() {
            app.key(key(KeyCode::Char(c)));
        }
        match app.key(key(KeyCode::Enter)) {
            Some(Effect::BackupSetup {
                repository: given,
                password,
                description,
            }) => {
                assert_eq!(given, repository);
                assert_eq!(password.0, "pw");
                assert_eq!(format!("{password:?}"), "Secret(…)", "never printed");
                assert_eq!(description.as_deref(), Some("Drive"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_backup_folder_is_chosen_in_the_built_in_browser() {
        let (_dir, mut app) = sample(false);
        app.picker_preference = Some("builtin".into());
        let parent = app.archive.root().parent().unwrap().to_owned();
        std::fs::create_dir_all(parent.join("Backups")).unwrap();
        app.start_folder = parent.clone();
        app.key(key(KeyCode::Char('9')));
        assert_eq!(app.key(key(KeyCode::Char('s'))), None);
        let browser = app.browser.as_ref().expect("the built-in browser opens");
        let at = browser
            .entries
            .iter()
            .position(|(name, _)| name == "Backups")
            .unwrap();
        assert!(screen(&app, 220, 30).contains("choose the folder"));
        for _ in 0..at {
            app.key(key(KeyCode::Down));
        }
        app.key(key(KeyCode::Char('i')));
        assert!(app.browser.is_none());
        match &app.input {
            Some(super::screens::Input {
                purpose:
                    super::screens::InputPurpose::BackupPassword {
                        repository, new, ..
                    },
                ..
            }) => {
                assert_eq!(repository, &parent.join("Backups"));
                assert!(*new, "an empty folder makes new backups");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            app.picking,
            super::app::Picking::Ingest,
            "picking goes back to ingest"
        );
    }

    #[test]
    fn the_browser_opens_where_it_was_last_left() {
        let (_dir, mut app) = sample(false);
        app.picker_preference = Some("builtin".into());
        let parent = app.archive.root().parent().unwrap().to_owned();
        let inbox = parent.join("Inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::write(inbox.join("letter.txt"), "hi").unwrap();
        app.start_folder = parent.clone();

        app.key(key(KeyCode::Char('i')));
        let at = app
            .browser
            .as_ref()
            .unwrap()
            .entries
            .iter()
            .position(|(name, _)| name == "Inbox")
            .unwrap();
        for _ in 0..at {
            app.key(key(KeyCode::Down));
        }
        app.key(key(KeyCode::Enter));
        assert_eq!(app.browser.as_ref().unwrap().dir, inbox);
        app.key(key(KeyCode::Esc));
        assert_eq!(app.start_folder, inbox, "closing remembers where it was");

        app.key(key(KeyCode::Char('i')));
        assert_eq!(
            app.browser.as_ref().unwrap().dir,
            inbox,
            "and opens there again"
        );
        app.key(key(KeyCode::Char('i')));
        assert_eq!(app.start_folder, inbox);

        // A backup folder isn't remembered for ingest.
        app.key(key(KeyCode::Esc));
        app.key(key(KeyCode::Char('9')));
        app.key(key(KeyCode::Char('s')));
        app.key(key(KeyCode::Left));
        app.key(key(KeyCode::Esc));
        assert_eq!(app.start_folder, inbox);
    }

    #[test]
    fn review_shows_the_queue_by_destination_beside_the_chosen_proposal() {
        let (_dir, mut app) = review_sample();
        app.key(key(KeyCode::Char('2')));
        let text = screen(&app, 120, 30);
        assert!(text.contains("Holding"), "{text}");
        let holding = text.find("Holding").unwrap();
        let hardware = text.find("Projects/Hardware").unwrap();
        let reading = text.find("Reading").unwrap();
        assert!(
            holding < hardware && hardware < reading,
            "grouped by destination, in order"
        );
        assert!(text.contains("A scan  (medium)"));
        assert!(text.contains("proposal"), "the chosen proposal in full");
        assert!(text.lines().last().unwrap().contains("a approve"));
    }

    #[test]
    fn review_keys_decide_the_chosen_or_the_marked_proposals() {
        let (_dir, mut app) = review_sample();
        app.key(key(KeyCode::Char('2')));
        let ids: Vec<Id> = app.queue.iter().map(|e| e.id).collect();
        assert_eq!(
            app.key(key(KeyCode::Char('a'))),
            Some(Effect::Decide {
                ids: vec![ids[0]],
                holding: false
            })
        );
        app.key(key(KeyCode::Char(' ')));
        app.key(key(KeyCode::Char(' ')));
        assert_eq!(app.marked.len(), 2);
        assert!(screen(&app, 120, 30).contains("2 marked"));
        assert_eq!(app.chosen, 2, "marking moves on");
        match app.key(key(KeyCode::Char('h'))) {
            Some(Effect::Decide {
                ids: chosen,
                holding: true,
            }) => {
                assert_eq!(chosen.len(), 2);
                assert!(chosen.contains(&ids[0]) && chosen.contains(&ids[1]));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            app.key(key(KeyCode::Char('A'))),
            Some(Effect::ApproveAllHigh)
        );
        assert_eq!(app.key(key(KeyCode::Char('e'))), Some(Effect::Edit(ids[2])));

        app.marked.clear();
        assert_eq!(app.key(key(KeyCode::Char('x'))), None);
        assert!(screen(&app, 120, 30).contains("Reject 1 proposal?"));
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::Reject(vec![ids[2]]))
        );

        app.busy = Some("Filing 2 proposals…".into());
        assert_eq!(
            app.key(key(KeyCode::Char('a'))),
            None,
            "one batch at a time"
        );
        assert!(app.message.as_deref().unwrap().starts_with("Still busy"));
        assert!(
            screen(&app, 120, 30)
                .lines()
                .last()
                .unwrap()
                .contains("Filing 2 proposals…")
        );
    }

    #[test]
    fn z_keeps_a_compressed_file_compressed_or_expands_it() {
        use carrel_core::compressed::plan::{Choice, Plan};
        let (_dir, mut app) = review_sample();
        app.key(key(KeyCode::Char('2')));
        assert_eq!(app.key(key(KeyCode::Char('z'))), None);
        assert_eq!(app.message.as_deref(), Some("Not a compressed file"));
        let queue = carrel_core::review::ReviewQueue::of(&app.archive);
        let mut entry = app.queue[app.chosen].clone();
        entry.compressed = Some(Plan {
            format: carrel_core::detect::Compressed::Zip,
            action: Choice::Expand,
            reason: None,
            by_owner: false,
            files: Some(2),
            size: Some(10),
            kind: Some(carrel_core::card::Kind::Folder),
            sealed: None,
            name: Some("Export".into()),
            extra: carrel_core::card::Extra::new(),
        });
        queue.save(&entry).unwrap();
        app.reload().unwrap();
        let chosen = app.queue.iter().position(|e| e.id == entry.id).unwrap();
        app.chosen = chosen;
        assert!(app.keys().contains(&("z", "keep compressed")));
        assert_eq!(
            app.key(key(KeyCode::Char('z'))),
            Some(Effect::Compressed {
                ids: vec![entry.id],
                choice: Choice::Keep
            })
        );
    }

    #[test]
    fn the_picker_narrows_collections_as_you_type() {
        let (_dir, mut app) = review_sample();
        app.key(key(KeyCode::Char('2')));
        let id = app.queue[app.chosen].id;
        app.key(key(KeyCode::Char('m')));
        assert!(screen(&app, 120, 30).contains("Move to (type to narrow)"));
        for c in "hard".chars() {
            app.key(key(KeyCode::Char(c)));
        }
        assert_eq!(
            app.picker.as_ref().unwrap().matching(),
            ["Projects/Hardware"]
        );
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::Move {
                ids: vec![id],
                to: "Projects/Hardware".into()
            })
        );
        assert!(app.picker.is_none());
        app.key(key(KeyCode::Char('m')));
        app.key(key(KeyCode::Esc));
        assert!(app.picker.is_none(), "Esc cancels");
        assert!(!app.quit, "and doesn't quit");
    }

    #[test]
    fn c_opens_a_line_for_the_correction_without_leaving_the_screen() {
        let (_dir, mut app) = review_sample();
        app.key(key(KeyCode::Char('2')));
        let first = app.queue[0].id;
        assert_eq!(app.key(key(KeyCode::Char('c'))), None);
        assert!(screen(&app, 120, 30).contains("What's wrong with it?"));
        assert!(
            app.keys().iter().any(|(k, _)| *k == "Enter"),
            "typing keys, not Review's, while the line is open"
        );
        for c in "created: 2019-03-31".chars() {
            app.key(key(KeyCode::Char(c)));
        }
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::Correction(super::app::Step::Work {
                target: super::screens::Correcting::Proposal(first),
                words: "created: 2019-03-31".into(),
                answering: None,
            }))
        );
        assert!(app.keys().iter().any(|(k, v)| *k == "c" && *v == "correct"));

        let (_dir, mut app) = sample(false);
        app.key(key(KeyCode::Char('5')));
        app.key(key(KeyCode::Char('c')));
        assert!(app.input.is_none(), "a collection isn't an item");
        app.key(key(KeyCode::Right));
        let card = app.browse_items[app.browse_item].0.clone();
        app.key(key(KeyCode::Char('c')));
        app.key(key(KeyCode::Char('x')));
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::Correction(super::app::Step::Work {
                target: super::screens::Correcting::Item(card),
                words: "x".into(),
                answering: None,
            }))
        );
    }

    #[test]
    fn the_correction_pops_up_and_saves_only_on_y() {
        let (dir, mut app) = review_sample();
        app.key(key(KeyCode::Char('2')));
        let id = app.queue[0].id.to_string();
        let prepared =
            crate::correct::prepare_review(&app.archive, &id, "created: 2019-03-31").unwrap();
        app.correction = Some(prepared);
        let shown = screen(&app, 120, 30);
        assert!(shown.contains("2019-03-31"), "{shown}");
        assert!(shown.contains("y save"), "{shown}");
        assert_eq!(app.keys(), vec![("y", "save"), ("n", "discard")]);
        assert_eq!(app.key(key(KeyCode::Char('n'))), None);
        assert!(app.correction.is_none());
        assert!(app.message.as_deref().unwrap().contains("discarded"));

        app.correction =
            Some(crate::correct::prepare_review(&app.archive, &id, "created: 2019-03-31").unwrap());
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::Correction(super::app::Step::Save))
        );
        let saved = crate::correct::save(&app.archive, app.correction.take().unwrap()).unwrap();
        assert!(saved.message.starts_with("Saved"));
        drop(dir);
    }

    #[test]
    fn e_edits_the_browsed_item_in_the_editor() {
        let (_dir, mut app) = sample(false);
        app.key(key(KeyCode::Char('5')));
        assert_eq!(
            app.key(key(KeyCode::Char('e'))),
            None,
            "a collection isn't an item"
        );
        app.key(key(KeyCode::Right));
        let card = app.browse_items[app.browse_item].0.clone();
        assert!(app.keys().iter().any(|(k, v)| *k == "e" && *v == "edit"));
        assert_eq!(
            app.key(key(KeyCode::Char('e'))),
            Some(Effect::EditItem {
                card: card.clone(),
                body: false
            })
        );
        assert!(
            app.keys()
                .iter()
                .any(|(k, v)| *k == "b" && *v == "edit notes")
        );
        assert_eq!(
            app.key(key(KeyCode::Char('b'))),
            Some(Effect::EditItem { card, body: true })
        );
    }

    #[test]
    fn browse_moves_through_collections_items_and_cards_and_searches() {
        let (_dir, mut app) = sample(false);
        app.key(key(KeyCode::Char('5')));
        assert_eq!(app.collections[app.browse_collection].path, "Holding");
        assert!(screen(&app, 120, 30).contains("Untitled scan"));
        app.key(key(KeyCode::Down));
        assert_eq!(app.collections[app.browse_collection].path, "Projects");
        app.key(key(KeyCode::Right));
        assert_eq!(app.browse_pane, Pane::Items);
        assert_eq!(
            app.tab,
            Tab::Browse,
            "→ moves between panes here, not screens"
        );
        assert!(!app.item.text.is_empty(), "the chosen item's card");
        app.key(key(KeyCode::Left));
        assert_eq!(app.browse_pane, Pane::Collections);

        let search = |app: &mut App, typed: &str| {
            app.key(key(KeyCode::Char('/')));
            while app.input.as_ref().is_some_and(|i| !i.text.is_empty()) {
                app.key(key(KeyCode::Backspace));
            }
            for c in typed.chars() {
                app.key(key(KeyCode::Char(c)));
            }
            app.key(key(KeyCode::Enter));
        };
        search(&mut app, "oscillators");
        let titles: Vec<&str> = app.browse_items.iter().map(|(_, t)| t.as_str()).collect();
        assert!(
            titles.contains(&"A note on coupled oscillators"),
            "{titles:?}"
        );
        assert!(titles.len() >= 2, "across collections: {titles:?}");
        let text = screen(&app, 140, 30);
        assert!(
            text.contains(&format!("{} found for “oscillators”", titles.len())),
            "{text}"
        );
        assert!(text.contains("Matched: title"), "why it came back: {text}");

        search(&mut app, "oscillators kind:bundle");
        let titles: Vec<&str> = app.browse_items.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(
            titles,
            ["oscillator-sim"],
            "the same filters as carrel search"
        );

        search(&mut app, "kind:nonsense");
        assert_eq!(app.browse_items, Vec::new());
        assert!(
            app.message.as_deref().is_some_and(|m| m.contains("kind")),
            "{:?}",
            app.message
        );
        app.key(key(KeyCode::Esc));
        assert!(app.search.is_none());
        assert!(!app.quit, "Esc clears the search first");
    }

    #[test]
    fn an_items_view_shows_its_changes_with_undo_and_its_links() {
        use carrel_core::history::{Change, Operation};
        let (_dir, mut app) = sample(true);
        let items = app.archive.items().unwrap();
        let parts = items
            .iter()
            .find(|i| i.card == "Projects/Hardware/parts-list.csv.md")
            .unwrap();
        let lease = DiskLock::for_archive(&app.archive)
            .acquire(
                Holder {
                    person: Id::new_v7(),
                    device: Id::new_v7(),
                },
                "test",
                DEFAULT_LEASE,
            )
            .unwrap();
        let card = app.archive.root().join(&parts.card);
        let text = std::fs::read_to_string(&card).unwrap();
        std::fs::write(&card, format!("{text}\nA note.\n")).unwrap();
        let mut change = Change::new(
            Operation::Edit,
            "Correct the parts list",
            Id::new_v7(),
            Id::new_v7(),
        );
        change.items = vec![parts.id];
        let commit = History::open(&app.archive)
            .unwrap()
            .commit(&app.archive, &change, &lease)
            .unwrap()
            .unwrap();
        lease.release().unwrap();
        app.reload().unwrap();

        app.key(key(KeyCode::Char('5')));
        app.browse_collection = app
            .collections
            .iter()
            .position(|c| c.path == "Projects/Hardware")
            .unwrap();
        app.load_items();
        app.key(key(KeyCode::Right));
        app.browse_item = app
            .browse_items
            .iter()
            .position(|(card, _)| card == "Projects/Hardware/parts-list.csv.md")
            .unwrap();
        app.load_card();
        assert_eq!(
            app.item.history.len(),
            1,
            "only the changes that touched it"
        );
        let text = screen(&app, 140, 30);
        assert!(text.contains("History"), "{text}");
        assert!(text.contains("Correct the parts list"), "{text}");
        assert!(text.contains("part-of  Desk lamp build"), "{text}");

        app.key(key(KeyCode::Enter));
        assert_eq!(app.browse_pane, Pane::Item);
        assert!(app.keys().iter().any(|(k, v)| *k == "u" && *v == "undo"));
        app.key(key(KeyCode::Char('u')));
        assert!(matches!(&app.confirm, Some(Confirm::Undo { id, .. }) if *id == commit));
        assert_eq!(app.key(key(KeyCode::Char('y'))), Some(Effect::Undo(commit)));

        app.key(key(KeyCode::Down));
        assert!(
            app.keys()
                .iter()
                .any(|(k, v)| *k == "Enter" && *v == "go to it")
        );
        app.key(key(KeyCode::Char('u')));
        assert!(app.confirm.is_none(), "a link isn't a change");
        app.key(key(KeyCode::Enter));
        assert_eq!(app.browse_pane, Pane::Items);
        assert_eq!(
            app.browse_items[app.browse_item].0,
            "Projects/Hardware/lamp-build.md"
        );
        assert_eq!(app.item.history.len(), 0);
        app.key(key(KeyCode::Enter));
        app.key(key(KeyCode::Esc));
        assert_eq!(app.browse_pane, Pane::Items, "Esc leaves the item's view");
        assert!(!app.quit);
    }

    /// A unit vector of 32 numbers counting `text`'s words, as the fake embedder in the
    /// core's index tests makes them, so texts sharing words are near each other.
    pub(super) fn word_vector(text: &str) -> Vec<f32> {
        let mut v = [0.0f32; 32];
        for word in text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
        {
            let place = word
                .to_lowercase()
                .chars()
                .fold(7usize, |h, c| h.wrapping_mul(31).wrapping_add(c as usize));
            v[place % 32] += 1.0;
        }
        let length = v.iter().map(|n| n * n).sum::<f32>().sqrt();
        v.iter().map(|n| n / length).collect()
    }

    /// Builds the archive's index from each item's card text, as a model making `vector`'s
    /// vectors would, with no model.
    fn build_index(archive: &Archive, vector: &dyn Fn(&str, &str) -> Vec<f32>) {
        use carrel_core::index::{Entry, Index, Part, plan};
        let model = archive.settings().embedding_model().unwrap().to_owned();
        let mut index = Index::new(archive, &model);
        for pending in plan(archive, None).unwrap().pending {
            let entry = Entry {
                id: pending.id,
                card: carrel_core::card::Checksum::of_bytes(pending.card_text.as_bytes()),
                text: pending.contents.clone(),
                parts: vec![Part::Card],
            };
            let v = vector(&pending.card, &pending.card_text);
            index.put(entry, &[v]).unwrap();
        }
        index.save().unwrap();
    }

    #[test]
    fn an_items_view_lists_the_items_nearest_it_by_meaning() {
        use super::item::Relation;
        use carrel_core::card::Sensitivity;
        let (_dir, mut app) = sample(false);
        app.key(key(KeyCode::Char('5')));
        let parts = "Projects/Hardware/parts-list.csv.md";
        let notes = "Projects/Hardware/notes.md.md";
        let show = |app: &mut App, card: &str| {
            app.browse_items = vec![(card.to_owned(), String::new())];
            app.browse_item = 0;
            app.load_card();
        };

        // No index yet: only the links, and why.
        show(&mut app, parts);
        assert_eq!(app.item.related.len(), 1, "{:?}", app.item.related);
        let why = app.item.links_only.clone().unwrap();
        assert!(why.contains("carrel index rebuild"), "{why}");
        let text = screen(&app, 200, 40);
        assert!(text.contains("no meaning index"), "{text}");

        // Hardware notes, made restricted, is embedded by its card like any other, here as
        // near the parts list as can be.
        let path = app.archive.root().join(notes);
        let card = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            card.replace("sensitivity: personal", "sensitivity: restricted"),
        )
        .unwrap();
        let parts_text = std::fs::read_to_string(app.archive.root().join(parts)).unwrap();
        let parts_card = carrel_core::card_file::CardFile::parse(&parts_text).unwrap();
        let parts_vector = word_vector(&carrel_core::index::chunk::card_text(
            parts_card.card(),
            "Projects/Hardware",
        ));
        build_index(&app.archive, &|card, text| {
            if card == notes {
                parts_vector.clone()
            } else {
                word_vector(text)
            }
        });

        show(&mut app, parts);
        assert_eq!(app.item.links_only, None);
        let related = &app.item.related;
        assert!(matches!(&related[0].relation, Relation::Link(how) if how == "part-of"));
        let near: Vec<_> = related[1..].iter().collect();
        assert_eq!(
            near.len(),
            carrel_core::index::related::NEAREST,
            "{related:?}"
        );
        assert!(near.iter().all(|r| matches!(r.relation, Relation::Near(_))));
        assert!(
            near.iter().all(|r| {
                let card = r.card.as_deref();
                card != Some(parts) && card != Some("Projects/Hardware/lamp-build.md")
            }),
            "neither itself nor what it links to: {near:?}"
        );
        assert_eq!(near[0].card.as_deref(), Some(notes));
        assert_eq!(near[0].relation, Relation::Near(Sensitivity::Restricted));
        let text = screen(&app, 200, 40);
        assert!(
            text.contains("near  Hardware notes  [restricted]"),
            "{text}"
        );

        // Enter on a near item goes to it, as on a link.
        app.browse_pane = Pane::Item;
        app.item.chosen = 1;
        app.key(key(KeyCode::Enter));
        assert_eq!(app.browse_pane, Pane::Items);
        assert_eq!(app.browse_items[app.browse_item].0, notes);

        // A card the index hasn't caught up with lists its links only, and says why.
        let path = app.archive.root().join(parts);
        std::fs::write(
            &path,
            parts_text.replace("Desk lamp parts list", "Lamp parts"),
        )
        .unwrap();
        show(&mut app, parts);
        assert_eq!(app.item.related.len(), 1);
        let why = app.item.links_only.clone().unwrap();
        assert!(why.contains("hasn't caught up"), "{why}");
    }

    #[test]
    fn questions_are_answered_in_place_or_dismissed() {
        use carrel_core::questions::{QuestionEntry, QuestionKind, QuestionQueue, Suggestion};
        let (_dir, app) = sample(false);
        let proposal = app.queue[0].id;
        let queue = QuestionQueue::of(&app.archive);
        let question = |kind, subject: &str, question: &str, choices: Vec<String>| {
            let mut entry = QuestionEntry::new(kind, subject, question);
            entry.items = vec![proposal];
            entry.choices = choices;
            entry
        };
        let term = question(QuestionKind::Term, "ABX", "What is “ABX”?", Vec::new());
        let mut place = question(
            QuestionKind::Collection,
            "lamp",
            "Which collection do items about “lamp” belong in?",
            vec!["Projects".into(), "Projects/Hardware".into()],
        );
        place.suggestions = vec![Suggestion::new(
            "Projects/Hardware",
            Some("where one was going".into()),
        )];
        queue.save(&term).unwrap();
        queue.save(&place).unwrap();
        let mut app = App::new(app.archive.clone()).unwrap();
        app.key(key(KeyCode::Char('4')));
        let text = screen(&app, 160, 30);
        assert!(
            text.contains("Which collection do items about “lamp” belong in?  (1 item)"),
            "{text}"
        );
        assert_eq!(
            app.asked[0].id, term.id,
            "as worth answering, so the older first"
        );
        app.key(key(KeyCode::Down));
        let text = screen(&app, 160, 30);
        assert!(
            text.contains("1  Projects/Hardware  (where one was going)"),
            "{text}"
        );
        assert_eq!(
            app.key(key(KeyCode::Char('1'))),
            Some(Effect::Answer {
                id: place.id,
                answer: "1".into(),
                learn: false,
            }),
            "a number picks a suggestion"
        );

        app.key(key(KeyCode::Enter));
        assert!(screen(&app, 120, 30).contains("They belong in (type to narrow)"));
        app.key(key(KeyCode::Down));
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::Answer {
                id: place.id,
                answer: "Projects/Hardware".into(),
                learn: false,
            })
        );

        app.key(key(KeyCode::Up));
        assert!(
            screen(&app, 120, 30).contains("Desk lamp diffuser (print file)"),
            "what waits on it"
        );
        app.key(key(KeyCode::Enter));
        for c in "An assay kit.".chars() {
            app.key(key(KeyCode::Char(c)));
        }
        assert!(screen(&app, 120, 30).contains("> An assay kit."));
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::Answer {
                id: term.id,
                answer: "An assay kit.".into(),
                learn: false,
            })
        );
        app.key(key(KeyCode::Enter));
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            None,
            "an empty answer isn't sent"
        );
        assert_eq!(
            app.key(key(KeyCode::Char('s'))),
            Some(Effect::Skip(term.id))
        );
        assert_eq!(
            app.key(key(KeyCode::Char('d'))),
            Some(Effect::Dismiss(term.id))
        );
    }

    #[test]
    fn item_and_sender_questions_ask_before_changing_anything() {
        use carrel_core::questions::{QuestionEntry, QuestionKind, QuestionQueue, suggest};
        let (_dir, app) = sample(false);
        let proposal = app.queue[0].id;
        let queue = QuestionQueue::of(&app.archive);
        let mut item = QuestionEntry::new(
            QuestionKind::Item,
            "Desk lamp diffuser",
            "What was this project for?",
        );
        item.items = vec![proposal];
        let mut sender = QuestionEntry::new(
            QuestionKind::Sender,
            "Okafor",
            "Should “Okafor” have its own collection?",
        );
        sender.items = vec![proposal];
        sender.suggestions = vec![
            suggest::new_collection("Projects/Okafor", "most are in Projects"),
            suggest::no_change("No, keep them in Projects", None),
        ];
        queue.save(&item).unwrap();
        queue.save(&sender).unwrap();
        let mut app = App::new(app.archive.clone()).unwrap();
        app.key(key(KeyCode::Char('4')));
        assert_eq!(app.asked[0].id, item.id);

        app.key(key(KeyCode::Enter));
        for c in "A reading light".chars() {
            app.key(key(KeyCode::Char(c)));
        }
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::Correction(super::app::Step::Work {
                target: super::screens::Correcting::Proposal(proposal),
                words: "Asked “What was this project for?”, the owner said: A reading light".into(),
                answering: Some(item.id),
            })),
            "an item answer is a correction, shown before it's saved"
        );

        app.key(key(KeyCode::Down));
        assert_eq!(app.key(key(KeyCode::Char('1'))), None, "it asks first");
        let text = screen(&app, 160, 30);
        assert!(text.contains("y with a rule   n without a rule"), "{text}");
        assert_eq!(
            app.key(key(KeyCode::Char('n'))),
            Some(Effect::Answer {
                id: sender.id,
                answer: "1".into(),
                learn: false,
            }),
            "no to the rule still gathers them"
        );
        app.key(key(KeyCode::Char('1')));
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::Answer {
                id: sender.id,
                answer: "1".into(),
                learn: true,
            })
        );
        assert_eq!(
            app.key(key(KeyCode::Char('2'))),
            Some(Effect::Answer {
                id: sender.id,
                answer: "2".into(),
                learn: false,
            }),
            "no change needs no rule"
        );
    }

    #[test]
    fn i_adds_files_through_a_plan_and_the_queue_can_be_changed() {
        let (dir, mut app) = sample(false);
        app.device = Some(Id::new_v7());
        let file = dir.path().join("a note.txt");
        std::fs::write(&file, "a note\n").unwrap();

        app.picker_preference = Some("builtin".into());
        app.start_folder = dir.path().to_owned();
        assert_eq!(app.key(key(KeyCode::Char('i'))), None);
        assert!(app.browser.is_some(), "the built-in browser, as preferred");
        app.key(key(KeyCode::Char('/')));
        assert!(app.input.is_some(), "/ types a path instead");
        for c in format!("'{}'", file.display()).chars() {
            app.key(key(KeyCode::Char(c)));
        }
        app.key(key(KeyCode::Enter));
        assert_eq!(app.tab, Tab::Ingest);
        let plan = app.planned.clone().expect("a plan before queueing");
        assert_eq!(plan.count, 1);
        assert!(screen(&app, 100, 30).contains("1 item to queue"));

        let effect = app.key(key(KeyCode::Enter));
        assert_eq!(
            effect,
            Some(Effect::Queue {
                paths: vec![file.clone()],
                options: carrel_core::ingest::IntakeOptions::default(),
            })
        );
        assert_eq!(app.planned, None);

        // As the run loop would: queue it, then read the queue.
        carrel_core::intake::add(
            &app.archive,
            std::slice::from_ref(&file),
            carrel_core::ingest::IntakeOptions::default(),
            app.device.unwrap(),
        )
        .unwrap();
        app.load_intake();
        assert_eq!(app.intake.len(), 1);
        assert!(screen(&app, 100, 30).contains("a note.txt"));
        let id = app.intake[0].id;
        assert_eq!(app.key(key(KeyCode::Char('x'))), Some(Effect::Unqueue(id)));
        assert_eq!(app.key(key(KeyCode::Char('t'))), Some(Effect::Front(id)));
        assert_eq!(app.key(key(KeyCode::Char('r'))), Some(Effect::Retry(id)));
    }

    #[test]
    fn a_pile_folder_waits_for_each_or_whole() {
        let (dir, mut app) = sample(false);
        app.device = Some(Id::new_v7());
        let pile = dir.path().join("Downloads");
        std::fs::create_dir(&pile).unwrap();
        std::fs::write(pile.join("one.txt"), "1").unwrap();
        std::fs::write(pile.join("two.txt"), "2").unwrap();
        app.plan(vec![pile], carrel_core::ingest::IntakeOptions::default());
        assert!(app.planned.as_ref().unwrap().problem.is_some());
        assert_eq!(app.key(key(KeyCode::Enter)), None, "nothing to queue yet");
        app.key(key(KeyCode::Char('e')));
        let plan = app.planned.clone().unwrap();
        assert_eq!((plan.count, plan.problem), (2, None));
        app.key(key(KeyCode::Esc));
        assert_eq!(app.planned, None);
    }

    #[test]
    fn i_runs_the_owners_picker_when_there_is_one() {
        let (dir, mut app) = sample(false);
        app.picker_preference = Some("lf".into());
        assert_eq!(
            app.key(key(KeyCode::Char('i'))),
            Some(Effect::Pick(vec![
                "lf".into(),
                "-selection-path={out}".into()
            ]))
        );
        app.device = Some(Id::new_v7());
        let file = dir.path().join("picked.txt");
        std::fs::write(&file, "x").unwrap();
        super::picked_files(&mut app, Ok(vec![file]));
        assert_eq!(app.planned.as_ref().map(|p| p.count), Some(1));
        super::picked_files(
            &mut app,
            Err(crate::Failure::new("couldn't start the picker lf")),
        );
        assert!(app.browser.is_some(), "falls back to the built-in browser");
    }

    #[test]
    fn settings_are_shown_and_changed_the_way_the_command_does() {
        let (_dir, mut app) = sample(false);
        app.key(key(KeyCode::Char('8')));
        assert_eq!(app.tab, Tab::Settings);
        let screen_text = screen(&app, 120, 30);
        assert!(screen_text.contains("This device"), "{screen_text}");
        assert!(screen_text.contains("ingest.originals"), "{screen_text}");

        let originals = app
            .settings
            .iter()
            .position(|s| s.label == "ingest.originals")
            .unwrap();
        app.settings_chosen = originals;
        app.key(key(KeyCode::Enter));
        let picker = app.picker.clone().expect("a choice of values");
        assert_eq!(picker.collections, ["move", "keep"]);
        app.key(key(KeyCode::Down));
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::SetSetting {
                key: "ingest.originals".into(),
                value: "keep".into(),
            })
        );

        app.settings_chosen = 0;
        app.key(key(KeyCode::Enter));
        let pickers = app.picker.clone().expect("a choice of pickers");
        assert!(pickers.collections.contains(&"builtin".to_owned()));
        assert!(pickers.collections.contains(&"yazi".to_owned()));
    }

    /// The sample archive with its parts list checked out to `out/`, due back on 30 November,
    /// changed there and with two pending history entries, and a folder on the desk of the
    /// device the app shows.
    pub(super) fn active_sample() -> (tempfile::TempDir, App) {
        use carrel_core::card_file::CardFile;
        use carrel_core::entries::HistoryEntry;
        let (dir, app) = sample(true);
        let archive = app.archive.clone();
        let device = Id::new_v7();
        let holder = Holder {
            person: Id::new_v7(),
            device,
        };
        let lease = DiskLock::for_archive(&archive)
            .acquire(holder, "test", DEFAULT_LEASE)
            .unwrap();
        let items = archive.items().unwrap();
        let item = items
            .iter()
            .find(|i| i.card == "Projects/Hardware/parts-list.csv.md")
            .unwrap();
        let out = dir.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        let due = carrel_core::card::DateOrTime("2026-11-30".into());
        let checked =
            carrel_core::checkout::check_out(&archive, item, &out, Some(due), &lease).unwrap();
        let card = archive.root().join(&item.card);
        let mut file = CardFile::parse(&std::fs::read_to_string(&card).unwrap()).unwrap();
        let mut log = file.history().unwrap();
        for (date, minutes) in [("2026-10-30", 70), ("2026-10-31", 20)] {
            log.add(HistoryEntry::pending(
                date,
                Some(minutes),
                vec!["Changed 1 file(s): parts-list.csv".into()],
            ));
        }
        file.set_history(&log).unwrap();
        std::fs::write(&card, file.to_text().unwrap()).unwrap();
        let thesis = dir.path().join("Thesis");
        std::fs::create_dir_all(&thesis).unwrap();
        std::fs::write(thesis.join("draft.md"), "# Draft\n").unwrap();
        carrel_core::desk::add(&archive, &thesis, Some("Thesis"), None, None, &lease).unwrap();
        lease.release().unwrap();
        std::fs::write(&checked.working, "part,count\nlamp,2\n").unwrap();
        let mut app = App::new(archive).unwrap();
        app.device = Some(device);
        app.reload().unwrap();
        (dir, app)
    }

    /// Chooses the parts list on the Active screen, and returns it.
    pub(super) fn choose_parts_list(app: &mut App) -> super::active::ActiveItem {
        let i = app
            .active
            .iter()
            .position(|a| a.card.ends_with("parts-list.csv.md"))
            .unwrap();
        app.active_chosen = app
            .active_rows()
            .iter()
            .position(|r| *r == super::active::Row::Item(i))
            .unwrap();
        app.active[i].clone()
    }

    #[test]
    fn active_lists_checkouts_and_desk_items_with_their_pending_entries() {
        let (_dir, mut app) = active_sample();
        app.key(key(KeyCode::Char('3')));
        assert_eq!(app.tab, Tab::Active);
        assert_eq!(
            app.active.len(),
            3,
            "the sample's own checkout, ours, and the desk"
        );
        let title = choose_parts_list(&mut app).title;
        let keys = screen(&app, 100, 30).lines().last().unwrap().to_owned();
        assert!(keys.ends_with("? help  q quit"), "it fits: {keys}");
        let text = screen(&app, 140, 30);
        assert!(
            text.contains(&format!("out   {title}  (changed, 2 pending)")),
            "{text}"
        );
        assert!(
            text.contains("2. 2026-10-30 · 1h 10m"),
            "newest first: {text}"
        );
        assert!(text.contains("desk  Thesis  (on the desk)"), "{text}");
        assert!(text.contains("Working copy:"), "the details: {text}");
        assert!(text.contains("Due 2026-11-30"), "{text}");
        assert!(text.contains("Files: 1 changed"), "{text}");
        assert!(text.contains("Pending history entries:"), "{text}");
        let keys = text.lines().last().unwrap();
        for shown in ["r return", "o open", "a approve", "c combine", "d drop"] {
            assert!(keys.contains(shown), "{shown}: {keys}");
        }
        assert!(keys.contains("x cancel"), "{keys}");

        app.key(key(KeyCode::End));
        assert_eq!(app.active_item().unwrap().title, "Thesis");
        let text = screen(&app, 140, 30);
        assert!(text.contains("On the desk at "), "{text}");
        assert!(text.contains("No pending history entries"), "{text}");
        let keys = app.keys();
        assert!(
            keys.iter().all(|(k, _)| !["r", "a", "Space"].contains(k))
                && keys.contains(&("x", "take off"))
                && keys.contains(&("f", "archive")),
            "only what a desk item without entries answers to: {keys:?}"
        );
        assert_eq!(app.key(key(KeyCode::Char('r'))), None);
        assert!(
            app.message
                .as_deref()
                .unwrap()
                .contains("isn't checked out")
        );
        assert_eq!(app.key(key(KeyCode::Char('a'))), None);
        assert!(app.message.as_deref().unwrap().contains("no pending"));

        app.key(key(KeyCode::Char('?')));
        assert!(screen(&app, 100, 30).contains("return, open, cancel checkout (Active)"));
        app.key(key(KeyCode::Char('?')));
        app.picker_preference = Some("builtin".into());
        app.start_folder = app.archive.root().to_owned();
        app.key(key(KeyCode::Char('i')));
        assert!(
            app.keys().iter().any(|(_, what)| *what == "hidden"),
            "the browser's keys while it's open: {:?}",
            app.keys()
        );
    }

    #[test]
    fn active_keys_open_decide_entries_and_return_after_asking() {
        let (_dir, mut app) = active_sample();
        app.key(key(KeyCode::Char('3')));
        let item = choose_parts_list(&mut app);
        let row = app.active_chosen;
        assert_eq!(
            app.key(key(KeyCode::Char('o'))),
            Some(Effect::Open(item.working.clone()))
        );
        let entries = |numbers: Vec<usize>, decision| Effect::Entries {
            item: item.id.to_string(),
            numbers,
            decision,
        };
        assert_eq!(
            app.key(key(KeyCode::Char('a'))),
            Some(entries(vec![], crate::sessions::Decision::Approve)),
            "all of them, with none marked"
        );
        assert_eq!(app.key(key(KeyCode::Char('e'))), None);
        assert_eq!(
            app.message.as_deref(),
            Some("Choose an entry to edit it"),
            "two entries: which?"
        );
        app.key(key(KeyCode::Down));
        assert!(app.keys().contains(&("e", "edit")), "{:?}", app.keys());
        assert_eq!(
            app.key(key(KeyCode::Char('e'))),
            Some(Effect::EditEntry {
                item: item.id.to_string(),
                number: 1
            })
        );
        app.key(key(KeyCode::Char(' ')));
        assert_eq!(app.active_chosen, row + 2, "marking moves on");
        assert!(screen(&app, 140, 30).contains("1 marked"));
        assert_eq!(
            app.key(key(KeyCode::Char('d'))),
            Some(entries(vec![1], crate::sessions::Decision::Drop))
        );
        app.key(key(KeyCode::Char(' ')));
        app.key(key(KeyCode::Up));
        assert_eq!(
            app.key(key(KeyCode::Char('c'))),
            Some(entries(vec![1, 2], crate::sessions::Decision::Combine))
        );

        assert_eq!(
            app.key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)),
            None
        );
        assert!(app.confirm.is_none(), "Ctrl-R refreshes; it doesn't return");
        app.active_chosen = row;
        app.key(key(KeyCode::Char('r')));
        assert!(matches!(app.confirm, Some(Confirm::Return { .. })));
        let text = screen(&app, 140, 30);
        assert!(
            text.contains(&format!(
                "Return “{}”, replacing the archive copy?",
                item.title
            )),
            "{text}"
        );
        assert!(text.contains("changed"), "what changed: {text}");
        assert!(text.contains("History entry:"), "{text}");
        assert_eq!(app.keys(), vec![("y", "yes"), ("n", "no")]);
        assert_eq!(app.key(key(KeyCode::Char('n'))), None);
        assert_eq!(app.message.as_deref(), Some("Nothing changed"));
        app.key(key(KeyCode::Char('r')));
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::Return(super::active::Returning {
                id: item.id,
                from: None,
                trim: false,
            }))
        );

        app.key(key(KeyCode::Char('x')));
        assert!(screen(&app, 140, 30).contains("Cancel the checkout of"));
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::CancelCheckout(item.id))
        );
    }

    #[test]
    fn active_effects_run_as_the_commands_do() {
        let (dir, mut app) = active_sample();
        let item = choose_parts_list(&mut app);
        let (busy, _) = super::active_work(
            app.archive.root().to_owned(),
            Effect::Entries {
                item: item.query(),
                numbers: vec![2],
                decision: crate::sessions::Decision::Approve,
            },
        );
        assert_eq!(busy, "Approving…");
        assert_eq!(
            super::active_work(
                app.archive.root().to_owned(),
                Effect::Return(super::active::Returning {
                    id: item.id,
                    from: None,
                    trim: false,
                })
            )
            .0,
            "Returning it…"
        );
        let desk = app.active.last().unwrap();
        assert_eq!(
            desk.query(),
            desk.card.display().to_string(),
            "a desk item is named by its card"
        );
        let missing = dir.path().join("nowhere");
        assert!(super::open(&missing).unwrap_err().0.contains("isn't there"));

        // As the run loop does when the work is done: marks go, and everything is read again.
        app.active_marked.insert((item.id, 1));
        std::fs::remove_file(&item.working).unwrap();
        super::finished(&mut app, Ok("Approve 1 history entry of it".into()));
        assert!(app.active_marked.is_empty());
        assert_eq!(choose_parts_list(&mut app).state, "missing");
        assert_eq!(
            app.message.as_deref(),
            Some("Approve 1 history entry of it")
        );
    }
}
