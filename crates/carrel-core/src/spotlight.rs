//! What Spotlight finds on the Mac (`docs/design/macos-integration.md`, "Spotlight"): each
//! item's card, the collections and the glossary's facts, as entries the Mac app's agent adds
//! to Core Spotlight. Spotlight indexes cards, not files: macOS already indexes the files
//! themselves where it can.
//!
//! Anyone using the Mac sees Spotlight, so `spotlight.index` (FORMAT.md §7.2) says how much
//! goes in. By default, public and personal items in full and sensitive ones by title only.
//! Restricted items never go in, whatever it says.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::archive::{Archive, ArchiveError};
use crate::card::{Card, Extra, Sensitivity};
use crate::card_file::CardFile;
use crate::ingest::card::Glossary;

/// How much of the archive Spotlight finds (`spotlight.index`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpotlightIndex {
    /// Public and personal items in full, sensitive ones by title only, and the collections
    /// and glossary facts.
    #[default]
    Standard,
    /// Every item but restricted ones in full, and the collections and glossary facts.
    AllButRestricted,
    /// Nothing.
    Off,
}

impl SpotlightIndex {
    /// Every choice, most found first.
    pub const ALL: [Self; 3] = [Self::AllButRestricted, Self::Standard, Self::Off];

    /// Its name in `settings.json`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::AllButRestricted => "all-but-restricted",
            Self::Off => "off",
        }
    }

    /// The choice named `name`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == name.trim())
    }

    /// How much of an item with `sensitivity` is found: `None` for none of it.
    #[must_use]
    pub fn shows(self, sensitivity: Sensitivity) -> Option<Shown> {
        match (self, sensitivity) {
            (Self::Off, _) | (_, Sensitivity::Restricted) => None,
            (Self::Standard, Sensitivity::Sensitive) => Some(Shown::Title),
            _ => Some(Shown::Full),
        }
    }
}

/// How much of an item Spotlight finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    /// Its title only.
    Title,
    /// Its card: title, summary, tags, collection, dates and kind.
    Full,
}

/// `settings.json`'s `spotlight` (FORMAT.md §7.2).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SpotlightSettings {
    /// How much Spotlight finds.
    #[serde(default)]
    pub index: SpotlightIndex,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl SpotlightSettings {
    /// Whether these are the defaults, so `settings.json` can leave them out.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// What an entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// An item's card.
    Item,
    /// A collection.
    Collection,
    /// A glossary fact.
    Fact,
}

/// One thing Spotlight finds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotlightEntry {
    /// Its identifier within the archive: `item/<id>`, `collection/<path>` or `fact/<term>`.
    pub id: String,
    /// What it is.
    pub kind: EntryKind,
    /// Its title: an item's or collection's title, or a fact's term.
    pub title: String,
    /// What it says: an item's summary, a collection's description, or a fact.
    pub description: Option<String>,
    /// An item's tags.
    pub keywords: Vec<String>,
    /// The collection it's in, or the collection's own path, as `Parent / Child`.
    pub collection: String,
    /// An item's kind, such as `file`.
    pub item_kind: Option<String>,
    /// An item's own date, such as a publication date.
    pub document_date: Option<String>,
    /// When an item was added.
    pub added: Option<String>,
    /// The item's identifier, to open it with.
    pub item: Option<String>,
    /// The collection's path relative to the archive root, to open it with.
    pub collection_path: Option<String>,
    /// Whether only the title is found, for a sensitive item.
    pub title_only: bool,
    /// A checksum of everything above, so an entry is indexed again only when it changes.
    pub version: String,
}

impl SpotlightEntry {
    fn new(id: String, kind: EntryKind, title: String) -> Self {
        Self {
            id,
            kind,
            title,
            description: None,
            keywords: Vec::new(),
            collection: String::new(),
            item_kind: None,
            document_date: None,
            added: None,
            item: None,
            collection_path: None,
            title_only: false,
            version: String::new(),
        }
    }

