//! Restructure proposals in Review (`docs/design/restructure.md`, "In each interface"): the
//! chosen proposal's changes, each with its reason and how invasive it is, beside the archive
//! as it is now and as the chosen changes would leave it. `Space` leaves a change out, `a`
//! approves the rest, `x` rejects the proposal, as `carrel restructure approve [--only]` and
//! `reject` do; `t` proposes tag changes and `p` a new shape for the whole archive, as
//! `carrel restructure tags` and `carrel restructure propose` do.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use carrel_core::card::Id;
use carrel_core::discover::Catalog;
use carrel_core::graph::Graph;
use carrel_core::restructure::eagerness::{self, Eagerness, Source};
use carrel_core::restructure::preview::{Preview, PreviewCollection, preview};
use carrel_core::restructure::{Proposal, ProposalKind};
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};

use super::app::{App, Confirm, Effect};
use super::review::Handled;
use super::ui::Theme;

/// Something to do with restructure proposals, in the background.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestructureAction {
    /// Approve the proposal: the changes `only` names, or all of them when it's empty.
    Approve {
        /// The proposal.
        id: Id,
        /// The changes approved, by number; empty for all.
        only: Vec<String>,
    },
    /// Reject the proposal, changing nothing.
    Reject(Id),
    /// Propose merging, splitting and retiring tags.
    ProposeTags,
    /// Propose a new shape for this collection and everything in it, or the whole archive.
    ProposeSection(Option<String>),
    /// Have the crawler check some items now, as `carrel crawl run` does.
    CrawlNow,
    /// Undo the crawler's changes this week, as `carrel crawl undo 7d` does.
    UndoCrawler,
    /// Set this collection's own eagerness, or clear it with `default`.
    Eagerness {
        /// The collection, by path.
        collection: String,
        /// `settled`, `balanced`, `eager` or `default`.
        level: String,
    },
}

/// The restructure proposals waiting in Review, and what's chosen.
#[derive(Debug, Default)]
pub struct RestructureReview {
    /// The waiting proposals, oldest first.
    pub proposals: Vec<Proposal>,
    /// The proposal shown.
    pub chosen: usize,
    /// The change chosen in it.
    pub change: usize,
    /// The changes left out of each proposal, by number.
    pub left_out: BTreeMap<Id, BTreeSet<String>>,
    /// The archive's graph, which the preview starts from.
    graph: Graph,
    /// The archive as it is and as the chosen changes would leave it.
    pub preview: Preview,
}

impl RestructureReview {
    /// The proposal shown, if any.
    #[must_use]
    pub fn shown(&self) -> Option<&Proposal> {
        self.proposals.get(self.chosen)
    }

    /// The changes of `proposal` not left out, by number.
    #[must_use]
    pub fn chosen_changes(&self, proposal: &Proposal) -> Vec<String> {
        let out = self.left_out.get(&proposal.id);
        proposal
            .changes
            .iter()
            .map(|c| c.id.clone())
            .filter(|id| out.is_none_or(|o| !o.contains(id)))
            .collect()
    }

    fn refresh_preview(&mut self) {
        self.preview = match self.shown() {
            Some(proposal) => preview(&self.graph, proposal, &self.chosen_changes(proposal)),
            None => Preview::default(),
        };
    }
}

impl App {
    /// Reads the waiting restructure proposals again, keeping the choice.
    pub(super) fn load_restructures(&mut self) {
        let proposals = carrel_core::restructure::waiting(&self.archive, None).unwrap_or_default();
        let review = &mut self.restructure;
        let shown = review.shown().map(|p| p.id);
        review
            .left_out
            .retain(|id, _| proposals.iter().any(|p| p.id == *id));
        review.chosen = shown
            .and_then(|id| proposals.iter().position(|p| p.id == id))
            .unwrap_or(0);
        if shown != proposals.get(review.chosen).map(|p| p.id) {
            review.change = 0;
        }
        review.proposals = proposals;
        review.graph = Catalog::read(&self.archive)
            .map(|catalog| Graph::build(&catalog))
            .unwrap_or_default();
        review.refresh_preview();
    }

