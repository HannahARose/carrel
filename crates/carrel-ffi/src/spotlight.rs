//! Spotlight on the Mac (`docs/design/macos-integration.md`, "Spotlight"): what the agent adds
//! to Core Spotlight for an archive, by its `spotlight.index` setting. Restricted items are
//! never among it.

use carrel_core::spotlight::{self, EntryKind};

use crate::{ArchiveHandle, CarrelError};

/// What a Spotlight entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SpotlightKind {
    /// An item's card.
    Item,
    /// A collection.
    Collection,
    /// A glossary fact.
    Fact,
}

/// One thing Spotlight finds in an archive.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SpotlightEntry {
    /// Its identifier within the archive: `item/<id>`, `collection/<path>` or `fact/<term>`.
    pub id: String,
    /// What it is.
    pub kind: SpotlightKind,
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
    /// Changes whenever anything above does, so an entry is indexed again only then.
    pub version: String,
}

#[uniffi::export]
impl ArchiveHandle {
    /// Everything Spotlight should find in the archive, by its `spotlight.index` setting:
    /// cards, collections and glossary facts. Empty when the setting is `off`; restricted
    /// items are never included.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive's items or collections can't be listed.
    pub fn spotlight_entries(&self) -> Result<Vec<SpotlightEntry>, CarrelError> {
        let archive = self.fresh()?;
        let index = archive.settings().spotlight.index;
        Ok(spotlight::entries(&archive, index)?
            .into_iter()
            .map(|e| SpotlightEntry {
                id: e.id,
                kind: match e.kind {
                    EntryKind::Item => SpotlightKind::Item,
                    EntryKind::Collection => SpotlightKind::Collection,
                    EntryKind::Fact => SpotlightKind::Fact,
                },
                title: e.title,
                description: e.description,
                keywords: e.keywords,
                collection: e.collection,
                item_kind: e.item_kind,
                document_date: e.document_date,
                added: e.added,
                item: e.item,
                collection_path: e.collection_path,
                title_only: e.title_only,
                version: e.version,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::Sample;

    #[test]
    fn the_setting_decides_what_spotlight_finds_and_restricted_items_never_reach_it() {
        let sample = Sample::new();
        let note = sample.item_titled("A note on coupled oscillators");
        let card = sample
            .root()
            .join("Reading/Coupled oscillators note.pdf.md");
        let text = std::fs::read_to_string(&card).unwrap();
        std::fs::write(
            &card,
            text.replace("sensitivity: public", "sensitivity: restricted"),
        )
        .unwrap();
        let id = format!("item/{note}");
        for choice in ["standard", "all-but-restricted", "off"] {
            sample
                .handle
                .set_setting("spotlight.index".into(), choice.into())
                .unwrap();
            let entries = sample.handle.spotlight_entries().unwrap();
            assert!(entries.iter().all(|e| e.id != id), "{choice}");
            assert_eq!(entries.is_empty(), choice == "off", "{choice}");
        }
    }
}
