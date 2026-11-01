//! Drawing the interactive mode from its state.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use carrel_core::card::Sensitivity;
use carrel_core::review::{ReviewEntry, State};
use carrel_core::search::{Found, Hit};

use super::app::{App, Confirm, Input, Picker, Tab};
use super::item::{Pane, Relation, Row};
use super::model_changes::{download_confirm, model_change_confirm};
use super::screens::InputPurpose;

/// The smallest terminal the interactive mode lays out in.
pub const MIN_SIZE: (u16, u16) = (80, 24);

/// Styles that never carry meaning by color alone, and drop color under `NO_COLOR`.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    color: bool,
}

impl Theme {
    /// Color unless `NO_COLOR` is set (<https://no-color.org>).
    pub fn from_env() -> Self {
        Self {
            color: std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()),
        }
    }

    /// No color at all, as in tests.
    #[cfg(test)]
    pub fn plain() -> Self {
        Self { color: false }
    }

    pub(super) fn selected(self) -> Style {
        let style = Style::new().add_modifier(Modifier::REVERSED);
        if self.color {
            style.fg(Color::Cyan)
        } else {
            style
        }
    }

    pub(super) fn key(self) -> Style {
        let style = Style::new().add_modifier(Modifier::BOLD);
        if self.color {
            style.fg(Color::Yellow)
        } else {
            style
        }
    }

    pub(super) fn dim(self) -> Style {
        if self.color {
            Style::new().fg(Color::DarkGray)
        } else {
            Style::new()
        }
    }
}

/// Where each tab's title starts on the top line, for mouse clicks: after a leading space,
/// each is drawn as ` N Title ` followed by a space.
pub fn tab_starts() -> Vec<u16> {
    let mut starts = Vec::new();
    let mut x = 1u16;
    for tab in Tab::ALL {
        starts.push(x);
        x += u16::try_from(tab.title().len() + 5).unwrap_or(0);
    }
    starts
}

/// Draws the whole screen.
pub fn draw(frame: &mut Frame, app: &App, theme: Theme) {
    let area = frame.area();
    // Browse's item view leaves room for a picture again if it shows one.
    app.previews.room.set(None);
    if area.width < MIN_SIZE.0 || area.height < MIN_SIZE.1 {
        let text = format!(
            "Carrel needs a terminal of at least {}×{}; this one is {}×{}.\nMake it larger, or press q to quit.",
            MIN_SIZE.0, MIN_SIZE.1, area.width, area.height
        );
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
        return;
    }
    let [top, body, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_tabs(frame, app, theme, top);
    let block = Block::bordered().title(format!(" {} ", app.archive.doc().name));
    let inner = block.inner(body);
    frame.render_widget(block, body);
    match app.tab {
        Tab::History => draw_history(frame, app, theme, inner),
        Tab::Review => draw_review(frame, app, theme, inner),
        Tab::Active => draw_active(frame, app, theme, inner),
        Tab::Questions => draw_questions(frame, app, theme, inner),
        Tab::Browse => draw_browse(frame, app, theme, inner),
        Tab::Status => draw_report(frame, app, inner, Some(waiting_line(app))),
        Tab::Ingest => draw_ingest(frame, app, theme, inner),
        Tab::Settings => draw_settings(frame, app, theme, inner),
        Tab::Backups => draw_backups(frame, app, theme, inner),
        Tab::Librarian => super::ui_discovery::draw_librarian(frame, app, theme, inner),
        Tab::Discover => super::ui_discovery::draw_discover(frame, app, theme, inner),
    }
    draw_help_line(frame, app, theme, bottom);
    if app.help {
        draw_help(frame, theme, area);
    }
    if let Some(picker) = &app.picker {
        draw_picker(frame, picker, theme, area);
    }
    if let Some(browser) = &app.browser {
        draw_browser(frame, browser, theme, area);
    }
    if let Some(input) = &app.input {
        draw_input(frame, input, theme, area);
    }
    if let Some(correction) = &app.correction {
        draw_correction(frame, correction, theme, area);
    }
    if let Some(confirm) = &app.confirm {
        draw_confirm(frame, confirm, theme, area);
    }
}

fn draw_tabs(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let mut spans = vec![Span::raw(" ")];
    for tab in Tab::ALL {
        let title = format!("{} {}", tab.key(), tab.title());
        let style = if tab == app.tab {
            theme.selected()
        } else {
            Style::new()
        };
        spans.push(Span::styled(format!(" {title} "), style));
        spans.push(Span::raw(" "));
    }
    frame.render_widget(Line::from(spans), area);
}

fn waiting_line(app: &App) -> String {
    let (proposals, questions) = app.waiting;
    match (proposals, questions) {
        (0, 0) => "Nothing is waiting for you.".into(),
        // Each tab's key is its place in the row of tabs.
        _ => format!(
            "Waiting for you: {proposals} proposal{} ({} to review), {questions} question{} ({} to answer).",
            if proposals == 1 { "" } else { "s" },
            Tab::Review.index() + 1,
            if questions == 1 { "" } else { "s" },
            Tab::Questions.index() + 1,
        ),
    }
}

fn draw_report(frame: &mut Frame, app: &App, area: Rect, first: Option<String>) {
    let mut lines: Vec<Line> = Vec::new();
    if let Some(first) = first {
        lines.push(Line::from(first));
        lines.push(Line::from(""));
    }
    lines.extend(app.report().iter().map(|l| Line::from(l.as_str())));
    let scroll = app.scroll[Tab::ALL.iter().position(|t| *t == app.tab).unwrap_or(0)];
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), area);
}

