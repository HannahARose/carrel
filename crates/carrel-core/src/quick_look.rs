//! Quick Look on the Mac (`docs/design/macos-integration.md`, "Quick Look"): a card laid out
//! as the app's inspector shows it, and Carrel's own drawings kept where the sandboxed preview
//! extension can read them.
//!
//! The extension reads only the file it's previewing, so each preview is made from that
//! file's text and path. A restricted item's card shows its title only.
//!
//! Drawings (STEP, IGES, 3MF) come from providers the extension can't run. The agent, which
//! can, keeps a copy of each drawing in a store folder the extension may read, named by
//! [`drawing_key`] of the item's file: the extension finds it by the file it's given. Nothing
//! is kept there for a restricted item.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::archive::Archive;
use crate::card::{Card, Sensitivity};
use crate::card_file::{CardFile, CardFileError, HISTORY};
use crate::previews::{self, PreviewCache};
use crate::providers::Registry;
use crate::sections;

/// What a restricted card's preview says in place of the rest of the card.
pub const RESTRICTED_NOTE: &str = "Restricted: open in Carrel to see it";

/// One history entry in a preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewEntry {
    /// Its heading: the date, how long and what wrote it, such as `2026-10-29 · 2h 10m`.
    pub heading: String,
    /// Whether it waits for the owner's approval.
    pub pending: bool,
    /// Its text.
    pub text: String,
}

/// A card laid out for Quick Look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardPreview {
    /// The item's title.
    pub title: String,
    /// The item's kind, such as `file`; empty for a restricted card.
    pub kind: String,
    /// Its summary, when it has one.
    pub summary: Option<String>,
    /// The card's other fields, labeled for people, in the order the inspector shows them.
    pub fields: Vec<(String, String)>,
    /// Its tags.
    pub tags: Vec<String>,
    /// Its collection, as `Parent / Child`; empty at the archive's root.
    pub collection: String,
    /// The owner's notes, the body without its history section, as Markdown.
    pub notes: String,
    /// Its history, newest first, as the card writes it.
    pub history: Vec<PreviewEntry>,
    /// Whether it's restricted, so everything but the title is left out.
    pub restricted: bool,
}

/// The preview of the card whose full text is `text`, in `collection` (its folder relative to
/// the archive root, with `/` separators).
///
/// # Errors
///
/// [`CardFileError`] if `text` isn't a card.
pub fn card_preview(text: &str, collection: &str) -> Result<CardPreview, CardFileError> {
    let file = CardFile::parse(text)?;
    let card = file.card();
    if card.sensitivity == Sensitivity::Restricted {
        return Ok(CardPreview {
            title: card.title.clone(),
            kind: String::new(),
            summary: None,
            fields: Vec::new(),
            tags: Vec::new(),
            collection: String::new(),
            notes: String::new(),
            history: Vec::new(),
            restricted: true,
        });
    }
    let history = file
        .history()
        .unwrap_or_default()
        .entries
        .iter()
        .map(|entry| {
            let mut heading = vec![entry.date.clone()];
            heading.extend(entry.duration.clone());
            heading.extend(entry.tags.iter().cloned());
            PreviewEntry {
                heading: heading.join(" · "),
                pending: entry.pending,
                text: entry.lines.join("\n").trim().to_owned(),
            }
        })
        .collect();
    Ok(CardPreview {
        title: card.title.clone(),
        kind: word(card.kind),
        summary: card
            .summary
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        fields: fields(card),
        tags: card.tags.iter().map(|t| t.0.clone()).collect(),
        collection: collection
            .split('/')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" / "),
        notes: notes(file.body()),
        history,
        restricted: false,
    })
}

/// A serialized enum's word, such as `file` or `personal`.
fn word(value: impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// The card's fields for people, beside the title, summary and tags.
fn fields(card: &Card) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let mut add = |label: &str, value: Option<String>| {
        if let Some(value) = value.filter(|v| !v.trim().is_empty()) {
            fields.push((label.to_owned(), value));
        }
    };
    add("Item", card.item.clone());
    add("Address", card.url.clone());
    add("Location", card.location.clone());
    if let Some(dates) = &card.dates {
        for (label, date) in [
            ("Document date", &dates.document),
            ("Created", &dates.created),
            ("Modified", &dates.modified),
            ("Added", &dates.added),
            ("Last worked on", &dates.worked),
        ] {
            add(label, date.as_ref().map(|d| d.0.clone()));
        }
    }
    add("Size", card.size.map(crate::human_size));
    add("Sealed", card.sealed.map(word));
    add("Sensitivity", Some(word(card.sensitivity)));
    if let Some(checkout) = &card.checked_out {
        let due = checkout
            .due
            .as_ref()
            .map(|d| format!(", due back {}", d.0))
            .unwrap_or_default();
        add(
            "Checked out",
            Some(format!(
                "to {} since {}{due}",
                checkout.to, checkout.since.0
            )),
        );
    }
    fields
}

