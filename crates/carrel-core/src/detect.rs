//! Detecting what kind of item a file or folder is (FORMAT.md §4) and which folders inside an
//! item are regenerable (§4.3), using the rules in `_System/rules/` (§7.5).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::card::{Extra, Kind, Sealing};

/// Files and folders whose presence at a folder's top level makes it a hard-sealed bundle.
pub const DEFAULT_MARKERS: [&str; 9] = [
    ".git",
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "go.mod",
    "Package.swift",
    "*.xcodeproj",
    "*.xcworkspace",
    "*.kicad_pro",
];

/// Folder names treated as regenerable: kept, but left out of backups.
pub const DEFAULT_REGENERABLE: [&str; 9] = [
    "node_modules",
    ".venv",
    "venv",
    "target",
    "build",
    "dist",
    "__pycache__",
    ".ipynb_checkpoints",
    "DerivedData",
];

/// A name pattern where `*` matches any run of characters within a name (FORMAT.md §7.5).
///
/// ```
/// use carrel_core::detect::NamePattern;
///
/// let pattern = NamePattern("*.kicad_pro".into());
/// assert!(pattern.matches("Lamp.KICAD_PRO"));
/// assert!(!pattern.matches("Lamp.kicad_pro.bak"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NamePattern(pub String);

impl NamePattern {
    /// Whether `name` matches. Matching is case-insensitive, like the file systems on Apple
    /// platforms (FORMAT.md §2.5).
    #[must_use]
    pub fn matches(&self, name: &str) -> bool {
        glob(&self.0.to_lowercase(), &name.to_lowercase())
    }
}

/// Matches `name` against `pattern`, where each `*` matches any run of characters (possibly
/// none). The text before the first `*` must start the name, the text after the last must end
/// it, and the pieces in between must appear in order without overlapping.
fn glob(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(i) => rest = &rest[i + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

/// `_System/rules/markers.yaml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkerRules {
    /// The marker patterns.
    pub markers: Vec<NamePattern>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// `_System/rules/regenerable.yaml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegenerableRules {
    /// The regenerable folder-name patterns.
    pub regenerable: Vec<NamePattern>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// The detection rules an archive uses.
#[derive(Debug, Clone, PartialEq)]
pub struct Rules {
    /// Patterns that make a folder a hard-sealed bundle.
    pub markers: Vec<NamePattern>,
    /// Patterns for regenerable folder names.
    pub regenerable: Vec<NamePattern>,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            markers: DEFAULT_MARKERS
                .iter()
                .map(|p| NamePattern((*p).into()))
                .collect(),
            regenerable: DEFAULT_REGENERABLE
                .iter()
                .map(|p| NamePattern((*p).into()))
                .collect(),
        }
    }
}

/// Why rules couldn't be loaded.
#[derive(Debug)]
pub struct RulesError {
    /// The rules file.
    pub path: PathBuf,
    /// What went wrong.
    pub message: String,
}

impl std::fmt::Display for RulesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for RulesError {}

impl Rules {
    /// Loads the rules from an archive's `_System/rules/`. A missing file means that file's
    /// defaults.
    ///
    /// # Errors
    ///
    /// [`RulesError`] if a rules file exists but can't be read or parsed.
    pub fn load(archive_root: &Path) -> Result<Self, RulesError> {
        let defaults = Self::default();
        let markers = read_yaml::<MarkerRules>(&archive_root.join("_System/rules/markers.yaml"))?
            .map_or(defaults.markers, |r| r.markers);
        let regenerable =
            read_yaml::<RegenerableRules>(&archive_root.join("_System/rules/regenerable.yaml"))?
                .map_or(defaults.regenerable, |r| r.regenerable);
        Ok(Self {
            markers,
            regenerable,
        })
    }

    /// The YAML for a `markers.yaml` holding these rules' markers.
    #[must_use]
    pub fn markers_yaml(&self) -> String {
        flow_list("markers", &self.markers)
    }

    /// The YAML for a `regenerable.yaml` holding these rules' regenerable names.
    #[must_use]
    pub fn regenerable_yaml(&self) -> String {
        flow_list("regenerable", &self.regenerable)
    }

    /// Whether a folder name is regenerable.
    #[must_use]
    pub fn is_regenerable(&self, name: &str) -> bool {
        self.regenerable.iter().any(|p| p.matches(name))
    }