fn draw_history(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    if app.history.is_empty() {
        frame.render_widget(Paragraph::new("No changes recorded"), area);
        return;
    }
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);
    let items: Vec<ListItem> = app
        .history
        .iter()
        .map(|e| {
            let operation = e.operation.map_or("?", |o| o.as_str());
            ListItem::new(format!(
                "{}  {:<11} {}",
                crate::commands::local_time(e.time),
                operation,
                e.summary
            ))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(app.selected));
    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.selected()),
        list_area,
        &mut state,
    );
    let entry = &app.history[app.selected];
    let mut lines = vec![
        Line::from(entry.summary.clone()).style(Style::new().add_modifier(Modifier::BOLD)),
        Line::from(format!("change {}", &entry.id[..entry.id.len().min(12)])),
    ];
    if let Some(snapshot) = &entry.snapshot {
        lines.push(Line::from(format!("snapshot {snapshot}")));
    }
    if !entry.items.is_empty() {
        lines.push(Line::from(format!("{} item(s)", entry.items.len())));
    }
    for m in &entry.moves {
        lines.push(Line::from(format!("moved {} → {}", m.from, m.to)));
    }
    if let Some(reverts) = &entry.reverts {
        lines.push(Line::from(format!(
            "undoes {}",
            &reverts[..reverts.len().min(12)]
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("u to undo it", theme.dim())));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::new().borders(ratatui::widgets::Borders::LEFT)),
        detail_area,
    );
}

fn draw_help_line(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let mut spans = Vec::new();
    if let Some(busy) = app.busy_line() {
        spans.push(Span::styled(format!(" {busy}  ·"), theme.key()));
    }
    if let Some(worker) = app.worker_line() {
        spans.push(Span::styled(format!(" {worker}  ·"), theme.key()));
    }
    if let Some(note) = app.marked_note().or_else(|| app.active_marked_note()) {
        spans.push(Span::raw(format!(" {note}  ·")));
    }
    if let Some(message) = &app.message {
        spans.push(Span::raw(format!(" {message}  ·")));
    }
    for (key, what) in app.keys() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(key, theme.key()));
        spans.push(Span::raw(format!(" {what} ")));
    }
    frame.render_widget(Line::from(spans), area);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

/// Every key, for `?` help.
const HELP_ROWS: &[(&str, &str)] = &[
    (
        "1 to 9, L, D, Tab, ←→",
        "switch screens; L the librarian, D discover",
    ),
    (
        "i",
        "add files and folders to ingest, with your file picker or the built-in one",
    ),
    ("↑↓, j k, PgUp PgDn, Home End", "scroll, or choose a change"),
    ("a  h", "approve, or hold in Holding (Review)"),
    (
        "m  e  c  x",
        "move, edit, correct in words, or reject (Review)",
    ),
    (
        "v  b  s  [ ]",
        "a duplicate: file as a version, keep both, skip; other matches (Review)",
    ),
    (
        "z",
        "unpack a zip when it's approved, or keep it compressed (Review)",
    ),
    ("d", "draft again with the models here now (Review)"),
    (
        "Space  A",
        "mark several; approve all high-confidence (Review)",
    ),
    ("r  o  x", "return, open, cancel checkout (Active)"),
    ("m", "return a copy that moved (Active)"),
    (
        "n  x  f",
        "put a file or folder on the desk; take off, archive a desk item (Active)",
    ),
    (
        "a  e  c  d  Space",
        "approve, edit, combine, drop; mark (Active)",
    ),
    (
        "Enter  1-9  s  d",
        "answer, pick a suggestion, skip or dismiss a question (Questions)",
    ),
    (
        "←→  e  c  /  Esc",
        "collections, items, an item; edit, correct; search; clear (Browse)",
    ),
    (
        "u  Enter",
        "undo the item's chosen change; go to a related item (Browse)",
    ),
    (
        "o  r  z  n",
        "check out, rename, expand an item; new collection (Browse)",
    ),
    (
        "a",
        "answer Carrel's question about the item (Browse, Librarian, wander)",
    ),
    ("A  w", "its questions; wander from it (Browse)"),
    ("u", "undo the chosen change (History)"),
    (
        "Enter  s  n  y",
        "ask; save to the Journal; new conversation; make a suggestion (Librarian)",
    ),
    (
        "u  U  o",
        "undo the librarian's change, or all this week's; open the item (Librarian)",
    ),
    (
        "d  w  t  m  h",
        "on this day, wander, timeline, map, activity (Discover)",
    ),
    (
        "Space  n  p",
        "next step, new walk (wander); periods (timeline)",
    ),
    ("/  b  r", "timeline words, date; activity days"),
    (
        "n  v  r  s",
        "snapshot now, verify, restore an item, set up (Backups)",
    ),
    ("x  t  r", "remove, move to the front, retry (Ingest)"),
    (
        "e  w",
        "each entry, or whole folder, before queueing (Ingest)",
    ),
    (
        "Enter",
        "change a setting; rebuild the search index or switch its model; install the agent (Settings)",
    ),
    ("x", "uninstall the background agent, on its row (Settings)"),
    ("Ctrl-O", "open another archive, or create one"),
    ("Ctrl-R", "read everything again"),
    ("?", "this help"),
    ("q, Esc, Ctrl-C", "quit"),
];

fn draw_help(frame: &mut Frame, theme: Theme, area: Rect) {
    let rows = HELP_ROWS;
    let lines: Vec<Line> = rows
        .iter()
        .map(|(key, what)| {
            Line::from(vec![
                Span::styled(format!("{key:<30}"), theme.key()),
                Span::raw(*what),
            ])
        })
        .collect();
    let popup = centered(area, 70, u16::try_from(rows.len()).unwrap_or(6) + 4);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" Keys (any key closes) ")),
        popup,
    );
}

