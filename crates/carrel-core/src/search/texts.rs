//! The text cache: each item's text as drafting read it, so search never reads a file twice
//! or runs text recognition again (`docs/design/search.md`).
//!
//! Texts are kept in `_System/cache.nosync/text/`, one file per contents checksum, so an item
//! whose contents change is read again and an item that moves keeps its text. The cache is
//! regenerable and device-specific (FORMAT.md §7.12): it's left out of history, backups and
//! sync. Restricted items' text is never kept, so it can't be found, or read back, from here.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::archive::Archive;
use crate::card::{Card, Checksum, Kind, Sensitivity};
use crate::card_file::CardFile;
use crate::ingest::Candidate;
use crate::ingest::text::{Unread, read};
use crate::providers::Registry;

/// The cache's folder, relative to the archive root.
pub const TEXTS: &str = "_System/cache.nosync/text";

/// The most of an item's text that's kept, in bytes. Search finds a long document by what's
/// near its start, as the embedding index does by its first passages.
pub const MAX_TEXT: usize = 256 * 1024;

/// How long a file nothing on this device could read waits before it's tried again, as when
/// a provider that reads it has been added since.
pub const RETRY_UNREAD: std::time::Duration = std::time::Duration::from_hours(7 * 24);

/// The text cache of one archive on this device.
#[derive(Debug, Clone)]
pub struct TextCache {
    dir: PathBuf,
}

impl TextCache {
    /// The cache of `archive`, which may not exist yet.
    #[must_use]
    pub fn of(archive: &Archive) -> Self {
        Self::at(&archive.root().join(TEXTS))
    }

    /// A cache in `dir`.
    #[must_use]
    pub fn at(dir: &Path) -> Self {
        Self {
            dir: dir.to_owned(),
        }
    }

    fn path(&self, checksum: &Checksum) -> Option<PathBuf> {
        let hex = checksum.0.strip_prefix("sha256:")?;
        checksum
            .is_valid()
            .then(|| self.dir.join(format!("{hex}.txt")))
    }

    /// The text kept for the contents with `checksum`, if any.
    #[must_use]
    pub fn get(&self, checksum: &Checksum) -> Option<String> {
        std::fs::read_to_string(self.path(checksum)?).ok()
    }

    /// Whether there's text kept for `checksum`, even if it's empty.
    #[must_use]
    pub fn has(&self, checksum: &Checksum) -> bool {
        self.path(checksum).is_some_and(|p| p.is_file())
    }

    /// Keeps `text` for the contents with `checksum`, cut to [`MAX_TEXT`]. An invalid
    /// checksum keeps nothing.
    ///
    /// # Errors
    ///
    /// An error creating the folder or writing the file.
    pub fn put(&self, checksum: &Checksum, text: &str) -> std::io::Result<()> {
        let Some(path) = self.path(checksum) else {
            return Ok(());
        };
        std::fs::create_dir_all(&self.dir)?;
        let mut end = text.len().min(MAX_TEXT);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        // Written beside it and renamed, so a reader never sees half a text.
        let partial = path.with_extension(format!("{}.partial", std::process::id()));
        std::fs::write(&partial, &text[..end])?;
        std::fs::rename(&partial, &path)?;
        let _ = std::fs::remove_file(path.with_extension("unread"));
        Ok(())
    }

    /// Forgets the text for `checksum`, if any.
    ///
    /// # Errors
    ///
    /// An error removing the file, other than its not being there.
    pub fn remove(&self, checksum: &Checksum) -> std::io::Result<()> {
        let Some(path) = self.path(checksum) else {
            return Ok(());
        };
        for path in [path.with_extension("unread"), path] {
            match std::fs::remove_file(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        Ok(())
    }

    fn marker(&self, checksum: &Checksum) -> Option<PathBuf> {
        self.path(checksum).map(|p| p.with_extension("unread"))
    }

    /// Notes that nothing on this device read the contents with `checksum`, so they aren't
    /// tried again until [`RETRY_UNREAD`] has passed.
    ///
    /// # Errors
    ///
    /// An error creating the folder or writing the note.
    pub fn mark_unread(&self, checksum: &Checksum) -> std::io::Result<()> {
        let Some(path) = self.marker(checksum) else {
            return Ok(());
        };
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(path, "")
    }

    /// Whether nothing on this device read the contents with `checksum` lately.
    #[must_use]
    pub fn unread_lately(&self, checksum: &Checksum) -> bool {
        self.marker(checksum)
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < RETRY_UNREAD)
    }

    /// The checksums of every kept text.
    fn kept(&self) -> Vec<Checksum> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        entries
            .filter_map(|e| {
                let name = e.ok()?.file_name().to_string_lossy().into_owned();
                let hex = name
                    .strip_suffix(".txt")
                    .or_else(|| name.strip_suffix(".unread"))?;
                Some(Checksum(format!("sha256:{hex}")))
            })
            .filter(Checksum::is_valid)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

/// The checksum that names an item's contents: a file's checksum, or a folder's or bundle's
/// manifest content checksum.
#[must_use]
pub fn fingerprint(card: &Card) -> Option<&Checksum> {
    card.checksum
        .as_ref()
        .or_else(|| card.manifest.as_ref().map(|m| &m.checksum))
}

/// An item whose text isn't in the cache yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unindexed {
    /// The item's contents, relative to the archive root.
    pub item: String,
    /// Its kind.
    pub kind: Kind,
    /// Its contents checksum, naming the text in the cache.
    pub checksum: Checksum,
}

/// What [`fill`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filled {
    /// Items whose text was read and kept.
    pub read: usize,
    /// Items nothing on this device reads, tried again after [`RETRY_UNREAD`].
    pub unread: usize,
}

