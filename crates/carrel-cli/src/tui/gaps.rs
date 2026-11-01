//! The Gaps view on the interactive mode's Status (#455): what Carrel couldn't do on this
//! device, as `carrel report gaps` lists it. `g` opens it; `r` drafts an issue for the chosen
//! gap, with any words the owner adds, and opens it in the browser to read, edit and submit
//! (Carrel never sends it); `l` records the issue's address once it's submitted; `m` reports a
//! missing feature in the owner's own words. On an item in Browse, `M` does that for the item,
//! recording its type, never its name.

use std::path::PathBuf;

use carrel_core::gaps::GapLog;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};

use super::app::App;
use super::review::Handled;
use super::screens::{Input, InputPurpose};
use super::ui::Theme;

/// The Gaps view's state.
#[derive(Debug, Default)]
pub struct GapsView {
    /// Whether Status shows the gaps rather than its report.
    pub shown: bool,
    /// This device's gaps.
    pub log: GapLog,
    /// The chosen gap.
    pub chosen: usize,
    /// The configuration folder the log is in: this device's, except in tests.
    pub config: Option<PathBuf>,
    /// The last draft's address, once it's been opened.
    pub opened: Option<String>,
}

/// What a typed line about gaps is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GapInput {
    /// Words of the owner's own to add to gap `number`'s issue draft, or nothing.
    Add {
        /// The gap.
        number: u32,
    },
    /// The address of the issue gap `number` was reported in.
    Reported {
        /// The gap.
        number: u32,
    },
    /// A missing feature, in the owner's words, about the item with this card, if any.
    Missing {
        /// The item's card path.
        card: Option<String>,
    },
}

impl GapInput {
    /// What to type, for the help line.
    pub fn prompt(&self) -> &'static str {
        match self {
            Self::Add { .. } => "anything to add to the draft, or nothing",
            Self::Reported { .. } => "the issue's address, or nothing to clear it",
            Self::Missing { .. } => "what Carrel should be able to do",
        }
    }

    /// The pop-up's title and the line above what's typed.
    pub fn text(&self) -> (&'static str, String) {
        match self {
            Self::Add { number } => (
                " Report ",
                format!(
                    "Gap {number}'s issue draft opens in your browser, to read, edit and submit there; \
                     Carrel sends nothing. Anything to add to it, in your own words?"
                ),
            ),
            Self::Reported { number } => (
                " Reported ",
                format!("The address of the issue gap {number} was reported in:"),
            ),
            Self::Missing { card } => (
                " Missing feature ",
                if card.is_some() {
                    "What should Carrel be able to do with this item? Its type is recorded with \
                     your words, never its name:"
                        .to_owned()
                } else {
                    "What should Carrel be able to do? Your words are recorded as a gap, to report \
                     to development:"
                        .to_owned()
                },
            ),
        }
    }
}

impl App {
    /// Reads this device's gaps again.
    pub(super) fn load_gaps(&mut self) {
        if self.gaps.config.is_none() && !cfg!(test) {
            self.gaps.config = carrel_core::local::config_dir();
        }
        self.gaps.log = self
            .gaps
            .config
            .as_deref()
            .map(carrel_core::gaps::list_in)
            .unwrap_or_default();
        self.gaps.chosen = self
            .gaps
            .chosen
            .min(self.gaps.log.gaps.len().saturating_sub(1));
    }

    /// The line Status shows about the gaps.
    pub(super) fn gaps_status_lines(&self) -> Vec<String> {
        let count = self.gaps.log.gaps.len();
        if count == 0 {
            return Vec::new();
        }
        vec![
            String::new(),
            format!(
                "Gaps: {} Carrel couldn't do on this device (g lists them, to report to development).",
                crate::commands::plural(count, "thing")
            ),
        ]
    }