/// The answers a confirmation takes besides yes and no: deleting a desk item's card too,
/// or trimming a returned checkout's session snapshots.
fn extra_answers(confirm: &Confirm, theme: Theme) -> Vec<Span<'static>> {
    let (key, what) = match confirm {
        Confirm::DeskRemove { .. } => ("d", " delete its card too   "),
        Confirm::Return {
            snapshots: true, ..
        } => ("t", " return it and forget its session snapshots   "),
        _ => return Vec::new(),
    };
    vec![Span::styled(key, theme.key()), Span::raw(what)]
}

/// A correction worked out: each change before and after, waiting for `y` or `n`.
fn draw_correction(
    frame: &mut Frame,
    correction: &crate::correct::Prepared,
    theme: Theme,
    area: Rect,
) {
    let mut lines: Vec<Line> = correction
        .shown
        .lines()
        .map(|l| Line::from(l.to_owned()))
        .collect();
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("y", theme.key()),
        Span::raw(" save   "),
        Span::styled("n", theme.key()),
        Span::raw(" discard"),
    ]));
    let width = area.width.saturating_sub(8).clamp(40, 110);
    // Long lines wrap, so leave room for them.
    let wrapped: usize = lines
        .iter()
        .map(|l| {
            l.width()
                .div_ceil(usize::from(width.saturating_sub(4)).max(1))
                .max(1)
        })
        .sum();
    let height = u16::try_from(wrapped + 2)
        .unwrap_or(u16::MAX)
        .min(area.height);
    let popup = centered(area, width, height);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(format!(" Correct “{}” ", correction.title))),
        popup,
    );
}

fn draw_confirm(frame: &mut Frame, confirm: &Confirm, theme: Theme, area: Rect) {
    let (title, question, explanation, yes) = match confirm {
        Confirm::Undo { summary, .. } => (
            " Undo ",
            format!("Undo “{summary}”?"),
            "A new change reverses it: items it moved go back, and the cards and documents it \
             changed return to how they were."
                .to_owned(),
            " undo   ",
        ),
        Confirm::UndoLibrarian(count) => super::librarian::confirm_text(*count),
        Confirm::Agent(change) => super::agent::confirm_popup(change),
        Confirm::Download { name, about } => download_confirm(name, about),
        Confirm::ModelChange { change, about } => model_change_confirm(change, about),
        Confirm::Learn { about, fact } => (
            " Glossary ",
            format!("Add “{about}” to the glossary?"),
            fact.clone(),
            " add it   ",
        ),
        Confirm::RejectFindings(ids) => super::review_crawler::reject_findings_confirm(ids),
        Confirm::UndoCrawler(count) => super::review_crawler::undo_crawler_confirm(*count),
        Confirm::RejectRestructure { what, .. } => {
            super::review_restructure::reject_restructure_confirm(what)
        }
        Confirm::Reject(ids) => (
            " Reject ",
            format!(
                "Reject {} proposal{}?",
                ids.len(),
                if ids.len() == 1 { "" } else { "s" }
            ),
            "The items stay where they are. Rejecting isn't a change to the archive, so History \
             can't undo it; ingest them again to propose them anew."
                .to_owned(),
            " reject   ",
        ),
        Confirm::Return { title, changes, .. } => (
            " Return ",
            format!("Return “{title}”, replacing the archive copy?"),
            changes.trim_end().to_owned(),
            " return it   ",
        ),
        Confirm::TrashWorkingCopy(working) => super::active::trash_working_copy_confirm(working),
        Confirm::CancelCheckout { title, .. } => (
            " Cancel checkout ",
            format!("Cancel the checkout of “{title}”?"),
            "The archive copy is unlocked as it was, and the working copy's changes aren't \
             brought back; the working copy stays where it is."
                .to_owned(),
            " cancel it   ",
        ),
        other => super::actions::confirm_text(other),
    };
    let mut lines = vec![Line::from(question), Line::from("")];
    lines.extend(explanation.lines().map(|l| Line::from(l.to_owned())));
    let mut answers = vec![Span::styled("y", theme.key()), Span::raw(yes)];
    answers.extend(extra_answers(confirm, theme));
    if matches!(confirm, Confirm::Rule { .. }) {
        answers.extend([
            Span::styled("n", theme.key()),
            Span::raw(" without a rule   "),
            Span::styled("Esc", theme.key()),
            Span::raw(" not now"),
        ]);
    } else {
        answers.extend([Span::styled("n", theme.key()), Span::raw(" keep it")]);
    }
    lines.extend([Line::from(""), Line::from(answers)]);
    let width = if matches!(confirm, Confirm::Return { .. }) {
        area.width.saturating_sub(8).clamp(64, 100)
    } else {
        64
    };
    // Long lines wrap, so leave room for them.
    let wrapped: usize = lines
        .iter()
        .map(|l| {
            l.width()
                .div_ceil(usize::from(width.saturating_sub(4)).max(1))
                .max(1)
        })
        .sum();
    let height = u16::try_from(wrapped + 2).unwrap_or(u16::MAX).max(9);
    let popup = centered(area, width, height);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(title)),
        popup,
    );
}

fn confidence(entry: &ReviewEntry) -> String {
    format!("{:?}", entry.destination.confidence).to_lowercase()
}

