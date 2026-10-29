//! Archive-level documents: `ARCHIVE.md` (FORMAT.md §6.2), `_collection.md` (§6.3),
//! `_System/archive.json` (§7.1) and `_System/settings.json` (§7.2).
//!
//! Like cards, every type keeps unknown fields in `extra` and writes them back unchanged.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::FormatVersion;
use crate::card::{DateOrTime, Extra, Id, Sensitivity, Tag};

/// The frontmatter of `ARCHIVE.md`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchiveDoc {
    /// Document type; always `archive`.
    pub carrel: ArchiveMarker,
    /// Format version the document was written with.
    pub format: FormatVersion,
    /// The archive's permanent identifier; must match `archive.json`.
    pub id: Id,
    /// The archive's name.
    pub name: String,
    /// Custom link types, beyond the built-in ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub link_types: Vec<NameDescription>,
    /// The archive-wide tag vocabulary.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<TagDef>,
    /// Sensitivity given to new items unless something says otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensitivity_default: Option<Sensitivity>,
    /// Items larger than this many bytes prompt a placement choice at ingest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_threshold: Option<u64>,
    /// Minimum items before a new collection is proposed (literary warrant).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_collection_items: Option<u32>,
    /// Fields this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// The `carrel: archive` marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveMarker {
    /// The only value.
    #[default]
    Archive,
}

/// The frontmatter of a `_collection.md`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionDoc {
    /// Document type; always `collection`.
    pub carrel: CollectionMarker,
    /// Format version the document was written with.
    pub format: FormatVersion,
    /// The collection's permanent identifier.
    pub id: Id,
    /// The collection's title.
    pub title: String,
    /// One line describing the collection, used in listings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// A card template name for this collection's items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_template: Option<String>,
    /// This collection's tag vocabulary.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<TagDef>,
    /// Fields this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// The `carrel: collection` marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CollectionMarker {
    /// The only value.
    #[default]
    Collection,
}

/// A tag in a vocabulary, with what it means.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TagDef {
    /// The tag.
    pub name: Tag,
    /// What it means.
    pub description: String,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// A named thing with a description, such as a custom link type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NameDescription {
    /// The name.
    pub name: String,
    /// What it means.
    pub description: String,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// `_System/archive.json`: what makes a folder a Carrel archive (FORMAT.md §7.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchiveInfo {
    /// Document type; always `archive`.
    pub carrel: ArchiveMarker,
    /// The archive's format version.
    pub format: FormatVersion,
    /// The archive's permanent identifier.
    pub id: Id,
    /// When the archive was created.
    pub created: DateOrTime,
    /// The program that created it.
    pub created_by: String,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// `_System/settings.json`: settings every device follows (FORMAT.md §7.2).
/// `Settings::default()` gives FORMAT.md's defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// How changes made outside Carrel are handled.
    #[serde(default)]
    pub outside_edits: OutsideEdits,
    /// Checkout behavior.
    #[serde(default)]
    pub checkout: CheckoutSettings,
    /// Work-session detection.
    #[serde(default)]
    pub sessions: SessionSettings,
    /// Which reminders are on.
    #[serde(default = "default_reminders")]
    pub reminders: Vec<String>,
    /// The search embedding model, once one is chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Value>,
    /// The backup repository's location and type (never the password).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backups: Option<Value>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// Reminders on by default (FORMAT.md §7.2).
fn default_reminders() -> Vec<String> {
    ["idle", "weekly", "menu-bar"].map(String::from).to_vec()
}

/// How changes made outside Carrel to archived items are handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutsideEdits {
    /// Flag each change for review before recording it.
    #[default]
    Flag,
    /// Record changes automatically as manual edits.
    Reconcile,
    /// Treat changes as drift and offer to undo them.
    Drift,
}

/// Checkout settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckoutSettings {
    /// Where items are checked out to: `desktop` or a path.
    #[serde(default = "default_destination")]
    pub destination: String,
    /// What happens to the working copy after return: `ask`, `trash` or `keep`.
    #[serde(default = "default_after_return")]
    pub after_return: String,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for CheckoutSettings {
    fn default() -> Self {
        Self {
            destination: default_destination(),
            after_return: default_after_return(),
            extra: Extra::new(),
        }
    }
}

/// Checkout destination by default (FORMAT.md §7.2).
fn default_destination() -> String {
    "desktop".into()
}

/// What happens to a working copy after return by default (FORMAT.md §7.2).
fn default_after_return() -> String {
    "ask".into()
}

/// Work-session settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSettings {
    /// Minutes without changes that end a work session.
    #[serde(default = "default_idle_minutes")]
    pub idle_minutes: u32,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            idle_minutes: default_idle_minutes(),
            extra: Extra::new(),
        }
    }
}

/// Minutes without changes that end a work session by default (FORMAT.md §7.2).
fn default_idle_minutes() -> u32 {
    45
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            outside_edits: OutsideEdits::default(),
            checkout: CheckoutSettings::default(),
            sessions: SessionSettings::default(),
            reminders: default_reminders(),
            embedding: None,
            backups: None,
            extra: Extra::new(),
        }
    }
}
