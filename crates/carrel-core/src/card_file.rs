//! Whole card files: frontmatter owned by programs, body owned by the owner (FORMAT.md §5).
//!
//! A [`CardFile`] keeps the file's original bytes. Writing it back after changing card fields
//! replaces only the frontmatter; the body is never touched except inside managed sections,
//! and only through the section methods. An unchanged card is written back byte for byte.

use crate::card::{Card, CardError};
use crate::frontmatter::{self, SplitError};
use crate::sections::{self, SectionError};

/// The managed section holding a card's history (FORMAT.md §5.5).
pub const HISTORY: &str = "history";

/// A card file, parsed but with its original text kept.
#[derive(Debug, Clone, PartialEq)]
pub struct CardFile {
    card: Card,
    original_card: Card,
    original_yaml: String,
    body: String,
}

/// Why a card file couldn't be read, edited or written.
#[derive(Debug)]
pub enum CardFileError {
    /// The file couldn't be split into frontmatter and body.
    Split(SplitError),
    /// The frontmatter isn't a valid card.
    Card(CardError),
    /// The body's managed sections can't be used.
    Section(SectionError),
}

impl std::fmt::Display for CardFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Split(e) => e.fmt(f),
            Self::Card(e) => e.fmt(f),
            Self::Section(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for CardFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Split(e) => Some(e),
            Self::Card(e) => Some(e),
            Self::Section(e) => Some(e),
        }
    }
}

impl From<SplitError> for CardFileError {
    fn from(e: SplitError) -> Self {
        Self::Split(e)
    }
}

impl From<CardError> for CardFileError {
    fn from(e: CardError) -> Self {
        Self::Card(e)
    }
}

impl From<SectionError> for CardFileError {
    fn from(e: SectionError) -> Self {
        Self::Section(e)
    }
}

impl CardFile {
    /// Parses a card file's full text.
    ///
    /// # Errors
    ///
    /// [`CardFileError::Split`] if the frontmatter delimiters are missing or malformed, and
    /// [`CardFileError::Card`] if the frontmatter isn't a card. A body with broken section
    /// markers still parses; the problem surfaces when a section is used.
    pub fn parse(text: &str) -> Result<Self, CardFileError> {
        let split = frontmatter::split(text)?;
        let card = Card::from_yaml(split.yaml)?;
        Ok(Self {
            original_card: card.clone(),
            card,
            original_yaml: split.yaml.to_owned(),
            body: split.body.to_owned(),
        })
    }

    /// A new card file for `card`, with an empty history section after `owner_text`.
    ///
    /// # Errors
    ///
    /// [`CardFileError::Section`] if `owner_text` already contains a history section or has
    /// broken markers.
    pub fn new(card: Card, owner_text: &str) -> Result<Self, CardFileError> {
        let body = sections::append_section(owner_text, HISTORY, "## History")?;
        Ok(Self {
            original_card: card.clone(),
            original_yaml: String::new(),
            card,
            body,
        })
    }

    /// The card's fields.
    #[must_use]
    pub fn card(&self) -> &Card {
        &self.card
    }

    /// The card's fields, for editing. Changes take effect when the file is written.
    pub fn card_mut(&mut self) -> &mut Card {
        &mut self.card
    }

    /// The body, exactly as it will be written.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    /// The content of managed section `name`, if it exists.
    ///
    /// # Errors
    ///
    /// [`CardFileError::Section`] if the body's markers don't pair up cleanly.
    pub fn section(&self, name: &str) -> Result<Option<&str>, CardFileError> {
        Ok(sections::get(&self.body, name)?)
    }

    /// Replaces the content of an existing managed section; nothing else in the body changes.
    ///
    /// # Errors
    ///
    /// [`CardFileError::Section`] if the section doesn't exist or the markers are broken.
    pub fn set_section(&mut self, name: &str, content: &str) -> Result<(), CardFileError> {
        self.body = sections::replace(&self.body, name, content)?;
        Ok(())
    }

    /// Whether writing now would change the frontmatter.
    #[must_use]
    pub fn frontmatter_changed(&self) -> bool {
        self.original_yaml.is_empty() || self.card != self.original_card
    }

    /// The file's full text. If the card's fields are unchanged since parsing, the original
    /// frontmatter bytes are reused; otherwise the frontmatter is written afresh.
    ///
    /// # Errors
    ///
    /// [`CardFileError::Card`] if the card can't be serialized.
    pub fn to_text(&self) -> Result<String, CardFileError> {
        let yaml = if self.frontmatter_changed() {
            self.card.to_yaml()?
        } else {
            self.original_yaml.clone()
        };
        Ok(frontmatter::join(&yaml, &self.body))
    }
}
