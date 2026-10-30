//! Progress for long commands, on standard error so standard output keeps the report.
//!
//! On a terminal it's one line redrawn in place: what's being done, a bar, the count, about
//! how long is left, and the item being worked on. Written to a file or pipe, it's a plain
//! line each time a step reaches another tenth, so logs show how far a run got. With `--json`
//! it's one JSON line per update.

use std::fmt::Write as _;
use std::io::{IsTerminal as _, Write as _};
use std::time::{Duration, Instant};

use carrel_core::ingest::run::{Progress, Stage};
use serde_json::json;

/// The width of the bar, in characters.
const BAR: usize = 24;

/// The longest item name shown on a terminal.
const ITEM: usize = 40;

/// How progress is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Terminal,
    Log,
    Json,
}

/// A progress display for one long command.
pub struct Display {
    mode: Mode,
    /// Shown first, such as the model a benchmark is measuring.
    prefix: String,
    /// What's being counted now, such as an ingest stage.
    step: Option<String>,
    /// When this step started being shown, and its count then, for the time left.
    started: Instant,
    start_done: usize,
    /// The last tenth logged, in log mode.
    tenth: usize,
    drawn: bool,
}

impl Display {
    /// A display that writes in the way standard error and `json` call for.
    pub fn new(prefix: impl Into<String>, json: bool) -> Self {
        let mode = if json {
            Mode::Json
        } else if std::io::stderr().is_terminal() {
            Mode::Terminal
        } else {
            Mode::Log
        };
        Self::with_mode(mode, prefix.into())
    }

    fn with_mode(mode: Mode, prefix: String) -> Self {
        Self {
            mode,
            prefix,
            step: None,
            started: Instant::now(),
            start_done: 0,
            tenth: 0,
            drawn: false,
        }
    }

    /// Shows an ingest run's progress.
    pub fn update(&mut self, progress: Progress<'_>) {
        let step = format!(
            "[{}/{}] {}",
            progress.stage.number(),
            Stage::COUNT,
            progress.stage.describe()
        );
        self.count(&step, progress.done, progress.total, progress.item);
    }

    /// Shows that `done` of `total` are finished in `step`, and `item` is being worked on.
    pub fn count(&mut self, step: &str, done: usize, total: usize, item: Option<&str>) {
        if self.step.as_deref() != Some(step) {
            self.step = Some(step.to_owned());
            self.started = Instant::now();
            self.start_done = done;
            self.tenth = 0;
        }
        match self.mode {
            Mode::Json => eprintln!(
                "{}",
                json!({
                    "event": "progress",
                    "step": step,
                    "done": done,
                    "total": total,
                    "item": item,
                })
            ),
            Mode::Log => {
                let tenth = (done * 10).checked_div(total).unwrap_or(10);
                if done == self.start_done || tenth > self.tenth {
                    self.tenth = tenth;
                    eprintln!("{}", self.line(step, done, total, None));
                }
            }
            Mode::Terminal => {
                eprint!("\r\x1b[2K{}", self.line(step, done, total, item));
                let _ = std::io::stderr().flush();
                self.drawn = true;
            }
        }
    }

    /// Ends the display, leaving the terminal's cursor on a fresh line.
    pub fn finish(&mut self) {
        if self.drawn {
            eprintln!();
            self.drawn = false;
        }
    }

    fn line(&self, step: &str, done: usize, total: usize, item: Option<&str>) -> String {
        let mut line = String::new();
        if !self.prefix.is_empty() {
            line.push_str(&self.prefix);
            line.push_str(": ");
        }
        line.push_str(step);
        if self.mode == Mode::Terminal {
            let filled = (done * BAR).checked_div(total).unwrap_or(BAR).min(BAR);
            line.push(' ');
            line.push_str(&"█".repeat(filled));
            line.push_str(&"░".repeat(BAR - filled));
        }
        let _ = write!(line, " {done}/{total}");
        if let Some(left) = left(
            self.started.elapsed(),
            done.saturating_sub(self.start_done),
            total.saturating_sub(done),
        ) {
            let _ = write!(line, ", {left}");
        }
        if let Some(item) = item {
            let shown: String = item.chars().take(ITEM).collect();
            let more = if item.chars().count() > ITEM {
                "…"
            } else {
                ""
            };
            let _ = write!(line, "  {shown}{more}");
        }
        line
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        self.finish();
    }
}

/// About how long is left, given that `done` took `elapsed` and `remaining` are to go.
fn left(elapsed: Duration, done: usize, remaining: usize) -> Option<String> {
    if done == 0 || remaining == 0 {
        return None;
    }
    #[allow(clippy::cast_precision_loss)] // item counts
    let seconds = elapsed.as_secs_f64() / done as f64 * remaining as f64;
    Some(if seconds < 60.0 {
        "under a minute left".to_owned()
    } else if seconds < 3600.0 {
        format!("about {:.0} min left", (seconds / 60.0).ceil())
    } else {
        format!("about {:.1} h left", seconds / 3600.0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_left_reads_naturally() {
        let s = Duration::from_secs;
        assert_eq!(left(s(10), 0, 10), None);
        assert_eq!(left(s(10), 10, 0), None);
        assert_eq!(left(s(10), 5, 5).as_deref(), Some("under a minute left"));
        assert_eq!(left(s(100), 2, 8).as_deref(), Some("about 7 min left"));
        assert_eq!(left(s(3600), 1, 2).as_deref(), Some("about 2.0 h left"));
    }

    #[test]
    fn a_line_shows_the_step_bar_count_and_item() {
        let display = Display::with_mode(Mode::Terminal, "llama/qwen".into());
        let line = display.line("[2/4] Drafting cards", 6, 24, Some("receipt.txt"));
        assert!(
            line.starts_with("llama/qwen: [2/4] Drafting cards ██████░"),
            "{line}"
        );
        assert!(line.contains(" 6/24"), "{line}");
        assert!(line.ends_with("  receipt.txt"), "{line}");
    }

    #[test]
    fn logs_have_no_bar_or_item() {
        let display = Display::with_mode(Mode::Log, String::new());
        let line = display.line("items", 3, 10, None);
        assert!(line.starts_with("items 3/10"), "{line}");
        assert!(!line.contains('█'), "{line}");
    }
}
