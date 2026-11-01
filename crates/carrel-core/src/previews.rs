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

pub mod photos;
pub mod png;

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

    /// The thumbnail of the PNG, JPEG or TIFF at `path`, whose key is `key`: the kept one, or
    /// the picture shrunk to at most [`THUMBNAIL`] pixels a side now and kept. `None` when it
    /// can't be read or kept.
    #[must_use]
    pub fn shrink(&self, path: &Path, key: &str) -> Option<PathBuf> {
        let kept = self.thumbnail(key);
        if kept.is_file() {
            return Some(kept);
        }
        let bytes = std::fs::read(path).ok()?;
        let pixels = png::decode(&bytes).or_else(|| photos::decode_jpeg_or_tiff(&bytes))?;
        let small = pixels.fit(THUMBNAIL, THUMBNAIL);
        std::fs::create_dir_all(kept.parent()?).ok()?;
        // Written beside it and renamed, so a half-written thumbnail is never read.
        let partial = kept.with_extension("partial");
        std::fs::write(&partial, small.encode()).ok()?;
        std::fs::rename(&partial, &kept).ok()?;
        Some(kept)
    }
}

/// The most entries a folder's preview lists.
pub const LISTED: usize = 200;

/// What to show as an item's preview, from [`preview`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preview {
    /// A picture: a PNG when `png` is true, otherwise an image file in another format, which
    /// only a viewer that reads that format can show.
    Picture {
        /// The file to show.
        path: PathBuf,
        /// Whether it's a PNG.
        png: bool,
    },
    /// A folder or bundle's contents, as lines: folders end in `/`, and Carrel's own files
    /// and hidden ones are left out.
    Listing(Vec<String>),
    /// Nothing: a restricted item, a card without a file, or a file this device has no
    /// picture of.
    Nothing,
}

/// What makes pictures for [`preview`] that aren't cached yet.
#[derive(Clone, Copy)]
pub struct Makers<'a> {
    /// Draws STEP, IGES and 3MF files; `None` leaves them undrawn.
    pub registry: Option<&'a Registry>,
    /// Whether a PNG, JPEG or TIFF item's own thumbnail may be made and kept.
    pub thumbnails: bool,
}

/// The preview of the item at `relative` in the archive at `root`, its picture made and kept
/// in `cache` when it's an image or a drawing: a cached thumbnail, such as the Mac app makes
/// with Quick Look; a drawing of a 3D model; a PNG, JPEG or TIFF's own thumbnail, shrunk
/// here; another image file as it is; or a folder's listing. Nothing at all for a `restricted` item.
///
/// Slow for a large image or a model: run it in the background.
#[must_use]
pub fn preview(
    root: &Path,
    relative: &str,
    restricted: bool,
    cache: &PreviewCache,
    makers: Makers<'_>,
) -> Preview {
    let path = root.join(relative);
    if restricted || relative.is_empty() {
        return Preview::Nothing;
    }
    if path.is_dir() {
        return Preview::Listing(listing(&path));
    }
    // A file iCloud evicted to a `.icloud` placeholder has no key until it's downloaded.
    let Some(key) = key(root, relative).or_else(|| {
        crate::cloud_files::ensure_downloaded(&path).ok()?;
        key(root, relative)
    }) else {
        return Preview::Nothing;
    };
    let kept = cache.thumbnail(&key);
    if kept.is_file() {
        return Preview::Picture {
            path: kept,
            png: true,
        };
    }
    if crate::cloud_files::ensure_downloaded(&path).is_err() {
        return Preview::Nothing;
    }
    if is_drawn(&path) {
        return makers
            .registry
            .and_then(|registry| cache.draw(registry, &path, &key))
            .map_or(Preview::Nothing, |path| Preview::Picture {
                path,
                png: true,
            });
    }
    match media_type(&path).as_str() {
        "image/png" => {
            let shrunk = makers
                .thumbnails
                .then(|| cache.shrink(&path, &key))
                .flatten();
            Preview::Picture {
                path: shrunk.unwrap_or(path),
                png: true,
            }
        }
        // Shown as they are when no thumbnail can be made, to a viewer that reads them.
        "image/jpeg" | "image/tiff" => makers
            .thumbnails
            .then(|| cache.shrink(&path, &key))
            .flatten()
            .map_or(Preview::Picture { path, png: false }, |path| {
                Preview::Picture { path, png: true }
            }),
        image if image.starts_with("image/") => Preview::Picture { path, png: false },
        _ => Preview::Nothing,
    }
}