    /// Sets `version` from the rest.
    fn versioned(mut self) -> Self {
        let mut hash = Sha256::new();
        hash.update(format!("{self:?}").as_bytes());
        self.version = hash.finalize().iter().fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        });
        self
    }
}

/// `collection` (a folder relative to the archive root) as people read it: `Parent / Child`.
fn readable(collection: &str) -> String {
    collection
        .split('/')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

/// The entry for an item with `card`, filed in `collection`, as much of it as `shown` says.
#[must_use]
pub fn item_entry(card: &Card, collection: &str, shown: Shown) -> SpotlightEntry {
    let mut entry = SpotlightEntry::new(
        format!("item/{}", card.id),
        EntryKind::Item,
        card.title.trim().to_owned(),
    );
    entry.item = Some(card.id.to_string());
    if shown == Shown::Title {
        entry.title_only = true;
        return entry.versioned();
    }
    entry.description = card
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    entry.keywords = card.tags.iter().map(|t| t.0.clone()).collect();
    entry.collection = readable(collection);
    entry.item_kind = serde_json::to_value(card.kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned));
    if let Some(dates) = &card.dates {
        entry.document_date = dates.document.as_ref().map(|d| d.0.clone());
        entry.added = dates.added.as_ref().map(|d| d.0.clone());
    }
    entry.versioned()
}

