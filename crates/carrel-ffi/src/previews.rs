//! Previews of items (`docs/design/mac-app.md`, "Previews"): where each item's thumbnail is
//! kept in this device's preview cache, and Carrel's own drawings of the formats Quick Look
//! can't show.

use carrel_core::card::Sensitivity;
use carrel_core::card_file::CardFile;
use carrel_core::previews::{self, PreviewCache};

use crate::{ArchiveHandle, CarrelError, text};

/// What an app needs to preview an item.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ItemPreview {
    /// The item's identifier.
    pub id: String,
    /// The item's file, folder or bundle, as an absolute path; `None` for card-only items, and
    /// when it isn't there.
    pub file: Option<String>,
    /// Where its thumbnail is kept, or goes once it's made; `None` when nothing is kept for it,
    /// for a restricted item or one with no file.
    pub thumbnail: Option<String>,
    /// Whether Carrel draws it ([`ArchiveHandle::draw_preview`]), because Quick Look can't
    /// show it: STEP, IGES and 3MF.
    pub drawn: bool,
    /// Whether it's restricted, so its preview waits until it's asked for.
    pub restricted: bool,
}

/// The longest side of a thumbnail in the preview cache, in pixels.
#[uniffi::export]
#[must_use]
pub fn thumbnail_pixels() -> u32 {
    previews::THUMBNAIL
}

#[uniffi::export]
impl ArchiveHandle {
    /// How to preview each cataloged item, by card path.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive or a card can't be read.
    pub fn previews(&self) -> Result<Vec<ItemPreview>, CarrelError> {
        let archive = self.fresh()?;
        let root = archive.root();
        let cache = PreviewCache::of(&archive);
        archive
            .items()?
            .into_iter()
            .map(|item| {
                let file = CardFile::parse(&std::fs::read_to_string(root.join(&item.card))?)?;
                let restricted = file.card().sensitivity == Sensitivity::Restricted;
                let key = item.item.as_deref().and_then(|p| previews::key(root, p));
                let path = item.item.as_deref().map(|p| root.join(p));
                Ok(ItemPreview {
                    id: item.id.to_string(),
                    file: key.as_ref().and(path.as_deref()).map(text),
                    thumbnail: key
                        .as_deref()
                        .filter(|_| !restricted)
                        .map(|k| text(&cache.thumbnail(k))),
                    drawn: path.as_deref().is_some_and(previews::is_drawn),
                    restricted,
                })
            })
            .collect()
    }

    /// Carrel's drawing of the item `id`, as a PNG: the one kept in the preview cache, or one
    /// drawn now by this device's providers (the CAD provider, or the 3MF reader's thumbnail).
    /// A restricted item's drawing isn't kept: it's a temporary file for the caller to delete.
    /// `None` when nothing here draws it.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if there's no such item, or [`CarrelError::Failed`] if its
    /// card can't be read.
    pub fn draw_preview(&self, id: String) -> Result<Option<String>, CarrelError> {
        let archive = self.fresh()?;
        let item = Self::item(&archive, &id)?;
        let Some(relative) = item.item else {
            return Ok(None);
        };
        let root = archive.root();
        let path = root.join(&relative);
        let file = CardFile::parse(&std::fs::read_to_string(root.join(&item.card))?)?;
        if file.card().sensitivity == Sensitivity::Restricted {
            return Ok(previews::draw(&self.registry(), &path).map(|p| text(&p)));
        }
        let Some(key) = previews::key(root, &relative) else {
            return Ok(None);
        };
        Ok(PreviewCache::of(&archive)
            .draw(&self.registry(), &path, &key)
            .map(|p| text(&p)))
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::Sample;

    #[test]
    fn thumbnails_are_kept_in_the_device_cache_but_never_for_restricted_items() {
        let sample = Sample::new();
        let note = sample.item_titled("A note on coupled oscillators");
        let root = std::path::PathBuf::from(sample.handle.root());
        let previews = sample.handle.previews().unwrap();
        let preview = previews.iter().find(|p| p.id == note).unwrap();
        let file = root.join("Reading/Coupled oscillators note.pdf");
        assert_eq!(preview.file.as_deref(), Some(crate::text(&file).as_str()));
        let cache = root.join("_System/cache.nosync/previews/thumbnails");
        assert!(
            preview
                .thumbnail
                .as_ref()
                .unwrap()
                .starts_with(&crate::text(&cache))
        );
        assert!(!preview.drawn && !preview.restricted);
        // Card-only items have nothing to preview.
        assert!(
            previews
                .iter()
                .any(|p| p.file.is_none() && p.thumbnail.is_none())
        );

        let card = sample
            .root()
            .join("Reading/Coupled oscillators note.pdf.md");
        let text = std::fs::read_to_string(&card).unwrap();
        std::fs::write(
            &card,
            text.replace("sensitivity: public", "sensitivity: restricted"),
        )
        .unwrap();
        let previews = sample.handle.previews().unwrap();
        let preview = previews.iter().find(|p| p.id == note).unwrap();
        assert!(preview.restricted);
        assert_eq!(preview.thumbnail, None);
        assert!(preview.file.is_some());
        // Only the built-in providers here: nothing draws a PDF, and nothing is written.
        assert_eq!(sample.handle.draw_preview(note).unwrap(), None);
        assert!(!sample.root().join("_System/cache.nosync/previews").exists());
    }
}
