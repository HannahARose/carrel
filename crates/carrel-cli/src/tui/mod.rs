//! The interactive mode (`docs/design/interactive.md`): `carrel` in a terminal, or
//! `carrel tui`. Everything it does is also a command.

mod app;
mod browser;
mod intake;
mod review;
mod screens;
mod settings;
mod ui;

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

pub use app::App;
use app::Effect;

/// Opens the interactive mode on the archive commands would use, and returns when the owner
/// quits. The terminal is restored however it ends.
pub fn run(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let mut app = App::new(archive)?;
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

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    theme: ui::Theme,
) -> Result<(), Failure> {
    let starts = ui::tab_starts();
    let mut running: Option<Done> = None;
    let mut checked = std::time::Instant::now();
    while !app.quit {
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
        if let Some(done) = &running
            && let Ok(result) = done.try_recv()
        {
            running = None;
            app.busy = None;
            finished(app, result);
        }
        terminal
            .draw(|frame| ui::draw(frame, app, theme))
            .map_err(|e| Failure::new(format!("couldn't draw: {e}")))?;
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
                *terminal = enter();
                finished(app, result.map(|r| r.text).map_err(|Failure(e)| e));
            }
            Some(Effect::Pick(words)) => {
                // The picker needs the terminal to itself, as the editor does.
                leave();
                let picked = crate::picker::run(&words, &app.start_folder);
                *terminal = enter();
                if let Some(folder) = picked_files(app, picked)
                    && let Ok(mut prefs) = crate::config::Prefs::load()
                {
                    prefs.last_folder = Some(folder);
                    let _ = prefs.save();
                }
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
    if let Err(Failure(message)) = app.reload() {
        app.message = Some(message);
    }
}

/// Starts work that takes the writer lease, or does it at once when it's quick. The
/// commands do the work, so the interactive mode reports exactly what they report.
fn background(app: &mut App, effect: Effect) -> Option<Done> {
    let root: PathBuf = app.archive.root().to_owned();
    let ids = |ids: &[carrel_core::card::Id]| -> Vec<String> {
        ids.iter().map(ToString::to_string).collect()
    };
    let (busy, work): (String, Box<dyn FnOnce() -> Result<Report, Failure> + Send>) = match effect {
        Effect::Undo(id) => {
            let result = crate::commands::undo(Some(&root), &id);
            finished(app, result.map(|r| r.text).map_err(|Failure(e)| e));
            return None;
        }
        Effect::Reject(chosen) => {
            let result = crate::review::reject(Some(&root), &ids(&chosen));
            finished(app, result.map(|r| r.text).map_err(|Failure(e)| e));
            return None;
        }
        Effect::Move { ids: chosen, to } => {
            let mut text = String::new();
            for id in ids(&chosen) {
                match crate::review::move_to(Some(&root), &id, &to) {
                    Ok(report) => text.push_str(&report.text),
                    Err(Failure(e)) => writeln!(text, "{e}").unwrap(),
                }
            }
            finished(app, Ok(text));
            return None;
        }
        Effect::Edit(_) | Effect::Pick(_) => return None,
        Effect::Dismiss(id) => {
            let result = crate::questions::dismiss(Some(&root), &id.to_string());
            finished(app, result.map(|r| r.text).map_err(|Failure(e)| e));
            return None;
        }
        Effect::SetSetting { key, value } => {
            let result = crate::settings::set(Some(&root), &key, &value);
            finished(app, result.map(|r| r.text).map_err(|Failure(e)| e));
            return None;
        }
        Effect::Download(name) => (
            format!("Downloading {name}…"),
            Box::new(move || crate::models::download_quietly(&name)),
        ),
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
        Effect::Answer { id, answer } => {
            // A term's answer drafts its items again, which can take a while with a model.
            let id = id.to_string();
            (
                "Answering, and drafting its items again…".into(),
                Box::new(move || crate::questions::answer(Some(&root), &id, &[answer])),
            )
        }
        Effect::Decide {
            ids: chosen,
            holding,
        } => {
            let count = chosen.len();
            let chosen = ids(&chosen);
            let busy = format!(
                "{} {count} proposal{}…",
                if holding { "Holding" } else { "Filing" },
                if count == 1 { "" } else { "s" }
            );
            let work: Box<dyn FnOnce() -> Result<Report, Failure> + Send> = if holding {
                Box::new(move || crate::review::hold(Some(&root), &chosen))
            } else {
                Box::new(move || crate::review::approve(Some(&root), &chosen, false))
            };
            (busy, work)
        }
        Effect::ApproveAllHigh => (
            "Filing every high-confidence proposal…".into(),
            Box::new(move || crate::review::approve(Some(&root), &[], true)),
        ),
    };
    app.busy = Some(busy);
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = work().map(|r| r.text).map_err(|Failure(e)| e);
        let _ = send.send(result);
    });
    Some(receive)
}

