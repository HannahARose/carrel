//! Browse's actions on the archive, each asked about first and each one change undo can
//! reverse, as the commands are: checking an item out (`carrel checkout`), creating a
//! collection (`carrel collection new`), renaming an item (`carrel rename`) and expanding a
//! filed compressed file (`carrel expand`). The commands do the work; this asks for what they
//! need.

use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::{App, Confirm, Effect, Picking};
use super::review::Handled;
use super::screens::{Input, InputPurpose};

/// A collection to create: its path, and what its design document starts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCollection {
    /// Its path in the archive, such as `Projects/Lamp`.
    pub path: String,
    /// What belongs there, in a sentence.
    pub description: Option<String>,
    /// Its tags, as `name=meaning`.
    pub tags: Vec<String>,
}

/// The title and the line above a typed line, for the lines this module asks for.
pub fn input_text(purpose: &InputPurpose) -> Option<(&'static str, String)> {
    Some(match purpose {
        InputPurpose::CheckoutFolder => (
            " Check out ",
            "The folder to put the working copy in (drop it here, or paste):".to_owned(),
        ),
        InputPurpose::Due { title, .. } => (
            " Check out ",
            format!(
                "When is “{title}” due back? A date as YYYY-MM-DD, or nothing for no due date:"
            ),
        ),
        InputPurpose::DeskPath => (
            " Desk ",
            "The file or folder to put on the desk, where it is (drop it here, or paste):"
                .to_owned(),
        ),
        InputPurpose::CollectionPath => (
            " New collection ",
            "The new collection's path in the archive, such as Projects/Lamp:".to_owned(),
        ),
        InputPurpose::CollectionDescription { path } => (
            " New collection ",
            format!(
                "What belongs in {path}, in a sentence? Models read it before filing anything. \
                 Leave it empty to write it in the design document later:"
            ),
        ),
        InputPurpose::CollectionTags { path, .. } => (
            " New collection ",
            format!(
                "Tags for {path}, as name=meaning, separated by ; (such as \
                 receipts=proof of purchase). Leave it empty for none:"
            ),
        ),
        InputPurpose::Rename { from, .. } => (
            " Rename ",
            format!(
                "A new name for “{from}”: the item and its card are renamed together, and \
                 links to it follow. The name shown works on every system:"
            ),
        ),
        _ => return None,
    })
}

impl App {
    /// The chosen item in Browse, as its card's path and title.
    fn browsed(&self) -> Option<(String, String)> {
        self.browse_items.get(self.browse_item).cloned()
    }

    /// Handles Browse's action keys: `n` creates a collection anywhere in Browse; on an item,
    /// `o` checks it out, `r` renames it and `z` expands it.
    pub(super) fn browse_action_key(&mut self, key: KeyEvent, on_item: bool) -> Handled {
        match key.code {
            KeyCode::Char('n') => {
                let start = self
                    .collections
                    .get(self.browse_collection)
                    .filter(|_| self.search.is_none())
                    .map(|c| format!("{}/", c.path))
                    .unwrap_or_default();
                self.input = Some(Input {
                    purpose: InputPurpose::CollectionPath,
                    text: start,
                });
            }
            KeyCode::Char('o' | 'r' | 'z') if on_item => {
                let Some((card, title)) = self.browsed() else {
                    return Handled::No;
                };
                return Handled::Yes(match key.code {
                    KeyCode::Char('o') => self.start_checkout(card, title),
                    KeyCode::Char('r') => {
                        self.ask_new_name(card);
                        None
                    }
                    _ => {
                        self.ask_to_expand(card, title);
                        None
                    }
                });
            }
            _ => return Handled::No,
        }
        Handled::Yes(None)
    }

    /// Starts checking out the item at `card`: the folder first, from the owner's file picker
    /// or the built-in browser.
    fn start_checkout(&mut self, card: String, title: String) -> Option<Effect> {
        self.message = Some(format!("Choose the folder to check “{title}” out to"));
        self.checking_out = Some((card, title));
        self.picking = Picking::CheckoutFolder;
        self.start_picker()
    }

