//! The Active screen: what's checked out and what's on this device's desk, each with its
//! pending history entries, and returning, opening and deciding them, the way
//! `carrel checkouts`, `carrel desk`, `carrel return` and `carrel sessions` do
//! (`docs/design/checkout.md`).

use std::collections::BTreeSet;

use carrel_core::after_return::AfterReturn;
use carrel_core::card::Id;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::{App, Confirm, Effect, step};
use super::review::Handled;
use crate::sessions::Decision;

pub use carrel_core::active::{ActiveItem, Kind};

/// A checkout to return, as `carrel return ITEM [--from PATH] [--trim-snapshots]` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Returning {
    /// The item.
    pub id: Id,
    /// Where the working copy is now, if it moved.
    pub from: Option<std::path::PathBuf>,
    /// Whether to forget the checkout's session snapshots once it's returned.
    pub trim: bool,
}

/// A row of the Active list: an item, or one of its pending entries (numbered from 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// The item at this position.
    Item(usize),
    /// The item's pending entry with this number.
    Entry(usize, usize),
}

impl App {
    /// Reads what's checked out and what's on this device's desk.
    pub(super) fn load_active(&mut self) {
        let active = carrel_core::active::items(&self.archive, self.device);
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
    /// checkouts, taking off and archiving for desk items, and deciding for items with pending
    /// entries. They need the room; the screen keys are in ? help.
    pub(super) fn active_keys(&self) -> Vec<(&'static str, &'static str)> {
        let mut keys = vec![("↑↓", "choose")];
        let Some(item) = self.active_item() else {
            keys.extend([
                ("n", "add to desk"),
                ("1-9", "screens"),
                ("?", "help"),
                ("q", "quit"),
            ]);
            return keys;
        };
        if item.kind == Kind::Desk {
            // With a checkout chosen there's no room; it's in ? help.
            keys.push(("n", "add to desk"));
        }
        if item.kind == Kind::CheckedOut {
            // `m`, for a working copy that moved, has no room; it's in ? help.
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
        match item.kind {
            Kind::CheckedOut => keys.push(("x", "cancel")),
            Kind::Desk => keys.extend([("x", "take off"), ("f", "archive")]),
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
        let KeyCode::Char(c @ ('r' | 'm' | 'o' | 'a' | 'e' | 'c' | 'd' | ' ' | 'x' | 'n' | 'f')) =
            key.code
        else {
            return Handled::No;
        };
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            // Ctrl-R refreshes, as on every screen.
            return Handled::No;
        }
        if c == 'n' {
            self.picking = super::app::Picking::DeskItem;
            self.message =
                Some("Choose a file or folder to put on the desk; it stays where it is".into());
            return Handled::Yes(self.start_picker());
        }
        let Some(item) = self.active_item().cloned() else {
            self.message = Some("Nothing is checked out or on the desk".into());
            return Handled::Yes(None);
        };
        Handled::Yes(match c {
            'o' => Some(Effect::Open(item.working)),
            'r' | 'm' if item.kind == Kind::Desk => {
                self.message =
                    Some("A desk item isn't checked out, so there's nothing to return".into());
                None
            }
            'x' if item.kind == Kind::Desk => {
                self.confirm = Some(Confirm::DeskRemove {
                    path: item.working,
                    title: item.title,
                });
                None
            }
            'f' if item.kind == Kind::Desk => {
                self.confirm = Some(Confirm::DeskArchive {
                    path: item.working,
                    title: item.title,
                });
                None
            }
            'f' => {
                self.message = Some(
                    "A checkout is already in the archive; return it to bring its changes back"
                        .into(),
                );
                None
            }
            'r' | 'm' => {
                self.start_return(c, item.id);
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

    /// `r` asks to return the checkout `id` from where it was checked out to; `m` first asks
    /// where its working copy moved to.
    fn start_return(&mut self, c: char, id: Id) {
        if c == 'm' {
            self.ask_option(super::option_lines::OptionLine::ReturnFrom { id });
        } else {
            self.ask_to_return(id, None);
        }
    }

    /// Works out returning the checkout `id`, from `from` if its working copy moved, and asks,
    /// showing what changed; or says why it can't.
    pub(super) fn ask_to_return(&mut self, id: Id, from: Option<std::path::PathBuf>) {
        let from = from.map(|p| crate::paths::expand(&p.to_string_lossy()));
        let plan = self.archive.items().ok().and_then(|items| {
            let found = items.into_iter().find(|i| i.id == id)?;
            Some(carrel_core::checkout::plan_return(
                &self.archive,
                &found,
                from.as_deref(),
            ))
        });
        match plan {
            Some(Ok(plan)) => {
                self.confirm = Some(Confirm::Return {
                    id,
                    title: plan.title.clone(),
                    changes: crate::checkout::describe_return(&plan),
                    from,
                    snapshots: self.backups_set_up(),
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

impl App {
    /// Notes the working copy of the checkout `returning` returns, when `checkout.after_return`
    /// says to ask about it once it's back.
    pub(super) fn note_returning(&mut self, returning: &Returning) {
        if AfterReturn::of(self.archive.settings()) != AfterReturn::Ask {
            return;
        }
        self.returning_working_copy = returning.from.clone().or_else(|| {
            self.active
                .iter()
                .find(|a| a.id == returning.id)
                .map(|a| a.working.clone())
        });
    }

    /// After background work: when it was a return that `checkout.after_return` asks about,
    /// and it `succeeded`, asks whether to put the working copy in the Trash.
    pub(super) fn ask_about_working_copy(&mut self, succeeded: bool) {
        if let Some(working) = self.returning_working_copy.take()
            && succeeded
            && working.exists()
        {
            self.confirm = Some(Confirm::TrashWorkingCopy(working));
        }
    }
}

/// The confirmation for putting a returned working copy in the Trash: its title, question,
/// detail and yes key.
#[must_use]
pub fn trash_working_copy_confirm(
    working: &std::path::Path,
) -> (&'static str, String, String, &'static str) {
    (
        " Returned ",
        "Put the working copy in the Trash?".into(),
        format!(
            "{} is returned, so the archive has its changes. It can be recovered from the Trash. checkout.after_return can do this, or keep it, without asking.",
            working.display()
        ),
        " trash it   ",
    )
}

/// The work for putting a returned working copy in the Trash, for the archive at `root`.
pub(super) fn trash_working_copy_work(
    root: std::path::PathBuf,
    working: std::path::PathBuf,
) -> (String, super::Work) {
    (
        "Putting the working copy in the Trash…".into(),
        Box::new(move || {
            carrel_core::after_return::trash_working_copy(&root, &working)
                .map_err(|e| crate::Failure::new(e.to_string()))?;
            Ok(crate::Report::new(
                format!("Put {} in the Trash.\n", working.display()),
                serde_json::json!({ "working_copy": "trashed", "path": working }),
            ))
        }),
    )
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::KeyCode;

    use super::super::app::{Confirm, Effect, Tab};
    use super::super::tests::{active_sample, choose_parts_list, key};
    use super::Returning;

    #[test]
    fn a_returned_working_copy_is_asked_about_once_it_is_back() {
        let (_dir, mut app) = active_sample();
        app.tab = Tab::Active;
        let item = choose_parts_list(&mut app);
        app.confirm = Some(Confirm::Return {
            id: item.id,
            title: item.title.clone(),
            changes: String::new(),
            from: None,
            snapshots: false,
        });
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::Return(Returning {
                id: item.id,
                from: None,
                trim: false,
            }))
        );

        // checkout.after_return asks by default: once it's back, the working copy is asked about.
        app.ask_about_working_copy(true);
        assert_eq!(
            app.confirm,
            Some(Confirm::TrashWorkingCopy(item.working.clone()))
        );
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::TrashWorkingCopy(item.working.clone()))
        );

        // A return that failed asks nothing.
        app.returning_working_copy = Some(item.working);
        app.ask_about_working_copy(false);
        assert_eq!(app.confirm, None);
        assert_eq!(app.returning_working_copy, None);
    }
}
