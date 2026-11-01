//! Flagging in Browse (FORMAT.md §5.9), as `carrel flag` and `carrel unflag` do: `f` flags
//! the chosen item or collection with a note saying what needs work, or offers to clear its
//! flag; `F` lists the flagged items, as a search for `flagged` does. Flagged collections and
//! items carry a ⚑ in the lists, Status lists them, and once an edit or a correction is saved
//! on a flagged card, Browse offers to clear its flag.

use std::collections::BTreeSet;
use std::path::Path;

use carrel_core::card_file::CardFile;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::{App, Confirm, Effect};
use super::item::Pane;
use super::review::Handled;
use super::screens::{Input, InputPurpose};

/// The mark flagged things carry in the lists.
pub const MARK: &str = "⚑ ";

/// Setting or clearing a flag, on an item by its card's path or a collection by its folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flagging {
    /// Flag it, with what needs doing.
    Set {
        /// The item's card or the collection's folder.
        target: String,
        /// What needs doing, if the owner said.
        note: Option<String>,
    },
    /// Clear its flag.
    Clear {
        /// The item's card or the collection's folder.
        target: String,
    },
}

impl App {
    /// Handles `f` and `F` in Browse, on the collections (`on_item` false) or an item.
    pub(super) fn flag_key(&mut self, key: KeyEvent, on_item: bool) -> Option<Handled> {
        match key.code {
            KeyCode::Char('f') => {
                let (target, title, flag) = if on_item {
                    let (card, title) = self.browse_items.get(self.browse_item)?.clone();
                    let flag = self.flag_of_card(&card);
                    (card, title, flag)
                } else {
                    let collection = self.collections.get(self.browse_collection)?;
                    (
                        collection.path.clone(),
                        collection.doc.title.clone(),
                        collection.doc.needs_work.clone(),
                    )
                };
                match flag {
                    Some(flag) => {
                        self.confirm = Some(Confirm::Unflag {
                            target,
                            title,
                            note: flag.note,
                        });
                    }
                    None => {
                        self.input = Some(Input {
                            purpose: InputPurpose::FlagNote { target, title },
                            text: String::new(),
                        });
                    }
                }
                Some(Handled::Yes(None))
            }
            KeyCode::Char('F') => {
                self.search = Some("flagged".into());
                self.browse_item = 0;
                self.load_items();
                self.browse_pane = Pane::Items;
                if self.browse_items.is_empty() {
                    self.message = Some(
                        "No items are flagged; flagged collections carry a ⚑ in the list".into(),
                    );
                }
                Some(Handled::Yes(None))
            }
            _ => None,
        }
    }

    /// The flag on the card at `card`, if it has one.
    fn flag_of_card(&self, card: &str) -> Option<carrel_core::card::NeedsWork> {
        let text = std::fs::read_to_string(self.archive.root().join(card)).ok()?;
        CardFile::parse(&text).ok()?.card().needs_work.clone()
    }

    /// The listed cards that are flagged, for their ⚑.
    pub(super) fn flagged_cards(&self) -> BTreeSet<String> {
        self.browse_items
            .iter()
            .filter(|(card, _)| self.flag_of_card(card).is_some())
            .map(|(card, _)| card.clone())
            .collect()
    }

    /// After an edit or a correction of the card at `card` was saved: offers to clear its
    /// flag, when it has one and nothing else is being asked.
    pub(super) fn offer_to_unflag(&mut self, card: Option<&str>) {
        let Some(card) = card.filter(|_| self.confirm.is_none()) else {
            return;
        };
        let Some(flag) = self.flag_of_card(card) else {
            return;
        };
        let title = self
            .browse_items
            .iter()
            .find(|(c, _)| c == card)
            .map_or_else(|| card.to_owned(), |(_, t)| t.clone());
        self.confirm = Some(Confirm::Unflag {
            target: card.to_owned(),
            title,
            note: flag.note,
        });
    }
}

/// The note the owner typed for flagging `target`: flags it.
pub(super) fn flag_note(target: String, text: &str) -> Effect {
    let note = text.trim();
    Effect::Flag(Flagging::Set {
        target,
        note: (!note.is_empty()).then(|| note.to_owned()),
    })
}

/// The card an edit (`effect`) saved, so its flag can be offered to clear: none when the edit
/// failed or changed nothing.
pub(super) fn edited_card(
    effect: &Effect,
    result: &Result<crate::Report, crate::Failure>,
) -> Option<String> {
    match (effect, result) {
        (Effect::EditItem(card) | Effect::EditItemBody(card), Ok(report))
            if report.json["commit"].is_string() =>
        {
            Some(card.clone())
        }
        _ => None,
    }
}

/// What the confirmation to clear a flag shows: its title, the question, what saying yes
/// does, and the word for yes.
pub fn confirm_text(
    title: &str,
    note: Option<&str>,
) -> (&'static str, String, String, &'static str) {
    (
        " Flag ",
        format!("Clear the flag on “{title}”?"),
        format!(
            "It's flagged as needing work{}. Clearing it is one change, which undo can reverse \
             from History.",
            note.map(|n| format!(": {n}")).unwrap_or_default()
        ),
        " clear it   ",
    )
}