    /// Opens the line to type or paste the path being picked, for a checkout or the desk.
    pub(super) fn ask_for_path(&mut self) {
        self.input = Some(Input {
            purpose: if self.picking == Picking::DeskItem {
                InputPurpose::DeskPath
            } else {
                InputPurpose::CheckoutFolder
            },
            text: String::new(),
        });
    }

    /// Goes on with what was picked for a checkout or the desk: the due date for a
    /// checkout's folder (the folder a picked file is in), or asking to put it on the desk.
    pub(super) fn picked_path(&mut self, paths: &[PathBuf]) {
        let picking = std::mem::replace(&mut self.picking, Picking::Ingest);
        let Some(first) = paths.first().cloned() else {
            self.checking_out = None;
            self.message = Some("Nothing chosen; nothing changed".into());
            return;
        };
        if picking == Picking::DeskItem {
            self.tab = super::app::Tab::Active;
            self.confirm = Some(Confirm::DeskAdd(first));
            return;
        }
        let Some((card, title)) = self.checking_out.take() else {
            return;
        };
        let to = if first.is_file() {
            first.parent().map_or_else(|| first.clone(), PathBuf::from)
        } else {
            first
        };
        self.tab = super::app::Tab::Browse;
        self.input = Some(Input {
            purpose: InputPurpose::Due { card, title, to },
            text: String::new(),
        });
    }

    /// Opens the line for the item's new name, starting from the name `carrel rename`
    /// suggests: one that works on every system.
    fn ask_new_name(&mut self, card: String) {
        let from = self
            .archive
            .items()
            .ok()
            .and_then(|items| items.into_iter().find(|i| i.card == card))
            .map(|i| i.item.unwrap_or(i.card))
            .map_or_else(|| card.clone(), |path| name_of(&path).to_owned());
        let suggested = carrel_core::names::portable(&from);
        self.input = Some(Input {
            purpose: InputPurpose::Rename { card, from },
            text: suggested,
        });
    }

    /// Asks before expanding the item at `card`, saying why it's kept compressed when it's
    /// expanded all the same; or says why it can't be.
    fn ask_to_expand(&mut self, card: String, title: String) {
        let item = self
            .archive
            .items()
            .ok()
            .and_then(|items| items.into_iter().find(|i| i.card == card));
        let Some(item) = item else {
            self.message = Some("It isn't in the archive any more".into());
            return;
        };
        match carrel_core::compressed::item::examine(&self.archive, &item) {
            Ok(anyway) => {
                self.confirm = Some(Confirm::Expand {
                    card,
                    title,
                    anyway,
                });
            }
            Err(e) => self.message = Some(format!("Can't expand it: {e}")),
        }
    }

    /// What happens when a line this module asked for is entered.
    pub(super) fn action_input(&mut self, purpose: InputPurpose, text: String) -> Option<Effect> {
        match purpose {
            InputPurpose::CheckoutFolder | InputPurpose::DeskPath => {
                self.picking = if purpose == InputPurpose::DeskPath {
                    Picking::DeskItem
                } else {
                    Picking::CheckoutFolder
                };
                self.picked_path(&crate::paths::split_paths(&text));
            }
            InputPurpose::Due { card, title, to } => {
                if !text.is_empty() && text.parse::<jiff::civil::Date>().is_err() {
                    self.message = Some(format!("{text} isn't a date; give it as YYYY-MM-DD"));
                    self.input = Some(Input {
                        purpose: InputPurpose::Due { card, title, to },
                        text,
                    });
                } else {
                    self.confirm = Some(Confirm::Checkout {
                        card,
                        title,
                        to,
                        due: (!text.is_empty()).then_some(text),
                    });
                }
            }
            InputPurpose::CollectionPath => {
                let path = text.trim_matches('/').to_owned();
                if path.is_empty() {
                    self.message = Some("No path given; nothing created".into());
                } else {
                    self.input = Some(Input {
                        purpose: InputPurpose::CollectionDescription { path },
                        text: String::new(),
                    });
                }
            }
            InputPurpose::CollectionDescription { path } => {
                self.input = Some(Input {
                    purpose: InputPurpose::CollectionTags {
                        path,
                        description: (!text.is_empty()).then_some(text),
                    },
                    text: String::new(),
                });
            }
            InputPurpose::CollectionTags { path, description } => {
                let tags = text
                    .split(';')
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_owned)
                    .collect();
                self.confirm = Some(Confirm::NewCollection(NewCollection {
                    path,
                    description,
                    tags,
                }));
            }
            InputPurpose::Rename { card, from } => {
                if text.is_empty() || text == from {
                    self.message = Some("Same name; nothing changed".into());
                } else {
                    self.confirm = Some(Confirm::Rename {
                        card,
                        from,
                        to: text,
                    });
                }
            }
            _ => {}
        }
        None
    }
}