    /// Status's keys, with the Gaps view's when it's shown: `g` opens and closes it.
    pub(super) fn status_or_gaps_key(&mut self, key: KeyEvent) -> Handled {
        if !self.gaps.shown {
            if key.code == KeyCode::Char('g') {
                self.load_gaps();
                self.gaps.shown = true;
                return Handled::Yes(None);
            }
            return self.status_key(key);
        }
        let chosen = self.gaps.log.gaps.get(self.gaps.chosen).map(|g| g.number);
        match key.code {
            KeyCode::Esc | KeyCode::Char('g') => self.gaps.shown = false,
            KeyCode::Down | KeyCode::Char('j') => {
                self.gaps.chosen =
                    (self.gaps.chosen + 1).min(self.gaps.log.gaps.len().saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.gaps.chosen = self.gaps.chosen.saturating_sub(1);
            }
            KeyCode::Char('r') if chosen.is_some() => {
                self.gap_input(GapInput::Add {
                    number: chosen.unwrap_or_default(),
                });
            }
            KeyCode::Char('l') if chosen.is_some() => {
                self.gap_input(GapInput::Reported {
                    number: chosen.unwrap_or_default(),
                });
            }
            KeyCode::Char('m') => self.gap_input(GapInput::Missing { card: None }),
            _ => return Handled::No,
        }
        Handled::Yes(None)
    }

    /// Asks for a line about gaps.
    pub(super) fn gap_input(&mut self, purpose: GapInput) {
        self.input = Some(Input {
            purpose: InputPurpose::Gap(purpose),
            text: String::new(),
        });
    }

    /// The Gaps view's keys, for the help line.
    pub(super) fn gaps_keys(&self) -> Vec<(&'static str, &'static str)> {
        let mut keys = vec![("Esc", "back")];
        if !self.gaps.log.gaps.is_empty() {
            keys.extend([("↑↓", "choose"), ("r", "report…"), ("l", "reported at")]);
        }
        keys.push(("m", "missing feature"));
        keys
    }

    /// Does what the line typed for `purpose`, `text`, asks.
    pub(super) fn gap_typed(&mut self, purpose: GapInput, text: &str) {
        let Some(config) = self
            .gaps
            .config
            .clone()
            .or_else(|| (!cfg!(test)).then(carrel_core::local::config_dir).flatten())
        else {
            self.message = Some("There's no configuration folder for the gap log".into());
            return;
        };
        self.message = Some(match purpose {
            GapInput::Add { number } => self.open_gap_draft(number, text),
            GapInput::Reported { number } => {
                match carrel_core::gaps::mark_reported_in(&config, number, text) {
                    Ok(gap) if gap.reported.is_some() => {
                        format!("Gap {number} is marked as reported")
                    }
                    Ok(_) => format!("Gap {number} is no longer marked as reported"),
                    Err(e) => e.to_string(),
                }
            }
            GapInput::Missing { card } => {
                let items = card
                    .as_ref()
                    .and_then(|_| self.archive.items().ok())
                    .unwrap_or_default();
                let item = card
                    .as_ref()
                    .and_then(|card| items.iter().find(|i| i.card == *card));
                match carrel_core::gaps::report_missing_feature_in(
                    &config,
                    Some(&self.archive),
                    item,
                    text,
                ) {
                    Ok(gap) => format!(
                        "Recorded gap {}; Status's Gaps (g) reports it to development",
                        gap.number
                    ),
                    Err(e) => e.to_string(),
                }
            }
        });
        self.load_gaps();
    }

    /// Opens gap `number`'s issue draft, with the owner's `added` words, in the browser. In
    /// tests it's only kept.
    fn open_gap_draft(&mut self, number: u32, added: &str) -> String {
        let Some(gap) = self.gaps.log.get(number) else {
            return format!("There's no gap {number}");
        };
        let url = crate::report_gaps::issue_draft(gap, Some(added)).url();
        let opened = if cfg!(test) {
            Ok(())
        } else {
            carrel_core::gaps::draft::open_in_browser(&url)
        };
        self.gaps.opened = Some(url.clone());
        match opened {
            Ok(()) => format!(
                "Opened gap {number}'s issue draft in your browser: submit it there, then l records its address"
            ),
            Err(e) => format!("Couldn't open the browser ({e}); the draft is at {url}"),
        }
    }
}

/// The Gaps view: the gaps on the left, and on the right the chosen one and what its issue
/// draft holds, so the owner sees what would be reported.
pub fn draw_gaps(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let gaps = &app.gaps;
    if gaps.log.gaps.is_empty() {
        frame.render_widget(
            Paragraph::new(
                "No gaps: Carrel has done all it was asked on this device so far.\n\n\
                 m reports a missing feature in your own words.",
            )
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);
    let items: Vec<ListItem> = gaps
        .log
        .gaps
        .iter()
        .map(|g| {
            let mark = if g.reported.is_some() { "✓" } else { " " };
            ListItem::new(format!("{mark}{:>3}  {}", g.number, g.summary()))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(gaps.chosen));
    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.selected()),
        list_area,
        &mut state,
    );
    if let Some(gap) = gaps.log.gaps.get(gaps.chosen) {
        let lines = crate::report_gaps::gap_lines(&GapLog {
            gaps: vec![gap.clone()],
            ..GapLog::default()
        });
        let draft = crate::report_gaps::issue_draft(gap, None);
        let detail = format!(
            "{}\n\nReport… (r) opens this issue draft in your browser, to edit and submit there; \
             Carrel sends nothing:\n\n{}\n\n{}",
            lines[1..].join("\n").trim_start(),
            draft.title,
            draft.body
        );
        frame.render_widget(
            Paragraph::new(detail).wrap(Wrap { trim: false }),
            detail_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use carrel_core::gaps::{GapKind, Sighting};
    use ratatui::crossterm::event::KeyCode;

    use super::super::app::Tab;
    use super::super::tests::{key, sample, screen};

    fn typed(app: &mut super::App, text: &str) {
        for c in text.chars() {
            app.key(key(KeyCode::Char(c)));
        }
        app.key(key(KeyCode::Enter));
    }

    #[test]
    fn status_lists_the_gaps_and_drafts_an_issue_carrel_never_sends() {
        let (dir, mut app) = sample(false);
        let config = dir.path().join("config");
        let mut log = carrel_core::gaps::GapLog::default();
        log.note(
            &Sighting::new(GapKind::UnreadFormat, "model/3mf", "3mf", "text extraction"),
            4,
            "2026-10-30",
            "0.6.0",
        );
        log.save(&carrel_core::gaps::file_in(&config)).unwrap();
        app.gaps.config = Some(config.clone());
        app.tab = Tab::Status;

        app.key(key(KeyCode::Char('g')));
        let shown = screen(&app, 140, 40);
        assert!(
            shown.contains("no reader for the file type: .3mf"),
            "{shown}"
        );
        assert!(shown.contains("r report…"), "{shown}");

        app.key(key(KeyCode::Char('r')));
        typed(&mut app, "Bambu plates");
        let url = app.gaps.opened.clone().expect("a draft");
        assert!(url.contains("Bambu%20plates"), "{url}");
        assert!(!url.contains("archive"), "{url}");

        app.key(key(KeyCode::Char('l')));
        typed(&mut app, "https://github.com/Horizon-Labs/carrel/issues/7");
        assert_eq!(
            app.gaps.log.gaps[0].reported.as_deref(),
            Some("https://github.com/Horizon-Labs/carrel/issues/7")
        );
        app.key(key(KeyCode::Esc));
        assert!(!app.gaps.shown);
    }

    #[test]
    fn a_missing_feature_on_an_item_records_its_type_and_the_owners_words() {
        let (dir, mut app) = sample(false);
        let config = dir.path().join("config");
        app.gaps.config = Some(config.clone());
        app.gap_typed(
            super::GapInput::Missing {
                card: Some("Reading/Coupled oscillators note.pdf.md".into()),
            },
            "Read its equations",
        );
        let gap = &app.gaps.log.gaps[0];
        assert_eq!(gap.kind, GapKind::OwnerReported);
        assert_eq!(gap.extension, "pdf", "{gap:?}");
        let saved = std::fs::read_to_string(carrel_core::gaps::file_in(&config)).unwrap();
        assert!(!saved.contains("oscillators"), "{saved}");
    }
}
