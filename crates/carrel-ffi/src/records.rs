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
    /// Typed links to other items. A proposal has none until it's filed.
    pub links: Vec<CardLink>,
    /// Where it is, for a physical object or an external reference.
    pub location: Option<String>,
    /// Its address, for a URL item.
    pub url: Option<String>,
    /// Whether its working copy is snapshotted only at checkout and return, not after every
    /// work session, as for large data.
    pub snapshot_on_return_only: bool,
}

/// A link from a card to another item (FORMAT.md §5.4).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CardLink {
    /// Its type, such as `version-of` or `related`.
    pub link_type: String,
    /// The item linked to: its identifier.
    pub item: String,
    /// A short explanation.
    pub note: Option<String>,
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
            links: fields
                .links
                .iter()
                .map(|l| CardLink {
                    link_type: l.link_type.clone(),
                    item: l.id.clone(),
                    note: l.note.clone(),
                })
                .collect(),
            location: fields.location.clone(),
            url: fields.url.clone(),
            snapshot_on_return_only: fields.snapshots.is_some(),
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
            links: Vec::new(),
            location: None,
            url: None,
            snapshots: None,
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
            links: self
                .links
                .into_iter()
                .map(|l| carrel_core::actions::LinkField {
                    link_type: l.link_type,
                    id: l.item,
                    note: l.note.filter(|n| !n.trim().is_empty()),
                })
                .collect(),
            location: self.location,
            url: self.url,
            snapshots: self
                .snapshot_on_return_only
                .then_some(carrel_core::card::Snapshots::ReturnOnly),
        }
    }
}

/// What an edit of a card's fields did.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
#[expect(
    clippy::large_enum_variant,
    reason = "uniffi records can't be boxed; an outcome is made once per save"
)]
pub enum EditOutcome {
    /// Nothing changed, so nothing was written.
    Unchanged,
    /// These fields changed and were saved; for a filed card, in this history commit.
    Saved {
        /// The fields, by name: `title`, `summary`, `tags`, `dates`, `sensitivity`, `links`,
        /// `location`, `url` or `snapshots`.
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
    /// These fields were saved another way while they were being edited, to other values
    /// than the edit's; nothing was written. The app offers to reload `current`, or to keep
    /// the edit, saved again against `current`.
    FieldsChanged {
        /// The fields, by name, as in [`EditOutcome::Saved`].
        fields: Vec<String>,
        /// The card's fields as they are now.
        current: CardFields,
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
