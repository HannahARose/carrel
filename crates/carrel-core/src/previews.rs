//! Previews of items: thumbnails, and drawings of the formats a platform's previewer can't
//! show, cached on this device (`docs/design/mac-app.md`, "Previews").
//!
//! The cache is `_System/cache.nosync/previews/`, laid out as:
//!
//! ```text
//! previews/
//!   thumbnails/<key>.png   a thumbnail, at most THUMBNAIL pixels on its longer side
//!   drawings/<key>.png     Carrel's own drawing, from the CAD provider or a 3MF's thumbnail
//! ```
//!
//! `<key>` is [`key`]: a SHA-256, in hex, of the item's path relative to the archive root, its
//! size and its modification time, so a file that changes or moves gets a new preview and an
//! old one is never shown. Each interface makes what it can and reads what the others made:
//! the Mac app writes thumbnails with Quick Look, the interactive mode draws them in the
//! terminal. Like the text cache, it's regenerable and device-specific (FORMAT.md §7.12):
//! left out of history, backups and sync, and safe to delete. Nothing is kept for a restricted
//! item: its preview is made when it's asked for and thrown away.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};

use crate::archive::Archive;
use crate::ingest::text::media_type;
use crate::providers::{Capability, ExtractionRequest, Registry};

/// The cache's folder, relative to the archive root.
pub const PREVIEWS: &str = "_System/cache.nosync/previews";

/// The longest side of a cached thumbnail, in pixels.
pub const THUMBNAIL: u32 = 512;

/// The media types Carrel draws itself, because the platform's previewer can't show them.
pub const DRAWN: [&str; 3] = ["model/step", "model/iges", "model/3mf"];

/// The preview cache of one archive on this device.
#[derive(Debug, Clone)]
pub struct PreviewCache {
    dir: PathBuf,
}

impl PreviewCache {
    /// The cache of `archive`, which may not exist yet.
    #[must_use]
    pub fn of(archive: &Archive) -> Self {
        Self::at(&archive.root().join(PREVIEWS))
    }

    /// A cache in `dir`.
    #[must_use]
    pub fn at(dir: &Path) -> Self {
        Self {
            dir: dir.to_owned(),
        }
    }

    /// Where the thumbnail for `key` is kept, whether or not it's there yet.
    #[must_use]
    pub fn thumbnail(&self, key: &str) -> PathBuf {
        self.dir.join("thumbnails").join(format!("{key}.png"))
    }

    /// Where the drawing for `key` is kept, whether or not it's there yet.
    #[must_use]
    pub fn drawing(&self, key: &str) -> PathBuf {
        self.dir.join("drawings").join(format!("{key}.png"))
    }

    /// The drawing of the file at `path`, whose key is `key`: the kept one, or one drawn now
    /// by the providers in `registry` and kept. `None` when nothing on this device draws it.
    #[must_use]
    pub fn draw(&self, registry: &Registry, path: &Path, key: &str) -> Option<PathBuf> {
        let kept = self.drawing(key);
        if kept.is_file() {
            return Some(kept);
        }
        let picture = draw(registry, path)?;
        let placed = std::fs::create_dir_all(kept.parent()?)
            .and_then(|()| std::fs::copy(&picture, &kept).map(drop));
        let _ = std::fs::remove_file(&picture);
        placed.ok().map(|()| kept)
    }
}

/// The key of the item at `relative` (to the archive at `root`) in the cache: changes when the
/// file is changed, replaced or moved. `None` when it isn't there.
#[must_use]
pub fn key(root: &Path, relative: &str) -> Option<String> {
    let metadata = std::fs::metadata(root.join(relative)).ok()?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    let mut hash = Sha256::new();
    hash.update(relative.as_bytes());
    hash.update([0]);
    hash.update(metadata.len().to_le_bytes());
    hash.update(modified.to_le_bytes());
    Some(hash.finalize().iter().fold(String::new(), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    }))
}

/// Whether Carrel draws the file at `path` itself, rather than the platform's previewer.
#[must_use]
pub fn is_drawn(path: &Path) -> bool {
    DRAWN.contains(&media_type(path).as_str())
}

