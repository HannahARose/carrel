//! Card frontmatter (FORMAT.md §5): the typed form of the YAML between a card's `---` lines.
//!
//! Parsing is lenient where FORMAT.md asks programs to flag rather than reject (malformed
//! tags, unexpected kinds of dates), and strict where a value can't be understood at all.
//! [`Card::problems`] reports everything that doesn't conform. Unknown fields, at the top
//! level and in every nested map, are kept in `extra` and written back unchanged.

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::FormatVersion;

/// Unknown keys, preserved in their original order (FORMAT.md §5.2).
pub type Extra = IndexMap<String, Value>;

/// A card: the description of one item, stored beside it (FORMAT.md §5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(
    title = "Carrel card",
    description = "Frontmatter of a Carrel card (FORMAT.md §5)."
)]
pub struct Card {
    /// Document type; always `card`.
    pub carrel: CardMarker,
    /// Format version the card was written with, such as `"1.0"`.
    pub format: FormatVersion,
    /// The item's permanent identifier.
    pub id: Id,
    /// What kind of item this card describes.
    pub kind: Kind,
    /// Human-readable title (the owner may edit it).
    pub title: String,
    /// The item's name relative to the card's folder; required for files, folders and bundles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    /// How strictly a bundle is protected; required for bundles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed: Option<Sealing>,
    /// For soft-sealed bundles: whether a program may add a `README.md` inside (default `true`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_readme: Option<bool>,
    /// Two or three sentences describing the item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Tags from the vocabularies along the item's path (FORMAT.md §6.5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<Tag>,
    /// Typed links to other items (FORMAT.md §5.4).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
    /// Dates associated with the item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dates: Option<Dates>,
    /// How carefully the item must be handled.
    pub sensitivity: Sensitivity,
    /// The file's checksum; required for files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<Checksum>,
    /// Size in bytes; required for files, folders and bundles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// The folder's or bundle's manifest; required for folders and bundles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<ManifestRef>,
    /// Where the item is, for physical objects and external references.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// The resource's address; required for URL items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Present while the item is checked out (FORMAT.md §9.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_out: Option<Checkout>,
    /// When the item's working copy is snapshotted while it's being worked on (FORMAT.md
    /// §5.2); every session unless set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshots: Option<Snapshots>,
    /// How program-written fields were produced (FORMAT.md §5.6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated: Option<Provenance>,
    /// How sure filing was of the item's collection, when it wasn't (FORMAT.md §5.8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<PlacementRecord>,
    /// Why and when the item was removed; required for tombstones (FORMAT.md §10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<Removal>,
    /// The compressed file the item was expanded from (FORMAT.md §5.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded_from: Option<ExpandedFrom>,
    /// The owner's mark that the item or its card needs work (FORMAT.md §5.9).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "needs_work_field"
    )]
    #[schemars(with = "Option<NeedsWorkForm>")]
    pub needs_work: Option<NeedsWork>,
    /// Fields this version doesn't know, preserved unchanged.
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

/// The owner's mark that an item or a collection needs work (FORMAT.md §5.9), with what, who
/// and when. Written by hand it may be only `true` or a note; programs write the whole record.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct NeedsWork {
    /// What needs doing, in a few words, such as "wrong title" or "split this folder".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The person who flagged it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person: Option<Id>,
    /// When it was flagged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<DateOrTime>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

/// The forms `needs_work` may take: `true` (or `false`, the same as leaving it out), a note,
/// or the whole record.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(crate) enum NeedsWorkForm {
    /// Flagged, or with `false`, not.
    Flag(bool),
    /// Flagged, with what needs doing.
    Note(String),
    /// Flagged, with what, who and when.
    Record(NeedsWork),
}

/// Reads and writes `needs_work` in any of its forms, writing back the short form a record
/// that has only a note, or nothing, was read from.
pub(crate) mod needs_work_field {
    use serde::{Deserialize as _, Deserializer, Serialize as _, Serializer};