fn draw_review(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    match app.review_list {
        super::review_crawler::ReviewList::Crawler => {
            super::review_crawler::draw_crawler_findings(frame, app, theme, area);
            return;
        }
        super::review_crawler::ReviewList::Restructure => {
            super::review_restructure::draw_restructures(frame, app, theme, area);
            return;
        }
        super::review_crawler::ReviewList::Proposals => {}
    }
    if app.queue.is_empty() {
        let also = super::review_crawler::lists_line(app);
        frame.render_widget(Paragraph::new(format!("Nothing to review. {also}")), area);
        return;
    }
    let also = super::review_crawler::lists_line(app);
    let area = if also.is_empty() {
        area
    } else {
        let [line_area, rest] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        frame.render_widget(Paragraph::new(also), line_area);
        rest
    };
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(area);
    let mut items: Vec<ListItem> = Vec::new();
    // Rows of the list, with group headings, and which row each proposal is on.
    let mut chosen_row = 0;
    let mut destination: Option<&str> = None;
    for (i, entry) in app.queue.iter().enumerate() {
        if destination != Some(entry.destination.path.as_str()) {
            destination = Some(&entry.destination.path);
            items.push(ListItem::new(Line::from(Span::styled(
                entry.destination.path.clone(),
                Style::new().add_modifier(Modifier::BOLD),
            ))));
        }
        if i == app.chosen {
            chosen_row = items.len();
        }
        let mark = if app.marked.contains(&entry.id) {
            "■"
        } else {
            " "
        };
        let mut flags = vec![confidence(entry)];
        if entry.state != State::Proposed {
            flags.push(entry.state.as_str().to_owned());
        }
        if entry.card.sensitivity >= Sensitivity::Sensitive {
            flags.push(
                serde_json::to_value(entry.card.sensitivity)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default(),
            );
        }
        if carrel_core::ingest::redraft::wants_redraft(entry) {
            flags.push("draft again?".into());
        }
        if entry.duplicate_of.is_some() {
            flags.push("duplicate".into());
        } else if !entry.likely_duplicates.is_empty() {
            flags.push("likely duplicate".into());
        }
        items.push(ListItem::new(format!(
            "{mark} {}  ({})",
            entry.card.title,
            flags.join(", ")
        )));
    }
    let mut state = ListState::default().with_selected(Some(chosen_row));
    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.selected()),
        list_area,
        &mut state,
    );
    let detail_area = match app.comparisons.get(app.compared) {
        Some(comparison) => {
            let height = u16::try_from(comparison.rows.len() + 3).unwrap_or(u16::MAX);
            let [compare_area, rest] =
                Layout::vertical([Constraint::Length(height), Constraint::Min(0)])
                    .areas(detail_area);
            draw_comparison(frame, app, comparison, theme, compare_area);
            rest
        }
        None => detail_area,
    };
    let lines: Vec<Line> = app.detail.iter().map(|l| Line::from(l.as_str())).collect();
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::new().borders(ratatui::widgets::Borders::LEFT)),
        detail_area,
    );
}

/// The proposal beside an archived item it duplicates, the rows that differ highlighted.
fn draw_comparison(
    frame: &mut Frame,
    app: &App,
    comparison: &carrel_core::actions::Comparison,
    theme: Theme,
    area: Rect,
) {
    let exact = comparison.why == ["same checksum"];
    let count = if app.comparisons.len() > 1 {
        format!("  {} of {}", app.compared + 1, app.comparisons.len())
    } else {
        String::new()
    };
    let title = format!(
        " {} ({}){count} ",
        if exact {
            "Duplicate"
        } else {
            "Likely duplicate"
        },
        comparison.why.join(", ")
    );
    let header = ratatui::widgets::Row::new(["", "this item", "archived"])
        .style(Style::new().add_modifier(Modifier::BOLD));
    let rows = comparison.rows.iter().map(|row| {
        let cells = [
            row.label.to_owned(),
            row.proposed.clone(),
            row.archived.clone(),
        ];
        let line = ratatui::widgets::Row::new(cells);
        if row.differs {
            line.style(theme.key())
        } else {
            line
        }
    });
    frame.render_widget(
        ratatui::widgets::Table::new(
            rows,
            [
                Constraint::Length(11),
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ],
        )
        .header(header)
        .block(
            Block::new()
                .borders(ratatui::widgets::Borders::LEFT | ratatui::widgets::Borders::BOTTOM)
                .title(title),
        ),
        area,
    );
}

fn draw_picker(frame: &mut Frame, picker: &Picker, theme: Theme, area: Rect) {
    let matching = picker.matching();
    let items: Vec<ListItem> = matching.iter().map(|c| ListItem::new(*c)).collect();
    let height = u16::try_from(matching.len().clamp(1, 12)).unwrap_or(12) + 3;
    let wide = matches!(
        picker.purpose,
        super::review::Purpose::Archive(_) | super::review::Purpose::Index(_)
    );
    let popup = centered(area, if wide { 76 } else { 50 }, height);
    frame.render_widget(Clear, popup);
    let what = match picker.purpose {
        super::review::Purpose::Move(_) => "Move to",
        super::review::Purpose::Answer(_) => "They belong in",
        super::review::Purpose::Setting(_) => "Set it to",
        super::review::Purpose::Models(_) => "Models",
        super::review::Purpose::Index(_) => "Search index",
        super::review::Purpose::Eagerness(_) => "How eager to rearrange",
        super::review::Purpose::Archive(_) => "Archives",
    };
    let title = if picker.filter.is_empty() {
        format!(" {what} (type to narrow) ")
    } else {
        format!(" {what}: {} ", picker.filter)
    };
    let mut state = ListState::default().with_selected(Some(picker.chosen));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(theme.selected())
            .block(Block::bordered().title(title)),
        popup,
        &mut state,
    );
}

