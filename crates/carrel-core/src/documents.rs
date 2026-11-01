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
    pub embedding: Option<EmbeddingSettings>,
    /// The backup repository's location and type (never the password).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backups: Option<Value>,
    /// Ingest behavior.
    #[serde(default, skip_serializing_if = "IngestSettings::is_default")]
    pub ingest: IngestSettings,
    /// When the structure shows signs of needing a restructure.
    #[serde(default, skip_serializing_if = "SignalSettings::is_default")]
    pub signals: SignalSettings,
    /// How the question queue behaves (`docs/design/librarian.md`, "Questions").
    #[serde(default, skip_serializing_if = "QuestionSettings::is_default")]
    pub questions: QuestionSettings,
    /// What happens to compressed files at ingest: `expand`, `keep` or `ask`
    /// (`docs/design/compressed.md`).
    #[serde(default, skip_serializing_if = "is_default_choice")]
    pub compressed_files: crate::compressed::plan::Choice,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// Search embedding settings (FORMAT.md §7.2, `docs/design/search.md`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EmbeddingSettings {
    /// The identifier of the model every device embeds with, such as `embeddinggemma-300m`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The model the archive is switching to, while its index is built beside the current
    /// model's (`carrel index model`); `model` changes once it's complete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub switching_to: Option<String>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Settings {
    /// The archive's embedding model, once one is chosen.
    #[must_use]
    pub fn embedding_model(&self) -> Option<&str> {
        self.embedding
            .as_ref()
            .and_then(|e| e.model.as_deref())
            .filter(|m| !m.trim().is_empty())
    }

    /// Chooses `model` as the archive's embedding model, keeping any other embedding settings.
    pub fn set_embedding_model(&mut self, model: &str) {
        self.embedding
            .get_or_insert_with(EmbeddingSettings::default)
            .model = Some(model.to_owned());
    }

    /// The model the archive is switching to, while its index is being built.
    #[must_use]
    pub fn embedding_switch(&self) -> Option<&str> {
        self.embedding
            .as_ref()
            .and_then(|e| e.switching_to.as_deref())
            .filter(|m| !m.trim().is_empty())
    }

    /// Records that the archive is switching to `model`, or, with `None`, that it isn't.
    pub fn set_embedding_switch(&mut self, model: Option<&str>) {
        if model.is_none() && self.embedding.is_none() {
            return;
        }
        self.embedding
            .get_or_insert_with(EmbeddingSettings::default)
            .switching_to = model.map(str::to_owned);
    }
}

/// Restructure signal settings (FORMAT.md §7.2, `docs/design/signals.md`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalSettings {
    /// How many loose items make a collection with no subcollections crowded.
    #[serde(default = "default_crowded_items")]
    pub crowded_items: u32,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// How the question queue behaves (FORMAT.md §7.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionSettings {
    /// How many days a skipped question rests before it's asked again.
    #[serde(default = "default_skip_days")]
    pub skip_days: u32,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

fn default_skip_days() -> u32 {
    14
}

impl Default for QuestionSettings {
    fn default() -> Self {
        Self {
            skip_days: default_skip_days(),
            extra: Extra::new(),
        }
    }
}

impl QuestionSettings {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

fn default_crowded_items() -> u32 {
    30
}

impl Default for SignalSettings {
    fn default() -> Self {
        Self {
            crowded_items: default_crowded_items(),
            extra: Extra::new(),
        }
    }
}

impl SignalSettings {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Ingest settings (FORMAT.md §7.2).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct IngestSettings {
    /// What happens to an item's original when its proposal is approved.
    #[serde(default)]
    pub originals: Originals,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl IngestSettings {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// What happens to an item's original when it's filed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Originals {
    /// The original is moved into the archive.
    #[default]
    Move,
    /// It's copied, and the original is left for the owner to delete.
    Keep,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde passes a reference
fn is_default_choice(choice: &crate::compressed::plan::Choice) -> bool {
    *choice == crate::compressed::plan::Choice::default()
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
            ingest: IngestSettings::default(),
            signals: SignalSettings::default(),
            questions: QuestionSettings::default(),
            compressed_files: crate::compressed::plan::Choice::default(),
            extra: Extra::new(),
        }
    }
}
