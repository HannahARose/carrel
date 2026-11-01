//! The crawler's findings in Review (`docs/design/restructure.md`, "In each interface"): what
//! it found could be better about items already filed, each with its reason and how sure it
//! was, approved or rejected as `carrel review approve` and `reject` do. `f` switches Review
//! between the ingest proposals, these and restructure proposals. Above them is how far the crawler has got, as
//! `carrel crawl status` says; `r` runs a pass now and `u` undoes this week's changes it made
//! on its own, as `carrel crawl run` and `carrel crawl undo 7d` do.

use std::collections::BTreeSet;

use carrel_core::card::Id;
use carrel_core::crawl::findings::CrawlEntry;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};

use super::app::{App, Confirm, Effect};
use super::review::Handled;
use super::ui::Theme;

/// Which of Review's lists is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReviewList {
    /// What ingest proposes to file.
    #[default]
    Proposals,
    /// What the crawler found could be better about items already filed.
    Crawler,
    /// Restructure proposals: sections and tags.
    Restructure,
}

impl ReviewList {
    /// The list `f` shows next.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Proposals => Self::Crawler,
            Self::Crawler => Self::Restructure,
            Self::Restructure => Self::Proposals,
        }
    }
}

/// The crawler's findings waiting in Review, and which are chosen and marked.
#[derive(Debug, Default)]
pub struct CrawlerReview {
    /// The findings, oldest first.
    pub entries: Vec<CrawlEntry>,
    /// The one chosen.
    pub chosen: usize,
    /// Those marked with `Space`, for a decision on several.
    pub marked: BTreeSet<Id>,
    /// How far the crawler has got, as lines.
    pub status: Vec<String>,
    /// The changes it made on its own this week, newest first.
    pub recent: usize,
}

/// How many days of the crawler's own changes `u` undoes, as Status lists them.
const RECENT_DAYS: i64 = 7;

impl App {
    /// Reads the crawler's findings again, keeping the choice and the marks still there.
    pub(super) fn load_crawler_findings(&mut self) {
        let entries = carrel_core::crawl::findings::list(&self.archive).unwrap_or_default();
        let crawler = &mut self.crawler;
        let chosen = crawler.entries.get(crawler.chosen).map(|e| e.id);
        crawler
            .marked
            .retain(|id| entries.iter().any(|e| e.id == *id));
        crawler.chosen = chosen
            .and_then(|id| entries.iter().position(|e| e.id == id))
            .unwrap_or(crawler.chosen)
            .min(entries.len().saturating_sub(1));
        crawler.entries = entries;
        let since = jiff::Timestamp::now().as_second() - RECENT_DAYS * 86_400;
        crawler.recent = carrel_core::librarian::changes::recent_by(
            &self.history,
            carrel_core::crawl::AGENT,
            since,
        )
        .len();
        crawler.status = status_lines(&self.archive);
    }

    /// `f` on Review: switches between the ingest proposals, the crawler's findings and
    /// restructure proposals; the keys of the last two.
    pub(super) fn review_list_key(&mut self, key: KeyEvent) -> Option<Handled> {
        if key.code == KeyCode::Char('f') {
            self.review_list = self.review_list.next();
            return Some(Handled::Yes(None));
        }
        match self.review_list {
            ReviewList::Proposals => None,
            ReviewList::Crawler => Some(self.crawler_key(key)),
            ReviewList::Restructure => Some(self.restructure_key(key)),
        }
    }

    /// A key while Review shows the crawler's findings.
    fn crawler_key(&mut self, key: KeyEvent) -> Handled {
        let crawler = &mut self.crawler;
        let targets: Vec<Id> = if crawler.marked.is_empty() {
            crawler
                .entries
                .get(crawler.chosen)
                .map(|e| e.id)
                .into_iter()
                .collect()
        } else {
            crawler.marked.iter().copied().collect()
        };
        match key.code {
            KeyCode::Up | KeyCode::Down => {
                let last = crawler.entries.len().saturating_sub(1);
                crawler.chosen = if key.code == KeyCode::Up {
                    crawler.chosen.saturating_sub(1)
                } else {
                    (crawler.chosen + 1).min(last)
                };
                Handled::Yes(None)
            }
            KeyCode::Char(' ') => {
                if let Some(entry) = crawler.entries.get(crawler.chosen) {
                    let id = entry.id;
                    if !crawler.marked.remove(&id) {
                        crawler.marked.insert(id);
                    }
                    crawler.chosen = (crawler.chosen + 1).min(crawler.entries.len() - 1);
                }
                Handled::Yes(None)
            }
            KeyCode::Char('a' | 'x') if targets.is_empty() => {
                self.message = Some("The crawler has found nothing to review".into());
                Handled::Yes(None)
            }
            KeyCode::Char('a') => Handled::Yes(Some(Effect::Decide {
                ids: targets,
                holding: false,
            })),
            KeyCode::Char('x') => {
                self.confirm = Some(Confirm::RejectFindings(targets));
                Handled::Yes(None)
            }
            KeyCode::Char('r') => Handled::Yes(Some(Effect::Restructure(
                super::review_restructure::RestructureAction::CrawlNow,
            ))),
            KeyCode::Char('u') if crawler.recent == 0 => {
                self.message = Some("The crawler made no changes of its own this week".into());
                Handled::Yes(None)
            }
            KeyCode::Char('u') => {
                self.confirm = Some(Confirm::UndoCrawler(crawler.recent));
                Handled::Yes(None)
            }
            KeyCode::Char('o') => {
                if let Some(card) = crawler.entries.get(crawler.chosen).map(|e| e.card.clone()) {
                    self.tab = super::app::Tab::Browse;
                    self.go_to(&card);
                }
                Handled::Yes(None)
            }
            _ => Handled::No,
        }
    }
}

