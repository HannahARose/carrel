//! Checksums and manifests (FORMAT.md §2.6, §7.7).
//!
//! A manifest lists every file in a folder or bundle with its checksum, size and modification
//! time. Building one again reuses checksums for files whose size and modification time are
//! unchanged, so large items are only re-read where something changed. Comparing two manifests
//! gives the files added, removed, changed and renamed.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use crate::card::{Checksum, DateOrTime, Id};
use crate::detect::{Rules, is_ignored};

/// The first line every manifest starts with, before its fields.
const HEADER: &str = "# carrel manifest 1.0";

impl Checksum {
    /// The SHA-256 checksum of `bytes`.
    #[must_use]
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self::from_digest(Sha256::digest(bytes).as_slice())
    }

    /// The SHA-256 checksum of a file's contents, read in chunks.
    ///
    /// # Errors
    ///
    /// Any error opening or reading the file.
    pub fn of_file(path: &Path) -> std::io::Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0; 256 * 1024];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        Ok(Self::from_digest(hasher.finalize().as_slice()))
    }

    fn from_digest(digest: &[u8]) -> Self {
        let mut text = String::with_capacity(7 + digest.len() * 2);
        text.push_str("sha256:");
        for byte in digest {
            let _ = write!(text, "{byte:02x}");
        }
        Self(text)
    }
}

/// One file in a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Path relative to the item's folder, with `/` separators.
    pub path: String,
    /// The file's checksum. For a symbolic link, the checksum of its target path.
    pub checksum: Checksum,
    /// Size in bytes. For a symbolic link, the length of its target path.
    pub size: u64,
    /// When the file was last modified.
    pub modified: DateOrTime,
}

/// A folder's or bundle's manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The item the manifest describes.
    pub item: Id,
    /// Its files, sorted by path (byte order).
    pub entries: Vec<Entry>,
}

/// Why a manifest couldn't be built or read.
#[derive(Debug)]
pub enum ManifestError {
    /// Reading a file or folder failed.
    Io {
        /// The path concerned.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// A file name isn't valid UTF-8, so it can't be written in a manifest.
    NonUtf8Name(PathBuf),
    /// A manifest file is malformed.
    Malformed {
        /// The line number, starting at 1.
        line: usize,
        /// What is wrong.
        message: String,
    },
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::NonUtf8Name(p) => write!(f, "{}: file name isn't valid UTF-8", p.display()),
            Self::Malformed { line, message } => write!(f, "manifest line {line}: {message}"),
        }
    }
}

impl std::error::Error for ManifestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ManifestError + '_ {
    move |source| ManifestError::Io {
        path: path.to_owned(),
        source,
    }
}

/// A modification time with the file system's full precision (nanoseconds on APFS), so two
/// edits within the same second still look different when deciding whether to re-read a file.
fn timestamp(time: SystemTime) -> DateOrTime {
    let ts = jiff::Timestamp::try_from(time).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
    let zoned = ts.to_zoned(jiff::tz::TimeZone::system());
    DateOrTime(zoned.strftime("%Y-%m-%dT%H:%M:%S%.f%:z").to_string())
}

