//! The Ingest screen (`docs/design/tui-ingest.md`): choosing what to add, the plan before it's
//! queued, the queue itself, and the worker's progress on every screen.

use std::path::PathBuf;

use carrel_core::card::Id;
use carrel_core::ingest::IntakeOptions;
use carrel_core::intake::{IntakeEntry, preview, waiting};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::{App, Effect, Tab, step};
use super::review::Handled;
use super::screens::{Input, InputPurpose};

/// What the worker was doing when last looked at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Worker {
    /// Its status, while it's running and has written one.
    pub status: Option<crate::worker::Status>,
    /// Whether one is running.
    pub running: bool,
    /// How many items it had finished.
    pub done: usize,
}

/// What would be added, shown before anything is queued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The paths chosen.
    pub paths: Vec<PathBuf>,
    /// How folders among them are read.
    pub options: IntakeOptions,
    /// The plan in words: each item, and what's left out and why.
    pub lines: Vec<String>,
    /// How many items would be queued.
    pub count: usize,
    /// Why it can't be queued as it is, such as a folder that needs `e` or `w`.
    pub problem: Option<String>,
    /// A web address to add instead of paths, as a `url` item.
    pub url: Option<String>,
    /// The owner's note, for the drafts and the cards.
    pub note: Option<String>,
}

/// Whether `text` typed on the Ingest line is a web address rather than paths.
pub(super) fn is_web_address(text: &str) -> bool {
    let text = text.trim().to_ascii_lowercase();
    !text.contains(char::is_whitespace)
        && (text.starts_with("https://") || text.starts_with("http://"))
}

impl App {
    /// Reads this device's queue and the worker's status.
    pub(super) fn load_intake(&mut self) {
        self.intake = self
            .device
            .and_then(|device| waiting(&self.archive, device, true).ok())
            .unwrap_or_default();
        self.intake_chosen = self.intake_chosen.min(self.intake.len().saturating_sub(1));
        self.poll_worker();
    }

    /// Reads the worker's status, returning whether it has finished items since last time,
    /// so the queues should be read again.
    pub fn poll_worker(&mut self) -> bool {
        let status = crate::worker::status(&self.archive);
        let running = crate::worker::running(&self.archive).unwrap_or(false);
        let done = status.as_ref().map_or(0, |s| s.done);
        let changed = done != self.worker.done || running != self.worker.running;
        self.worker = Worker {
            status,
            running,
            done,
        };
        changed
    }

    /// The worker's activity, for the status line, while it's running.
    pub fn worker_line(&self) -> Option<String> {
        if !self.worker.running {
            return None;
        }
        Some(match &self.worker.status {
            Some(status) => {
                let stage = status
                    .stage
                    .as_deref()
                    .map(|s| format!(" ({s})"))
                    .unwrap_or_default();
                format!(
                    "Drafting {}{stage}, {} waiting",
                    status.item.as_deref().unwrap_or("…"),
                    status.waiting
                )
            }
            None => "Starting the worker…".to_owned(),
        })
    }

    /// Starts choosing what to ingest: the owner's file picker, or the built-in browser.
    pub(super) fn start_ingest(&mut self) -> Option<Effect> {
        self.picking = super::app::Picking::Ingest;
        self.start_picker()
    }

    /// Opens the owner's file picker, or the built-in browser, for what `picking` says.
    pub(super) fn start_picker(&mut self) -> Option<Effect> {
        match crate::picker::choose(self.picker_preference.as_deref()) {
            crate::picker::Choice::Command(words) => Some(Effect::Pick(words)),
            crate::picker::Choice::BuiltIn => {
                if self.picker_preference.is_none()
                    && let Some(hint) = crate::picker::KNOWN
                        .iter()
                        .find_map(|k| k.install_here().map(|i| (k.name, i)))
                {
                    self.message = Some(format!(
                        "No file picker found, so using the built-in one; {} is a good one ({})",
                        hint.0, hint.1
                    ));
                }
                self.open_browser();
                None
            }
        }
    }

