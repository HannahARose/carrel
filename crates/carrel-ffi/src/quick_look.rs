//! Quick Look on the Mac (`docs/design/macos-integration.md`, "Quick Look"): cards laid out
//! for the preview extension, and the store of Carrel's own drawings the agent keeps for it.

use std::path::{Path, PathBuf};

use carrel_core::quick_look;

use crate::{ArchiveHandle, CarrelError, text};

/// One history entry in a card's preview.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct QuickLookEntry {
    /// Its heading: the date, how long and what wrote it.
    pub heading: String,
    /// Whether it waits for the owner's approval.
    pub pending: bool,
    /// Its text.
    pub text: String,
}

/// A labeled field of a card's preview.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct QuickLookField {
    /// The field's name, for people.
    pub label: String,
    /// Its value.
    pub value: String,
}

/// A card laid out as the app's inspector shows it. A restricted card has its title only.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct QuickLookCard {
    /// The item's title.
    pub title: String,
    /// The item's kind, such as `file`; empty for a restricted card.
    pub kind: String,
    /// Its summary, when it has one.
    pub summary: Option<String>,
    /// Its other fields.
    pub fields: Vec<QuickLookField>,
    /// Its tags.
    pub tags: Vec<String>,
    /// Its collection, as `Parent / Child`; empty at the archive's root.
    pub collection: String,
    /// The owner's notes, as Markdown.
    pub notes: String,
    /// Its history, newest first.
    pub history: Vec<QuickLookEntry>,
    /// Whether it's restricted.
    pub restricted: bool,
    /// What a restricted card says in place of the rest: "Restricted: open in Carrel to see
    /// it". Empty otherwise.
    pub restricted_note: String,
}

/// The preview of the card at `path` whose full text is `text`. `roots` are the archives the
/// agent follows, to name its collection by; outside them, it's the card's folder.
///
/// # Errors
///
/// [`CarrelError::Invalid`] if `text` isn't a card, such as Markdown that isn't Carrel's.
#[uniffi::export]
pub fn quick_look_card(
    text: String,
    path: String,
    roots: Vec<String>,
) -> Result<QuickLookCard, CarrelError> {
    let roots: Vec<PathBuf> = roots.iter().map(PathBuf::from).collect();
    let collection = quick_look::collection_of(Path::new(&path), &roots);
    let preview = quick_look::card_preview(&text, &collection)
        .map_err(|e| CarrelError::invalid(format!("not a Carrel card: {e}")))?;
    Ok(QuickLookCard {
        title: preview.title,
        kind: preview.kind,
        summary: preview.summary,
        fields: preview
            .fields
            .into_iter()
            .map(|(label, value)| QuickLookField { label, value })
            .collect(),
        tags: preview.tags,
        collection: preview.collection,
        notes: preview.notes,
        history: preview
            .history
            .into_iter()
            .map(|e| QuickLookEntry {
                heading: e.heading,
                pending: e.pending,
                text: e.text,
            })
            .collect(),
        restricted_note: if preview.restricted {
            quick_look::RESTRICTED_NOTE.to_owned()
        } else {
            String::new()
        },
        restricted: preview.restricted,
    })
}

/// Where the drawing of the file at `path` is kept in `store`, whether or not it's there.
/// `None` when the file isn't there.
#[uniffi::export]
#[must_use]
pub fn quick_look_drawing(store: String, path: String) -> Option<String> {
    let key = quick_look::drawing_key(Path::new(&path))?;
    Some(text(&quick_look::drawing_in(Path::new(&store), &key)))
}

/// Deletes the drawings in `store` but those named in `keep`, which
/// [`ArchiveHandle::keep_quick_look_drawings`] returned for each archive followed.
#[uniffi::export]
pub fn prune_quick_look_drawings(store: String, keep: Vec<String>) {
    quick_look::prune_drawings(Path::new(&store), &keep);
}