    use super::{NeedsWork, NeedsWorkForm};

    #[allow(clippy::ref_option)] // serde's `with` passes the field by reference
    pub fn serialize<S: Serializer>(
        value: &Option<NeedsWork>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            None => serializer.serialize_none(),
            Some(flag) if flag.person.is_none() && flag.at.is_none() && flag.extra.is_empty() => {
                match &flag.note {
                    Some(note) => NeedsWorkForm::Note(note.clone()),
                    None => NeedsWorkForm::Flag(true),
                }
                .serialize(serializer)
            }
            Some(flag) => NeedsWorkForm::Record(flag.clone()).serialize(serializer),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<NeedsWork>, D::Error> {
        Ok(match Option::<NeedsWorkForm>::deserialize(deserializer)? {
            None | Some(NeedsWorkForm::Flag(false)) => None,
            Some(NeedsWorkForm::Flag(true)) => Some(NeedsWork::default()),
            Some(NeedsWorkForm::Note(note)) => Some(NeedsWork {
                note: Some(note).filter(|n| !n.trim().is_empty()),
                ..NeedsWork::default()
            }),
            Some(NeedsWorkForm::Record(flag)) => Some(flag),
        })
    }
}

/// The `carrel: card` marker that identifies a Markdown file as a card (FORMAT.md §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CardMarker {
    /// The only value.
    #[default]
    Card,
}

/// A permanent identifier: a UUID, which FORMAT.md §2.3 requires to be version 7.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct Id(pub uuid::Uuid);

impl Id {
    /// A new identifier for something created now.
    #[must_use]
    pub fn new_v7() -> Self {
        Self(uuid::Uuid::now_v7())
    }

    /// Whether this is a version 7 UUID, as FORMAT.md requires.
    #[must_use]
    pub fn is_v7(&self) -> bool {
        self.0.get_version_num() == 7
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.hyphenated().fmt(f)
    }
}

/// Item kinds (FORMAT.md §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A single file.
    File,
    /// A folder whose contents programs may reorganize with approval.
    Folder,
    /// A folder stored and moved whole.
    Bundle,
    /// Something outside the archive, such as a book; card only.
    Physical,
    /// A web resource; card only.
    Url,
    /// A file or folder stored outside the archive; card only.
    External,
    /// A record of a removed item; card only.
    Tombstone,
}

impl Kind {
    /// Whether the card exists without a file or folder beside it.
    #[must_use]
    pub fn is_card_only(self) -> bool {
        matches!(
            self,
            Self::Physical | Self::Url | Self::External | Self::Tombstone
        )
    }
}

/// When a checked-out or desk item's working copy is snapshotted (FORMAT.md §5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Snapshots {
    /// Only at checkout and return, not after each work session: for large data.
    ReturnOnly,
}

/// How strictly a bundle is protected (FORMAT.md §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Sealing {
    /// Never write inside.
    Hard,
    /// Never write inside, except an optional `README.md`.
    Soft,
}

/// Sensitivity levels.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
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

/// A tag: lowercase words joined by `-` (FORMAT.md §6.5).
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct Tag(#[schemars(pattern(r"^[a-z0-9]+(-[a-z0-9]+)*$"))] pub String);

impl Tag {
    /// Whether the tag follows FORMAT.md §6.5.
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        !self.0.is_empty()
            && !self.0.starts_with('-')
            && !self.0.ends_with('-')
            && !self.0.contains("--")
            && self
                .0
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    }
}

/// A typed link to another item (FORMAT.md §5.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Link {
    /// A built-in link type or a custom one declared in `ARCHIVE.md`.
    #[serde(rename = "type")]
    pub link_type: LinkType,
    /// The target item's identifier.
    pub id: Id,
    /// The target card's path relative to this card, kept current as a convenience.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// A short explanation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

/// A link type. Built-in types are listed in [`LinkType::BUILT_IN`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct LinkType(pub String);