/// A question's suggestions, numbered, each with why it's likely.
fn suggestion_lines(
    question: &carrel_core::questions::QuestionEntry,
    theme: Theme,
) -> Vec<Line<'static>> {
    question
        .suggestions
        .iter()
        .enumerate()
        .map(|(i, suggestion)| {
            let mut spans = vec![
                Span::styled(format!("{}", i + 1), theme.key()),
                Span::raw(format!("  {}", suggestion.answer)),
            ];
            let mut notes = Vec::new();
            if suggestion.new_collection {
                notes.push("new".to_owned());
            }
            notes.extend(suggestion.why.clone());
            if !notes.is_empty() {
                spans.push(Span::styled(
                    format!("  ({})", notes.join("; ")),
                    theme.dim(),
                ));
            }
            Line::from(spans)
        })
        .collect()
}

fn draw_questions(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    use carrel_core::questions::QuestionKind;
    if app.asked.is_empty() {
        let text = if app.resting == 0 {
            "No questions".to_owned()
        } else {
            format!(
                "No questions; {} skipped one{} resting",
                app.resting,
                if app.resting == 1 { " is" } else { "s are" }
            )
        };
        frame.render_widget(Paragraph::new(text), area);
        return;
    }
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);
    let items: Vec<ListItem> = app
        .asked
        .iter()
        .zip(&app.asked_settles)
        .map(|(q, settles)| {
            ListItem::new(format!(
                "{}  ({} item{})",
                q.question,
                settles,
                if *settles == 1 { "" } else { "s" }
            ))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(app.asked_chosen));
    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.selected()),
        list_area,
        &mut state,
    );
    let question = &app.asked[app.asked_chosen];
    let key = app.question_preview_key();
    let detail_area = draw_preview(frame, app, theme, detail_area, key.as_deref());
    let mut lines = vec![
        Line::from(question.question.clone()).style(Style::new().add_modifier(Modifier::BOLD)),
        Line::from(""),
    ];
    let about = super::question_preview::subject_lines(&app.question_subjects);
    if !about.is_empty() {
        lines.extend(about);
        lines.push(Line::from(""));
    }
    if !question.suggestions.is_empty() {
        lines.push(Line::from(""));
        lines.extend(suggestion_lines(question, theme));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        match question.kind {
            QuestionKind::Term => {
                "Enter to say what it is; Carrel remembers it and drafts these again."
            }
            QuestionKind::Collection | QuestionKind::Group => "Enter to choose where they belong.",
            QuestionKind::Sender => {
                "Enter to answer yes, no or a collection; Carrel asks before adding a rule."
            }
            QuestionKind::Item => "Enter to answer; you see what it changes before it's saved.",
        },
        theme.dim(),
    )));
    lines.push(Line::from(Span::styled(
        if question.suggestions.is_empty() {
            "s skips it for now, d dismisses it."
        } else {
            "A number picks a suggestion; s skips it for now, d dismisses it."
        },
        theme.dim(),
    )));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::new().borders(ratatui::widgets::Borders::LEFT)),
        detail_area,
    );
}

fn draw_browse(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let [tree_area, items_area, card_area] = Layout::horizontal([
        Constraint::Percentage(25),
        Constraint::Percentage(30),
        Constraint::Percentage(45),
    ])
    .areas(area);
    let collections: Vec<ListItem> = app
        .collections
        .iter()
        .map(|c| {
            let depth = c.path.matches('/').count();
            ListItem::new(format!("{}{}", "  ".repeat(depth), c.doc.title))
        })
        .collect();
    // The pane the arrows move in is highlighted; the other keeps its choice dimly.
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let (tree_style, items_style) = match app.browse_pane {
        Pane::Collections => (theme.selected(), bold),
        Pane::Items => (bold, theme.selected()),
        Pane::Item => (bold, bold),
    };
    let mut tree_state = ListState::default().with_selected(Some(app.browse_collection));
    frame.render_stateful_widget(
        List::new(collections).highlight_style(tree_style),
        tree_area,
        &mut tree_state,
    );
    let title = app
        .search
        .as_ref()
        .map(|typed| app.search_title(typed))
        .unwrap_or_default();
    let items: Vec<ListItem> = if app.browse_items.is_empty() {
        vec![ListItem::new(if app.search.is_some() {
            "Nothing found"
        } else {
            "No items"
        })]
    } else {
        app.browse_items
            .iter()
            .enumerate()
            .map(
                |(i, (_, title))| match app.searched.hit(i).map(|h| &h.found) {
                    Some(Found::Item(item)) => ListItem::new(format!(
                        "{title}{}  ({})",
                        crate::related::mark(item.sensitivity),
                        item.collection
                    )),
                    _ => ListItem::new(title.as_str()),
                },
            )
            .collect()
    };
    let mut items_state = ListState::default().with_selected(Some(app.browse_item));
    frame.render_stateful_widget(
        List::new(items).highlight_style(items_style).block(
            Block::new()
                .borders(ratatui::widgets::Borders::LEFT)
                .title(title),
        ),
        items_area,
        &mut items_state,
    );
    draw_item(frame, app, theme, card_area);
}

/// Why a search result came back: where it matched, and the passage with the words marked.
fn why_lines(hit: &Hit, theme: Theme) -> Vec<Line<'static>> {
    let why: Vec<String> = hit.why.iter().map(ToString::to_string).collect();
    let mut lines = vec![Line::styled(
        format!("Matched: {}", why.join(", ")),
        theme.dim(),
    )];
    if let Some(passage) = &hit.passage {
        let marked = theme
            .key()
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
        let mut spans = Vec::new();
        let mut at = 0;
        for range in &passage.highlights {
            spans.push(Span::raw(passage.text[at..range.start].to_owned()));
            spans.push(Span::styled(passage.text[range.clone()].to_owned(), marked));
            at = range.end;
        }
        spans.push(Span::raw(passage.text[at..].to_owned()));
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));
    lines
}

