//! An item's view in Browse (`docs/design/search.md`, Browsing): its card, every change that
//! touched it, with undo from there, and the items related to it: those its card links to,
//! and those nearest it in the embedding index.

use carrel_core::archive::Archive;
use carrel_core::card::Sensitivity;
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
    /// on.
    Link(String),
    /// It's among the items nearest the shown one in the embedding index, and this sensitive,
    /// so a restricted or sensitive one is marked.
    Near(Sensitivity),
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
    /// The items related to it: its links, then the items nearest it by meaning.
    pub related: Vec<Related>,
    /// Why no items near it by meaning are listed, when they can't be.
    pub links_only: Option<String>,
    /// The chosen row: a change, then a related item.
    pub chosen: usize,
    /// The item's file or folder, relative to the archive root, if it has one.
    pub item: Option<String>,
    /// Whether it's restricted, so it shows no preview.
    pub restricted: bool,
}

impl ItemView {
    /// The view of the item whose card is at `card`, with its changes from `history` and the
    /// items nearest it in the archive's embedding index, as `index` last read it from disk.
    pub fn load(
        archive: &Archive,
        card: &str,
        history: &[Entry],
        index: &mut crate::related::Cached,
    ) -> Self {
        let shown = match crate::commands::describe(archive, card) {
            Ok(shown) => shown,
            Err(crate::Failure(e)) => {
                return Self {
                    card: Some(card.to_owned()),
                    text: vec![e],
                    ..Self::default()
                };
            }
        };
        let near = index
            .get(archive)
            .and_then(|index| crate::related::nearest(archive, index, &shown));
        let links = shown.links.iter().map(|link| Related {
            relation: Relation::Link(link.link_type.clone()),
            title: link.title.clone().unwrap_or_else(|| link.id.to_string()),
            card: link.card.clone(),
        });
        let nearby = near.as_deref().unwrap_or_default().iter().map(|n| Related {
            relation: Relation::Near(n.sensitivity),
            title: n.title.clone(),
            card: Some(n.card.clone()),
        });
        Self {
            card: Some(card.to_owned()),
            text: shown.text.lines().map(str::to_owned).collect(),
            history: history
                .iter()
                .filter(|e| e.touches(shown.id))
                .cloned()
                .collect(),
            related: links.chain(nearby).collect(),
            links_only: near.err(),
            chosen: 0,
            item: shown
                .items
                .iter()
                .find(|i| i.card == shown.card_path)
                .and_then(|i| i.item.clone()),
            restricted: shown.card.sensitivity == Sensitivity::Restricted,
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
        let mut view = ItemView::load(&self.archive, card, &self.history, &mut self.index);
        if view.card == self.item.card {
            view.chosen = self.item.chosen.min(view.rows().len().saturating_sub(1));
        }
        self.item = view;
        self.previews.want(&self.archive, &self.item);
        // Showing an item is opening its collection, as `carrel show` does.
        if self.tab == super::app::Tab::Browse
            && let Some((collection, _)) = card.rsplit_once('/')
        {
            let collection = collection.to_owned();
            self.records.opened(&collection);
        }
        self.load_item_questions();
    }

    /// Handles a key in an item's view: ↑↓ choose a change or related item, `u` undoes the
    /// chosen change, Enter goes to the chosen related item, linked or near it, and ← or Esc go back to the items.
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

    /// Browse's keys, for the help line: what the pane the arrows move in answers to. They
    /// need the room; the screen keys are in ? help.
    pub(super) fn browse_keys(&self) -> Vec<(&'static str, &'static str)> {
        let mut keys = vec![("↑↓", "choose")];
        if self.browse_pane != Pane::Collections && !self.item_questions.is_empty() {
            keys.push(("a", "answer its question"));
        }
        match self.browse_pane {
            Pane::Collections => keys.extend([
                ("→", "items"),
                ("n", "new collection"),
                ("E", "eagerness"),
                ("R", "propose a shape"),
            ]),
            Pane::Items => keys.push(("←→", "collections/item")),
            Pane::Item => {
                keys.push(("←", "items"));
                match self.item.chosen_row() {
                    Some(Row::Change(_)) => keys.push(("u", "undo")),
                    Some(Row::Related(_)) => keys.push(("Enter", "go to it")),
                    None => {}
                }
                keys.extend([("w", "wander from it"), ("A", "its questions")]);
            }
        }
        if self.browse_pane != Pane::Collections && !self.browse_items.is_empty() {
            keys.extend([
                ("e", "edit"),
                ("E", "edit body"),
                ("c", "correct"),
                ("o", "check out"),
                ("r", "rename"),
            ]);
            if self.browsed_is_compressed() {
                keys.push(("z", "expand"));
            }
        }
        keys.extend([("/", "search"), ("?", "help"), ("q", "quit")]);
        keys
    }

    /// Whether the chosen item looks like a compressed file, by its name, so `z` is offered.
    fn browsed_is_compressed(&self) -> bool {
        self.browse_items
            .get(self.browse_item)
            .and_then(|(card, _)| card.strip_suffix(".md"))
            .and_then(|item| item.rsplit('/').next())
            .and_then(carrel_core::detect::Compressed::from_name)
            .is_some()
    }

    /// Moves the choice among the item's rows.
    pub(super) fn item_move(&mut self, delta: i32) {
        self.item.chosen = step(self.item.chosen, delta, self.item.rows().len());
    }

    /// Shows the item whose card is at `card` in its collection, leaving any search.
    pub(super) fn go_to(&mut self, card: &str) {
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