/// The last part of a path in the archive.
fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// What a confirmation this module asks shows: its title, the question, what saying yes does,
/// and the word for yes.
pub fn confirm_text(confirm: &Confirm) -> (&'static str, String, String, &'static str) {
    const UNDO: &str = "It's one change, which undo can reverse from History.";
    match confirm {
        Confirm::Checkout { title, to, due, .. } => (
            " Check out ",
            format!("Check out “{title}” to {}?", to.display()),
            format!(
                "A copy goes there to work on{}. The archive copy is read-only until it's \
                 returned from Active.",
                due.as_deref()
                    .map(|d| format!(", due back {d}"))
                    .unwrap_or_default()
            ),
            " check it out   ",
        ),
        Confirm::DeskAdd(path) => (
            " Desk ",
            format!("Put {} on the desk?", path.display()),
            "It stays where it is: a card goes beside it, drafted by this device's text model, \
             and Active lists it with its work sessions."
                .to_owned(),
            " put it on   ",
        ),
        Confirm::DeskRemove { title, .. } => (
            " Desk ",
            format!("Take “{title}” off the desk?"),
            "The file or folder itself isn't touched. Its card stays beside it, unless you \
             delete the card too."
                .to_owned(),
            " take it off   ",
        ),
        Confirm::DeskArchive { title, .. } => (
            " Archive ",
            format!("Archive “{title}”?"),
            "It's queued for ingest, keeping its card and history, and taken off the desk. A \
             worker drafts its proposal, which waits in Review; nothing moves into the archive \
             until you approve it."
                .to_owned(),
            " archive it   ",
        ),
        Confirm::NewCollection(new) => {
            let mut about = String::from("A folder with a design document");
            if let Some(description) = &new.description {
                about.push_str(&format!(" starting “{description}”"));
            }
            if !new.tags.is_empty() {
                about.push_str(&format!(", and the tags {}", new.tags.join(", ")));
            }
            (
                " New collection ",
                format!("Create the collection {}?", new.path),
                format!(
                    "{about}. Say what belongs there in the design document: models read it \
                     before filing anything. {UNDO}"
                ),
                " create it   ",
            )
        }
        Confirm::Rename { from, to, .. } => (
            " Rename ",
            format!("Rename “{from}” to “{to}”?"),
            format!("The item and its card are renamed together, and links to it follow. {UNDO}"),
            " rename it   ",
        ),
        Confirm::Expand { title, anyway, .. } => (
            " Expand ",
            if anyway.is_some() {
                format!("Expand “{title}” all the same?")
            } else {
                format!("Expand “{title}”?")
            },
            format!(
                "{}What it holds takes its place, and the compressed file goes to the Trash. \
                 {UNDO} The compressed file then comes back from the Trash.",
                anyway
                    .as_deref()
                    .map(|why| format!("It's kept compressed: {why}. "))
                    .unwrap_or_default()
            ),
            " expand it   ",
        ),
        _ => (" ", String::new(), String::new(), " yes   "),
    }
}
