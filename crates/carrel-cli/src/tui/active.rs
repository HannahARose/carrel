//! The Active screen: what's checked out and what's on this device's desk, each with its
//! pending history entries, and returning, opening and deciding them, the way
//! `carrel checkouts`, `carrel desk`, `carrel return` and `carrel sessions` do
//! (`docs/design/checkout.md`).

use std::collections::BTreeSet;
use std::path::PathBuf;

use carrel_core::card::Id;
use carrel_core::card_file::CardFile;
use carrel_core::entries::HistoryEntry;
use carrel_core::manifest::Diff;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::{App, Confirm, Effect, step};
use super::review::Handled;
use crate::sessions::Decision;

/// Whether an active item is checked out of the archive or on the desk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Checked out: the working copy is a copy, returned to the archive.
    CheckedOut,
    /// On this device's desk: worked on where it is, not archived.
    Desk,
}

/// An item being worked on, as the Active screen shows it.
#[derive(Debug, Clone)]
pub struct ActiveItem {
    /// Checked out or on the desk.
    pub kind: Kind,
    /// Its card's identifier.
    pub id: Id,
    /// Its title.
    pub title: String,
    /// Its card: in the archive for a checkout, beside the item for a desk item.
    pub card: PathBuf,
    /// The working copy, or the desk item itself.
    pub working: PathBuf,
    /// When it was checked out or put on the desk.
    pub since: String,
    /// When a checkout is due back.
    pub due: Option<String>,
    /// How it compares with how it went out: `unchanged`, `changed`, `missing`, and so on.
    pub state: &'static str,
    /// What changed in a checkout's working copy, when it could tell.
    pub diff: Option<Diff>,
    /// Its pending history entries, in order: `1` is the first.
    pub pending: Vec<HistoryEntry>,
}

impl ActiveItem {
    /// How `carrel sessions` names it: a checkout by its identifier, a desk item by its card.
    pub fn query(&self) -> String {
        match self.kind {
            Kind::CheckedOut => self.id.to_string(),
            Kind::Desk => self.card.display().to_string(),
        }
    }
}

/// A row of the Active list: an item, or one of its pending entries (numbered from 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// The item at this position.
    Item(usize),
    /// The item's pending entry with this number.
    Entry(usize, usize),
}

/// The pending entries of the card at `path`.
fn pending(path: &std::path::Path) -> Vec<HistoryEntry> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(log) = CardFile::parse(&text).and_then(|f| f.history()) else {
        return Vec::new();
    };
    log.pending()
        .into_iter()
        .map(|i| log.entries[i].clone())
        .collect()
}

impl App {
    /// Reads what's checked out and what's on this device's desk.
    pub(super) fn load_active(&mut self) {
        let mut active = Vec::new();
        let rules = self.archive.rules().ok();
        for item in self.archive.items().unwrap_or_default() {
            let card = self.archive.root().join(&item.card);
            let Ok(text) = std::fs::read_to_string(&card) else {
                continue;
            };
            let Ok(file) = CardFile::parse(&text) else {
                continue;
            };
            let Some(out) = file.card().checked_out.clone() else {
                continue;
            };
            let working = PathBuf::from(&out.to);
            let (state, diff) = rules.as_ref().map_or(("not compared", None), |rules| {
                crate::checkout::compared(&self.archive, rules, item.id, &working)
            });
            active.push(ActiveItem {
                kind: Kind::CheckedOut,
                id: item.id,
                title: file.card().title.clone(),
                pending: pending(&card),
                card,
                working,
                since: out.since.0,
                due: out.due.map(|d| d.0),
                state,
                diff,
            });
        }
        let desk = self
            .device
            .and_then(|device| carrel_core::desk::Desk::load(&self.archive, device).ok())
            .unwrap_or_default();
        for entry in desk.items {
            let title = std::fs::read_to_string(&entry.card)
                .ok()
                .and_then(|t| CardFile::parse(&t).ok())
                .map_or_else(
                    || entry.path.display().to_string(),
                    |f| f.card().title.clone(),
                );
            active.push(ActiveItem {
                kind: Kind::Desk,
                id: entry.id,
                title,
                pending: pending(&entry.card),
                state: if entry.path.exists() {
                    "on the desk"
                } else {
                    "missing"
                },
                card: entry.card,
                working: entry.path,
                since: entry.added.0,
                due: None,
                diff: None,
            });
        }
        self.active = active;
        self.active_marked.retain(|(id, n)| {
            self.active
                .iter()
                .any(|a| a.id == *id && *n <= a.pending.len())
        });
        self.active_chosen = self
            .active_chosen
            .min(self.active_rows().len().saturating_sub(1));
    }

