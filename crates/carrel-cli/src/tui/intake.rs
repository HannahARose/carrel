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

    /// Handles a key on the Ingest screen.
    pub(super) fn intake_key(&mut self, key: KeyEvent) -> Handled {
        if let Some(plan) = self.planned.clone() {
            return Handled::Yes(match key.code {
                KeyCode::Enter if plan.count > 0 && plan.problem.is_none() => {
                    self.planned = None;
                    Some(Effect::Queue {
                        paths: plan.paths,
                        options: plan.options,
                    })
                }
                KeyCode::Enter => {
                    self.message = Some("Nothing to queue".into());
                    None
                }
                KeyCode::Char('e') => {
                    let each = !plan.options.each;
                    self.plan(plan.paths, IntakeOptions { each, whole: false });
                    None
                }
                KeyCode::Char('w') => {
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

/// Paths as typed, pasted or dropped from a file manager: separated by spaces, with quotes or
/// backslashes keeping spaces in a name, and `~` for the home folder.
#[must_use]
pub fn split_paths(text: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None | Some('"'), '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                    started = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    paths.push(expand(&std::mem::take(&mut current)));
                    started = false;
                }
            }
            (_, c) => {
                current.push(c);
                started = true;
            }
        }
    }
    if started {
        paths.push(expand(&current));
    }
    paths
}

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => carrel_core::local::home_dir()
            .map_or_else(
                || PathBuf::from(path),
                |home| home.join(rest.trim_start_matches('/')),
            ),
        _ => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::split_paths;
    use std::path::PathBuf;

    #[test]
    fn paths_are_split_as_a_shell_would() {
        assert_eq!(
            split_paths(r"/a/b.txt '/c/with space.pdf' /d/escaped\ name.md"),
            [
                PathBuf::from("/a/b.txt"),
                PathBuf::from("/c/with space.pdf"),
                PathBuf::from("/d/escaped name.md"),
            ]
        );
        assert_eq!(split_paths("  \"/x y\"  "), [PathBuf::from("/x y")]);
        assert_eq!(split_paths(""), Vec::<PathBuf>::new());
        let home = carrel_core::local::home_dir().unwrap();
        assert_eq!(split_paths("~/Downloads"), [home.join("Downloads")]);
    }
}