/// The title and the line above the note's typed line.
pub fn input_text(title: &str) -> (&'static str, String) {
    (
        " Flag ",
        format!(
            "What needs work on “{title}”, in a few words (such as wrong title, or split this \
             folder)? Leave it empty to flag it without a note:"
        ),
    )
}

/// The work for a flag, as `carrel flag` and `carrel unflag` do it.
pub(super) fn flag_work(flagging: Flagging) -> (&'static str, super::ArchiveWork) {
    match flagging {
        Flagging::Set { target, note } => (
            "Flagging it…",
            Box::new(move |root: &Path| crate::flag::flag(Some(root), &target, note.as_deref())),
        ),
        Flagging::Clear { target } => (
            "Clearing its flag…",
            Box::new(move |root: &Path| crate::flag::unflag(Some(root), &target)),
        ),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::KeyCode;

    use super::super::app::{Confirm, Effect};
    use super::super::item::Pane;
    use super::super::screens::InputPurpose;
    use super::super::tests::{key, sample, screen};
    use super::Flagging;

    const ORIGIN: &str = "Reading/On the Origin of Species.md";

    fn browse_to_origin(app: &mut super::App) {
        app.key(key(KeyCode::Char('5')));
        app.browse_collection = app
            .collections
            .iter()
            .position(|c| c.path == "Reading")
            .unwrap();
        app.load_items();
        app.browse_item = app
            .browse_items
            .iter()
            .position(|(c, _)| c == ORIGIN)
            .unwrap();
        app.browse_pane = Pane::Items;
        app.load_card();
    }

    #[test]
    fn f_flags_an_item_with_a_note_and_offers_to_clear_a_flag() {
        let (_dir, mut app) = sample(true);
        browse_to_origin(&mut app);
        assert!(app.keys().contains(&("f", "flag")));
        app.key(key(KeyCode::Char('f')));
        assert!(matches!(
            &app.input.as_ref().unwrap().purpose,
            InputPurpose::FlagNote { target, .. } if target == ORIGIN
        ));
        assert!(screen(&app, 120, 30).contains("What needs work on"));
        for c in "wrong title".chars() {
            app.key(key(KeyCode::Char(c)));
        }
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::Flag(Flagging::Set {
                target: ORIGIN.into(),
                note: Some("wrong title".into())
            }))
        );
        // The work itself is `carrel flag`'s, which the CLI's tests run.
        let (busy, _) = super::flag_work(Flagging::Set {
            target: ORIGIN.into(),
            note: Some("wrong title".into()),
        });
        assert_eq!(busy, "Flagging it…");

        // A flagged card: ⚑ in the list, and `f` offers to clear it.
        let path = app.archive.root().join(ORIGIN);
        let text = std::fs::read_to_string(&path).unwrap().replacen(
            "\nsensitivity:",
            "\nneeds_work: \"wrong title\"\nsensitivity:",
            1,
        );
        std::fs::write(&path, text).unwrap();
        app.load_items();
        assert!(app.browse_flagged.contains(ORIGIN));
        assert!(screen(&app, 120, 30).contains("⚑ On the Origin"));
        app.key(key(KeyCode::Char('f')));
        assert!(matches!(app.confirm, Some(Confirm::Unflag { .. })));
        assert!(screen(&app, 120, 30).contains("Clear the flag on"));
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::Flag(Flagging::Clear {
                target: ORIGIN.into()
            }))
        );

        // After an edit is saved on a flagged card, Browse offers to clear its flag.
        app.offer_to_unflag(Some(ORIGIN));
        assert!(matches!(app.confirm, Some(Confirm::Unflag { .. })));
        app.key(key(KeyCode::Char('n')));

        // `F` lists what's flagged.
        app.key(key(KeyCode::Char('F')));
        assert_eq!(app.search.as_deref(), Some("flagged"));
        assert_eq!(app.browse_items.len(), 1);
        assert_eq!(app.browse_items[0].0, ORIGIN);
    }

    #[test]
    fn f_on_a_collection_flags_it_and_a_flagged_one_carries_the_mark() {
        let (_dir, mut app) = sample(true);
        app.key(key(KeyCode::Char('5')));
        app.browse_collection = app
            .collections
            .iter()
            .position(|c| c.path == "Projects")
            .unwrap();
        app.load_items();
        app.browse_pane = Pane::Collections;
        app.key(key(KeyCode::Char('f')));
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::Flag(Flagging::Set {
                target: "Projects".into(),
                note: None
            }))
        );
        let document = app.archive.root().join("Projects/_collection.md");
        let text = std::fs::read_to_string(&document).unwrap().replacen(
            "\ntitle:",
            "\nneeds_work: true\ntitle:",
            1,
        );
        std::fs::write(&document, text).unwrap();
        app.load_browse().unwrap();
        assert!(screen(&app, 120, 30).contains("⚑ Projects"));
    }
}