/// The body without its history section, trimmed. A body whose markers are broken is shown
/// whole.
fn notes(body: &str) -> String {
    let history = sections::find_all(body)
        .ok()
        .and_then(|found| found.into_iter().find(|s| s.name == HISTORY));
    match history {
        Some(section) => format!(
            "{}{}",
            &body[..section.whole.start],
            &body[section.whole.end..]
        ),
        None => body.to_owned(),
    }
    .trim()
    .to_owned()
}

/// The collection of the card at `card`, an absolute path: its folder relative to whichever of
/// `roots` holds it, with `/` separators. When none does, its folder's name.
#[must_use]
pub fn collection_of(card: &Path, roots: &[PathBuf]) -> String {
    let folder = card.parent().unwrap_or(card);
    let within = roots
        .iter()
        .filter_map(|root| folder.strip_prefix(root).ok())
        .min_by_key(|relative| relative.components().count());
    match within {
        Some(relative) => relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
        None => folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

/// The name of the file at `path`'s drawing in the store, without `.png`: a SHA-256 of its
/// full path, size and modification time, so a changed or moved file is drawn again. `None`
/// when it isn't there.
#[must_use]
pub fn drawing_key(path: &Path) -> Option<String> {
    let path = path.canonicalize().ok()?;
    previews::key(Path::new("/"), &path.to_string_lossy())
}

/// Where the drawing for `key` is kept in `store`.
#[must_use]
pub fn drawing_in(store: &Path, key: &str) -> PathBuf {
    store.join(format!("{key}.png"))
}

/// Keeps a drawing of each of `archive`'s drawn items (STEP, IGES, 3MF) in `store`, drawing
/// with `registry` what the archive's preview cache doesn't have yet. Restricted items are
/// left out. Returns the keys of the drawings `archive` has in `store`, for
/// [`prune_drawings`].
///
/// # Errors
///
/// An error if the archive's items can't be listed or the store can't be made.
pub fn keep_drawings(
    archive: &Archive,
    registry: &Registry,
    store: &Path,
) -> std::io::Result<Vec<String>> {
    let root = archive.root();
    let cache = PreviewCache::of(archive);
    let items = archive.items().map_err(std::io::Error::other)?;
    let mut kept = Vec::new();
    for item in items {
        let Some(relative) = item.item.as_deref() else {
            continue;
        };
        let path = root.join(relative);
        if !previews::is_drawn(&path) {
            continue;
        }
        let restricted = std::fs::read_to_string(root.join(&item.card))
            .ok()
            .and_then(|text| CardFile::parse(&text).ok())
            .is_none_or(|file| file.card().sensitivity == Sensitivity::Restricted);
        if restricted {
            continue;
        }
        let (Some(key), Some(cached)) = (drawing_key(&path), previews::key(root, relative)) else {
            continue;
        };
        let target = drawing_in(store, &key);
        if !target.exists() {
            let Some(drawing) = cache.draw(registry, &path, &cached) else {
                continue;
            };
            std::fs::create_dir_all(store)?;
            std::fs::copy(drawing, &target)?;
        }
        kept.push(key);
    }
    Ok(kept)
}

/// Deletes the drawings in `store` whose keys aren't in `keep`: those of items gone, changed,
/// made restricted, or in archives no longer followed.
pub fn prune_drawings(store: &Path, keep: &[String]) {
    let keep: BTreeSet<&str> = keep.iter().map(String::as_str).collect();
    let Ok(entries) = std::fs::read_dir(store) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let stale = path.extension().is_some_and(|e| e == "png")
            && path
                .file_stem()
                .is_some_and(|stem| !keep.contains(stem.to_string_lossy().as_ref()));
        if stale {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARD: &str = "---\ncarrel: card\nformat: \"1.0\"\nid: 01a18701-0500-776a-9867-080924eb019c\n\
        kind: bundle\ntitle: \"oscillator-sim\"\nitem: \"oscillator-sim\"\nsealed: hard\n\
        summary: \"A tiny simulator.\"\ntags: [simulation, oscillators]\n\
        dates:\n  created: \"2026-09-01\"\n  added: \"2026-10-24\"\nsensitivity: personal\n\
        size: 613\n---\n\nSealed because it is a Rust project.\n\n<!-- carrel:history -->\n\
        ## History\n\n> [!pending] 2026-10-28 · 2h 10m · auto-generated\n> Added a parameter.\n\n\
        ### 2026-10-27 · 45m\nSet up the project.\nMy notes: try a smaller step.\n\
        <!-- /carrel:history -->\n";

    #[test]
    fn a_card_is_laid_out_as_the_inspector_shows_it() {
        let preview = card_preview(CARD, "Projects/Software").unwrap();
        assert_eq!(preview.title, "oscillator-sim");
        assert_eq!(preview.kind, "bundle");
        assert_eq!(preview.summary.as_deref(), Some("A tiny simulator."));
        assert_eq!(preview.tags, ["simulation", "oscillators"]);
        assert_eq!(preview.collection, "Projects / Software");
        assert_eq!(preview.notes, "Sealed because it is a Rust project.");
        let labels: Vec<&str> = preview.fields.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            labels,
            ["Item", "Created", "Added", "Size", "Sealed", "Sensitivity"]
        );
        assert!(preview.fields.contains(&("Size".into(), "613 B".into())));
        assert_eq!(preview.history.len(), 2);
        assert_eq!(
            preview.history[0].heading,
            "2026-10-28 · 2h 10m · auto-generated"
        );
        assert!(preview.history[0].pending);
        assert_eq!(preview.history[0].text, "Added a parameter.");
        assert_eq!(
            preview.history[1].text,
            "Set up the project.\nMy notes: try a smaller step."
        );
        assert!(!preview.restricted);
    }

    #[test]
    fn a_restricted_card_shows_its_title_only() {
        let text = CARD.replace("sensitivity: personal", "sensitivity: restricted");
        let preview = card_preview(&text, "Projects").unwrap();
        assert!(preview.restricted);
        assert_eq!(preview.title, "oscillator-sim");
        assert_eq!(
            (
                preview.kind.as_str(),
                preview.summary,
                preview.fields.len(),
                preview.tags.len(),
                preview.collection.as_str(),
                preview.notes.as_str(),
                preview.history.len()
            ),
            ("", None, 0, 0, "", "", 0)
        );
    }

    #[test]
    fn a_markdown_file_that_isnt_a_card_isnt_previewed() {
        assert!(card_preview("# Notes\n\nJust notes.\n", "").is_err());
        assert!(card_preview("---\ntitle: Post\n---\nText\n", "").is_err());
    }

    #[test]
    fn the_collection_is_the_cards_folder_within_its_archive() {
        let roots = [PathBuf::from("/Archive"), PathBuf::from("/Other")];
        let card = Path::new("/Archive/Projects/Hardware/board.kicad.md");
        assert_eq!(collection_of(card, &roots), "Projects/Hardware");
        assert_eq!(collection_of(Path::new("/Archive/top.md"), &roots), "");
        assert_eq!(
            collection_of(Path::new("/Elsewhere/Reading/a.pdf.md"), &roots),
            "Reading"
        );
    }

    #[test]
    fn drawings_are_named_by_the_file_and_stale_ones_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("part.step");
        std::fs::write(&file, "ISO-10303-21;").unwrap();
        let key = drawing_key(&file).unwrap();
        assert_eq!(key.len(), 64);
        assert_eq!(drawing_key(&file), Some(key.clone()));
        std::fs::write(&file, "ISO-10303-21; changed").unwrap();
        assert_ne!(drawing_key(&file), Some(key.clone()));
        assert_eq!(drawing_key(&dir.path().join("gone.step")), None);

        let store = dir.path().join("store");
        std::fs::create_dir(&store).unwrap();
        std::fs::write(drawing_in(&store, "kept"), "png").unwrap();
        std::fs::write(drawing_in(&store, "stale"), "png").unwrap();
        prune_drawings(&store, &["kept".to_owned()]);
        assert!(drawing_in(&store, "kept").exists());
        assert!(!drawing_in(&store, "stale").exists());
    }
}