/// Everything in `archive` that Spotlight finds under `index`: its items' cards, its
/// collections and its glossary facts. Restricted items are never among them; cards that
/// can't be read are left out.
///
/// # Errors
///
/// [`ArchiveError`] if the items or collections can't be listed.
pub fn entries(
    archive: &Archive,
    index: SpotlightIndex,
) -> Result<Vec<SpotlightEntry>, ArchiveError> {
    if index == SpotlightIndex::Off {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for item in archive.items()? {
        let Ok(text) = std::fs::read_to_string(archive.root().join(&item.card)) else {
            continue;
        };
        let Ok(file) = CardFile::parse(&text) else {
            continue;
        };
        let Some(shown) = index.shows(file.card().sensitivity) else {
            continue;
        };
        let collection = item.card.rsplit_once('/').map_or("", |(folder, _)| folder);
        entries.push(item_entry(file.card(), collection, shown));
    }
    for collection in archive.collections()? {
        let mut entry = SpotlightEntry::new(
            format!("collection/{}", collection.path),
            EntryKind::Collection,
            collection.doc.title.trim().to_owned(),
        );
        entry.description.clone_from(&collection.doc.description);
        entry.collection = readable(&collection.path);
        entry.collection_path = Some(collection.path.clone());
        entries.push(entry.versioned());
    }
    for (term, fact) in Glossary::of(archive).facts {
        let mut entry = SpotlightEntry::new(format!("fact/{term}"), EntryKind::Fact, term);
        entry.description = Some(fact);
        entries.push(entry.versioned());
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (tempfile::TempDir, Archive) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("archive");
        crate::bench::findability::copy_dir(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
            &root,
        )
        .unwrap();
        let archive = Archive::open(&root).unwrap();
        (dir, archive)
    }

    /// Sets every card in `archive` to `sensitivity`, but the first, which is restricted.
    fn mark(archive: &Archive, sensitivity: &str) -> Vec<String> {
        let mut restricted = Vec::new();
        for (n, item) in archive.items().unwrap().into_iter().enumerate() {
            let path = archive.root().join(&item.card);
            let text = std::fs::read_to_string(&path).unwrap();
            let file = CardFile::parse(&text).unwrap();
            let now = format!(
                "sensitivity: {}",
                serde_json::to_value(file.card().sensitivity)
                    .unwrap()
                    .as_str()
                    .unwrap()
            );
            let to = if n == 0 { "restricted" } else { sensitivity };
            if n == 0 {
                restricted.push(format!("item/{}", item.id));
            }
            std::fs::write(&path, text.replacen(&now, &format!("sensitivity: {to}"), 1)).unwrap();
        }
        restricted
    }

    #[test]
    fn restricted_cards_never_reach_the_index_under_any_setting() {
        let (_dir, archive) = sample();
        let restricted = mark(&archive, "public");
        for index in SpotlightIndex::ALL {
            let found = entries(&archive, index).unwrap();
            assert!(
                found.iter().all(|e| !restricted.contains(&e.id)),
                "{index:?}"
            );
        }
        // Whatever the sample's own sensitivities, none restricted is ever found.
        for item in archive.items().unwrap() {
            let text = std::fs::read_to_string(archive.root().join(&item.card)).unwrap();
            let restricted =
                CardFile::parse(&text).unwrap().card().sensitivity == Sensitivity::Restricted;
            for index in SpotlightIndex::ALL {
                let found = entries(&archive, index).unwrap();
                let id = format!("item/{}", item.id);
                assert!(!restricted || found.iter().all(|e| e.id != id));
            }
        }
    }

    #[test]
    fn sensitive_items_are_found_by_title_only_unless_everything_is_asked_for() {
        let (_dir, archive) = sample();
        mark(&archive, "sensitive");
        let items = |index| {
            entries(&archive, index)
                .unwrap()
                .into_iter()
                .filter(|e| e.kind == EntryKind::Item)
                .collect::<Vec<_>>()
        };
        let standard = items(SpotlightIndex::Standard);
        assert_ne!(standard.len(), 0);
        for entry in &standard {
            assert!(entry.title_only);
            assert_eq!(
                (
                    entry.description.as_ref(),
                    entry.keywords.len(),
                    entry.collection.as_str()
                ),
                (None, 0, "")
            );
        }
        let all = items(SpotlightIndex::AllButRestricted);
        assert_eq!(all.len(), standard.len());
        assert!(all.iter().all(|e| !e.title_only));
        assert_eq!(entries(&archive, SpotlightIndex::Off).unwrap(), Vec::new());
    }

    #[test]
    fn cards_collections_and_facts_are_found_and_versioned() {
        let (_dir, archive) = sample();
        let found = entries(&archive, SpotlightIndex::Standard).unwrap();
        let sim = found.iter().find(|e| e.title == "oscillator-sim").unwrap();
        assert_eq!(sim.kind, EntryKind::Item);
        assert_eq!(sim.collection, "Projects");
        assert_eq!(sim.keywords, ["simulation", "oscillators"]);
        assert_eq!(sim.item_kind.as_deref(), Some("bundle"));
        assert!(sim.item.is_some() && sim.version.len() == 64);
        assert!(
            found.iter().any(|e| e.kind == EntryKind::Collection
                && e.collection_path.as_deref() == Some("Projects"))
        );
        assert!(found.iter().any(|e| e.kind == EntryKind::Fact));

        let card = archive.root().join("Projects/oscillator-sim.md");
        let text = std::fs::read_to_string(&card).unwrap();
        std::fs::write(
            &card,
            text.replace("A tiny Rust program", "A small Rust program"),
        )
        .unwrap();
        let again = entries(&archive, SpotlightIndex::Standard).unwrap();
        let changed = again.iter().find(|e| e.id == sim.id).unwrap();
        assert_ne!(changed.version, sim.version);
        let other = |list: &[SpotlightEntry]| {
            list.iter()
                .find(|e| e.title == "Projects")
                .map(|e| e.version.clone())
        };
        assert_eq!(other(&found), other(&again));
    }

    #[test]
    fn the_setting_names_its_choices() {
        for index in SpotlightIndex::ALL {
            assert_eq!(SpotlightIndex::parse(index.as_str()), Some(index));
        }
        assert_eq!(SpotlightIndex::parse("everything"), None);
        assert_eq!(SpotlightIndex::default(), SpotlightIndex::Standard);
    }
}