/// Review's keys while it shows the crawler's findings.
#[must_use]
pub fn crawler_keys() -> Vec<(&'static str, &'static str)> {
    vec![
        ("a", "approve"),
        ("x", "reject"),
        ("Space", "mark"),
        ("o", "open the item"),
        ("r", "check some now"),
        ("u", "undo this week's"),
        ("f", "restructure"),
        ("?", "help"),
    ]
}

/// What waits in each of Review's lists, the one shown first, for the line above it. Empty
/// when only ingest proposals wait.
#[must_use]
pub fn lists_line(app: &App) -> String {
    let findings = app.crawler.entries.len();
    let restructures = app.restructure.proposals.len();
    let plural = |n: usize, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
    let shown = match app.review_list {
        ReviewList::Proposals if findings + restructures == 0 => return String::new(),
        ReviewList::Proposals => "Proposals to file",
        ReviewList::Crawler => "From the crawler",
        ReviewList::Restructure => "Restructure",
    };
    format!(
        "{shown}. f switches: {} to file, {} from the crawler, {} to restructure.",
        plural(app.queue.len(), "proposal"),
        plural(findings, "finding"),
        plural(restructures, "proposal")
    )
}

/// How far the crawler has got through `archive`, in words, as `carrel crawl status` says it.
fn status_lines(archive: &carrel_core::archive::Archive) -> Vec<String> {
    let device = crate::config::Identity::load_or_create().map(|i| i.device);
    let status = device.map_err(|e| e.to_string()).and_then(|device| {
        carrel_core::crawl::status(archive, device, jiff::Timestamp::now())
            .map_err(|e| e.to_string())
    });
    let status = match status {
        Ok(status) => status,
        Err(e) => return vec![e],
    };
    let mut lines = vec![format!(
        "Pace {}; it {} (crawler.changes, in Settings). Checked {} of {} items.",
        status.pace.as_str(),
        if status.changes {
            "makes the changes it's sure of itself"
        } else {
            "only suggests changes"
        },
        status.judged,
        status.items
    )];
    if !status.waiting.is_empty() {
        let due: Vec<String> = status
            .waiting
            .iter()
            .map(|(stage, count)| format!("{count} {}", stage.describe()))
            .collect();
        lines.push(format!("Due: {}.", due.join("; ")));
    }
    for (collection, failed, judged) in &status.misfits {
        lines.push(format!(
            "{collection}: {failed} of {judged} items checked don't fit."
        ));
    }
    lines.push(status.last_pass.map_or_else(
        || "No pass has run on this device yet.".to_owned(),
        |at| format!("Last pass on this device: {at}."),
    ));
    lines
}

/// The confirmation for undoing the crawler's `count` changes this week: its title, question,
/// detail and yes key.
#[must_use]
pub fn undo_crawler_confirm(count: usize) -> (&'static str, String, String, &'static str) {
    (
        " Undo ",
        format!(
            "Undo the {count} change{} the crawler made this week?",
            if count == 1 { "" } else { "s" }
        ),
        "Each is undone as a new change in History, newest first.".into(),
        " undo   ",
    )
}

/// The work for a pass of the crawler now, or with `undo` for undoing its changes this week,
/// on the archive at `root`.
pub(super) fn crawler_work(root: std::path::PathBuf, undo: bool) -> (String, super::Work) {
    if undo {
        (
            "Undoing the crawler's changes…".into(),
            Box::new(move || crate::crawl::undo(Some(&root), &format!("{RECENT_DAYS}d"))),
        )
    } else {
        (
            "The crawler is checking items…".into(),
            Box::new(move || crate::crawl::run(Some(&root), None)),
        )
    }
}