impl Manifest {
    /// Builds the manifest of the folder at `root`.
    ///
    /// Every file is listed except ignored files (FORMAT.md §2.8) and the contents of
    /// regenerable folders (§4.3), which change too often to track and can be rebuilt.
    /// Symbolic links are recorded, never followed. When `previous` is given, files whose size
    /// and modification time match its entry keep that entry's checksum instead of being read
    /// again.
    ///
    /// # Errors
    ///
    /// [`ManifestError::Io`] if a file can't be read, and [`ManifestError::NonUtf8Name`] for a
    /// name that isn't UTF-8.
    pub fn build(
        root: &Path,
        item: Id,
        rules: &Rules,
        previous: Option<&Manifest>,
    ) -> Result<Self, ManifestError> {
        let known: HashMap<&str, &Entry> = previous
            .map(|m| m.entries.iter().map(|e| (e.path.as_str(), e)).collect())
            .unwrap_or_default();
        let mut entries = Vec::new();
        let mut stack = vec![(root.to_path_buf(), String::new())];
        while let Some((dir, prefix)) = stack.pop() {
            for dir_entry in std::fs::read_dir(&dir).map_err(io(&dir))? {
                let dir_entry = dir_entry.map_err(io(&dir))?;
                let path = dir_entry.path();
                let name = dir_entry
                    .file_name()
                    .into_string()
                    .map_err(|_| ManifestError::NonUtf8Name(path.clone()))?;
                if is_ignored(&name) {
                    continue;
                }
                let relative = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                let meta = std::fs::symlink_metadata(&path).map_err(io(&path))?;
                if meta.is_dir() {
                    if !rules.is_regenerable(&name) {
                        stack.push((path, relative));
                    }
                    continue;
                }
                let modified = timestamp(meta.modified().map_err(io(&path))?);
                let entry = if meta.file_type().is_symlink() {
                    let target = std::fs::read_link(&path).map_err(io(&path))?;
                    let target = target.to_string_lossy();
                    Entry {
                        path: relative,
                        checksum: Checksum::of_bytes(target.as_bytes()),
                        size: target.len() as u64,
                        modified,
                    }
                } else {
                    let size = meta.len();
                    let checksum = match known.get(relative.as_str()) {
                        Some(old) if old.size == size && old.modified == modified => {
                            old.checksum.clone()
                        }
                        _ => Checksum::of_file(&path).map_err(io(&path))?,
                    };
                    Entry {
                        path: relative,
                        checksum,
                        size,
                        modified,
                    }
                };
                entries.push(entry);
            }
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Self { item, entries })
    }

    /// The total size of every file in the manifest.
    #[must_use]
    pub fn total_size(&self) -> u64 {
        self.entries.iter().map(|e| e.size).sum()
    }

    /// The manifest as written to `_System/manifests/<item-id>.tsv`.
    #[must_use]
    pub fn to_tsv(&self) -> String {
        let mut out = format!("{HEADER}\tid={}\n", self.item);
        for e in &self.entries {
            let _ = writeln!(
                out,
                "{}\t{}\t{}\t{}",
                encode_path(&e.path),
                e.checksum.0,
                e.size,
                e.modified.0
            );
        }
        out
    }

    /// The content checksum recorded in a card's `manifest.checksum` (FORMAT.md §7.7): the
    /// checksum of every row's encoded path, checksum and size, without modification times or
    /// the header. Two manifests of the same contents always have the same content checksum.
    #[must_use]
    pub fn checksum(&self) -> Checksum {
        let mut text = String::with_capacity(self.entries.len() * 100);
        for e in &self.entries {
            let _ = writeln!(
                text,
                "{}\t{}\t{}",
                encode_path(&e.path),
                e.checksum.0,
                e.size
            );
        }
        Checksum::of_bytes(text.as_bytes())
    }

    /// Reads a manifest file.
    ///
    /// # Errors
    ///
    /// [`ManifestError::Malformed`] naming the first bad line.
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let bad = |line: usize, message: &str| ManifestError::Malformed {
            line,
            message: message.into(),
        };
        let mut lines = text.lines().enumerate();
        let (_, header) = lines.next().ok_or_else(|| bad(1, "empty manifest"))?;
        let mut fields = header.split('\t');
        if fields.next() != Some(HEADER) {
            return Err(bad(1, "doesn't start with `# carrel manifest 1.0`"));
        }
        let item = fields
            .find_map(|f| f.strip_prefix("id="))
            .and_then(|id| uuid::Uuid::parse_str(id).ok())
            .map(Id)
            .ok_or_else(|| bad(1, "missing or malformed id"))?;

        let mut entries = Vec::new();
        for (i, line) in lines {
            if line.is_empty() {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            let [path, checksum, size, modified] = cols.as_slice() else {
                return Err(bad(i + 1, "expected four tab-separated columns"));
            };
            let checksum = Checksum((*checksum).to_owned());
            if !checksum.is_valid() {
                return Err(bad(i + 1, "malformed checksum"));
            }
            entries.push(Entry {
                path: decode_path(path)
                    .ok_or_else(|| bad(i + 1, "malformed percent-encoding in path"))?,
                checksum,
                size: size.parse().map_err(|_| bad(i + 1, "malformed size"))?,
                modified: DateOrTime((*modified).to_owned()),
            });
        }
        if entries.windows(2).any(|w| w[0].path >= w[1].path) {
            return Err(bad(1, "entries aren't sorted by path, or a path repeats"));
        }
        Ok(Self { item, entries })
    }