/// A drawing of the file at `path`, as a PNG in the system's temporary folder for the caller
/// to move or delete: the first picture any text extractor for its type gives back, such as
/// the CAD provider's four views or a 3MF's thumbnail. Other pictures are deleted. `None` when
/// nothing gives one.
#[must_use]
pub fn draw(registry: &Registry, path: &Path) -> Option<PathBuf> {
    let request = ExtractionRequest {
        path: path.to_owned(),
        media_type: media_type(path),
        context: None,
    };
    let temporary = std::env::temp_dir().canonicalize().ok()?;
    for (provider, offer) in registry.ranked(Capability::ExtractText) {
        if !offer.reads(&request.media_type) || offer.media_types.is_empty() {
            continue;
        }
        let Ok(extraction) = provider.extract(&offer.id, &request) else {
            continue;
        };
        // Pictures are the provider's files only when it wrote them in the temporary folder.
        let mut pictures = extraction
            .pictures
            .iter()
            .filter_map(|p| p.path.canonicalize().ok())
            .filter(|p| p.starts_with(&temporary) && p.is_file());
        let first = pictures.next();
        for other in pictures {
            let _ = std::fs::remove_file(other);
        }
        if first.is_some() {
            return first;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{Extraction, Offer, Picture, Provider, ProviderError, priority};

    /// A provider that reads STEP files and draws one picture of each.
    struct Drawer {
        offers: Vec<Offer>,
    }

    impl Drawer {
        fn new() -> Self {
            Self {
                offers: vec![Offer {
                    capability: Capability::ExtractText,
                    id: "draw".into(),
                    label: None,
                    priority: priority::BUILT_IN,
                    available: true,
                    reason: None,
                    media_types: vec!["model/step".into()],
                    extra: serde_json::Map::new(),
                }],
            }
        }
    }

    impl Provider for Drawer {
        fn name(&self) -> &'static str {
            "drawer"
        }

        fn offers(&self) -> &[Offer] {
            &self.offers
        }

        fn extract(
            &self,
            _offer: &str,
            request: &ExtractionRequest,
        ) -> Result<Extraction, ProviderError> {
            let picture = tempfile::Builder::new()
                .suffix(".png")
                .tempfile()
                .unwrap()
                .into_temp_path()
                .keep()
                .unwrap();
            std::fs::write(&picture, request.path.to_string_lossy().as_bytes()).unwrap();
            Ok(Extraction {
                text: String::new(),
                method: "geometry".into(),
                pages: None,
                created: None,
                located: false,
                pictures: vec![Picture {
                    path: picture,
                    context: "four views".into(),
                }],
            })
        }
    }

    #[test]
    fn keys_change_with_the_file_and_its_place() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "one").unwrap();
        let first = key(dir.path(), "a.txt").unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(key(dir.path(), "a.txt").unwrap(), first);
        std::fs::write(dir.path().join("a.txt"), "two!").unwrap();
        assert_ne!(key(dir.path(), "a.txt").unwrap(), first);
        std::fs::rename(dir.path().join("a.txt"), dir.path().join("b.txt")).unwrap();
        assert_ne!(key(dir.path(), "b.txt"), key(dir.path(), "a.txt"));
        assert_eq!(key(dir.path(), "a.txt"), None);
    }

    #[test]
    fn drawings_are_drawn_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("bracket.step");
        std::fs::write(&part, "ISO-10303-21;").unwrap();
        assert!(is_drawn(&part));
        assert!(!is_drawn(Path::new("notes.txt")));

        let cache = PreviewCache::at(&dir.path().join("previews"));
        assert_eq!(cache.draw(&Registry::with_built_ins(), &part, "k"), None);

        let mut registry = Registry::with_built_ins();
        registry.add(Box::new(Drawer::new()));
        let kept = cache.draw(&registry, &part, "k").unwrap();
        assert_eq!(kept, dir.path().join("previews/drawings/k.png"));
        assert_eq!(
            std::fs::read_to_string(&kept).unwrap(),
            part.to_string_lossy()
        );
        // Kept: the providers aren't asked again.
        std::fs::write(&kept, "kept").unwrap();
        assert_eq!(cache.draw(&registry, &part, "k").unwrap(), kept);
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), "kept");
        assert_eq!(
            cache.thumbnail("k"),
            dir.path().join("previews/thumbnails/k.png")
        );
    }
}