    /// Opens the built-in browser where picking starts.
    pub(super) fn open_browser(&mut self) {
        self.browser = Some(super::browser::Browser::open(self.start_folder.clone()));
    }

    /// Makes `folder` where picking files for ingest starts next time, here and in this
    /// device's preferences. Folders picked for backups aren't remembered: they'd send the
    /// next ingest to the backup drive.
    pub(super) fn remember_folder(&mut self, folder: PathBuf) {
        if self.picking != super::app::Picking::Ingest || !folder.is_dir() {
            return;
        }
        if self.preferences == super::app::Preferences::Saved
            && let Ok(mut prefs) = crate::config::Prefs::load()
        {
            prefs.last_folder = Some(folder.clone());
            let _ = prefs.save();
        }
        self.start_folder = folder;
    }

    /// What was picked goes where `picking` says: an ingest plan, backup setup, a checkout's
    /// folder, or the desk.
    pub(super) fn picked(&mut self, paths: Vec<PathBuf>) {
        match self.picking {
            super::app::Picking::BackupFolder => {
                self.picking = super::app::Picking::Ingest;
                self.backup_folder_chosen(&paths);
            }
            super::app::Picking::CheckoutFolder | super::app::Picking::DeskItem => {
                self.picked_path(&paths);
            }
            super::app::Picking::Ingest => self.plan(paths, IntakeOptions::default()),
        }
    }

    /// Opens the line to type or paste paths into.
    pub(super) fn ask_for_paths(&mut self) {
        self.input = Some(Input {
            purpose: InputPurpose::Ingest,
            text: String::new(),
        });
    }

    /// Plans `paths` with `options`, without running a model, and shows the plan on the
    /// Ingest screen.
    pub(super) fn plan(&mut self, paths: Vec<PathBuf>, options: IntakeOptions) {
        self.tab = Tab::Ingest;
        let Some(device) = self.device else {
            self.message = Some("This device has no identity yet; run `carrel init` first".into());
            return;
        };
        let mut plan = Plan {
            paths,
            options,
            lines: Vec::new(),
            count: 0,
            problem: None,
            url: None,
            note: self.planned.as_ref().and_then(|p| p.note.clone()),
        };
        match preview(&self.archive, &plan.paths, options, device) {
            Ok(queued) => {
                plan.count = queued.added.len();
                for entry in &queued.added {
                    plan.lines.push(format!("  {}", entry.path.display()));
                }
                if !queued.skipped.is_empty() {
                    plan.lines.push("Left out:".into());
                    for skipped in &queued.skipped {
                        plan.lines.push(format!(
                            "  {}: {}",
                            skipped.path.display(),
                            skipped.reason
                        ));
                    }
                }
            }
            Err(e) => plan.problem = Some(e.to_string()),
        }
        self.planned = Some(plan);
    }

    /// Plans the web address `url`, to be filed as a `url` item drafted from the address
    /// alone, and shows the plan on the Ingest screen.
    pub(super) fn plan_url(&mut self, url: &str) {
        self.tab = Tab::Ingest;
        self.planned = Some(Plan {
            paths: Vec::new(),
            options: IntakeOptions::default(),
            lines: vec![format!("  {url}, as a web address; nothing is fetched")],
            count: 1,
            problem: None,
            url: Some(url.to_owned()),
            note: None,
        });
    }

    /// Sets the note on the plan shown, from what the owner typed.
    pub(super) fn noted(&mut self, text: &str) {
        if let Some(plan) = &mut self.planned {
            plan.note = Some(text.trim().to_owned()).filter(|t| !t.is_empty());
        }
    }