impl LinkType {
    /// The link types every archive understands (FORMAT.md §5.4).
    pub const BUILT_IN: [&'static str; 6] = [
        "version-of",
        "part-of",
        "derived-from",
        "cites",
        "about",
        "related",
    ];

    /// Whether this is one of the built-in types.
    #[must_use]
    pub fn is_built_in(&self) -> bool {
        Self::BUILT_IN.contains(&self.0.as_str())
    }
}

/// Dates associated with an item.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Dates {
    /// When the item was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<DateOrTime>,
    /// When the item was last modified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<DateOrTime>,
    /// When the item was added to the archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<DateOrTime>,
    /// When the item was last worked on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worked: Option<DateOrTime>,
    /// The item's own date, such as a publication date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<DateOrTime>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

/// An ISO 8601 date (`2026-10-29`) or RFC 3339 timestamp with offset (FORMAT.md §2.4).
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct DateOrTime(
    #[schemars(pattern(r"^\d{4}-\d{2}-\d{2}(T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2}))?$"))]
    pub String,
);

impl DateOrTime {
    /// Whether the value is a valid date, or a valid timestamp with an offset.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let s = self.0.as_str();
        let (date, time) = match s.split_once('T') {
            Some((d, t)) => (d, Some(t)),
            None => (s, None),
        };
        valid_date(date) && time.is_none_or(valid_time_with_offset)
    }

    /// Whether the value includes a time, not just a date.
    #[must_use]
    pub fn has_time(&self) -> bool {
        self.0.contains('T')
    }
}

/// Parses `s` as an unsigned number if it is exactly `len` ASCII digits.
fn digits(s: &str, len: usize) -> Option<u32> {
    (s.len() == len && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok())?
}

/// Whether `s` is a real calendar date written `YYYY-MM-DD`, accounting for leap years.
fn valid_date(s: &str) -> bool {
    let mut parts = s.split('-');
    let (Some(y), Some(m), Some(d), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let (Some(y), Some(m), Some(d)) = (digits(y, 4), digits(m, 2), digits(d, 2)) else {
        return false;
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&d)
}

/// Whether `s` (the part after `T`) is `HH:MM:SS`, optionally with fractional seconds, followed
/// by `Z` or a `±HH:MM` offset. A time without an offset is invalid, as FORMAT.md §2.4 requires.
/// Seconds may be 60, for leap seconds.
fn valid_time_with_offset(s: &str) -> bool {
    let (clock, offset) = if let Some(c) = s.strip_suffix('Z') {
        (c, None)
    } else if s.len() > 6 && matches!(s.as_bytes()[s.len() - 6], b'+' | b'-') {
        (&s[..s.len() - 6], Some(&s[s.len() - 5..]))
    } else {
        return false;
    };
    let clock = clock.split_once('.').map_or(clock, |(c, frac)| {
        if !frac.is_empty() && frac.bytes().all(|b| b.is_ascii_digit()) {
            c
        } else {
            ""
        }
    });
    let hms: Vec<&str> = clock.split(':').collect();
    let ok_clock = matches!(hms.as_slice(), [h, m, s]
        if digits(h, 2).is_some_and(|h| h < 24)
        && digits(m, 2).is_some_and(|m| m < 60)
        && digits(s, 2).is_some_and(|s| s < 61));
    let ok_offset = offset.is_none_or(|o| {
        matches!(o.split_once(':'), Some((h, m))
            if digits(h, 2).is_some_and(|h| h < 24) && digits(m, 2).is_some_and(|m| m < 60))
    });
    ok_clock && ok_offset
}

/// A checksum written as `algorithm:hex` (FORMAT.md §2.6). Version 1.0 uses SHA-256.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct Checksum(#[schemars(pattern(r"^sha256:[0-9a-f]{64}$"))] pub String);

impl Checksum {
    /// Whether this is a well-formed SHA-256 checksum.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.0.strip_prefix("sha256:").is_some_and(|hex| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    }
}

/// Where a folder's or bundle's manifest is, and its checksum (FORMAT.md §7.7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ManifestRef {
    /// Checksum of the manifest file.
    pub checksum: Checksum,
    /// The manifest's path under `_System/manifests/`.
    pub path: String,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

/// The compressed file an item was expanded from (FORMAT.md §5.2), so it's clear the item
/// was, say, a zip, and which one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExpandedFrom {
    /// The compressed file's name.
    pub name: String,
    /// Its size in bytes.
    pub size: u64,
    /// Its checksum.
    pub checksum: Checksum,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

/// How sure filing was of an item's collection, kept when it wasn't sure (FORMAT.md §5.8), as
/// evidence for restructuring.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlacementRecord {
    /// How sure routing was.
    pub confidence: PlacementConfidence,
    /// What chose the collection.
    pub by: PlacedBy,
    /// When the item was filed.
    pub at: DateOrTime,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

/// How sure routing was of a placement worth recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PlacementConfidence {
    /// The routing passes disagreed, the model chose Holding, or validation needed a retry.
    Low,
    /// A model chose it and a second pass agreed.
    Medium,
}