/// The chosen item: its card, then its changes and related items, which the owner moves
/// among in the item's own pane.
fn draw_item(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let view = &app.item;
    let block = Block::new().borders(ratatui::widgets::Borders::LEFT);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let bold = Style::new().add_modifier(Modifier::BOLD);
    // Headings aren't rows; `selected` is the chosen row's place among these.
    let mut rows: Vec<ListItem> = Vec::new();
    let mut selected = None;
    if !view.history.is_empty() {
        rows.push(ListItem::new(Line::styled("History", bold)));
    }
    for (i, row) in view.rows().into_iter().enumerate() {
        if let (Row::Related(_), true) = (row, i == view.history.len()) {
            rows.push(ListItem::new(Line::styled("Related", bold)));
        }
        if i == view.chosen {
            selected = Some(rows.len());
        }
        rows.push(ListItem::new(match row {
            Row::Change(entry) => format!(
                "{}  {}",
                crate::commands::local_time(entry.time),
                entry.summary
            ),
            Row::Related(related) => match &related.relation {
                Relation::Link(how) => format!("{how}  {}", related.title),
                Relation::Near(sensitivity) => {
                    format!(
                        "near  {}{}",
                        related.title,
                        crate::related::mark(*sensitivity)
                    )
                }
            },
        }));
    }
    if let Some(why) = &view.links_only {
        if view.related.is_empty() {
            rows.push(ListItem::new(Line::styled("Related", bold)));
        }
        rows.push(ListItem::new(Line::styled(why.clone(), theme.dim())));
    }
    let list_height = u16::try_from(rows.len())
        .unwrap_or(u16::MAX)
        .min(inner.height / 2);
    let [card_area, list_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(list_height)]).areas(inner);
    let mut lines: Vec<Line> = Vec::new();
    if app.search.is_some()
        && let Some(hit) = app.searched.hit(app.browse_item)
    {
        lines.extend(why_lines(hit, theme));
    }
    if let Some(question) = app.item_questions.first() {
        for (i, line) in super::asking::question_lines(question)
            .into_iter()
            .enumerate()
        {
            lines.push(if i == 0 {
                Line::styled(line, theme.key())
            } else {
                Line::from(line)
            });
        }
        lines.push(Line::from(""));
    }
    lines.extend(view.text.iter().map(|l| Line::from(l.as_str())));
    let card_area = draw_preview(frame, app, theme, card_area, view.card.as_deref());
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), card_area);
    let highlight = if app.browse_pane == Pane::Item {
        theme.selected()
    } else {
        Style::new()
    };
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(
        List::new(rows).highlight_style(highlight),
        list_area,
        &mut state,
    );
}

/// The chosen item's preview, beside its card when there's room and above it when there
/// isn't: a folder's listing, or room left for the picture the run loop draws after the
/// frame. Returns what's left for the card.
pub(super) fn draw_preview(
    frame: &mut Frame,
    app: &App,
    theme: Theme,
    area: Rect,
    card: Option<&str>,
) -> Rect {
    let Some(ready) = app.previews.shown(card) else {
        return area;
    };
    let (card, preview) = if area.width >= 60 {
        let [card, preview] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .areas(area);
        (card, preview)
    } else {
        let [preview, card] =
            Layout::vertical([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(area);
        (card, preview)
    };
    let block = Block::new()
        .borders(ratatui::widgets::Borders::LEFT)
        .title(Line::styled(" Preview ", theme.dim()));
    let inner = block.inner(preview);
    frame.render_widget(block, preview);
    match ready {
        super::preview::Ready::Listing(lines) => {
            let lines: Vec<Line> = lines.iter().map(|l| Line::from(l.as_str())).collect();
            frame.render_widget(Paragraph::new(lines), inner);
        }
        super::preview::Ready::Picture(_) if !app.covered() && inner.area() > 0 => {
            app.previews.room.set(Some(inner));
        }
        _ => {}
    }
    card
}

fn draw_input(frame: &mut Frame, input: &Input, theme: Theme, area: Rect) {
    let (title, above) = match &input.purpose {
        InputPurpose::Answer { question, .. } => (" Answer ", question.clone()),
        InputPurpose::Ask => (
            " Librarian ",
            "Ask about your archive, in your own words (\"what do I have about the lamp?\"):"
                .to_owned(),
        ),
        InputPurpose::Search => (
            " Search ",
            "Words to find, and filters such as tag:receipts, kind:file, after:2025-01:".to_owned(),
        ),
        InputPurpose::Setting(key) => (" Settings ", format!("New value for {key}:")),
        InputPurpose::Ingest => (
            " Ingest ",
            "Files and folders to add, separated by spaces (drop them here, or paste):".to_owned(),
        ),
        InputPurpose::BackupRepository => (
            " Backups ",
            "The folder for the backup repository: an existing one to connect to, or a new one \
             (an external drive is safest):"
                .to_owned(),
        ),
        InputPurpose::BackupPassword { new, first, .. } => (
            " Backups ",
            match (new, first.is_some()) {
                (true, false) => format!("{} New password:", crate::backup::PASSWORD_WARNING),
                (true, true) => "The same password again:".to_owned(),
                (false, _) => "The repository's password:".to_owned(),
            },
        ),
        InputPurpose::Correct(_) => (
            " Correct ",
            "What's wrong with it? Say it in words (\"it's from 2019\", \"it's a picture of A, \
             not B\"), or write field: value, such as created: 2019-03-31 or sensitivity: \
             personal. Nothing is saved until you've seen the change."
                .to_owned(),
        ),
        InputPurpose::RestoreItem { .. } => (
            " Restore ",
            "The item to restore as of this snapshot (its identifier, or its card's or \
             contents' path). Next, the folder it goes in; nothing in the archive changes:"
                .to_owned(),
        ),
        InputPurpose::OpenArchive => (
            " Archives ",
            "The archive to open: its folder, or any folder in it (drop it here, or paste):"
                .to_owned(),
        ),
        InputPurpose::CreateArchive => (
            " Archives ",
            "The folder for the new archive, which is named after it, as `carrel init` makes \
             it. An empty or new folder is best:"
                .to_owned(),
        ),
        InputPurpose::Option(line) => line.text(),
        purpose => super::actions::input_text(purpose).unwrap_or((" ", String::new())),
    };
    let shown = if input.masked() {
        "•".repeat(input.text.chars().count())
    } else {
        input.text.clone()
    };
    let lines = vec![
        Line::from(above),
        Line::from(""),
        Line::from(vec![
            Span::styled("> ", theme.key()),
            Span::raw(shown),
            Span::styled("▏", theme.key()),
        ]),
    ];
    let popup = centered(area, 70, 8);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(title)),
        popup,
    );
}

