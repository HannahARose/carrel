//! Browse's actions on the archive, each asked about first and each one change undo can
//! reverse, as the commands are: checking an item out (`carrel checkout`), creating a
//! collection (`carrel collection new`), renaming an item (`carrel rename`) and expanding a
//! filed compressed file (`carrel expand`). The commands do the work; this asks for what they
//! need.

use std::fmt::Write as _;
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

    /// Opens the line to type or paste the path being picked, for a checkout or the desk. A
    /// checkout's starts with the archive's `checkout.destination`, the Desktop unless changed.
    pub(super) fn ask_for_path(&mut self) {
        self.input = Some(if self.picking == Picking::DeskItem {
            Input {
                purpose: InputPurpose::DeskPath,
                text: String::new(),
            }
        } else {
            let home = carrel_core::local::home_dir();
            Input {
                purpose: InputPurpose::CheckoutFolder,
                text: carrel_core::settings::checkout_folder(
                    self.archive.settings(),
                    home.as_deref(),
                )
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            }
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
        Confirm::Rule { subject, path, .. } => (
            " Routing rule ",
            format!("Gather “{subject}” in {path}, and file future items there too?"),
            format!(
                "Its items go to {path} now, each an undoable move, creating the collection if \
                 it's new. Saying yes also adds a routing rule, so items named for “{subject}” \
                 go there from now on."
            ),
            " with a rule   ",
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
                write!(about, " starting “{description}”").unwrap();
            }
            if !new.tags.is_empty() {
                write!(about, ", and the tags {}", new.tags.join(", ")).unwrap();
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use ratatui::crossterm::event::KeyCode;

    use super::super::app::{App, Confirm, Effect, Picking, Tab};
    use super::super::item::Pane;
    use super::super::screens::InputPurpose;
    use super::super::tests::{active_sample, choose_parts_list, key, sample, screen};
    use super::NewCollection;

    /// Types `text` into the open line, replacing what's there, and enters it.
    fn enter(app: &mut App, text: &str) -> Option<Effect> {
        while app.input.as_ref().is_some_and(|i| !i.text.is_empty()) {
            app.key(key(KeyCode::Backspace));
        }
        for c in text.chars() {
            app.key(key(KeyCode::Char(c)));
        }
        app.key(key(KeyCode::Enter))
    }

    /// Chooses the item whose card is at `card` in Browse, in the items pane.
    fn browse_to(app: &mut App, card: &str) {
        app.key(key(KeyCode::Char('5')));
        let (folder, _) = card.rsplit_once('/').unwrap();
        app.browse_collection = app
            .collections
            .iter()
            .position(|c| c.path == folder)
            .unwrap();
        app.browse_item = 0;
        app.load_items();
        app.browse_item = app
            .browse_items
            .iter()
            .position(|(c, _)| c == card)
            .unwrap_or_else(|| panic!("{card} in {:?}", app.browse_items));
        app.browse_pane = Pane::Items;
        app.load_card();
    }

    fn purpose(app: &App) -> &InputPurpose {
        &app.input.as_ref().expect("a line is open").purpose
    }

    const PARTS: &str = "Projects/Hardware/parts-list.csv.md";

    #[test]
    fn an_item_is_checked_out_to_a_chosen_folder_with_a_due_date() {
        let (dir, mut app) = sample(true);
        let out = dir.path().join("Work");
        std::fs::create_dir_all(&out).unwrap();
        app.picker_preference = Some("builtin".into());
        app.start_folder = dir.path().to_owned();
        browse_to(&mut app, PARTS);
        let title = app.browse_items[app.browse_item].1.clone();
        assert!(app.keys().contains(&("o", "check out")), "{:?}", app.keys());
        app.key(key(KeyCode::Right));
        assert_eq!(app.browse_pane, Pane::Item, "and from the item's view");
        assert!(app.keys().contains(&("o", "check out")), "{:?}", app.keys());

        assert_eq!(app.key(key(KeyCode::Char('o'))), None);
        assert_eq!(app.picking, Picking::CheckoutFolder);
        let browser = app.browser.as_ref().expect("the built-in browser opens");
        let at = browser
            .entries
            .iter()
            .position(|(name, _)| name == "Work")
            .unwrap();
        assert!(app.keys().contains(&("i", "check out here")));
        for _ in 0..at {
            app.key(key(KeyCode::Down));
        }
        app.key(key(KeyCode::Char('i')));
        assert!(app.browser.is_none());
        assert!(matches!(purpose(&app), InputPurpose::Due { to, .. } if *to == out));
        assert_eq!(app.picking, Picking::Ingest, "picking goes back to ingest");
        assert!(screen(&app, 120, 30).contains(&format!("When is “{title}” due back?")));

        assert_eq!(enter(&mut app, "soon"), None);
        assert_eq!(
            app.message.as_deref(),
            Some("soon isn't a date; give it as YYYY-MM-DD")
        );
        assert!(
            matches!(purpose(&app), InputPurpose::Due { .. }),
            "asked again"
        );
        enter(&mut app, "2026-12-01");
        assert!(matches!(
            &app.confirm,
            Some(Confirm::Checkout { to, due: Some(due), .. }) if *to == out && due == "2026-12-01"
        ));
        // A long folder, as on Windows, wraps in the pop-up, so only its start is looked for.
        let text = screen(&app, 120, 30);
        assert!(text.contains(&format!("Check out “{title}” to")), "{text}");
        assert!(text.contains("due back 2026-12-01"), "{text}");
        assert_eq!(app.key(key(KeyCode::Char('n'))), None);
        assert_eq!(app.message.as_deref(), Some("Nothing changed"));

        // Typed rather than browsed to, and with no due date.
        app.key(key(KeyCode::Char('o')));
        app.key(key(KeyCode::Char('/')));
        assert_eq!(purpose(&app), &InputPurpose::CheckoutFolder);
        let suggested = app
            .input
            .as_ref()
            .map(|i| i.text.clone())
            .unwrap_or_default();
        assert!(
            suggested.ends_with("Desktop"),
            "checkout.destination: {suggested}"
        );
        enter(&mut app, &out.display().to_string());
        enter(&mut app, "");
        assert!(matches!(
            app.confirm,
            Some(Confirm::Checkout { due: None, .. })
        ));
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::Checkout {
                item: PARTS.into(),
                to: out.clone(),
                due: None,
            })
        );

        // Cancelling the browser forgets the item.
        app.key(key(KeyCode::Char('o')));
        app.key(key(KeyCode::Esc));
        assert_eq!(app.checking_out, None);
        assert_eq!(app.picking, Picking::Ingest);

        let (busy, _) = super::super::archive_work(
            app.archive.root().to_owned(),
            Effect::Checkout {
                item: PARTS.into(),
                to: out,
                due: Some("2026-12-01".into()),
            },
        );
        assert_eq!(busy, "Checking it out…");
    }

    #[test]
    fn active_puts_things_on_the_desk_takes_them_off_and_archives_them() {
        let (dir, mut app) = active_sample();
        app.picker_preference = Some("builtin".into());
        app.start_folder = dir.path().to_owned();
        app.key(key(KeyCode::Char('3')));
        app.key(key(KeyCode::End));
        let desk = app.active_item().unwrap().clone();
        assert_eq!(desk.title, "Thesis");
        let keys = screen(&app, 100, 30).lines().last().unwrap().to_owned();
        for shown in ["n add to desk", "x take off", "f archive"] {
            assert!(keys.contains(shown), "{shown}: {keys}");
        }
        assert!(keys.ends_with("? help  q quit"), "it fits: {keys}");

        app.key(key(KeyCode::Char('x')));
        assert_eq!(
            app.keys(),
            vec![
                ("y", "keep its card"),
                ("d", "delete its card"),
                ("n", "no")
            ]
        );
        let text = screen(&app, 120, 30);
        assert!(text.contains("Take “Thesis” off the desk?"), "{text}");
        assert!(text.contains("d delete its card too"), "{text}");
        assert_eq!(
            app.key(key(KeyCode::Char('d'))),
            Some(Effect::DeskRemove {
                path: desk.working.clone(),
                delete_card: true
            })
        );
        app.key(key(KeyCode::Char('x')));
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::DeskRemove {
                path: desk.working.clone(),
                delete_card: false
            })
        );
        app.key(key(KeyCode::Char('x')));
        assert_eq!(app.key(key(KeyCode::Char('n'))), None);
        assert_eq!(app.message.as_deref(), Some("Nothing changed"));

        app.key(key(KeyCode::Char('f')));
        assert!(screen(&app, 120, 30).contains("Archive “Thesis”?"));
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::DeskArchive(desk.working.clone()))
        );

        let notes = dir.path().join("Notes");
        std::fs::create_dir_all(&notes).unwrap();
        assert_eq!(app.key(key(KeyCode::Char('n'))), None);
        assert_eq!(app.picking, Picking::DeskItem);
        assert!(app.keys().contains(&("i", "put on the desk")));
        app.key(key(KeyCode::Char('/')));
        assert_eq!(purpose(&app), &InputPurpose::DeskPath);
        enter(&mut app, &notes.display().to_string());
        assert_eq!(app.confirm, Some(Confirm::DeskAdd(notes.clone())));
        let text = screen(&app, 120, 30);
        assert!(
            text.contains(" Desk ") && text.contains("Put "),
            "the folder may wrap: {text}"
        );
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::DeskAdd(notes.clone()))
        );

        choose_parts_list(&mut app);
        assert_eq!(app.key(key(KeyCode::Char('f'))), None);
        assert!(
            app.message
                .as_deref()
                .unwrap()
                .contains("already in the archive")
        );
        let (busy, _) =
            super::super::archive_work(app.archive.root().to_owned(), Effect::DeskAdd(notes));
        assert_eq!(busy, "Putting it on the desk and drafting its card…");
    }

    #[test]
    fn a_collection_is_created_with_its_description_and_tags() {
        let (_dir, mut app) = sample(true);
        app.key(key(KeyCode::Char('5')));
        app.key(key(KeyCode::Down));
        assert!(app.keys().contains(&("n", "new collection")));
        app.key(key(KeyCode::Char('n')));
        assert_eq!(purpose(&app), &InputPurpose::CollectionPath);
        assert_eq!(
            app.input.as_ref().unwrap().text,
            "Projects/",
            "it starts in the chosen collection"
        );
        app.key(key(KeyCode::Char('L')));
        app.key(key(KeyCode::Char('a')));
        app.key(key(KeyCode::Char('m')));
        app.key(key(KeyCode::Char('p')));
        app.key(key(KeyCode::Enter));
        assert!(screen(&app, 120, 30).contains("What belongs in Projects/Lamp"));
        enter(&mut app, "The desk lamp build");
        enter(&mut app, "wiring=diagrams; parts=bills of materials");
        assert_eq!(
            app.confirm,
            Some(Confirm::NewCollection(NewCollection {
                path: "Projects/Lamp".into(),
                description: Some("The desk lamp build".into()),
                tags: vec!["wiring=diagrams".into(), "parts=bills of materials".into()],
            }))
        );
        let text = screen(&app, 120, 30);
        assert!(
            text.contains("Create the collection Projects/Lamp?"),
            "{text}"
        );
        assert!(text.contains("undo can reverse"), "{text}");
        assert!(matches!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::NewCollection(_))
        ));

        app.key(key(KeyCode::Char('n')));
        assert_eq!(enter(&mut app, ""), None);
        assert_eq!(
            app.message.as_deref(),
            Some("No path given; nothing created")
        );
    }

    #[test]
    fn an_item_is_renamed_from_the_name_suggested() {
        let (dir, mut app) = sample(true);
        browse_to(&mut app, PARTS);
        assert!(app.keys().contains(&("r", "rename")));
        app.key(key(KeyCode::Char('r')));
        assert!(
            matches!(purpose(&app), InputPurpose::Rename { from, .. } if from == "parts-list.csv")
        );
        assert_eq!(app.input.as_ref().unwrap().text, "parts-list.csv");
        app.key(key(KeyCode::Enter));
        assert_eq!(app.message.as_deref(), Some("Same name; nothing changed"));

        app.key(key(KeyCode::Char('r')));
        enter(&mut app, "lamp-parts.csv");
        assert!(screen(&app, 120, 30).contains("Rename “parts-list.csv” to “lamp-parts.csv”?"));
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::Rename {
                item: PARTS.into(),
                new_name: "lamp-parts.csv".into()
            })
        );
        let (busy, _) = super::super::archive_work(
            dir.path().to_owned(),
            Effect::Rename {
                item: PARTS.into(),
                new_name: "x".into(),
            },
        );
        assert_eq!(busy, "Renaming it…");
    }

    /// Files a zip holding `files` at `Projects/Hardware/NAME`, with its card.
    fn file_zip(app: &mut App, name: &str, files: &[(&str, &[u8])]) -> String {
        use std::io::Write as _;
        let folder = app.archive.root().join("Projects/Hardware");
        let path: PathBuf = folder.join(name);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (name, bytes) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        let card = format!(
            "---\ncarrel: card\nformat: \"1.0\"\nid: {}\nkind: file\ntitle: \"{name}\"\n\
             item: \"{name}\"\ndates:\n  added: \"2026-10-31\"\nsensitivity: personal\n---\n",
            carrel_core::card::Id::new_v7()
        );
        std::fs::write(folder.join(format!("{name}.md")), card).unwrap();
        app.reload().unwrap();
        format!("Projects/Hardware/{name}.md")
    }

    #[test]
    fn a_filed_zip_is_expanded_after_asking_and_all_the_same_only_when_asked() {
        let (_dir, mut app) = sample(true);
        browse_to(&mut app, PARTS);
        assert!(
            !app.keys().iter().any(|(k, _)| *k == "z"),
            "only a compressed file offers it: {:?}",
            app.keys()
        );
        app.key(key(KeyCode::Char('z')));
        assert!(app.confirm.is_none());
        assert!(
            app.message
                .as_deref()
                .unwrap()
                .contains("isn't a compressed file"),
            "{:?}",
            app.message
        );

        let notes = file_zip(&mut app, "Spectra.zip", &[("a.txt", b"a"), ("b.txt", b"b")]);
        browse_to(&mut app, &notes);
        assert!(app.keys().contains(&("z", "expand")), "{:?}", app.keys());
        app.key(key(KeyCode::Char('z')));
        assert!(
            matches!(&app.confirm, Some(Confirm::Expand { anyway: None, .. })),
            "{:?} {:?}",
            app.confirm,
            app.message
        );
        assert!(screen(&app, 120, 30).contains("Expand “Spectra.zip”?"));
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::Expand {
                item: notes.clone(),
                anyway: false
            })
        );

        let setup = file_zip(&mut app, "Utility.zip", &[("Setup.exe", b"MZ")]);
        browse_to(&mut app, &setup);
        app.key(key(KeyCode::Char('z')));
        assert!(
            matches!(
                &app.confirm,
                Some(Confirm::Expand {
                    anyway: Some(_),
                    ..
                })
            ),
            "{:?}",
            app.confirm
        );
        let text = screen(&app, 120, 30);
        assert!(
            text.contains("Expand “Utility.zip” all the same?"),
            "{text}"
        );
        assert!(text.contains("It's kept compressed:"), "{text}");
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::Expand {
                item: setup,
                anyway: true
            })
        );
        assert!(
            Path::new(app.archive.root()).join(&notes).is_file(),
            "nothing changed yet"
        );
        assert_eq!(app.tab, Tab::Browse);
    }
}