    /// What kind of item `path` is, and why.
    ///
    /// A file (or a symbolic link, which is never followed) is a [`Kind::File`]. A folder is a
    /// hard-sealed [`Kind::Bundle`] if any entry at its top level matches a marker, and an
    /// open [`Kind::Folder`] otherwise. Soft sealing is the owner's choice, never detected.
    ///
    /// ```
    /// use carrel_core::card::{Kind, Sealing};
    /// use carrel_core::detect::Rules;
    ///
    /// let dir = tempfile::tempdir()?;
    /// std::fs::create_dir(dir.path().join("app"))?;
    /// std::fs::write(dir.path().join("app/Cargo.toml"), "")?;
    ///
    /// let found = Rules::default().detect(&dir.path().join("app"))?;
    /// assert_eq!((found.kind, found.sealed), (Kind::Bundle, Some(Sealing::Hard)));
    /// assert_eq!(found.markers, ["Cargo.toml"]);
    /// # Ok::<(), std::io::Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Any error reading `path` or listing the folder.
    pub fn detect(&self, path: &Path) -> std::io::Result<Detected> {
        let meta = std::fs::symlink_metadata(path)?;
        if !meta.is_dir() {
            return Ok(Detected {
                kind: Kind::File,
                sealed: None,
                markers: Vec::new(),
            });
        }
        let mut markers = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            if is_ignored(&name) {
                continue;
            }
            if self.markers.iter().any(|p| p.matches(&name)) {
                markers.push(name);
            }
        }
        markers.sort();
        Ok(if markers.is_empty() {
            Detected {
                kind: Kind::Folder,
                sealed: None,
                markers,
            }
        } else {
            Detected {
                kind: Kind::Bundle,
                sealed: Some(Sealing::Hard),
                markers,
            }
        })
    }

    /// Every regenerable folder inside the item at `root`, relative to it, sorted. A
    /// regenerable folder's own contents aren't searched, and neither are `.git` folders.
    ///
    /// # Errors
    ///
    /// Any error listing a folder.
    pub fn regenerable_folders(&self, root: &Path) -> std::io::Result<Vec<PathBuf>> {
        let mut found = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                let path = entry.path();
                if self.is_regenerable(&name) {
                    found.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
                } else if name != ".git" {
                    stack.push(path);
                }
            }
        }
        found.sort();
        Ok(found)
    }
}

/// The result of [`Rules::detect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    /// [`Kind::File`], [`Kind::Folder`] or [`Kind::Bundle`].
    pub kind: Kind,
    /// `Some(Sealing::Hard)` for detected bundles.
    pub sealed: Option<Sealing>,
    /// The top-level entries that matched a marker, sorted.
    pub markers: Vec<String>,
}

/// Files programs never treat as items (FORMAT.md §2.8).
#[must_use]
pub fn is_ignored(name: &str) -> bool {
    matches!(name, ".DS_Store" | "Icon\r" | ".obsidian" | ".trash") || name.starts_with("._")
}

/// A compressed file Carrel can expand (`docs/design/compressed.md`): an archive holding many
/// files, or a compressor around one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Compressed {
    /// `.zip`.
    Zip,
    /// `.tar`.
    Tar,
    /// `.tar.gz` or `.tgz`.
    TarGz,
    /// `.tar.bz2`, `.tbz2` or `.tbz`.
    TarBz2,
    /// `.tar.xz` or `.txz`.
    TarXz,
    /// `.tar.zst` or `.tzst`.
    TarZst,
    /// `.gz` around one file.
    Gz,
    /// `.bz2` around one file.
    Bz2,
    /// `.xz` around one file.
    Xz,
    /// `.zst` around one file.
    Zst,
    /// `.7z`, read with an installed `7z` or `unar`.
    SevenZ,
    /// `.rar`, read with an installed `7z` or `unar`.
    Rar,
}

/// Name endings for each compressed format, longest first so `.tar.gz` wins over `.gz`.
const COMPRESSED_ENDINGS: [(&str, Compressed); 17] = [
    (".tar.bz2", Compressed::TarBz2),
    (".tar.zst", Compressed::TarZst),
    (".tar.gz", Compressed::TarGz),
    (".tar.xz", Compressed::TarXz),
    (".tzst", Compressed::TarZst),
    (".tbz2", Compressed::TarBz2),
    (".tgz", Compressed::TarGz),
    (".tbz", Compressed::TarBz2),
    (".txz", Compressed::TarXz),
    (".zip", Compressed::Zip),
    (".tar", Compressed::Tar),
    (".bz2", Compressed::Bz2),
    (".zst", Compressed::Zst),
    (".rar", Compressed::Rar),
    (".gz", Compressed::Gz),
    (".xz", Compressed::Xz),
    (".7z", Compressed::SevenZ),
];