    /// A key while Review shows restructure proposals.
    pub(super) fn restructure_key(&mut self, key: KeyEvent) -> Handled {
        let review = &mut self.restructure;
        let changes = review.shown().map_or(0, |p| p.changes.len());
        match key.code {
            KeyCode::Up => review.change = review.change.saturating_sub(1),
            KeyCode::Down => review.change = (review.change + 1).min(changes.saturating_sub(1)),
            KeyCode::Char('[' | ']') if !review.proposals.is_empty() => {
                let count = review.proposals.len();
                review.chosen = if key.code == KeyCode::Char(']') {
                    (review.chosen + 1) % count
                } else {
                    (review.chosen + count - 1) % count
                };
                review.change = 0;
                review.refresh_preview();
            }
            KeyCode::Char('t') => {
                return Handled::Yes(Some(Effect::Restructure(RestructureAction::ProposeTags)));
            }
            KeyCode::Char('p') => {
                return Handled::Yes(Some(Effect::Restructure(
                    RestructureAction::ProposeSection(None),
                )));
            }
            KeyCode::Char(' ' | 'a' | 'x') => return self.decide_restructure(key.code),
            _ => return Handled::No,
        }
        Handled::Yes(None)
    }

    /// `Space`, `a` or `x` on the proposal shown.
    fn decide_restructure(&mut self, code: KeyCode) -> Handled {
        let review = &mut self.restructure;
        let Some(proposal) = review.shown().cloned() else {
            self.message = Some("No restructure proposals are waiting; t or p proposes one".into());
            return Handled::Yes(None);
        };
        match code {
            KeyCode::Char(' ') => {
                if let Some(change) = proposal.changes.get(review.change) {
                    let out = review.left_out.entry(proposal.id).or_default();
                    if !out.remove(&change.id) {
                        out.insert(change.id.clone());
                    }
                    review.change = (review.change + 1).min(proposal.changes.len() - 1);
                    review.refresh_preview();
                }
                Handled::Yes(None)
            }
            KeyCode::Char('a') => {
                let chosen = review.chosen_changes(&proposal);
                if chosen.is_empty() {
                    self.message = Some("Every change is left out; Space puts one back".into());
                    return Handled::Yes(None);
                }
                let only = if chosen.len() == proposal.changes.len() {
                    Vec::new()
                } else {
                    chosen
                };
                Handled::Yes(Some(Effect::Restructure(RestructureAction::Approve {
                    id: proposal.id,
                    only,
                })))
            }
            _ => {
                self.confirm = Some(Confirm::RejectRestructure {
                    id: proposal.id,
                    what: what(&proposal),
                });
                Handled::Yes(None)
            }
        }
    }
}

impl App {
    /// On Browse's collections: `E` chooses how eager Carrel is to rearrange the chosen
    /// collection, and `R` proposes a new shape for it, as `carrel restructure eagerness` and
    /// `carrel restructure propose` do.
    pub(super) fn collection_restructure_key(&mut self, key: KeyEvent) -> Option<Handled> {
        let collection = self.collections.get(self.browse_collection)?.path.clone();
        match key.code {
            KeyCode::Char('R') => Some(Handled::Yes(Some(Effect::Restructure(
                RestructureAction::ProposeSection(Some(collection)),
            )))),
            KeyCode::Char('E') => {
                let (level, source) = eagerness::at(&self.archive, &self.collections, &collection);
                self.message = Some(format!(
                    "{collection} is {} ({})",
                    level.as_str(),
                    match source {
                        Source::Archive => "the archive's setting".to_owned(),
                        Source::Collection(c) if c == collection => "its own".to_owned(),
                        Source::Collection(c) => format!("set by {c}"),
                    }
                ));
                let mut levels: Vec<String> = Eagerness::ALL
                    .iter()
                    .map(|l| l.as_str().to_owned())
                    .collect();
                levels.push("default".into());
                self.picker = Some(super::review::Picker {
                    collections: levels,
                    filter: String::new(),
                    chosen: 0,
                    purpose: super::review::Purpose::Eagerness(collection),
                });
                Some(Handled::Yes(None))
            }
            _ => None,
        }
    }
}

/// What a proposal reorganizes, for people.
#[must_use]
pub fn what(proposal: &Proposal) -> String {
    match proposal.kind {
        ProposalKind::Tags => "tags".to_owned(),
        ProposalKind::Section if proposal.section.is_empty() => "the whole archive".to_owned(),
        ProposalKind::Section => proposal.section.clone(),
    }
}

/// Review's keys while it shows restructure proposals.
#[must_use]
pub fn restructure_keys() -> Vec<(&'static str, &'static str)> {
    vec![
        ("a", "approve the chosen"),
        ("Space", "leave out"),
        ("x", "reject"),
        ("[ ]", "other proposal"),
        ("t", "propose tags"),
        ("p", "propose a shape"),
        ("f", "proposals"),
        ("?", "help"),
    ]
}