/// What chose an item's collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PlacedBy {
    /// A text model.
    Model,
    /// An owner routing rule.
    Rule,
}

/// A checkout in progress (FORMAT.md §9.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Checkout {
    /// Where the working copy is, on the device that checked it out.
    pub to: String,
    /// The device that checked it out.
    pub device: Id,
    /// The person who checked it out.
    pub person: Id,
    /// When it was checked out.
    pub since: DateOrTime,
    /// When it is due back, if a due date was set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<DateOrTime>,
    /// The manifest checksum at checkout, for detecting changes on return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<Checksum>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

/// How program-written fields were produced (FORMAT.md §5.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Provenance {
    /// The card fields this describes.
    pub fields: Vec<String>,
    /// The program and version, such as `carrel 0.2.0`.
    pub by: String,
    /// The model, if one was used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The device that produced the fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<Id>,
    /// When the fields were produced.
    pub at: DateOrTime,
    /// Whether a stronger device should re-check the result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<QualityLevel>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

/// Whether a result was produced at its task's preferred level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum QualityLevel {
    /// Produced at the preferred level.
    Preferred,
    /// Produced below it; a stronger device should re-check.
    BelowPreferred,
}

/// Why and when an item was removed (FORMAT.md §10.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Removal {
    /// When the item was removed.
    pub at: DateOrTime,
    /// Why it was removed.
    pub reason: String,
    /// What kind of item it was.
    pub former_kind: Kind,
    /// Where it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub former_path: Option<String>,
    /// The backup snapshot it can be restored from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recoverable_from: Option<String>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

/// A way a card doesn't conform to FORMAT.md.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The field concerned, as a dotted path (`links[0].id`).
    pub field: String,
    /// What is wrong, in plain words.
    pub message: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

/// Why card frontmatter couldn't be read or written.
#[derive(Debug)]
pub enum CardError {
    /// The YAML is malformed, or a value has the wrong type.
    Yaml(String),
    /// The frontmatter's first key isn't `carrel`, or its value isn't `card`.
    NotACard,
}

impl std::fmt::Display for CardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Yaml(message) => write!(f, "card frontmatter: {message}"),
            Self::NotACard => f.write_str("not a card: frontmatter must start with `carrel: card`"),
        }
    }
}

impl std::error::Error for CardError {}