/// Extensions of documents and packages that are zip files inside but are never treated as
/// archives: their format is the zip.
pub const ZIP_PACKAGES: [&str; 22] = [
    "docx", "xlsx", "pptx", "docm", "xlsm", "pptm", "odt", "ods", "odp", "odg", "epub", "3mf",
    "jar", "war", "apk", "aab", "ipa", "whl", "usdz", "pages", "numbers", "key",
];

impl Compressed {
    /// The format a file name says, if any. Documents and packages built on the zip format,
    /// such as `.docx` or `.epub`, are not compressed files.
    ///
    /// ```
    /// use carrel_core::detect::Compressed;
    ///
    /// assert_eq!(Compressed::from_name("Export.TAR.GZ"), Some(Compressed::TarGz));
    /// assert_eq!(Compressed::from_name("data.csv.gz"), Some(Compressed::Gz));
    /// assert_eq!(Compressed::from_name("Thesis.docx"), None);
    /// ```
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let lower = name.to_lowercase();
        COMPRESSED_ENDINGS
            .iter()
            .find(|(ending, _)| lower.len() > ending.len() && lower.ends_with(ending))
            .map(|&(_, format)| format)
    }

    /// The format of the file at `path`: from its name, or for a file whose name says nothing,
    /// from its first bytes. A zip-based document or package is never a compressed file, and
    /// neither is a folder.
    #[must_use]
    pub fn of(path: &Path) -> Option<Self> {
        if !std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file()) {
            return None;
        }
        let name = path.file_name()?.to_string_lossy();
        if is_zip_package(&name) || split_part(&name).is_some() {
            return None;
        }
        if let Some(format) = Self::from_name(&name) {
            return Some(format);
        }
        if path.extension().is_some() {
            return None;
        }
        let mut head = [0u8; 512];
        let read = std::fs::File::open(path)
            .and_then(|mut f| std::io::Read::read(&mut f, &mut head))
            .unwrap_or(0);
        Self::sniff(&head[..read])
    }

    /// The format a file's first bytes say, if they're distinctive. A tar inside a compressor
    /// can't be told from its first bytes, so those read as the compressor alone.
    #[must_use]
    pub fn sniff(head: &[u8]) -> Option<Self> {
        let starts = |magic: &[u8]| head.starts_with(magic);
        if starts(b"PK\x03\x04") || starts(b"PK\x05\x06") {
            Some(Self::Zip)
        } else if starts(b"\x1f\x8b") {
            Some(Self::Gz)
        } else if starts(b"BZh") {
            Some(Self::Bz2)
        } else if starts(b"\xfd7zXZ\0") {
            Some(Self::Xz)
        } else if starts(b"\x28\xb5\x2f\xfd") {
            Some(Self::Zst)
        } else if starts(b"7z\xbc\xaf\x27\x1c") {
            Some(Self::SevenZ)
        } else if starts(b"Rar!\x1a\x07") {
            Some(Self::Rar)
        } else if head.len() >= 262 && &head[257..262] == b"ustar" {
            Some(Self::Tar)
        } else {
            None
        }
    }

    /// Whether it holds many files (an archive), rather than wrapping one.
    #[must_use]
    pub fn is_archive(self) -> bool {
        !matches!(self, Self::Gz | Self::Bz2 | Self::Xz | Self::Zst)
    }

    /// Whether Carrel reads it itself; 7z and RAR need an installed tool.
    #[must_use]
    pub fn is_portable(self) -> bool {
        !matches!(self, Self::SevenZ | Self::Rar)
    }

    /// The name of what it expands to: the file name without the format's ending, so
    /// `Export.tar.gz` becomes `Export` and `data.csv.gz` becomes `data.csv`.
    ///
    /// ```
    /// use carrel_core::detect::Compressed;
    ///
    /// assert_eq!(Compressed::TarGz.stem("Export.tar.gz"), "Export");
    /// assert_eq!(Compressed::Gz.stem("data.csv.gz"), "data.csv");
    /// assert_eq!(Compressed::Zip.stem("notes"), "notes");
    /// ```
    #[must_use]
    pub fn stem(self, name: &str) -> String {
        let lower = name.to_lowercase();
        COMPRESSED_ENDINGS
            .iter()
            .filter(|(_, format)| *format == self)
            .find(|(ending, _)| lower.len() > ending.len() && lower.ends_with(ending))
            .map_or_else(
                || name.to_owned(),
                |(ending, _)| name[..name.len() - ending.len()].to_owned(),
            )
    }

    /// Its media type.
    #[must_use]
    pub fn media_type(self) -> &'static str {
        match self {
            Self::Zip => "application/zip",
            Self::Tar => "application/x-tar",
            Self::TarGz | Self::Gz => "application/gzip",
            Self::TarBz2 | Self::Bz2 => "application/x-bzip2",
            Self::TarXz | Self::Xz => "application/x-xz",
            Self::TarZst | Self::Zst => "application/zstd",
            Self::SevenZ => "application/x-7z-compressed",
            Self::Rar => "application/vnd.rar",
        }
    }
}