/// Plans what the picker chose; if the picker couldn't run, says why and opens the built-in
/// browser instead. Returns the folder picked from, to start in next time.
fn picked_files(app: &mut App, picked: Result<Vec<PathBuf>, Failure>) -> Option<PathBuf> {
    match picked {
        Ok(paths) if paths.is_empty() => {
            app.message = Some("Nothing chosen".into());
            None
        }
        Ok(paths) => {
            let folder = paths[0].parent().filter(|f| f.is_dir()).map(Path::to_owned);
            if let Some(folder) = &folder {
                folder.clone_into(&mut app.start_folder);
            }
            app.plan(paths, carrel_core::ingest::IntakeOptions::default());
            folder
        }
        Err(Failure(e)) => {
            app.message = Some(format!("{e}; using the built-in browser"));
            app.open_browser();
            None
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
    use super::ui;

    fn copy_dir(from: &Path, to: &Path) {
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

    fn sample(with_history: bool) -> (tempfile::TempDir, App) {
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

    fn screen(app: &App, width: u16, height: u16) -> String {
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

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn status_shows_the_archive_what_waits_and_the_keys() {
        let (_dir, app) = sample(false);
        let text = screen(&app, 100, 30);
        assert!(
            text.starts_with("  1 Status   2 Review   3 Questions   4 Browse   5 History"),
            "{text}"
        );
        assert!(text.contains(" Sample archive "));
        assert!(
            text.contains("Waiting for you: 1 proposal (2 to review), 0 questions (3 to answer).")
        );
        assert!(text.contains("items in"), "the status report");
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
        app.key(key(KeyCode::Char('3')));
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
        assert_eq!(app.tab, Tab::Review);
        assert!(screen(&app, 100, 30).contains("Desk lamp diffuser (print file)"));

        let starts = ui::tab_starts();
        let click = |column| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        app.mouse(click(starts[4] + 2), &starts);
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
        app.key(key(KeyCode::Char('5')));
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
    fn browse_moves_through_collections_items_and_cards_and_searches() {
        let (_dir, mut app) = sample(false);
        app.key(key(KeyCode::Char('4')));
        assert_eq!(app.collections[app.browse_collection].path, "Holding");
        assert!(screen(&app, 120, 30).contains("Untitled scan"));
        app.key(key(KeyCode::Down));
        assert_eq!(app.collections[app.browse_collection].path, "Projects");
        app.key(key(KeyCode::Right));
        assert!(app.browse_in_items);
        assert_eq!(
            app.tab,
            Tab::Browse,
            "→ moves between panes here, not screens"
        );
        assert!(!app.card.is_empty(), "the chosen item's card");
        app.key(key(KeyCode::Left));
        assert!(!app.browse_in_items);

        app.key(key(KeyCode::Char('/')));
        for c in "oscillat".chars() {
            app.key(key(KeyCode::Char(c)));
        }
        assert!(screen(&app, 120, 30).contains("> oscillat"));
        app.key(key(KeyCode::Enter));
        let titles: Vec<&str> = app.browse_items.iter().map(|(_, t)| t.as_str()).collect();
        assert!(
            titles.contains(&"A note on coupled oscillators"),
            "{titles:?}"
        );
        assert!(titles.len() >= 2, "across collections: {titles:?}");
        assert!(screen(&app, 120, 30).contains("Found for “oscillat”"));
        app.key(key(KeyCode::Esc));
        assert!(app.search.is_none());
        assert!(!app.quit, "Esc clears the search first");
    }

    #[test]
    fn questions_are_answered_in_place_or_dismissed() {
        use carrel_core::questions::{QuestionEntry, QuestionKind, QuestionMarker, QuestionQueue};
        let (_dir, app) = sample(false);
        let proposal = app.queue[0].id;
        let queue = QuestionQueue::of(&app.archive);
        let question = |kind, subject: &str, question: &str, choices: Vec<String>| QuestionEntry {
            carrel: QuestionMarker::Question,
            format: carrel_core::FORMAT_VERSION,
            id: Id::new_v7(),
            batch: Id::new_v7(),
            created: carrel_core::card::DateOrTime("2026-10-30".into()),
            kind,
            subject: subject.into(),
            question: question.into(),
            items: vec![proposal],
            choices,
            extra: carrel_core::card::Extra::new(),
        };
        let term = question(QuestionKind::Term, "ABX", "What is “ABX”?", Vec::new());
        let mut place = question(
            QuestionKind::Collection,
            "lamp",
            "Which collection do items about “lamp” belong in?",
            vec!["Projects".into(), "Projects/Hardware".into()],
        );
        place.items.push(Id::new_v7());
        queue.save(&term).unwrap();
        queue.save(&place).unwrap();
        let mut app = App::new(app.archive.clone()).unwrap();
        app.key(key(KeyCode::Char('3')));
        let text = screen(&app, 160, 30);
        assert!(
            text.contains("Which collection do items about “lamp” belong in?  (2 items)"),
            "{text}"
        );
        assert_eq!(app.asked[0].id, place.id, "most items first");

        app.key(key(KeyCode::Enter));
        assert!(screen(&app, 120, 30).contains("They belong in (type to narrow)"));
        app.key(key(KeyCode::Down));
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::Answer {
                id: place.id,
                answer: "Projects/Hardware".into()
            })
        );

        app.key(key(KeyCode::Down));
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
                answer: "An assay kit.".into()
            })
        );
        app.key(key(KeyCode::Enter));
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            None,
            "an empty answer isn't sent"
        );
        assert_eq!(
            app.key(key(KeyCode::Char('d'))),
            Some(Effect::Dismiss(term.id))
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
        app.key(key(KeyCode::Char('7')));
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
}