    /// The differences from `self` (older) to `newer`. Modification times are ignored: only
    /// paths and contents count.
    #[must_use]
    pub fn diff(&self, newer: &Manifest) -> Diff {
        let old: BTreeMap<&str, &Entry> =
            self.entries.iter().map(|e| (e.path.as_str(), e)).collect();
        let new: BTreeMap<&str, &Entry> =
            newer.entries.iter().map(|e| (e.path.as_str(), e)).collect();
        let mut diff = Diff::default();
        let mut removed: Vec<&Entry> = Vec::new();
        let mut added: Vec<&Entry> = Vec::new();
        for (path, entry) in &old {
            match new.get(path) {
                Some(n) if n.checksum != entry.checksum => diff.changed.push((*path).to_owned()),
                Some(_) => {}
                None => removed.push(entry),
            }
        }
        for (path, entry) in &new {
            if !old.contains_key(path) {
                added.push(entry);
            }
        }
        // A removed file whose exact contents reappear under a new path was renamed. Pair them
        // in path order so the result is deterministic when several files share contents.
        let mut unclaimed: BTreeMap<&Checksum, Vec<&Entry>> = BTreeMap::new();
        for entry in &added {
            unclaimed.entry(&entry.checksum).or_default().push(entry);
        }
        for list in unclaimed.values_mut() {
            list.reverse();
        }
        for entry in removed {
            if let Some(target) = unclaimed.get_mut(&entry.checksum).and_then(Vec::pop) {
                diff.renamed.push((entry.path.clone(), target.path.clone()));
            } else {
                diff.removed.push(entry.path.clone());
            }
        }
        let renamed_to: std::collections::HashSet<&str> =
            diff.renamed.iter().map(|(_, to)| to.as_str()).collect();
        diff.added = added
            .into_iter()
            .filter(|e| !renamed_to.contains(e.path.as_str()))
            .map(|e| e.path.clone())
            .collect();
        diff
    }
}

/// What changed between two manifests. Every list is sorted by path.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Diff {
    /// Files that are new.
    pub added: Vec<String>,
    /// Files that are gone.
    pub removed: Vec<String>,
    /// Files at the same path whose contents changed.
    pub changed: Vec<String>,
    /// Files whose contents moved to a new path, as `(from, to)`.
    pub renamed: Vec<(String, String)>,
}

impl Diff {
    /// Whether nothing changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.changed.is_empty()
            && self.renamed.is_empty()
    }
}

/// Percent-encodes `%`, tab, CR and LF so every path fits on one tab-separated line.
#[must_use]
pub fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        match ch {
            '%' => out.push_str("%25"),
            '\t' => out.push_str("%09"),
            '\n' => out.push_str("%0A"),
            '\r' => out.push_str("%0D"),
            _ => out.push(ch),
        }
    }
    out
}

/// Reverses [`encode_path`]. Returns `None` for a `%` not followed by two hex digits.
#[must_use]
pub fn decode_path(encoded: &str) -> Option<String> {
    let bytes = encoded.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = encoded.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_match_known_values() {
        assert_eq!(
            Checksum::of_bytes(b"").0,
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            Checksum::of_bytes(b"abc").0,
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn path_encoding_round_trips() {
        for p in [
            "plain/name.txt",
            "100% done.md",
            "tab\there",
            "line\nbreak",
            "cr\r",
            "%09 literal",
        ] {
            let e = encode_path(p);
            assert!(!e.contains(['\t', '\n', '\r']), "{e:?}");
            assert_eq!(decode_path(&e).as_deref(), Some(p));
        }
        assert_eq!(decode_path("bad%zz"), None);
        assert_eq!(decode_path("short%4"), None);
    }
}