#[uniffi::export]
impl ArchiveHandle {
    /// Keeps a drawing of each of the archive's STEP, IGES and 3MF items in `store`, for Quick
    /// Look, drawing those not drawn yet with this device's providers. Restricted items are
    /// left out. Returns the drawings' keys, for [`prune_quick_look_drawings`].
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive can't be read or the store written.
    pub fn keep_quick_look_drawings(&self, store: String) -> Result<Vec<String>, CarrelError> {
        let archive = self.fresh()?;
        quick_look::keep_drawings(&archive, &self.registry(), Path::new(&store))
            .map_err(CarrelError::failed)
    }
}

#[cfg(test)]
mod tests {
    use carrel_core::previews::{PreviewCache, key};

    use super::*;
    use crate::tests::Sample;

    /// Adds a STEP item to the sample archive with a drawing already in its preview cache, as
    /// the CAD provider leaves one.
    fn add_drawn(sample: &Sample, name: &str, sensitivity: &str) {
        let root = sample.root();
        std::fs::write(
            root.join("Projects").join(name),
            format!("ISO-10303-21; {name}"),
        )
        .unwrap();
        let id = carrel_core::card::Id::new_v7();
        std::fs::write(
            root.join("Projects").join(format!("{name}.md")),
            format!(
                "---\ncarrel: card\nformat: \"1.0\"\nid: {id}\nkind: file\ntitle: \"{name}\"\n\
                 item: \"{name}\"\nsensitivity: {sensitivity}\nchecksum: \"sha256:{}\"\n\
                 size: 1\n---\n",
                "0".repeat(64)
            ),
        )
        .unwrap();
        let archive = carrel_core::archive::Archive::open(&root).unwrap();
        let cached = key(&root, &format!("Projects/{name}")).unwrap();
        let drawing = PreviewCache::of(&archive).drawing(&cached);
        std::fs::create_dir_all(drawing.parent().unwrap()).unwrap();
        std::fs::write(drawing, "png").unwrap();
    }

    #[test]
    fn drawings_are_kept_for_quick_look_but_never_for_restricted_items() {
        let sample = Sample::new();
        add_drawn(&sample, "bracket.step", "personal");
        add_drawn(&sample, "implant.step", "restricted");
        let store = sample.dir.path().join("QuickLook/drawings");
        let kept = sample
            .handle
            .keep_quick_look_drawings(text(&store))
            .unwrap();
        assert_eq!(kept.len(), 1);
        let bracket = sample.root().join("Projects/bracket.step");
        let implant = sample.root().join("Projects/implant.step");
        let shown = quick_look_drawing(text(&store), text(&bracket)).unwrap();
        assert_eq!(std::fs::read_to_string(shown).unwrap(), "png");
        let hidden = quick_look_drawing(text(&store), text(&implant)).unwrap();
        assert!(!Path::new(&hidden).exists());

        prune_quick_look_drawings(text(&store), Vec::new());
        assert_eq!(std::fs::read_dir(&store).unwrap().count(), 0);
    }

    #[test]
    fn cards_are_previewed_with_their_collection_and_restricted_ones_by_title() {
        let sample = Sample::new();
        let root = sample.root();
        let path = root.join("Projects/oscillator-sim.md");
        let card_text = std::fs::read_to_string(&path).unwrap();
        let card = quick_look_card(card_text.clone(), text(&path), vec![text(&root)]).unwrap();
        assert_eq!(card.title, "oscillator-sim");
        assert_eq!(card.collection, "Projects");
        assert_eq!(card.history.len(), 2);
        assert_eq!(card.restricted_note, "");

        let restricted = card_text.replace("sensitivity: personal", "sensitivity: restricted");
        let card = quick_look_card(restricted, text(&path), vec![text(&root)]).unwrap();
        assert!(card.restricted && card.summary.is_none() && card.history.is_empty());
        assert_eq!(card.restricted_note, "Restricted: open in Carrel to see it");

        assert!(matches!(
            quick_look_card("# Notes\n".into(), text(&path), Vec::new()),
            Err(CarrelError::Invalid { .. })
        ));
    }
}
