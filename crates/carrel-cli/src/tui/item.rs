//! An item's view in Browse (`docs/design/search.md`, Browsing): its card, every change that
//! touched it, with undo from there, and the items related to it.

use carrel_core::archive::Archive;
use carrel_core::history::Entry;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::{App, Confirm, Effect, step};
use super::review::Handled;

/// Where Browse's arrows move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    /// Among the collections.
    Collections,
    /// Among the chosen collection's items, or what a search found.
    Items,
    /// Among the chosen item's changes and related items.
    Item,
}

/// Why an item is related to the one shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relation {
    /// The shown item's card links to it this way: `version-of`, `related`, `part-of`, and so
    /// on. The items nearest it in the embedding index join these once there is one.
    Link(String),
}

/// An item related to the one shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Related {
    /// Why it's related.
    pub relation: Relation,
    /// Its card's path, if it's in the archive.
    pub card: Option<String>,
    /// Its title, or its identifier when its card can't be read.
    pub title: String,
}

/// A row the owner can choose in an item's view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row<'a> {
    /// A change that touched the item.
    Change(&'a Entry),
    /// An item related to it.
    Related(&'a Related),
}

/// The chosen item, as Browse shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemView {
    /// The card's path.
    pub card: Option<String>,
    /// The card's fields, summary and problems, as `carrel show` gives them.
    pub text: Vec<String>,
    /// Every change that touched it, newest first.
    pub history: Vec<Entry>,
    /// The items related to it.
    pub related: Vec<Related>,
    /// The chosen row: a change, then a related item.
    pub chosen: usize,
}

impl ItemView {
    /// The view of the item whose card is at `card`, with its changes from `history`.
    pub fn load(archive: &Archive, card: &str, history: &[Entry]) -> Self {
        match crate::commands::describe(archive, card) {
            Ok(shown) => Self {
                card: Some(card.to_owned()),
                text: shown.text.lines().map(str::to_owned).collect(),
                history: history
                    .iter()
                    .filter(|e| e.touches(shown.id))
                    .cloned()
                    .collect(),
                related: shown
                    .links
                    .into_iter()
                    .map(|link| Related {
                        relation: Relation::Link(link.link_type),
                        title: link.title.unwrap_or_else(|| link.id.to_string()),
                        card: link.card,
                    })
                    .collect(),
                chosen: 0,
            },
            Err(crate::Failure(e)) => Self {
                card: Some(card.to_owned()),
                text: vec![e],
                ..Self::default()
            },
        }
    }

    /// The rows to choose from: the changes, then the related items.
    pub fn rows(&self) -> Vec<Row<'_>> {
        self.history
            .iter()
            .map(Row::Change)
            .chain(self.related.iter().map(Row::Related))
            .collect()
    }

    /// The chosen row, if there is one.
    pub fn chosen_row(&self) -> Option<Row<'_>> {
        self.rows().get(self.chosen).copied()
    }
}

impl App {
    /// Reads the chosen item's view, keeping the chosen row if it's the same item.
    pub(super) fn load_card(&mut self) {
        let Some((card, _)) = self.browse_items.get(self.browse_item) else {
            self.item = ItemView::default();
            return;
        };
        let mut view = ItemView::load(&self.archive, card, &self.history);
        if view.card == self.item.card {
            view.chosen = self.item.chosen.min(view.rows().len().saturating_sub(1));
        }
        self.item = view;
    }

    /// Handles a key in an item's view: ↑↓ choose a change or related item, `u` undoes the
    /// chosen change, Enter goes to the chosen related item, and ← or Esc go back to the items.
    pub(super) fn item_key(&mut self, key: KeyEvent) -> Handled {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Esc => {
                self.browse_pane = Pane::Items;
            }
            KeyCode::Char('u') => match self.item.chosen_row() {
                Some(Row::Change(entry)) => {
                    self.confirm = Some(Confirm::Undo {
                        id: entry.id.clone(),
                        summary: entry.summary.clone(),
                    });
                }
                _ => self.message = Some("Choose a change to undo".into()),
            },
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => match self.item.chosen_row() {
                Some(Row::Related(Related {
                    card: Some(card), ..
                })) => {
                    let card = card.clone();
                    self.go_to(&card);
                }
                Some(Row::Related(related)) => {
                    self.message = Some(format!("{} isn't in this archive", related.title));
                }
                _ => {}
            },
            _ => return Handled::No,
        }
        Handled::Yes(None::<Effect>)
    }

    /// Browse's keys, for the help line: what the pane the arrows move in answers to.
    pub(super) fn browse_keys(&self) -> Vec<(&'static str, &'static str)> {
        match self.browse_pane {
            Pane::Collections => vec![("↑↓", "choose"), ("→", "items"), ("/", "search")],
            Pane::Items => vec![
                ("↑↓", "choose"),
                ("←→", "collections/item"),
                ("e", "edit"),
                ("c", "correct"),
                ("/", "search"),
            ],
            Pane::Item => {
                let mut keys = vec![("↑↓", "choose"), ("←", "items")];
                match self.item.chosen_row() {
                    Some(Row::Change(_)) => keys.push(("u", "undo")),
                    Some(Row::Related(_)) => keys.push(("Enter", "go to it")),
                    None => {}
                }
                keys.extend([("e", "edit"), ("c", "correct")]);
                keys
            }
        }
    }

    /// Moves the choice among the item's rows.
    pub(super) fn item_move(&mut self, delta: i32) {
        self.item.chosen = step(self.item.chosen, delta, self.item.rows().len());
    }

    /// Shows the item whose card is at `card` in its collection, leaving any search.
    fn go_to(&mut self, card: &str) {
        self.search = None;
        if let Some(i) = self
            .collections
            .iter()
            .enumerate()
            .filter(|(_, c)| card.starts_with(&format!("{}/", c.path)))
            .max_by_key(|(_, c)| c.path.len())
            .map(|(i, _)| i)
        {
            self.browse_collection = i;
        }
        self.browse_item = 0;
        self.load_items();
        match self.browse_items.iter().position(|(path, _)| path == card) {
            Some(i) => {
                self.browse_item = i;
                self.browse_pane = Pane::Items;
                self.load_card();
            }
            None => self.message = Some(format!("{card} isn't listed in a collection")),
        }
    }
}
