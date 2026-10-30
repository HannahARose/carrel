//! The interactive mode (`docs/design/interactive.md`): `carrel` in a terminal, or
//! `carrel tui`. Everything it does is also a command.

mod app;
mod ui;

use std::path::Path;
use std::time::Duration;

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
};
use ratatui::crossterm::execute;
use serde_json::Value;

use crate::commands::locate;
use crate::{Failure, Report};

pub use app::App;

/// Opens the interactive mode on the archive commands would use, and returns when the owner
/// quits. The terminal is restored however it ends.
pub fn run(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let mut app = App::new(archive)?;
    let theme = ui::Theme::from_env();
    // ratatui::init enters raw mode and the alternate screen, and restores both on a panic.
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture);
    let result = event_loop(&mut terminal, &mut app, theme);
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result.map(|()| Report::new(String::new(), Value::Null))
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    theme: ui::Theme,
) -> Result<(), Failure> {
    let starts = ui::tab_starts();
    while !app.quit {
        terminal
            .draw(|frame| ui::draw(frame, app, theme))
            .map_err(|e| Failure::new(format!("couldn't draw: {e}")))?;
        if !event::poll(Duration::from_millis(250))
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
        if let Some(effect) = effect {
            perform(app, effect);
        }
    }
    Ok(())
}

/// Carries out work that needs the writer lease, and says how it went.
fn perform(app: &mut App, effect: app::Effect) {
    let result = match effect {
        app::Effect::Undo(id) => undo(app, &id),
    };
    app.message = Some(match result {
        Ok(message) | Err(Failure(message)) => message,
    });
    if let Err(Failure(message)) = app.reload() {
        app.message = Some(message);
    }
}

fn undo(app: &App, id: &str) -> Result<String, Failure> {
    let report = crate::commands::undo(Some(app.archive.root()), id)?;
    Ok(report.text.trim_end().to_owned())
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
        assert!(screen(&app, 100, 30).contains("Projects  (3 items)"));
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
}