/// The confirmation for rejecting the proposal for `what`: its title, question, detail and
/// yes key.
#[must_use]
pub fn reject_restructure_confirm(what: &str) -> (&'static str, String, String, &'static str) {
    (
        " Reject ",
        format!("Reject the restructure proposal for {what}?"),
        "Nothing changes. Carrel may propose it again when the signals still say so.".into(),
        " reject   ",
    )
}

/// The work for `action`, on the archive at `root`.
pub(super) fn restructure_work(
    root: std::path::PathBuf,
    action: RestructureAction,
) -> (String, super::Work) {
    match action {
        RestructureAction::CrawlNow => super::review_crawler::crawler_work(root, false),
        RestructureAction::UndoCrawler => super::review_crawler::crawler_work(root, true),
        RestructureAction::Approve { id, only } => (
            "Restructuring…".into(),
            Box::new(move || {
                crate::restructure_sections::approve(Some(&root), Some(&id.to_string()), &only)
            }),
        ),
        RestructureAction::Reject(id) => (
            "Rejecting the proposal…".into(),
            Box::new(move || {
                crate::restructure_sections::reject(Some(&root), Some(&id.to_string()))
            }),
        ),
        RestructureAction::ProposeTags => (
            "Looking for tags to merge, split and retire…".into(),
            Box::new(move || crate::restructure::propose_tags(Some(&root))),
        ),
        RestructureAction::ProposeSection(collection) => (
            format!(
                "Working out a new shape for {}…",
                collection.as_deref().unwrap_or("the whole archive")
            ),
            Box::new(move || {
                crate::restructure_sections::propose(Some(&root), collection.as_deref(), false)
            }),
        ),
        RestructureAction::Eagerness { collection, level } => (
            format!("Setting how eager to rearrange {collection} is…"),
            Box::new(move || {
                crate::restructure_sections::eagerness(Some(&root), Some(&collection), Some(&level))
            }),
        ),
    }
}

/// Draws the proposal shown: its changes and the chosen one's detail, beside the archive now
/// and after.
pub fn draw_restructures(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let review = &app.restructure;
    let mut header = vec![super::review_crawler::lists_line(app)];
    let Some(proposal) = review.shown() else {
        header.push(String::new());
        header.push(
            "No restructure proposals are waiting. t proposes tag changes, p a new shape for \
             the whole archive; in Browse, R proposes one for a collection."
                .into(),
        );
        frame.render_widget(
            Paragraph::new(header.join("\n")).wrap(Wrap { trim: false }),
            area,
        );
        return;
    };
    header.push(format!(
        "{} of {}: {} · {}",
        review.chosen + 1,
        review.proposals.len(),
        what(proposal),
        proposal.invasiveness.describe()
    ));
    header.push(proposal.rationale.clone());
    let height = u16::try_from(header.len() + 1).unwrap_or(u16::MAX);
    let [header_area, rest] =
        Layout::vertical([Constraint::Length(height), Constraint::Min(0)]).areas(area);
    frame.render_widget(
        Paragraph::new(header.join("\n")).wrap(Wrap { trim: false }),
        header_area,
    );
    let [changes_area, tree_area] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(rest);
    let [list_area, detail_area] =
        Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)])
            .areas(changes_area);
    let chosen = review.chosen_changes(proposal);
    let items: Vec<ListItem> = proposal
        .changes
        .iter()
        .map(|c| {
            let mark = if chosen.contains(&c.id) { "[x]" } else { "[ ]" };
            ListItem::new(format!("{mark} {}. {}", c.id, c.change.describe()))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(review.change));
    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.selected()),
        list_area,
        &mut state,
    );
    if let Some(change) = proposal.changes.get(review.change) {
        let mut detail = crate::restructure::describe_change(change);
        writeln!(detail, "   {}", change.invasiveness.describe()).unwrap();
        frame.render_widget(
            Paragraph::new(detail).wrap(Wrap { trim: false }),
            detail_area,
        );
    }
    frame.render_widget(
        Paragraph::new(preview_lines(&review.preview)).wrap(Wrap { trim: false }),
        tree_area,
    );
}