/// Brings the cache up to date with `archive`'s cards: forgets what restricted items and
/// replaced contents left behind, and lists the items whose text is missing, for [`fill`].
/// Returns them, and how many texts were forgotten.
///
/// # Errors
///
/// An error listing the archive.
pub fn tidy(archive: &Archive) -> Result<(Vec<Unindexed>, usize), crate::archive::ArchiveError> {
    let cache = TextCache::of(archive);
    let mut wanted = Vec::new();
    let mut named = HashSet::new();
    let mut removed = 0;
    for item in archive.items()? {
        let Some(path) = &item.item else { continue };
        let Ok(text) = std::fs::read_to_string(archive.root().join(&item.card)) else {
            continue;
        };
        let Ok(file) = CardFile::parse(&text) else {
            continue;
        };
        let card = file.card();
        let Some(checksum) = fingerprint(card) else {
            continue;
        };
        if card.sensitivity == Sensitivity::Restricted {
            if cache.has(checksum) {
                removed += 1;
            }
            let _ = cache.remove(checksum);
            continue;
        }
        named.insert(checksum.clone());
        if !cache.has(checksum) && !cache.unread_lately(checksum) && !card.kind.is_card_only() {
            wanted.push(Unindexed {
                item: path.clone(),
                kind: card.kind,
                checksum: checksum.clone(),
            });
        }
    }
    for kept in cache.kept() {
        if !named.contains(&kept) && cache.remove(&kept).is_ok() {
            removed += 1;
        }
    }
    Ok((wanted, removed))
}

/// Reads the text of each item in `wanted`, as [`tidy`] lists them, and keeps it, calling
/// `progress` with how many are done, how many there are, and the item being read, if any. A
/// file nothing on this device reads isn't kept, but tried again after [`RETRY_UNREAD`], so
/// it's read once a provider can; one that's empty, or whose reader failed, is kept empty.
///
/// # Errors
///
/// An error reading the archive's rules.
pub fn fill(
    archive: &Archive,
    registry: &Registry,
    wanted: Vec<Unindexed>,
    progress: &mut dyn FnMut(usize, usize, Option<&str>),
) -> Result<Filled, crate::archive::ArchiveError> {
    let mut filled = Filled::default();
    if wanted.is_empty() {
        return Ok(filled);
    }
    let rules = archive
        .rules()
        .map_err(|e| crate::archive::ArchiveError::Invalid {
            path: e.path,
            message: e.message,
        })?;
    let cache = TextCache::of(archive);
    let total = wanted.len();
    for (done, item) in wanted.into_iter().enumerate() {
        progress(done, total, Some(&item.item));
        let path = archive.root().join(&item.item);
        if !path.exists() {
            continue;
        }
        let candidate = Candidate {
            context: None,
            inside: (item.kind == Kind::File)
                .then(|| crate::compressed::inspect(&path))
                .flatten(),
            path,
            kind: item.kind,
            sealed: None,
            markers: Vec::new(),
            checksum: item.checksum.clone(),
            size: 0,
            duplicate_of: None,
            likely: Vec::new(),
            same_as: None,
        };
        let text = read(registry, &rules, &candidate);
        if text.unread == Some(Unread::NoReader) {
            filled.unread += 1;
            let _ = cache.mark_unread(&item.checksum);
        } else if cache.put(&item.checksum, &text.text).is_ok() {
            filled.read += 1;
        }
    }
    progress(total, total, None);
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checksum(c: char) -> Checksum {
        Checksum(format!("sha256:{}", c.to_string().repeat(64)))
    }

    #[test]
    fn texts_are_kept_by_checksum_and_cut_to_size() {
        let dir = tempfile::tempdir().unwrap();
        let cache = TextCache::at(dir.path());
        assert_eq!(cache.get(&checksum('a')), None);
        cache.put(&checksum('a'), "invoice 2057-4038").unwrap();
        assert_eq!(cache.get(&checksum('a')).unwrap(), "invoice 2057-4038");
        assert!(cache.has(&checksum('a')));
        assert_eq!(cache.kept(), [checksum('a')]);

        let long = "é".repeat(MAX_TEXT);
        cache.put(&checksum('b'), &long).unwrap();
        let kept = cache.get(&checksum('b')).unwrap();
        assert!(kept.len() <= MAX_TEXT && kept.len() > MAX_TEXT - 2);

        cache.remove(&checksum('a')).unwrap();
        cache.remove(&checksum('a')).unwrap();
        assert!(!cache.has(&checksum('a')));

        assert!(!cache.unread_lately(&checksum('c')));
        cache.mark_unread(&checksum('c')).unwrap();
        assert!(cache.unread_lately(&checksum('c')));
        assert!(!cache.has(&checksum('c')));
        assert!(cache.kept().contains(&checksum('c')));
        cache.remove(&checksum('c')).unwrap();
        assert!(!cache.unread_lately(&checksum('c')));

        let bad = Checksum("sha256:../../etc".into());
        cache.put(&bad, "no").unwrap();
        assert_eq!(cache.get(&bad), None);
    }
}