    /// Handles a key on the Ingest screen.
    pub(super) fn intake_key(&mut self, key: KeyEvent) -> Handled {
        if let Some(plan) = self.planned.clone() {
            return Handled::Yes(match key.code {
                KeyCode::Enter if plan.count > 0 && plan.problem.is_none() => {
                    self.planned = None;
                    Some(Effect::Queue {
                        paths: plan.paths,
                        options: plan.options,
                        url: plan.url,
                        note: plan.note,
                    })
                }
                KeyCode::Enter => {
                    self.message = Some("Nothing to queue".into());
                    None
                }
                KeyCode::Char('n') => {
                    self.input = Some(Input {
                        purpose: InputPurpose::IngestNote,
                        text: plan.note.clone().unwrap_or_default(),
                    });
                    None
                }
                KeyCode::Char('e') if plan.url.is_none() => {
                    let each = !plan.options.each;
                    self.plan(plan.paths, IntakeOptions { each, whole: false });
                    None
                }
                KeyCode::Char('w') if plan.url.is_none() => {
                    let whole = !plan.options.whole;
                    self.plan(plan.paths, IntakeOptions { each: false, whole });
                    None
                }
                KeyCode::Esc => {
                    self.planned = None;
                    self.message = Some("Nothing queued".into());
                    None
                }
                _ => return Handled::No,
            });
        }
        let chosen = self.intake.get(self.intake_chosen).map(|e| e.id);
        let effect = match (key.code, chosen) {
            (KeyCode::Char('x'), Some(id)) => Effect::Unqueue(id),
            (KeyCode::Char('t'), Some(id)) => Effect::Front(id),
            (KeyCode::Char('r'), Some(id)) => Effect::Retry(id),
            _ => return Handled::No,
        };
        Handled::Yes(Some(effect))
    }

    /// Moves the choice in the queue.
    pub(super) fn intake_move(&mut self, delta: i32) {
        self.intake_chosen = step(self.intake_chosen, delta, self.intake.len());
    }

    /// The queue's entries split as the screen shows them: waiting, then failed.
    pub fn intake_lines(&self) -> Vec<(Id, String)> {
        self.intake
            .iter()
            .map(|entry| (entry.id, describe(entry)))
            .collect()
    }
}

fn describe(entry: &IntakeEntry) -> String {
    let name = entry.path.file_name().map_or_else(
        || entry.path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    match &entry.failure {
        Some(failure) if entry.stuck() => format!("{name}  failed: {failure}"),
        Some(failure) => format!("{name}  (trying again: {failure})"),
        None => name,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::KeyCode;

    use super::super::app::{App, Effect};
    use super::super::tests::{key, sample, screen};

    fn typed(app: &mut App, text: &str) -> Option<Effect> {
        for c in text.chars() {
            app.key(key(KeyCode::Char(c)));
        }
        app.key(key(KeyCode::Enter))
    }

    #[test]
    fn a_web_address_is_planned_as_one_item_and_n_adds_a_note() {
        let (_dir, mut app) = sample(false);
        assert!(super::is_web_address(" https://example.org/a "));
        assert!(!super::is_web_address("/Users/me/https://x"));
        assert!(!super::is_web_address("https://a b"));

        app.ask_for_paths();
        assert_eq!(typed(&mut app, "https://example.org/prc"), None);
        let text = screen(&app, 120, 30);
        assert!(
            text.contains("https://example.org/prc, as a web address"),
            "{text}"
        );
        assert_eq!(app.key(key(KeyCode::Char('e'))), None, "e is for folders");
        assert!(!app.planned.as_ref().unwrap().options.each);

        app.key(key(KeyCode::Char('n')));
        assert_eq!(typed(&mut app, "For chapter two"), None);
        assert!(screen(&app, 120, 30).contains("Note: For chapter two"));
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::Queue {
                paths: Vec::new(),
                options: carrel_core::ingest::IntakeOptions::default(),
                url: Some("https://example.org/prc".into()),
                note: Some("For chapter two".into()),
            })
        );
    }
}
