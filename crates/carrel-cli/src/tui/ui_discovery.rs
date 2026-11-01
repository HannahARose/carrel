//! Drawing the Librarian and Discover screens.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};

use super::app::App;
use super::discover::View;
use super::item::ItemView;
use super::librarian::{TalkRow, answer_lines, suggestion_row};
use super::ui::Theme;

/// Lines that fit the last `height` rows of `width` columns, once wrapped: how far to scroll
/// so the end of a conversation shows.
fn scroll_to_end(lines: &[Line], width: u16, height: u16) -> u16 {
    let width = usize::from(width.max(1));
    let rows: usize = lines.iter().map(|l| l.width().div_ceil(width).max(1)).sum();
    u16::try_from(rows.saturating_sub(usize::from(height))).unwrap_or(u16::MAX)
}

/// The Librarian screen: the conversation on the left, with the last answer's items and
/// suggestions and the librarian's changes to choose from; the chosen item on the right.
pub fn draw_librarian(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let mut lines: Vec<Line> = Vec::new();
    if app.talk.conversation.answers.is_empty() && app.talk.waiting.is_none() {
        lines.push(Line::from(
            "Ask the librarian about your archive: Enter, then your question.",
        ));
        lines.push(Line::styled(
            "It reads through search, cards, related items and history, on this device. \
             Nothing is kept unless you save the conversation (s).",
            theme.dim(),
        ));
    }
    for answer in &app.talk.conversation.answers {
        lines.extend(answer_lines(answer).into_iter().map(Line::from));
    }
    if let Some(waiting) = &app.talk.waiting {
        lines.push(Line::from(format!("You: {waiting}")));
        lines.push(Line::styled("The librarian is looking…", theme.dim()));
    }
    if let Some(question) = &app.talk.question {
        lines.push(Line::styled(
            format!(
                "While we're here, about “{}”: {} (a to answer)",
                question.subject, question.question
            ),
            theme.key(),
        ));
    }
    let rows = app.talk_rows();
    let mut items: Vec<ListItem> = Vec::new();
    let mut selected = None;
    let mut heading = "";
    for (i, row) in rows.iter().enumerate() {
        let (title, text) = match row {
            TalkRow::Item(item) => (
                "Items",
                format!(
                    "{}{}  {}",
                    item.title,
                    crate::related::mark(item.sensitivity),
                    item.collection
                ),
            ),
            TalkRow::Suggestion(s) => ("Suggestions", suggestion_row(s)),
            TalkRow::Change(entry) => (
                "The librarian's changes this week",
                format!(
                    "{}  {}",
                    crate::commands::local_time(entry.time),
                    entry.summary
                ),
            ),
        };
        if title != heading {
            items.push(ListItem::new(Line::styled(title, bold)));
            heading = title;
        }
        if i == app.talk.chosen {
            selected = Some(items.len());
        }
        items.push(ListItem::new(text));
    }
    let list_height = u16::try_from(items.len())
        .unwrap_or(u16::MAX)
        .min(left.height / 2);
    let [talk_area, rows_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(list_height)]).areas(left);
    let scroll = scroll_to_end(&lines, talk_area.width, talk_area.height);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        talk_area,
    );
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.selected()),
        rows_area,
        &mut state,
    );
    draw_card(
        frame,
        app,
        theme,
        right,
        &app.talk.item,
        "the item it's discussing",
    );
}

/// An item's card on the right of a screen, with its preview where the terminal draws one.
fn draw_card(frame: &mut Frame, app: &App, theme: Theme, area: Rect, view: &ItemView, what: &str) {
    let block = Block::new().borders(Borders::LEFT);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if view.card.is_none() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                format!("Choose an item for {what}"),
                theme.dim(),
            )),
            inner,
        );
        return;
    }
    let card_area = super::ui::draw_preview(frame, app, theme, inner, view.card.as_deref());
    let lines: Vec<Line> = view.text.iter().map(|l| Line::from(l.as_str())).collect();
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), card_area);
}

/// The Discover screen: the views as a line of keys, then the chosen one.
pub fn draw_discover(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let [top, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
    let mut spans = Vec::new();
    for (view, key, name) in View::ALL {
        let style = if view == app.stacks.view {
            theme.selected()
        } else {
            Style::new()
        };
        spans.push(ratatui::text::Span::styled(
            format!(" {key} {name} "),
            style,
        ));
        spans.push(ratatui::text::Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), top);
    if app.stacks.view != View::Wander {
        let lines: Vec<Line> = app
            .stacks
            .lines
            .iter()
            .map(|l| Line::from(l.as_str()))
            .collect();
        frame.render_widget(Paragraph::new(lines).scroll((app.stacks.scroll, 0)), body);
        return;
    }
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
    let mut lines: Vec<Line> = app
        .stacks
        .step
        .iter()
        .map(|l| Line::from(l.as_str()))
        .collect();
    if let Some(question) = &app.stacks.question {
        lines.push(Line::from(""));
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
    }
    lines.push(Line::from(""));
    lines.push(Line::styled(
        if app.stacks.done {
            "The walk has visited every item it can reach; n starts another."
        } else {
            "Space for the next step."
        },
        theme.dim(),
    ));
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), left);
    draw_card(frame, app, theme, right, &app.stacks.item, "the walk");
}