impl Card {
    /// Reads card frontmatter (the YAML between the `---` lines, without them).
    ///
    /// ```
    /// use carrel_core::card::{Card, Kind};
    ///
    /// let card = Card::from_yaml(
    ///     "carrel: card\nformat: \"1.0\"\nid: 01a18701-0500-776a-9867-0802f3a80e26\n\
    ///      kind: physical\ntitle: \"A book\"\nlocation: \"Shelf\"\nsensitivity: public\n",
    /// )?;
    /// assert_eq!(card.kind, Kind::Physical);
    /// assert!(card.problems().is_empty());
    /// # Ok::<(), carrel_core::card::CardError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`CardError::NotACard`] if the first key isn't `carrel: card`, and
    /// [`CardError::Yaml`] if the YAML is malformed or a field has the wrong type.
    pub fn from_yaml(yaml: &str) -> Result<Self, CardError> {
        let map: IndexMap<String, Value> =
            serde_saphyr::from_str(yaml).map_err(|e| CardError::Yaml(e.to_string()))?;
        match map.first() {
            Some((key, Value::String(value))) if key == "carrel" && value == "card" => {}
            _ => return Err(CardError::NotACard),
        }
        let object = Value::Object(map.into_iter().collect());
        serde_json::from_value(object).map_err(|e| CardError::Yaml(e.to_string()))
    }

    /// Writes the frontmatter as YAML, without the `---` lines. Known fields come first, in
    /// FORMAT.md's order, followed by any unknown fields in their original order.
    ///
    /// # Errors
    ///
    /// [`CardError::Yaml`] if serialization fails, which only happens for values YAML can't
    /// represent (such as non-finite numbers in unknown fields).
    pub fn to_yaml(&self) -> Result<String, CardError> {
        serde_saphyr::to_string(self).map_err(|e| CardError::Yaml(e.to_string()))
    }

    /// Everything about this card that doesn't conform to FORMAT.md §5, using only the card
    /// itself. Checks that need the archive (tag vocabularies, link targets, checksums against
    /// files) live elsewhere.
    ///
    /// ```
    /// use carrel_core::card::Card;
    ///
    /// // A bundle must say how it is sealed, and needs a manifest, size and item name.
    /// let card = Card::from_yaml(
    ///     "carrel: card\nformat: \"1.0\"\nid: 01a18701-0500-776a-9867-0802f3a80e26\n\
    ///      kind: bundle\ntitle: \"A repo\"\nsensitivity: personal\n",
    /// )?;
    /// let fields: Vec<_> = card.problems().into_iter().map(|p| p.field).collect();
    /// assert_eq!(fields, ["item", "sealed", "size", "manifest"]);
    /// # Ok::<(), carrel_core::card::CardError>(())
    /// ```
    #[must_use]
    pub fn problems(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        let mut flag = |field: &str, message: &str| {
            out.push(Problem {
                field: field.into(),
                message: message.into(),
            });
        };
        self.check_fields_for_kind(&mut flag);
        self.check_values(&mut flag);
        self.check_dates(&mut flag);
        out
    }

    /// Which fields each kind requires or forbids (FORMAT.md §4–5).
    fn check_fields_for_kind(&self, flag: &mut impl FnMut(&str, &str)) {
        if !self.id.is_v7() {
            flag("id", "must be a version 7 UUID");
        }
        if self.title.trim().is_empty() {
            flag("title", "must not be empty");
        }

        let has_item = matches!(self.kind, Kind::File | Kind::Folder | Kind::Bundle);
        match (has_item, &self.item) {
            (true, None) => flag("item", "required for files, folders and bundles"),
            (false, Some(_)) => flag("item", "only files, folders and bundles have an item"),
            (true, Some(name)) if name.contains('/') || name.is_empty() => {
                flag("item", "must be a name in the card's folder, not a path");
            }
            _ => {}
        }
        match (self.kind, self.sealed) {
            (Kind::Bundle, None) => flag("sealed", "required for bundles"),
            (Kind::Bundle, Some(_)) | (_, None) => {}
            (_, Some(_)) => flag("sealed", "only bundles are sealed"),
        }
        if self.allow_readme.is_some() && self.sealed != Some(Sealing::Soft) {
            flag("allow_readme", "only applies to soft-sealed bundles");
        }
        if self.kind == Kind::File {
            if self.checksum.is_none() {
                flag("checksum", "required for files");
            }
        } else if self.checksum.is_some() {
            flag(
                "checksum",
                "only files have a checksum; folders and bundles use a manifest",
            );
        }
        if has_item && self.size.is_none() {
            flag("size", "required for files, folders and bundles");
        }
        match (
            matches!(self.kind, Kind::Folder | Kind::Bundle),
            &self.manifest,
        ) {
            (true, None) => flag("manifest", "required for folders and bundles"),
            (false, Some(_)) => flag("manifest", "only folders and bundles have a manifest"),
            _ => {}
        }
        if matches!(self.kind, Kind::Physical | Kind::External) && self.location.is_none() {
            flag(
                "location",
                "required for physical objects and external references",
            );
        }
        if self.kind == Kind::Url && self.url.is_none() {
            flag("url", "required for URL items");
        }
        match (self.kind == Kind::Tombstone, &self.removed) {
            (true, None) => flag("removed", "required for tombstones"),
            (false, Some(_)) => flag("removed", "only tombstones have removal details"),
            _ => {}
        }
        if self.checked_out.is_some() && self.kind.is_card_only() {
            flag("checked_out", "card-only items can't be checked out");
        }
    }