fn draw_backups(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let status = crate::backup::status_line(&app.archive, Some(&app.history));
    let snapshots = app.snapshots();
    let [top, rest] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
    frame.render_widget(Paragraph::new(status.trim_end().to_owned()), top);
    if !app.backups_set_up() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from("Backups keep every version of the archive's files, encrypted, in a"),
                Line::from("Kopia repository, best on an external drive. Once set up, a snapshot"),
                Line::from("is taken with every batch you approve."),
                Line::from(""),
                Line::from(Span::styled("s to set them up", theme.dim())),
            ]),
            rest,
        );
        return;
    }
    if snapshots.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled("No snapshot yet; n to take one", theme.dim())),
            rest,
        );
        return;
    }
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(rest);
    let items: Vec<ListItem> = snapshots
        .iter()
        .map(|e| {
            ListItem::new(format!(
                "{}  {}",
                crate::commands::local_time(e.time),
                e.summary
            ))
        })
        .collect();
    let chosen = app.backup_chosen.min(snapshots.len() - 1);
    let mut state = ListState::default().with_selected(Some(chosen));
    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.selected()),
        list_area,
        &mut state,
    );
    let entry = snapshots[chosen];
    let mut lines = vec![
        Line::from(entry.summary.clone()).style(Style::new().add_modifier(Modifier::BOLD)),
        Line::from(format!("change {}", &entry.id[..entry.id.len().min(12)])),
    ];
    if let Some(snapshot) = &entry.snapshot {
        lines.push(Line::from(format!("snapshot {snapshot}")));
    }
    if !entry.items.is_empty() {
        lines.push(Line::from(format!("{} item(s) changed", entry.items.len())));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "r to restore an item as of this snapshot",
        theme.dim(),
    )));
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::new().borders(ratatui::widgets::Borders::LEFT)),
        detail_area,
    );
}

fn draw_ingest(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    if let Some(plan) = &app.planned {
        let mut lines = vec![
            Line::from(match plan.count {
                0 => "Nothing new to queue".to_owned(),
                1 => "1 item to queue:".to_owned(),
                n => format!("{n} items to queue:"),
            })
            .style(Style::new().add_modifier(Modifier::BOLD)),
        ];
        lines.extend(plan.lines.iter().map(|l| Line::from(l.as_str())));
        if let Some(note) = &plan.note {
            lines.push(Line::from(format!("Note: {note}")));
        }
        lines.push(Line::from(""));
        if let Some(problem) = &plan.problem {
            lines.push(Line::from(problem.as_str()));
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(
            "Enter adds them to the queue; a worker drafts each in the background, and its \
             proposal appears in Review when it's ready. e reads folders entry by entry, w as \
             one item; n adds a note for their cards.",
            theme.dim(),
        )));
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
        return;
    }
    let [top, list_area] =
        Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
    let head = app.worker_line().unwrap_or_else(|| {
        if app.intake.iter().any(|e| !e.stuck()) {
            "No worker is running; queue something or run `carrel worker` to start one".into()
        } else {
            "Press i to add files and folders.".into()
        }
    });
    frame.render_widget(Paragraph::new(head), top);
    if app.intake.is_empty() {
        frame.render_widget(Paragraph::new("The queue is empty"), list_area);
        return;
    }
    let items: Vec<ListItem> = app
        .intake_lines()
        .into_iter()
        .map(|(_, line)| ListItem::new(line))
        .collect();
    let mut state = ListState::default().with_selected(Some(app.intake_chosen));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(theme.selected())
            .block(Block::new().title(" Waiting ")),
        list_area,
        &mut state,
    );
}

fn draw_browser(frame: &mut Frame, browser: &super::browser::Browser, theme: Theme, area: Rect) {
    let popup = centered(
        area,
        area.width.saturating_sub(8),
        area.height.saturating_sub(4),
    );
    frame.render_widget(Clear, popup);
    let marked = if browser.marked.is_empty() {
        String::new()
    } else {
        format!(" · {} marked", browser.marked.len())
    };
    let block = Block::bordered().title(format!(" {}{marked} ", browser.dir.display()));
    if let Some(problem) = &browser.problem {
        frame.render_widget(Paragraph::new(problem.as_str()).block(block), popup);
        return;
    }
    let items: Vec<ListItem> = browser
        .entries
        .iter()
        .map(|(name, folder)| {
            let mark = if browser.marked.contains(&browser.dir.join(name)) {
                "* "
            } else {
                "  "
            };
            ListItem::new(format!("{mark}{name}{}", if *folder { "/" } else { "" }))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(browser.chosen));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(theme.selected())
            .block(block),
        popup,
        &mut state,
    );
}

