//! The file types this device couldn't read (`docs/design/models-3d.md`): one entry per media
//! type, extension and reason, counted across ingests, so it's clear which reader to build or
//! install next. It's kept per device, in Carrel's own folder, since what's readable depends on
//! what's installed. It never records file names or paths, so it can be shared.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::card::{Extra, Kind};
use crate::ingest::run::Run;
use crate::ingest::text::Unread;

/// One type of file that couldn't be read, for one reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Its media type, such as `model/stl`.
    pub media_type: String,
    /// Its extension, lowercased and without the dot; empty if it has none.
    pub extension: String,
    /// Why it couldn't be read.
    pub why: Unread,
    /// How many items.
    pub count: u64,
    /// The day it was first seen, `YYYY-MM-DD`.
    pub first_seen: String,
    /// The day it was last seen.
    pub last_seen: String,
}

/// This device's log of unreadable types: `unreadable.json` in Carrel's folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unreadable {
    /// The entries, most frequent first.
    #[serde(default)]
    pub entries: Vec<Entry>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Unreadable {
    /// Where it's kept on this device.
    #[must_use]
    pub fn path() -> Option<PathBuf> {
        crate::local::config_dir().map(|d| d.join("unreadable.json"))
    }

    /// The saved log, or an empty one.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Saves it.
    ///
    /// # Errors
    ///
    /// An error writing the file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, text + "\n")
    }

    /// Counts one item of `media_type` and `extension` that couldn't be read for `why`, on
    /// `today`. Whether the type and reason are new to the log.
    pub fn note(&mut self, media_type: &str, extension: &str, why: Unread, today: &str) -> bool {
        let found = self
            .entries
            .iter_mut()
            .find(|e| e.media_type == media_type && e.extension == extension && e.why == why);
        let new = if let Some(entry) = found {
            entry.count += 1;
            today.clone_into(&mut entry.last_seen);
            false
        } else {
            self.entries.push(Entry {
                media_type: media_type.to_owned(),
                extension: extension.to_owned(),
                why,
                count: 1,
                first_seen: today.to_owned(),
                last_seen: today.to_owned(),
            });
            true
        };
        self.entries
            .sort_by(|a, b| b.count.cmp(&a.count).then(b.last_seen.cmp(&a.last_seen)));
        new
    }

    /// Counts every file in `run` whose text couldn't be read. The types and reasons new to
    /// the log, as `(extension, why)`.
    pub fn record(&mut self, run: &Run, today: &str) -> Vec<(String, Unread)> {
        let mut new = Vec::new();
        for (candidate, text) in run.candidates.iter().zip(&run.texts) {
            let Some(text) = text else { continue };
            let (Some(why), Some(media_type)) = (text.unread, &text.media_type) else {
                continue;
            };
            if candidate.kind != Kind::File {
                continue;
            }
            let extension = extension(&candidate.path);
            if self.note(media_type, &extension, why, today) {
                new.push((extension, why));
            }
        }
        new
    }
}

/// `path`'s extension, lowercased; empty if it has none.
#[must_use]
pub fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// What would read `media_type`, when Carrel knows.
#[must_use]
pub fn hint(media_type: &str) -> Option<&'static str> {
    if media_type.starts_with("image/") || media_type == "application/pdf" {
        return Some(if cfg!(target_os = "macos") {
            "the Apple provider (carrel-provider-apple) reads it"
        } else {
            "install Tesseract for OCR"
        });
    }
    if MODELS.contains(&media_type) && cfg!(target_os = "macos") {
        return Some("the Apple provider (carrel-provider-apple) reads it");
    }
    None
}

/// The 3D model types the Apple provider reads, through `ModelIO`.
const MODELS: [&str; 5] = [
    "model/stl",
    "model/obj",
    "model/ply",
    "model/vnd.usdz+zip",
    "model/vnd.usd",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_are_counted_by_type_and_reason_most_frequent_first() {
        let mut log = Unreadable::default();
        assert!(log.note("model/stl", "stl", Unread::NoReader, "2026-10-30"));
        assert!(log.note("application/zip", "zip", Unread::NoReader, "2026-10-30"));
        assert!(!log.note("application/zip", "zip", Unread::NoReader, "2026-10-31"));
        assert!(log.note("application/zip", "zip", Unread::Failed, "2026-10-31"));
        let first = &log.entries[0];
        assert_eq!((first.extension.as_str(), first.count), ("zip", 2));
        assert_eq!(first.first_seen, "2026-10-30");
        assert_eq!(first.last_seen, "2026-10-31");
        assert_eq!(log.entries.len(), 3);
    }

    #[test]
    fn the_log_is_saved_and_loaded_without_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep/unreadable.json");
        let mut log = Unreadable::default();
        log.note("model/step", "step", Unread::NoReader, "2026-10-30");
        log.save(&path).unwrap();
        assert_eq!(Unreadable::load(&path), log);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"why\": \"no-reader\""), "{text}");
        assert_eq!(
            Unreadable::load(&dir.path().join("missing.json")),
            Unreadable::default()
        );
    }

    #[test]
    fn models_are_read_by_the_apple_provider_on_a_mac() {
        assert_eq!(
            hint("model/stl").is_some(),
            cfg!(target_os = "macos"),
            "{:?}",
            hint("model/stl")
        );
        assert_eq!(hint("application/x-unknown"), None);
    }

    #[test]
    fn extensions_are_lowercased() {
        assert_eq!(extension(Path::new("/a/Part.STEP")), "step");
        assert_eq!(extension(Path::new("/a/Makefile")), "");
    }
}