    /// The list's rows: each item, then its pending entries.
    pub fn active_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for (i, item) in self.active.iter().enumerate() {
            rows.push(Row::Item(i));
            rows.extend((1..=item.pending.len()).map(|n| Row::Entry(i, n)));
        }
        rows
    }

    /// The item the chosen row belongs to.
    pub fn active_item(&self) -> Option<&ActiveItem> {
        match self.active_rows().get(self.active_chosen)? {
            Row::Item(i) | Row::Entry(i, _) => self.active.get(*i),
        }
    }

    /// Moves the choice among rows.
    pub(super) fn active_move(&mut self, delta: i32) {
        self.active_chosen = step(self.active_chosen, delta, self.active_rows().len());
    }

    /// The Active screen's keys, for what's chosen: returning and cancelling are for
    /// checkouts, and deciding for items with pending entries. They need the room; the screen
    /// keys are in ? help.
    pub(super) fn active_keys(&self) -> Vec<(&'static str, &'static str)> {
        let mut keys = vec![("↑↓", "choose")];
        let Some(item) = self.active_item() else {
            keys.extend([("1-9", "screens"), ("?", "help"), ("q", "quit")]);
            return keys;
        };
        if item.kind == Kind::CheckedOut {
            keys.push(("r", "return"));
        }
        keys.push(("o", "open"));
        if !item.pending.is_empty() {
            keys.push(("a", "approve"));
            if matches!(
                self.active_rows().get(self.active_chosen),
                Some(Row::Entry(..))
            ) {
                keys.push(("e", "edit"));
            }
            keys.extend([("c", "combine"), ("d", "drop"), ("Space", "mark")]);
        }
        if item.kind == Kind::CheckedOut {
            keys.push(("x", "cancel"));
        }
        keys.extend([("?", "help"), ("q", "quit")]);
        keys
    }

    /// The marked entries of `item`, by number, or none to mean all of them.
    fn marked_numbers(&self, item: Id) -> Vec<usize> {
        self.active_marked
            .iter()
            .filter(|(id, _)| *id == item)
            .map(|(_, n)| *n)
            .collect()
    }

    /// Handles a key on the Active screen.
    pub(super) fn active_key(&mut self, key: KeyEvent) -> Handled {
        let KeyCode::Char(c @ ('r' | 'o' | 'a' | 'e' | 'c' | 'd' | ' ' | 'x')) = key.code else {
            return Handled::No;
        };
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            // Ctrl-R refreshes, as on every screen.
            return Handled::No;
        }
        let Some(item) = self.active_item().cloned() else {
            self.message = Some("Nothing is checked out or on the desk".into());
            return Handled::Yes(None);
        };
        Handled::Yes(match c {
            'o' => Some(Effect::Open(item.working)),
            'r' | 'x' if item.kind == Kind::Desk => {
                self.message = Some(
                    "A desk item isn't checked out, so there's nothing to return or cancel".into(),
                );
                None
            }
            'r' => {
                self.ask_to_return(&item);
                None
            }
            'x' => {
                self.confirm = Some(Confirm::CancelCheckout {
                    id: item.id,
                    title: item.title,
                });
                None
            }
            _ if item.pending.is_empty() => {
                self.message = Some(format!("“{}” has no pending history entries", item.title));
                None
            }
            'e' => {
                let row = self.active_rows().get(self.active_chosen).copied();
                match (row, item.pending.len()) {
                    (Some(Row::Entry(_, number)), _) => Some(Effect::EditEntry {
                        item: item.query(),
                        number,
                    }),
                    // With only one, there's no doubt which.
                    (_, 1) => Some(Effect::EditEntry {
                        item: item.query(),
                        number: 1,
                    }),
                    _ => {
                        self.message = Some("Choose an entry to edit it".into());
                        None
                    }
                }
            }
            ' ' => {
                if let Some(Row::Entry(_, n)) = self.active_rows().get(self.active_chosen) {
                    let mark = (item.id, *n);
                    if !self.active_marked.remove(&mark) {
                        self.active_marked.insert(mark);
                    }
                    self.active_move(1);
                } else {
                    self.message = Some("Choose an entry to mark it".into());
                }
                None
            }
            _ => Some(Effect::Entries {
                item: item.query(),
                numbers: self.marked_numbers(item.id),
                decision: match c {
                    'a' => Decision::Approve,
                    'c' => Decision::Combine,
                    _ => Decision::Drop,
                },
            }),
        })
    }

    /// Works out returning `item` and asks, showing what changed; or says why it can't.
    fn ask_to_return(&mut self, item: &ActiveItem) {
        let plan = self.archive.items().ok().and_then(|items| {
            let found = items.into_iter().find(|i| i.id == item.id)?;
            Some(carrel_core::checkout::plan_return(
                &self.archive,
                &found,
                None,
            ))
        });
        match plan {
            Some(Ok(plan)) => {
                self.confirm = Some(Confirm::Return {
                    id: item.id,
                    title: plan.title.clone(),
                    changes: crate::checkout::describe_return(&plan),
                });
            }
            Some(Err(e)) => self.message = Some(format!("Can't return it: {e}")),
            None => self.message = Some("It isn't in the archive any more".into()),
        }
    }

    /// The pending entries marked, for the line at the bottom.
    pub fn active_marked_note(&self) -> Option<String> {
        (self.tab == super::app::Tab::Active && !self.active_marked.is_empty())
            .then(|| format!("{} marked", self.active_marked.len()))
    }
}

/// The marks on pending entries: the item and the entry's number.
pub type Marks = BTreeSet<(Id, usize)>;