    /// Well-formedness of tags, links, checksums and identifiers.
    fn check_values(&self, flag: &mut impl FnMut(&str, &str)) {
        const SHA256: &str = "must be sha256: followed by 64 lowercase hex digits";
        for (i, tag) in self.tags.iter().enumerate() {
            if !tag.is_well_formed() {
                flag(
                    &format!("tags[{i}]"),
                    "must be lowercase letters and digits joined by '-'",
                );
            }
        }
        for (i, link) in self.links.iter().enumerate() {
            if !link.id.is_v7() {
                flag(&format!("links[{i}].id"), "must be a version 7 UUID");
            }
            if link.link_type.0.trim().is_empty() {
                flag(&format!("links[{i}].type"), "must not be empty");
            }
        }
        if let Some(c) = &self.checksum
            && !c.is_valid()
        {
            flag("checksum", SHA256);
        }
        if let Some(m) = &self.manifest
            && !m.checksum.is_valid()
        {
            flag("manifest.checksum", SHA256);
        }
        if let Some(c) = &self.checked_out {
            if !c.device.is_v7() {
                flag("checked_out.device", "must be a version 7 UUID");
            }
            if !c.person.is_v7() {
                flag("checked_out.person", "must be a version 7 UUID");
            }
        }
    }

    /// Every date and timestamp anywhere on the card (FORMAT.md §2.4).
    fn check_dates(&self, flag: &mut impl FnMut(&str, &str)) {
        let mut dates: Vec<(String, &DateOrTime)> = Vec::new();
        if let Some(d) = &self.dates {
            let named = [
                ("created", &d.created),
                ("modified", &d.modified),
                ("added", &d.added),
                ("worked", &d.worked),
                ("document", &d.document),
            ];
            dates.extend(
                named
                    .into_iter()
                    .filter_map(|(n, v)| Some((format!("dates.{n}"), v.as_ref()?))),
            );
        }
        if let Some(c) = &self.checked_out {
            dates.push(("checked_out.since".into(), &c.since));
            if let Some(due) = &c.due {
                dates.push(("checked_out.due".into(), due));
            }
        }
        if let Some(g) = &self.generated {
            dates.push(("generated.at".into(), &g.at));
        }
        if let Some(r) = &self.removed {
            dates.push(("removed.at".into(), &r.at));
        }
        for (field, value) in dates {
            if !value.is_valid() {
                flag(
                    &field,
                    "must be a date (2026-10-29) or a timestamp with an offset",
                );
            }
        }
    }
}

/// The JSON Schema for card frontmatter, as pretty-printed JSON.
///
/// # Panics
///
/// Never in practice: a generated schema is plain JSON and always serializes.
#[must_use]
pub fn json_schema() -> String {
    let schema = schemars::schema_for!(Card);
    let mut text = serde_json::to_string_pretty(&schema).expect("schemas always serialize");
    text.push('\n');
    text
}