/// The archive now and after, as indented trees of the collections that change and those
/// around them, then the tags the proposal touches.
fn preview_lines(preview: &Preview) -> String {
    let mut text = String::from("Now\n");
    tree(&mut text, &preview.before, "−");
    text.push_str("\nAfter the chosen changes\n");
    tree(&mut text, &preview.after, "+");
    if !preview.tags_before.is_empty() || !preview.tags_after.is_empty() {
        let tags = |list: &[carrel_core::restructure::preview::PreviewTag]| {
            if list.is_empty() {
                "none".to_owned()
            } else {
                list.iter()
                    .map(|t| format!("#{} ({})", t.tag, t.items))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        };
        write!(
            text,
            "\nTags now: {}\nTags after: {}\n",
            tags(&preview.tags_before),
            tags(&preview.tags_after)
        )
        .unwrap();
    }
    text
}

/// `collections` as an indented tree, each with how many items are directly in it, `mark`
/// before those the changes create or remove.
fn tree(text: &mut String, collections: &[PreviewCollection], mark: &str) {
    for c in collections {
        let depth = c.path.matches('/').count();
        let name = c.path.rsplit('/').next().unwrap_or(&c.path);
        let mark = if c.changed { mark } else { " " };
        writeln!(text, "{mark} {}{name} ({})", "  ".repeat(depth), c.items).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use carrel_core::restructure::{Proposal, ProposalKind};
    use ratatui::crossterm::event::KeyCode;
    use serde_json::json;

    use super::super::app::{Confirm, Effect, Tab};
    use super::super::review_crawler::ReviewList;
    use super::super::tests::{key, sample, screen};
    use super::RestructureAction;

    #[test]
    fn a_restructure_proposal_is_approved_in_part_beside_the_archive_after() {
        let (_dir, mut app) = sample(true);
        let mut proposal = Proposal::new(ProposalKind::Section, "Reading");
        proposal.rationale = "Reading has grown; give its papers a home.".into();
        proposal.changes = vec![
            serde_json::from_value(json!({
                "id": "1", "change": {"kind": "create-collection", "path": "Reading/Papers", "description": "Papers"},
                "reason": "A home for the papers.", "invasiveness": "subdividing",
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "id": "2", "change": {"kind": "rename-collection", "from": "Reading", "to": "Library"},
                "reason": "It holds more than reading now.", "invasiveness": "renaming",
            }))
            .unwrap(),
        ];
        carrel_core::restructure::queue(&app.archive, &proposal).unwrap();
        app.reload().unwrap();
        app.key(key(KeyCode::Char('2')));
        assert_eq!(app.tab, Tab::Review);
        app.key(key(KeyCode::Char('f')));
        app.key(key(KeyCode::Char('f')));
        assert_eq!(app.review_list, ReviewList::Restructure);
        let text = screen(&app, 160, 40);
        assert!(text.contains("1 of 1: Reading"), "{text}");
        assert!(text.contains("[x] 1."), "{text}");
        assert!(
            text.contains("A home for the papers."),
            "the chosen change's reason: {text}"
        );
        assert!(text.contains("After the chosen changes"), "{text}");
        assert!(
            text.contains("+   Papers"),
            "the new collection, after: {text}"
        );

        // Leave the rename out: the rest is approved, by number.
        app.key(key(KeyCode::Down));
        app.key(key(KeyCode::Char(' ')));
        assert!(screen(&app, 160, 40).contains("[ ] 2."));
        assert_eq!(
            app.key(key(KeyCode::Char('a'))),
            Some(Effect::Restructure(RestructureAction::Approve {
                id: proposal.id,
                only: vec!["1".into()],
            }))
        );
        app.key(key(KeyCode::Char('x')));
        assert_eq!(
            app.confirm,
            Some(Confirm::RejectRestructure {
                id: proposal.id,
                what: "Reading".into(),
            })
        );
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::Restructure(RestructureAction::Reject(proposal.id)))
        );
        assert_eq!(
            app.key(key(KeyCode::Char('t'))),
            Some(Effect::Restructure(RestructureAction::ProposeTags))
        );
    }
}

#[cfg(test)]
mod browse_tests {
    use ratatui::crossterm::event::KeyCode;

    use super::super::app::{Effect, Tab};
    use super::super::tests::{key, sample};
    use super::RestructureAction;

    #[test]
    fn a_collections_eagerness_is_chosen_and_a_shape_proposed_from_browse() {
        let (_dir, mut app) = sample(false);
        app.key(key(KeyCode::Char('5')));
        assert_eq!(app.tab, Tab::Browse);
        let collection = app.collections[app.browse_collection].path.clone();
        app.key(key(KeyCode::Char('E')));
        assert_eq!(
            app.message.as_deref(),
            Some(format!("{collection} is balanced (the archive's setting)").as_str())
        );
        let picker = app.picker.clone().expect("a choice of levels");
        assert_eq!(
            picker.collections,
            ["settled", "balanced", "eager", "default"]
        );
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::Restructure(RestructureAction::Eagerness {
                collection: collection.clone(),
                level: "settled".into(),
            }))
        );
        assert_eq!(
            app.key(key(KeyCode::Char('R'))),
            Some(Effect::Restructure(RestructureAction::ProposeSection(
                Some(collection)
            )))
        );
    }
}
