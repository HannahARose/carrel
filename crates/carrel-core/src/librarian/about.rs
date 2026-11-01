//! What the owner is looking at when they ask (#460): an item, a collection, a search or a
//! Discover view. Given with a question ([`super::Librarian::ask_about`]), it's loaded up
//! front, the item's card and its collection's design document, the collection's, or what the
//! search finds, and the prompt says what the owner is looking at, so "this", "here" and "is
//! this in the right place?" mean it.
//!
//! Every interface gives it the same way: the Mac app from its selection, the interactive
//! mode from the screen `L` was pressed on, `carrel ask --about`, and a flag handed to the
//! librarian (#454) from what was flagged ([`About::from`] a [`Flaggable`]).

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::archive::Archive;
use crate::card::Id;
use crate::flags::Flaggable;
use crate::providers::Registry;

use super::ItemLink;
use super::tools::{Ran, Shelf};

/// The most of what's loaded up front the prompt takes, in characters, so a small model's
/// context still has room for the question and its own steps.
const LOADED_CHARS: usize = 2_400;
/// The most of an item's collection the prompt takes, after the item's card.
const ITS_COLLECTION_CHARS: usize = 900;

/// What the owner is looking at when they ask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum About {
    /// An item, by its identifier.
    Item(Id),
    /// A collection, by its path.
    Collection(String),
    /// What a search found, by its words and filters.
    Search(String),
    /// A Discover view, by its name ("on this day", "timeline", …).
    Discover(String),
}

impl From<&Flaggable> for About {
    fn from(flagged: &Flaggable) -> Self {
        match flagged {
            Flaggable::Item { id, .. } => Self::Item(*id),
            Flaggable::Collection { path } => Self::Collection(path.clone()),
        }
    }
}

/// What a question about something is told before the model uses any tool.
#[derive(Debug, Default)]
pub(super) struct Loaded {
    /// What the owner is looking at, in words.
    pub described: String,
    /// What was loaded, for the prompt.
    pub text: String,
    /// The tool calls it stands for, so asking for them again is answered from here.
    pub calls: Vec<(&'static str, String, Ran)>,
}

impl About {
    /// What `name` names: a collection by its path, an item by its identifier (or the start
    /// of it), its card's or its own path, or its title, else a collection by its title.
    ///
    /// # Errors
    ///
    /// Why nothing, or more than one item, matches, in words.
    pub fn find(archive: &Archive, name: &str) -> Result<Self, String> {
        let wanted = name.trim().trim_matches('/');
        if wanted.is_empty() {
            return Err("say what the question is about: an item or a collection".into());
        }
        let collections = archive.collections().map_err(|e| e.to_string())?;
        if let Some(c) = collections.iter().find(|c| c.path == wanted) {
            return Ok(Self::Collection(c.path.clone()));
        }
        let providers = Registry::with_built_ins;
        let shelf = Shelf {
            archive,
            items: archive.items().map_err(|e| e.to_string())?,
            providers: &providers,
        };
        if let Ok(item) = shelf.resolve(wanted) {
            return Ok(Self::Item(item.id));
        }
        let lower = wanted.to_lowercase();
        let titled: Vec<_> = collections
            .iter()
            .filter(|c| c.path.to_lowercase() == lower || c.doc.title.to_lowercase() == lower)
            .collect();
        match titled.as_slice() {
            [c] => Ok(Self::Collection(c.path.clone())),
            [] => Err(format!("no item or collection is {wanted}")),
            _ => Err(format!(
                "several collections are called {wanted}; give its path"
            )),
        }
    }

    /// Its name, for a chip or a heading: the item's or collection's title, the search's
    /// words, or the view's name.
    #[must_use]
    pub fn title(&self, archive: &Archive) -> String {
        match self {
            Self::Item(id) => item_link(archive, *id).map_or_else(|| id.to_string(), |l| l.title),
            Self::Collection(path) => collection_title(archive, path),
            Self::Search(words) => format!("“{words}”"),
            Self::Discover(view) => format!("Discover: {view}"),
        }
    }

    /// What it is, in words, for the prompt and the answer: "the item “Desk lamp parts list”
    /// (Projects/Hardware/parts-list.csv.md)".
    #[must_use]
    pub fn describe(&self, archive: &Archive) -> String {
        match self {
            Self::Item(id) => item_link(archive, *id).map_or_else(
                || format!("the item {id}, which can't be found any more"),
                |l| format!("the item “{}” ({})", l.title, l.card),
            ),
            Self::Collection(path) => format!(
                "the collection “{}” ({path})",
                collection_title(archive, path)
            ),
            Self::Search(words) => format!("what a search for “{words}” found"),
            Self::Discover(view) => format!("Discover's {view} view"),
        }
    }

    /// What's loaded for the model before it starts: the item's card and its collection,
    /// the collection, or what the search found.
    pub(super) fn load(&self, shelf: &Shelf<'_>, question: &str) -> Loaded {
        let mut loaded = Loaded {
            described: self.describe(shelf.archive),
            ..Loaded::default()
        };
        match self {
            Self::Item(id) => {
                let input = id.to_string();
                let card = shelf.run("show", &input, question);
                loaded.text = clip(&card.text, LOADED_CHARS - ITS_COLLECTION_CHARS);
                if let Some(link) = card.found.first() {
                    let collection = shelf.run("collection", &link.collection, question);
                    write!(
                        loaded.text,
                        "\nIts collection: {}",
                        clip(&collection.text, ITS_COLLECTION_CHARS)
                    )
                    .unwrap();
                }
                loaded.calls.push(("show", input, card));
            }
            Self::Collection(path) => {
                let ran = shelf.run("collection", path, question);
                loaded.text = clip(&ran.text, LOADED_CHARS);
                loaded.calls.push(("collection", path.clone(), ran));
            }
            Self::Search(words) => {
                let ran = shelf.run("search", words, question);
                loaded.text = clip(&ran.text, LOADED_CHARS);
                loaded.calls.push(("search", words.clone(), ran));
            }
            Self::Discover(_) => {}
        }
        loaded
    }
}

/// The item `id`, as an answer links to it, if it's in `archive`.
fn item_link(archive: &Archive, id: Id) -> Option<ItemLink> {
    let providers = Registry::with_built_ins;
    let items = archive.items().ok()?;
    let shelf = Shelf {
        archive,
        items,
        providers: &providers,
    };
    let item = shelf.items.iter().find(|i| i.id == id)?;
    Some(shelf.link(item))
}

/// The title of the collection at `path`, or its path when it has none.
fn collection_title(archive: &Archive, path: &str) -> String {
    archive
        .collections()
        .ok()
        .and_then(|cs| cs.into_iter().find(|c| c.path == path))
        .map_or_else(|| path.to_owned(), |c| c.doc.title)
}

/// `text` cut to `chars` characters, with `…` where it was cut.
fn clip(text: &str, chars: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= chars {
        return text.to_owned();
    }
    text.chars().take(chars).collect::<String>() + "…"
}
