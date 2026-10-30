//! Drawing the interactive mode from its state.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use carrel_core::card::Sensitivity;
use carrel_core::review::{ReviewEntry, State};

use super::app::{App, Confirm, Input, Picker, Tab};
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

    fn selected(self) -> Style {
        let style = Style::new().add_modifier(Modifier::REVERSED);
        if self.color {
            style.fg(Color::Cyan)
        } else {
            style
        }
    }

    fn key(self) -> Style {
        let style = Style::new().add_modifier(Modifier::BOLD);
        if self.color {
            style.fg(Color::Yellow)
        } else {
            style
        }
    }

    fn dim(self) -> Style {
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
        Tab::Questions => draw_questions(frame, app, theme, inner),
        Tab::Browse => draw_browse(frame, app, theme, inner),
        Tab::Status => draw_report(frame, app, inner, Some(waiting_line(app))),
        Tab::Ingest => draw_ingest(frame, app, theme, inner),
        Tab::Settings => draw_settings(frame, app, theme, inner),
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
    if let Some(confirm) = &app.confirm {
        draw_confirm(frame, confirm, theme, area);
    }
}

fn draw_tabs(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let mut spans = vec![Span::raw(" ")];
    for (i, tab) in Tab::ALL.iter().enumerate() {
        let title = format!("{} {}", i + 1, tab.title());
        let style = if *tab == app.tab {
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
        _ => format!(
            "Waiting for you: {proposals} proposal{} (2 to review), {questions} question{} (3 to answer).",
            if proposals == 1 { "" } else { "s" },
            if questions == 1 { "" } else { "s" }
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
    if let Some(busy) = &app.busy {
        spans.push(Span::styled(format!(" {busy}  ·"), theme.key()));
    }
    if let Some(worker) = app.worker_line() {
        spans.push(Span::styled(format!(" {worker}  ·"), theme.key()));
    }
    if let Some(note) = app.marked_note() {
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

fn draw_help(frame: &mut Frame, theme: Theme, area: Rect) {
    let rows = [
        ("1 to 7, Tab, ←→", "switch screens"),
        (
            "i",
            "add files and folders to ingest, with your file picker or the built-in one",
        ),
        ("↑↓, j k, PgUp PgDn, Home End", "scroll, or choose a change"),
        ("a  h", "approve, or hold in Holding (Review)"),
        ("m  e  x", "move, edit or reject (Review)"),
        (
            "Space  A",
            "mark several; approve all high-confidence (Review)",
        ),
        ("Enter  d", "answer or dismiss a question (Questions)"),
        (
            "←→  /  Esc",
            "collections or items; search; clear it (Browse)",
        ),
        ("u", "undo the chosen change (History)"),
        ("x  t  r", "remove, move to the front, retry (Ingest)"),
        (
            "e  w",
            "each entry, or whole folder, before queueing (Ingest)",
        ),
        ("Ctrl-R", "read everything again"),
        ("?", "this help"),
        ("q, Esc, Ctrl-C", "quit"),
    ];
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

fn draw_confirm(frame: &mut Frame, confirm: &Confirm, theme: Theme, area: Rect) {
    let (title, question, explanation, yes) = match confirm {
        Confirm::Undo { summary, .. } => (
            " Undo ",
            format!("Undo “{summary}”?"),
            "A new change reverses it: items it moved go back, and the cards and documents it \
             changed return to how they were.",
            " undo   ",
        ),
        Confirm::Download { name, about } => (
            " Download ",
            format!("Download {name}?"),
            about.as_str(),
            " download   ",
        ),
        Confirm::Reject(ids) => (
            " Reject ",
            format!(
                "Reject {} proposal{}?",
                ids.len(),
                if ids.len() == 1 { "" } else { "s" }
            ),
            "The items stay where they are. Rejecting isn't a change to the archive, so History \
             can't undo it; ingest them again to propose them anew.",
            " reject   ",
        ),
    };
    let lines = vec![
        Line::from(question),
        Line::from(""),
        Line::from(explanation),
        Line::from(""),
        Line::from(vec![
            Span::styled("y", theme.key()),
            Span::raw(yes),
            Span::styled("n", theme.key()),
            Span::raw(" keep it"),
        ]),
    ];
    let popup = centered(area, 64, 9);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(Block::bordered().title(title)),
        popup,
    );
}

fn confidence(entry: &ReviewEntry) -> String {
    format!("{:?}", entry.destination.confidence).to_lowercase()
}

fn draw_review(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    if app.queue.is_empty() {
        frame.render_widget(Paragraph::new("Nothing to review"), area);
        return;
    }
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
        if entry.duplicate_of.is_some() {
            flags.push("duplicate".into());
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
    let lines: Vec<Line> = app.detail.iter().map(|l| Line::from(l.as_str())).collect();
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::new().borders(ratatui::widgets::Borders::LEFT)),
        detail_area,
    );
}

fn draw_picker(frame: &mut Frame, picker: &Picker, theme: Theme, area: Rect) {
    let matching = picker.matching();
    let items: Vec<ListItem> = matching.iter().map(|c| ListItem::new(*c)).collect();
    let height = u16::try_from(matching.len().clamp(1, 12)).unwrap_or(12) + 3;
    let popup = centered(area, 50, height);
    frame.render_widget(Clear, popup);
    let what = match picker.purpose {
        super::review::Purpose::Move(_) => "Move to",
        super::review::Purpose::Answer(_) => "They belong in",
        super::review::Purpose::Setting(_) => "Set it to",
        super::review::Purpose::Download => "Download",
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

fn draw_questions(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    if app.asked.is_empty() {
        frame.render_widget(Paragraph::new("No questions"), area);
        return;
    }
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);
    let items: Vec<ListItem> = app
        .asked
        .iter()
        .map(|q| {
            ListItem::new(format!(
                "{}  ({} item{})",
                q.question,
                q.items.len(),
                if q.items.len() == 1 { "" } else { "s" }
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
    let mut lines = vec![
        Line::from(question.question.clone()).style(Style::new().add_modifier(Modifier::BOLD)),
        Line::from(""),
        Line::from("Waiting on the answer:"),
    ];
    lines.extend(
        app.waiting_titles(question)
            .into_iter()
            .map(|t| Line::from(format!("  {t}"))),
    );
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        match question.kind {
            carrel_core::questions::QuestionKind::Term => {
                "Enter to say what it is; Carrel remembers it and drafts these again."
            }
            carrel_core::questions::QuestionKind::Collection => {
                "Enter to choose where they belong."
            }
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
    let (tree_style, items_style) = if app.browse_in_items {
        (Style::new().add_modifier(Modifier::BOLD), theme.selected())
    } else {
        (theme.selected(), Style::new().add_modifier(Modifier::BOLD))
    };
    let mut tree_state = ListState::default().with_selected(Some(app.browse_collection));
    frame.render_stateful_widget(
        List::new(collections).highlight_style(tree_style),
        tree_area,
        &mut tree_state,
    );
    let title = match &app.search {
        Some(words) => format!(" Found for “{words}” (Esc clears) "),
        None => String::new(),
    };
    let items: Vec<ListItem> = if app.browse_items.is_empty() {
        vec![ListItem::new(if app.search.is_some() {
            "Nothing found"
        } else {
            "No items"
        })]
    } else {
        app.browse_items
            .iter()
            .map(|(path, title)| {
                if app.search.is_some() {
                    ListItem::new(format!(
                        "{title}  ({})",
                        path.rsplit_once('/').map_or("", |p| p.0)
                    ))
                } else {
                    ListItem::new(title.as_str())
                }
            })
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
    let lines: Vec<Line> = app.card.iter().map(|l| Line::from(l.as_str())).collect();
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::new().borders(ratatui::widgets::Borders::LEFT)),
        card_area,
    );
}

fn draw_input(frame: &mut Frame, input: &Input, theme: Theme, area: Rect) {
    let (title, above) = match &input.purpose {
        InputPurpose::Answer { question, .. } => (" Answer ", question.clone()),
        InputPurpose::Search => (
            " Search ",
            "Words to find in titles, tags and summaries:".to_owned(),
        ),
        InputPurpose::Setting(key) => (" Settings ", format!("New value for {key}:")),
        InputPurpose::Ingest => (
            " Ingest ",
            "Files and folders to add, separated by spaces (drop them here, or paste):".to_owned(),
        ),
    };
    let lines = vec![
        Line::from(above),
        Line::from(""),
        Line::from(vec![
            Span::styled("> ", theme.key()),
            Span::raw(input.text.clone()),
            Span::styled("▏", theme.key()),
        ]),
    ];
    let popup = centered(area, 70, 6);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(title)),
        popup,
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
        lines.push(Line::from(""));
        if let Some(problem) = &plan.problem {
            lines.push(Line::from(problem.as_str()));
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(
            "Enter adds them to the queue; a worker drafts each in the background, and its \
             proposal appears in Review when it's ready. e reads folders entry by entry, w as \
             one item.",
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