/// Whether `name` is a document or package whose format is the zip, such as `Thesis.docx` or
/// `book.epub`: never treated as an archive.
#[must_use]
pub fn is_zip_package(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| ZIP_PACKAGES.contains(&e.to_string_lossy().to_lowercase().as_str()))
}

/// For one part of an archive split into several (`.z01`, `.zip.001`, `.7z.001`,
/// `.part1.rar`, `.r00`), the part's number; `None` for anything else. A split archive is kept
/// compressed.
///
/// ```
/// use carrel_core::detect::split_part;
///
/// assert_eq!(split_part("backup.z01"), Some(1));
/// assert_eq!(split_part("backup.7z.002"), Some(2));
/// assert_eq!(split_part("film.part3.rar"), Some(3));
/// assert_eq!(split_part("backup.zip"), None);
/// ```
#[must_use]
pub fn split_part(name: &str) -> Option<u32> {
    let lower = name.to_lowercase();
    let (rest, last) = lower.rsplit_once('.')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if let Some(number) = last.strip_prefix('z').or_else(|| last.strip_prefix('r'))
        && number.len() == 2
        && digits(number)
    {
        return number.parse().ok();
    }
    if digits(last) && last.len() == 3 {
        let inner = Path::new(rest)
            .extension()?
            .to_string_lossy()
            .to_lowercase();
        return matches!(inner.as_str(), "zip" | "7z" | "rar" | "tar" | "gz")
            .then(|| last.parse().ok())
            .flatten();
    }
    if last == "rar"
        && let Some((_, part)) = rest.rsplit_once('.')
        && let Some(number) = part.strip_prefix("part")
        && digits(number)
    {
        return number.parse().ok();
    }
    None
}

/// Reads and parses a YAML file, or returns `None` if it doesn't exist.
fn read_yaml<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, RulesError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(RulesError {
                path: path.to_owned(),
                message: e.to_string(),
            });
        }
    };
    serde_saphyr::from_str(&text)
        .map(Some)
        .map_err(|e| RulesError {
            path: path.to_owned(),
            message: e.to_string(),
        })
}

/// A one-line YAML flow list, such as `markers: [".git", "Cargo.toml"]`.
fn flow_list(key: &str, patterns: &[NamePattern]) -> String {
    let items: Vec<String> = patterns
        .iter()
        .map(|p| serde_json::to_string(&p.0).expect("strings serialize"))
        .collect();
    format!("{key}: [{}]\n", items.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> NamePattern {
        NamePattern(s.into())
    }

    #[test]
    fn patterns_match_whole_names() {
        assert!(p("*.kicad_pro").matches("board.kicad_pro"));
        assert!(p("*.kicad_pro").matches(".kicad_pro"));
        assert!(!p("*.kicad_pro").matches("board.kicad_pro.bak"));
        assert!(p("Cargo.toml").matches("cargo.TOML"), "case-insensitive");
        assert!(!p("Cargo.toml").matches("Cargo.toml.orig"));
        assert!(p("a*b*c").matches("aXbYc"));
        assert!(p("a*b*c").matches("abc"));
        assert!(!p("a*b*c").matches("acb"));
        assert!(
            !p("ab*ba").matches("aba"),
            "prefix and suffix can't overlap"
        );
        assert!(p("*").matches("anything"));
        assert!(p("**").matches(""));
    }

    #[test]
    fn default_rules_round_trip_through_yaml() {
        let rules = Rules::default();
        let markers: MarkerRules = serde_saphyr::from_str(&rules.markers_yaml()).unwrap();
        let regenerable: RegenerableRules =
            serde_saphyr::from_str(&rules.regenerable_yaml()).unwrap();
        assert_eq!(markers.markers, rules.markers);
        assert_eq!(regenerable.regenerable, rules.regenerable);
    }
}