/// The confirmation for rejecting the findings `ids`: its title, question, detail and yes key.
#[must_use]
pub fn reject_findings_confirm(ids: &[Id]) -> (&'static str, String, String, &'static str) {
    (
        " Reject ",
        format!(
            "Reject {} of the crawler's finding{}?",
            ids.len(),
            if ids.len() == 1 { "" } else { "s" }
        ),
        "Nothing changes, and the crawler won't suggest them again.".into(),
        " reject   ",
    )
}

/// Draws the crawler's findings: the list beside the chosen one's reason.
pub fn draw_crawler_findings(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let crawler = &app.crawler;
    let mut header = vec![lists_line(app)];
    header.extend(crawler.status.iter().cloned());
    let height = u16::try_from(header.len() + 1).unwrap_or(u16::MAX);
    let [line_area, rest] =
        Layout::vertical([Constraint::Length(height), Constraint::Min(0)]).areas(area);
    frame.render_widget(
        Paragraph::new(header.join("\n")).wrap(Wrap { trim: false }),
        line_area,
    );
    if crawler.entries.is_empty() {
        frame.render_widget(
            Paragraph::new("The crawler has found nothing to change yet."),
            rest,
        );
        return;
    }
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(rest);
    let items: Vec<ListItem> = crawler
        .entries
        .iter()
        .map(|e| {
            let mark = if crawler.marked.contains(&e.id) {
                "■"
            } else {
                " "
            };
            ListItem::new(format!(
                "{mark} {} ({:.0}% sure)",
                e.finding.describe(&e.title),
                e.confidence * 100.0
            ))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(crawler.chosen));
    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.selected()),
        list_area,
        &mut state,
    );
    if let Some(entry) = crawler.entries.get(crawler.chosen) {
        let detail = format!(
            "{}\n\nWhy: {}\n\n{:.0}% sure, by {}, on {}.\n{}\n\nApproving makes the change as yours, \
             and History can undo it.",
            entry.finding.describe(&entry.title),
            entry.reason,
            entry.confidence * 100.0,
            entry.model,
            entry.created.get(..10).unwrap_or(&entry.created),
            entry.card
        );
        frame.render_widget(
            Paragraph::new(detail).wrap(Wrap { trim: false }),
            detail_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use carrel_core::crawl::findings::{CrawlEntry, Finding, propose};
    use ratatui::crossterm::event::KeyCode;

    use super::super::app::{Confirm, Effect, Tab};
    use super::super::tests::{key, sample, screen};
    use super::ReviewList;

    #[test]
    fn the_crawlers_findings_are_approved_and_rejected_in_review() {
        let (_dir, mut app) = sample(true);
        let items = app.archive.items().unwrap();
        let item = items
            .iter()
            .find(|i| i.card.starts_with("Projects/"))
            .unwrap();
        let finding = CrawlEntry::new(
            item,
            "the item",
            Finding::Move {
                from: "Projects".into(),
                to: "Reading".into(),
            },
            "It's a paper, not a project.",
            0.8,
            "scripted/model",
        );
        assert!(propose(&app.archive, &finding).unwrap());
        app.reload().unwrap();
        app.key(key(KeyCode::Char('2')));
        assert_eq!(app.tab, Tab::Review);
        assert!(
            screen(&app, 140, 30).contains("1 finding from the crawler"),
            "{}",
            screen(&app, 140, 30)
        );

        app.key(key(KeyCode::Char('f')));
        let text = screen(&app, 120, 30);
        assert!(
            text.contains("move “the item” to Reading (80% sure)"),
            "{text}"
        );
        assert!(text.contains("It's a paper, not a project."), "{text}");
        assert!(app.keys().contains(&("x", "reject")));
        assert!(text.contains("Pace steady"), "how far it has got: {text}");
        assert_eq!(
            app.key(key(KeyCode::Char('r'))),
            Some(Effect::Restructure(
                super::super::review_restructure::RestructureAction::CrawlNow
            ))
        );
        assert_eq!(app.key(key(KeyCode::Char('u'))), None);
        assert_eq!(
            app.message.as_deref(),
            Some("The crawler made no changes of its own this week")
        );
        assert_eq!(
            app.key(key(KeyCode::Char('a'))),
            Some(Effect::Decide {
                ids: vec![finding.id],
                holding: false,
            })
        );
        assert_eq!(app.key(key(KeyCode::Char('x'))), None);
        assert_eq!(app.confirm, Some(Confirm::RejectFindings(vec![finding.id])));
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::Reject(vec![finding.id]))
        );

        app.key(key(KeyCode::Char('f')));
        assert_eq!(
            app.review_list,
            ReviewList::Restructure,
            "f goes on to restructure"
        );
    }
}
