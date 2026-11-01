//! Records and enums shared by the operation groups, and their translations from the core's
//! types.

use carrel_core::actions::CardFields as CoreFields;
use carrel_core::card::{DateOrTime, Dates, Sensitivity as CoreSensitivity};
use carrel_core::entries::HistoryEntry;

/// How sensitive an item is (FORMAT.md §6.6), least first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, uniffi::Enum)]
pub enum Sensitivity {
    /// Fine to describe anywhere.
    Public,
    /// The owner's own material.
    Personal,
    /// Needs care before it is described outside the device.
    Sensitive,
    /// Identity, medical or financial records.
    Restricted,
}

impl From<CoreSensitivity> for Sensitivity {
    fn from(level: CoreSensitivity) -> Self {
        match level {
            CoreSensitivity::Public => Self::Public,
            CoreSensitivity::Personal => Self::Personal,
            CoreSensitivity::Sensitive => Self::Sensitive,
            CoreSensitivity::Restricted => Self::Restricted,
        }
    }
}

impl From<Sensitivity> for CoreSensitivity {
    fn from(level: Sensitivity) -> Self {
        match level {
            Sensitivity::Public => Self::Public,
            Sensitivity::Personal => Self::Personal,
            Sensitivity::Sensitive => Self::Sensitive,
            Sensitivity::Restricted => Self::Restricted,
        }
    }
}

/// A card's dates, each a date (`2026-10-29`) or a time with an offset.
#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct CardDates {
    /// When the item was created.
    pub created: Option<String>,
    /// When it was last modified.
    pub modified: Option<String>,
    /// When it was added to the archive.
    pub added: Option<String>,
    /// When it was last worked on.
    pub worked: Option<String>,
    /// Its own date, such as a publication date.
    pub document: Option<String>,
}

/// The card fields the owner edits, in a proposal or a filed card.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CardFields {
    /// The title.
    pub title: String,
    /// The summary.
    pub summary: Option<String>,
    /// The tags, by name.
    pub tags: Vec<String>,
    /// The dates.
    pub dates: CardDates,
    /// How sensitive it is.
    pub sensitivity: Sensitivity,
}

fn date(value: Option<&DateOrTime>) -> Option<String> {
    value.map(|d| d.0.clone())
}

impl From<&CoreFields> for CardFields {
    fn from(fields: &CoreFields) -> Self {
        let dates = fields
            .dates
            .as_ref()
            .map_or_else(CardDates::default, |d| CardDates {
                created: date(d.created.as_ref()),
                modified: date(d.modified.as_ref()),
                added: date(d.added.as_ref()),
                worked: date(d.worked.as_ref()),
                document: date(d.document.as_ref()),
            });
        Self {
            title: fields.title.clone(),
            summary: fields.summary.clone(),
            tags: fields.tags.clone(),
            dates,
            sensitivity: fields.sensitivity.into(),
        }
    }
}

impl From<&carrel_core::correct::Fields> for CardFields {
    fn from(fields: &carrel_core::correct::Fields) -> Self {
        (&CoreFields {
            title: fields.title.clone(),
            summary: fields.summary.clone(),
            tags: fields.tags.iter().map(|t| t.0.clone()).collect(),
            dates: fields.dates.clone(),
            sensitivity: fields.sensitivity,
        })
            .into()
    }
}

impl CardFields {
    /// The core's fields, keeping what `was` has that these don't carry: dates this version
    /// doesn't know.
    pub(crate) fn into_core(self, was: &CoreFields) -> CoreFields {
        let filled = |d: Option<String>| d.filter(|d| !d.trim().is_empty()).map(DateOrTime);
        let mut dates = was.dates.clone().unwrap_or_default();
        dates.created = filled(self.dates.created);
        dates.modified = filled(self.dates.modified);
        dates.added = filled(self.dates.added);
        dates.worked = filled(self.dates.worked);
        dates.document = filled(self.dates.document);
        let empty = dates == Dates::default();
        CoreFields {
            title: self.title,
            summary: self.summary,
            tags: self.tags,
            dates: (!empty).then_some(dates),
            sensitivity: self.sensitivity.into(),
        }
    }
}

/// What an edit of a card's fields did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum EditOutcome {
    /// Nothing changed, so nothing was written.
    Unchanged,
    /// These fields changed and were saved; for a filed card, in this history commit.
    Saved {
        /// The fields, by name: `title`, `summary`, `tags`, `dates`, `sensitivity`.
        fields: Vec<String>,
        /// The history commit, for a filed card.
        commit: Option<String>,
    },
    /// The edit doesn't check out, for these reasons; nothing was written. The app shows
    /// them beside the fields.
    Problems {
        /// What's wrong, in words.
        problems: Vec<String>,
    },
    /// The card's body was saved another way while it was being edited, and the two changed
    /// the same lines; nothing was written. The app offers to reload `current` or to keep
    /// `both`, saved again against `current`.
    BodyChanged {
        /// The body as it is now.
        current: String,
        /// Both versions together, each changed line from each.
        both: String,
    },
}

/// A history entry on a card (FORMAT.md §6.8): a work session, a return, or the owner's own.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CardHistoryEntry {
    /// The day, as `YYYY-MM-DD`.
    pub date: String,
    /// How long, in minutes, when it says.
    pub minutes: Option<u64>,
    /// Whether it's waiting for the owner's approval.
    pub pending: bool,
    /// Its text, line by line.
    pub lines: Vec<String>,
}

impl From<&HistoryEntry> for CardHistoryEntry {
    fn from(entry: &HistoryEntry) -> Self {
        Self {
            date: entry.date.clone(),
            minutes: entry.minutes(),
            pending: entry.pending,
            lines: entry.lines.clone(),
        }
    }
}