fn draw_settings(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(area);
    let mut items = Vec::new();
    let mut rows = Vec::new();
    let mut device = None;
    for (i, shown) in app.settings.iter().enumerate() {
        let is_device = super::settings::is_device(shown.row);
        if device != Some(is_device) {
            device = Some(is_device);
            items.push(ListItem::new(Line::from(Span::styled(
                if is_device {
                    "This device"
                } else {
                    "This archive (shared by every device)"
                },
                theme.dim(),
            ))));
            rows.push(None);
        }
        items.push(ListItem::new(format!(
            "  {:22} {}",
            shown.label, shown.value
        )));
        rows.push(Some(i));
    }
    let selected = rows.iter().position(|r| *r == Some(app.settings_chosen));
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.selected()),
        list_area,
        &mut state,
    );
    let detail: Vec<Line> = app
        .settings
        .get(app.settings_chosen)
        .map(|s| s.detail.iter().map(|l| Line::from(l.as_str())).collect())
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(detail)
            .wrap(Wrap { trim: false })
            .block(Block::new().borders(ratatui::widgets::Borders::LEFT)),
        detail_area,
    );
}

fn draw_active(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    use super::active::{Kind, Row};
    if app.active.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from("Nothing is checked out or on the desk."),
                Line::from(""),
                Line::from(Span::styled(
                    "Check an item out with `carrel checkout ITEM`, or track a folder where it is \
                     with `carrel desk add PATH`.",
                    theme.dim(),
                )),
            ])
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(area);
    let items: Vec<ListItem> = app
        .active_rows()
        .into_iter()
        .map(|row| match row {
            Row::Item(i) => {
                let item = &app.active[i];
                let kind = match item.kind {
                    Kind::CheckedOut => "out ",
                    Kind::Desk => "desk",
                };
                let pending = match item.pending.len() {
                    0 => String::new(),
                    n => format!(", {n} pending"),
                };
                ListItem::new(format!("{kind}  {}  ({}{pending})", item.title, item.state))
            }
            Row::Entry(i, n) => {
                let item = &app.active[i];
                let entry = &item.pending[n - 1];
                let mark = if app.active_marked.contains(&(item.id, n)) {
                    "■"
                } else {
                    " "
                };
                ListItem::new(format!(
                    "    {mark} {n}. {}{}  {}",
                    entry.date,
                    entry
                        .duration
                        .as_ref()
                        .map(|d| format!(" · {d}"))
                        .unwrap_or_default(),
                    entry.lines.first().map_or("", String::as_str)
                ))
            }
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(app.active_chosen));
    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.selected()),
        list_area,
        &mut state,
    );
    let lines = app.active_item().map(active_detail).unwrap_or_default();
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::new().borders(ratatui::widgets::Borders::LEFT)),
        detail_area,
    );
}

/// The chosen active item in full: where it is, since when, what changed, and its pending
/// entries numbered.
fn active_detail(item: &super::active::ActiveItem) -> Vec<Line<'static>> {
    use super::active::Kind;
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let mut lines = vec![Line::from(item.title.clone()).style(bold)];
    match item.kind {
        Kind::CheckedOut => {
            lines.push(Line::from(format!(
                "Working copy: {}",
                item.working.display()
            )));
            lines.push(Line::from(format!("Checked out {}", item.since)));
            if let Some(due) = &item.due {
                lines.push(Line::from(format!("Due {due}")));
            }
        }
        Kind::Desk => {
            lines.push(Line::from(format!(
                "On the desk at {}",
                item.working.display()
            )));
            lines.push(Line::from(format!("Card: {}", item.card.display())));
            lines.push(Line::from(format!("Added {}", item.since)));
        }
    }
    lines.push(Line::from(format!("State: {}", item.state)));
    if let Some(diff) = item.diff.as_ref().filter(|d| !d.is_empty()) {
        lines.push(Line::from(""));
        let counts: Vec<String> = [
            (diff.changed.len(), "changed"),
            (diff.added.len(), "added"),
            (diff.removed.len(), "removed"),
            (diff.renamed.len(), "renamed"),
        ]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}"))
        .collect();
        lines.push(Line::from(format!("Files: {}", counts.join(", "))));
        let shown = diff
            .changed
            .iter()
            .map(|p| format!("  changed  {p}"))
            .chain(diff.added.iter().map(|p| format!("  added    {p}")))
            .chain(diff.removed.iter().map(|p| format!("  removed  {p}")))
            .chain(
                diff.renamed
                    .iter()
                    .map(|(a, b)| format!("  renamed  {a} → {b}")),
            );
        lines.extend(shown.take(8).map(Line::from));
    }
    lines.push(Line::from(""));
    if item.pending.is_empty() {
        lines.push(Line::from("No pending history entries"));
    } else {
        lines.push(Line::from("Pending history entries:").style(bold));
        for (n, entry) in item.pending.iter().enumerate() {
            lines.push(Line::from(format!(
                "{}. {}{}",
                n + 1,
                entry.date,
                entry
                    .duration
                    .as_ref()
                    .map(|d| format!(" · {d}"))
                    .unwrap_or_default()
            )));
            lines.extend(entry.lines.iter().map(|l| Line::from(format!("   {l}"))));
        }
    }
    lines
}