/// The entries of the folder at `path`, sorted, folders marked with `/`, at most [`LISTED`]
/// and a line saying how many more.
fn listing(path: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') || name.starts_with('_') {
                        return None;
                    }
                    let folder = entry.file_type().is_ok_and(|t| t.is_dir());
                    Some(if folder { format!("{name}/") } else { name })
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort_by_key(|n| n.to_lowercase());
    if names.is_empty() {
        return vec!["(empty)".to_owned()];
    }
    if names.len() > LISTED {
        let more = names.len() - LISTED;
        names.truncate(LISTED);
        names.push(format!("… and {more} more"));
    }
    names
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
    fn previews_show_pictures_listings_and_nothing_for_restricted_items() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let cache = PreviewCache::at(&root.join(PREVIEWS));
        let makers = Makers {
            registry: None,
            thumbnails: true,
        };
        let big = png::Pixels {
            width: 1024,
            height: 256,
            rgba: vec![200; 1024 * 256 * 4],
        };
        std::fs::write(root.join("big.png"), big.encode()).unwrap();
        std::fs::write(root.join("photo.jpg"), b"\xff\xd8\xff\xe0 jpeg").unwrap();
        std::fs::write(root.join("notes.txt"), "words").unwrap();
        std::fs::create_dir_all(root.join("folder/inner")).unwrap();
        std::fs::write(root.join("folder/b.txt"), "").unwrap();
        std::fs::write(root.join("folder/.hidden"), "").unwrap();

        let Preview::Picture { path, png: true } = preview(root, "big.png", false, &cache, makers)
        else {
            panic!("no picture");
        };
        assert_eq!(path, cache.thumbnail(&key(root, "big.png").unwrap()));
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(png::size(&bytes), Some((THUMBNAIL, THUMBNAIL / 4)));
        // Restricted: nothing, though a thumbnail is kept.
        assert_eq!(
            preview(root, "big.png", true, &cache, makers),
            Preview::Nothing
        );
        assert_eq!(
            preview(root, "photo.jpg", false, &cache, makers),
            Preview::Picture {
                path: root.join("photo.jpg"),
                png: false
            }
        );
        assert_eq!(
            preview(root, "notes.txt", false, &cache, makers),
            Preview::Nothing
        );
        assert_eq!(
            preview(root, "gone.png", false, &cache, makers),
            Preview::Nothing
        );
        assert_eq!(
            preview(root, "folder", false, &cache, makers),
            Preview::Listing(vec!["b.txt".into(), "inner/".into()])
        );
        // A thumbnail made elsewhere, such as by the Mac app, is used for any file.
        let notes = key(root, "notes.txt").unwrap();
        std::fs::write(cache.thumbnail(&notes), b"png").unwrap();
        assert_eq!(
            preview(root, "notes.txt", false, &cache, makers),
            Preview::Picture {
                path: cache.thumbnail(&notes),
                png: true
            }
        );
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

    /// A JPEG or TIFF item gets a thumbnail made here, as a PNG does, with no thumbnail made
    /// elsewhere first (#414).
    #[test]
    fn jpeg_and_tiff_items_get_thumbnails_made_here() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pictures");
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let cache = PreviewCache::at(&root.join(PREVIEWS));
        let makers = Makers {
            registry: None,
            thumbnails: true,
        };
        for name in ["photo.jpg", "scan.tiff"] {
            let bytes = std::fs::read(fixtures.join(name)).unwrap();
            let pixels = photos::decode_jpeg_or_tiff(&bytes).expect(name);
            assert_eq!((pixels.width, pixels.height), (96, 64), "{name}");
            // The gradient runs from no red on the left to full red on the right.
            let red = |x: usize| pixels.rgba[x * 4];
            assert!(
                red(0) < 40 && red(95) > 215,
                "{name}: {} to {}",
                red(0),
                red(95)
            );

            std::fs::copy(fixtures.join(name), root.join(name)).unwrap();
            let Preview::Picture { path, png: true } = preview(root, name, false, &cache, makers)
            else {
                panic!("{name} has no thumbnail");
            };
            assert_eq!(path, cache.thumbnail(&key(root, name).unwrap()));
            let thumbnail = png::decode(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!((thumbnail.width, thumbnail.height), (96, 64), "{name}");
            assert_eq!(
                preview(root, name, true, &cache, makers),
                Preview::Nothing,
                "{name}, restricted"
            );
        }
        // Without making thumbnails, they're shown as they are, to a viewer that reads them.
        let none = Makers {
            registry: None,
            thumbnails: false,
        };
        let other = PreviewCache::at(&root.join("other"));
        assert_eq!(
            preview(root, "photo.jpg", false, &other, none),
            Preview::Picture {
                path: root.join("photo.jpg"),
                png: false
            }
        );
        assert_eq!(photos::decode_jpeg_or_tiff(b"\x89PNG\r\n\x1a\n"), None);
    }
}
