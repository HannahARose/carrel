//! Creating and opening archives (FORMAT.md §3, §7.1, §12).

use std::collections::HashSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::card::{DateOrTime, Id};
use crate::detect::{Detected, Rules, RulesError};
use crate::documents::{
    ArchiveDoc, ArchiveInfo, ArchiveMarker, CollectionDoc, CollectionMarker, Settings,
};
use crate::frontmatter;
use crate::{Access, CARREL_VERSION, FORMAT_VERSION, FormatVersion};

/// This build's copy of the format specification, written into every new archive.
const FORMAT_SPEC: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../FORMAT.md"));

/// Names at the archive root reserved by the format (FORMAT.md §2.7).
pub const ROOT_RESERVED: [&str; 6] = [
    "ARCHIVE.md",
    "README.md",
    "Active",
    "Holding",
    "Journal",
    "_System",
];

/// An open archive.
#[derive(Debug, Clone, PartialEq)]
pub struct Archive {
    root: PathBuf,
    info: ArchiveInfo,
    doc: ArchiveDoc,
    settings: Settings,
    access: Access,
}

/// A collection found in an archive.
#[derive(Debug, Clone, PartialEq)]
pub struct Collection {
    /// The collection's folder, relative to the archive root, with `/` separators.
    pub path: String,
    /// Its design document's frontmatter.
    pub doc: CollectionDoc,
}

/// Options for [`Archive::init`].
#[derive(Debug, Clone)]
pub struct InitOptions {
    /// The archive's name.
    pub name: String,
    /// Create the archive even if the folder already has files in it. They become
    /// uncataloged content, to be offered for ingest.
    pub allow_existing_files: bool,
    /// The creation time; defaults to now in the local time zone.
    pub now: Option<DateOrTime>,
}

impl InitOptions {
    /// Options for a new, empty archive named `name`, created now.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            allow_existing_files: false,
            now: None,
        }
    }
}

/// Why an archive couldn't be created or opened.
#[derive(Debug)]
pub enum ArchiveError {
    /// Reading or writing a file failed.
    Io {
        /// The file or folder concerned.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The folder isn't an archive: `_System/archive.json` is missing.
    NotAnArchive(PathBuf),
    /// The folder is already an archive.
    AlreadyAnArchive(PathBuf),
    /// The folder has files in it and `allow_existing_files` wasn't set.
    NotEmpty(PathBuf),
    /// A document is malformed.
    Invalid {
        /// The file concerned.
        path: PathBuf,
        /// What is wrong.
        message: String,
    },
    /// `ARCHIVE.md` and `archive.json` disagree about the archive's identifier.
    IdMismatch {
        /// The identifier in `archive.json`.
        info: Id,
        /// The identifier in `ARCHIVE.md`.
        doc: Id,
    },
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::NotAnArchive(p) => write!(
                f,
                "{} isn't a Carrel archive (no _System/archive.json)",
                p.display()
            ),
            Self::AlreadyAnArchive(p) => write!(f, "{} is already a Carrel archive", p.display()),
            Self::NotEmpty(p) => write!(f, "{} already has files in it", p.display()),
            Self::Invalid { path, message } => write!(f, "{}: {message}", path.display()),
            Self::IdMismatch { info, doc } => {
                write!(
                    f,
                    "archive.json says the archive is {info}, but ARCHIVE.md says {doc}"
                )
            }
        }
    }
}

impl std::error::Error for ArchiveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Wraps an I/O error with the path it happened on.
fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ArchiveError + '_ {
    move |source| ArchiveError::Io {
        path: path.to_owned(),
        source,
    }
}

/// An [`ArchiveError::Invalid`] for `path`.
fn invalid(path: &Path, message: impl std::fmt::Display) -> ArchiveError {
    ArchiveError::Invalid {
        path: path.to_owned(),
        message: message.to_string(),
    }
}

/// Writes a file that must not already exist, creating parent folders.
fn create_file(path: &Path, contents: &str) -> Result<(), ArchiveError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io(parent))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io(path))?;
    file.write_all(contents.as_bytes()).map_err(io(path))
}

/// Pretty-printed JSON with a trailing newline, as archive JSON files are written.
fn json(value: &impl serde::Serialize) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("archive documents always serialize");
    text.push('\n');
    text
}

/// The current time in the system time zone, as an RFC 3339 timestamp with offset.
fn now_local() -> DateOrTime {
    DateOrTime(
        jiff::Zoned::now()
            .strftime("%Y-%m-%dT%H:%M:%S%:z")
            .to_string(),
    )
}

impl Archive {
    /// Creates a new archive at `root` (creating the folder if needed) and opens it.
    ///
    /// The new archive has `archive.json`, `ARCHIVE.md`, `README.md`, settings with FORMAT.md's
    /// defaults, a copy of FORMAT.md, empty glossary and rules, a `Holding` collection, and an
    /// empty `Active` section. The history repository is created separately.
    ///
    /// # Errors
    ///
    /// [`ArchiveError::AlreadyAnArchive`] if `root` is already an archive,
    /// [`ArchiveError::NotEmpty`] if it has files and `allow_existing_files` isn't set, and
    /// [`ArchiveError::Io`] if writing fails. A failed init may leave some files behind.
    pub fn init(root: &Path, options: &InitOptions) -> Result<Self, ArchiveError> {
        if root.join("_System/archive.json").exists() {
            return Err(ArchiveError::AlreadyAnArchive(root.to_owned()));
        }
        if root.exists() {
            let has_files = std::fs::read_dir(root)
                .map_err(io(root))?
                .filter_map(Result::ok)
                .any(|e| e.file_name() != ".DS_Store");
            if has_files && !options.allow_existing_files {
                return Err(ArchiveError::NotEmpty(root.to_owned()));
            }
        }
        std::fs::create_dir_all(root).map_err(io(root))?;

        let id = Id::new_v7();
        let created = options.now.clone().unwrap_or_else(now_local);
        let name = options.name.trim();
        let info = ArchiveInfo {
            carrel: ArchiveMarker::Archive,
            format: FORMAT_VERSION,
            id,
            created,
            created_by: format!("carrel {CARREL_VERSION}"),
            extra: crate::card::Extra::new(),
        };
        let doc = ArchiveDoc {
            carrel: ArchiveMarker::Archive,
            format: FORMAT_VERSION,
            id,
            name: name.to_owned(),
            link_types: Vec::new(),
            tags: Vec::new(),
            sensitivity_default: Some(crate::card::Sensitivity::Personal),
            size_threshold: Some(10 * 1024 * 1024 * 1024),
            min_collection_items: Some(5),
            extra: crate::card::Extra::new(),
        };
        let holding = CollectionDoc {
            carrel: CollectionMarker::Collection,
            format: FORMAT_VERSION,
            id: Id::new_v7(),
            title: "Holding".into(),
            description: Some("Items waiting for a better home".into()),
            item_template: None,
            tags: Vec::new(),
            extra: crate::card::Extra::new(),
        };
        create_file(&root.join("_System/archive.json"), &json(&info))?;
        create_file(
            &root.join("_System/settings.json"),
            &json(&Settings::default()),
        )?;
        create_file(&root.join("_System/FORMAT.md"), FORMAT_SPEC)?;
        create_file(&root.join("_System/glossary.md"), "# Glossary\n")?;
        create_file(&root.join("_System/rules/routing.yaml"), "rules: []\n")?;
        let rules = Rules::default();
        create_file(
            &root.join("_System/rules/markers.yaml"),
            &rules.markers_yaml(),
        )?;
        create_file(
            &root.join("_System/rules/regenerable.yaml"),
            &rules.regenerable_yaml(),
        )?;
        create_file(
            &root.join("ARCHIVE.md"),
            &frontmatter::join(&to_frontmatter(&doc)?, &archive_body(name)),
        )?;
        create_file(&root.join("README.md"), &readme(name))?;
        create_file(
            &root.join("Holding/_collection.md"),
            &frontmatter::join(&to_frontmatter(&holding)?, &holding_body(name)),
        )?;
        create_file(&root.join("Holding/CHANGELOG.md"), "# Changelog\n")?;
        create_file(&root.join("Active/_index.md"), ACTIVE_INDEX)?;
        std::fs::create_dir_all(root.join("Journal")).map_err(io(root))?;
        create_file(&root.join(".obsidian/app.json"), OBSIDIAN_APP)?;

        Self::open(root)
    }

    /// Opens the archive at `root`.
    ///
    /// An archive written with a newer major format version opens read-only (FORMAT.md §12).
    /// A missing `settings.json` means FORMAT.md's defaults.
    ///
    /// ```
    /// use carrel_core::Access;
    /// use carrel_core::archive::{Archive, InitOptions};
    ///
    /// let dir = tempfile::tempdir()?;
    /// let created = Archive::init(dir.path(), &InitOptions::new("Notes"))?;
    /// let opened = Archive::open(dir.path())?;
    /// assert_eq!(opened.info().id, created.info().id);
    /// assert_eq!(opened.access(), Access::ReadWrite);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`ArchiveError::NotAnArchive`] if `root` has no `_System/archive.json`,
    /// [`ArchiveError::Invalid`] if `archive.json`, `ARCHIVE.md` or `settings.json` is
    /// malformed, and [`ArchiveError::IdMismatch`] if `archive.json` and `ARCHIVE.md` disagree.
    pub fn open(root: &Path) -> Result<Self, ArchiveError> {
        let info_path = root.join("_System/archive.json");
        let info_text = match std::fs::read_to_string(&info_path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(ArchiveError::NotAnArchive(root.to_owned()));
            }
            other => other.map_err(io(&info_path))?,
        };
        let raw: serde_json::Value =
            serde_json::from_str(&info_text).map_err(|e| invalid(&info_path, e))?;
        let format = raw
            .get("format")
            .and_then(serde_json::Value::as_str)
            .and_then(FormatVersion::parse)
            .ok_or_else(|| invalid(&info_path, "missing or malformed `format`"))?;
        let access = format.access();
        let info: ArchiveInfo = serde_json::from_value(raw).map_err(|e| invalid(&info_path, e))?;

        let doc_path = root.join("ARCHIVE.md");
        let doc_text = std::fs::read_to_string(&doc_path).map_err(io(&doc_path))?;
        let split = frontmatter::split(&doc_text).map_err(|e| invalid(&doc_path, e))?;
        let doc: ArchiveDoc =
            serde_saphyr::from_str(split.yaml).map_err(|e| invalid(&doc_path, e))?;
        if doc.id != info.id {
            return Err(ArchiveError::IdMismatch {
                info: info.id,
                doc: doc.id,
            });
        }

        let settings_path = root.join("_System/settings.json");
        let settings = match std::fs::read_to_string(&settings_path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::default(),
            other => {
                let text = other.map_err(io(&settings_path))?;
                serde_json::from_str(&text).map_err(|e| invalid(&settings_path, e))?
            }
        };

        Ok(Self {
            root: root.to_owned(),
            info,
            doc,
            settings,
            access,
        })
    }

    /// Finds the archive containing `start` by looking in it and each parent folder for
    /// `_System/archive.json`, and opens it.
    ///
    /// # Errors
    ///
    /// [`ArchiveError::NotAnArchive`] with `start` if no parent is an archive, or any error
    /// from [`Archive::open`].
    pub fn find(start: &Path) -> Result<Self, ArchiveError> {
        let start = std::fs::canonicalize(start).map_err(io(start))?;
        let root = start
            .ancestors()
            .find(|dir| dir.join("_System/archive.json").is_file())
            .ok_or_else(|| ArchiveError::NotAnArchive(start.clone()))?;
        Self::open(root)
    }

    /// The archive's root folder.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The contents of `archive.json`.
    #[must_use]
    pub fn info(&self) -> &ArchiveInfo {
        &self.info
    }

    /// The frontmatter of `ARCHIVE.md`.
    #[must_use]
    pub fn doc(&self) -> &ArchiveDoc {
        &self.doc
    }

    /// The archive's settings.
    #[must_use]
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Whether this build may change the archive.
    #[must_use]
    pub fn access(&self) -> Access {
        self.access
    }

    /// The archive's detection rules from `_System/rules/`, with defaults for missing files.
    ///
    /// # Errors
    ///
    /// [`RulesError`] if a rules file exists but is malformed.
    pub fn rules(&self) -> Result<Rules, RulesError> {
        Rules::load(&self.root)
    }

    /// What kind of item `path` would be, using this archive's rules.
    ///
    /// # Errors
    ///
    /// [`ArchiveError::Invalid`] for malformed rules, or [`ArchiveError::Io`] if `path` can't
    /// be read.
    pub fn detect(&self, path: &Path) -> Result<Detected, ArchiveError> {
        let rules = self.rules().map_err(|e| invalid(&e.path, e.message))?;
        rules.detect(path).map_err(io(path))
    }

    /// Every collection in the archive, sorted by path. Folders that are items (described by a
    /// card beside them) are never searched, so a `_collection.md` inside a bundle doesn't
    /// count.
    ///
    /// # Errors
    ///
    /// [`ArchiveError::Io`] if a folder can't be read, and [`ArchiveError::Invalid`] if a
    /// `_collection.md` is malformed.
    pub fn collections(&self) -> Result<Vec<Collection>, ArchiveError> {
        let mut found = Vec::new();
        for entry in read_dir_sorted(&self.root)? {
            let name = entry
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            // Holding is reserved but is a collection; the other reserved names never are.
            if entry.is_dir() && (name == "Holding" || !ROOT_RESERVED.contains(&name.as_str())) {
                walk_collections(&self.root, &entry, &mut found)?;
            }
        }
        found.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(found)
    }
}

/// A folder's entries, sorted by path, leaving out hidden entries (names starting with `.`,
/// such as `.obsidian` or `.DS_Store`). Collection discovery never looks inside those.
fn read_dir_sorted(dir: &Path) -> Result<Vec<PathBuf>, ArchiveError> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(io(dir))?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()
        .map_err(io(dir))?;
    entries.retain(|p| {
        p.file_name()
            .is_some_and(|n| !n.to_string_lossy().starts_with('.'))
    });
    entries.sort();
    Ok(entries)
}

/// The names of items described by cards directly in `dir`.
fn described_items(dir: &Path) -> Result<HashSet<String>, ArchiveError> {
    let mut items = HashSet::new();
    for path in read_dir_sorted(dir)? {
        if !path.is_file()
            || !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(io(&path))?;
        if let Ok(split) = frontmatter::split(&text)
            && split.yaml.starts_with("carrel: card\n")
            && let Ok(card) = crate::card::Card::from_yaml(split.yaml)
            && let Some(item) = card.item
        {
            items.insert(item);
        }
    }
    Ok(items)
}

/// Records `dir` as a collection if it has a `_collection.md`, then searches its subfolders,
/// skipping any folder that a card in `dir` describes as an item: item folders (bundles, open
/// folders) can't contain collections.
fn walk_collections(
    root: &Path,
    dir: &Path,
    found: &mut Vec<Collection>,
) -> Result<(), ArchiveError> {
    let doc_path = dir.join("_collection.md");
    if doc_path.is_file() {
        let text = std::fs::read_to_string(&doc_path).map_err(io(&doc_path))?;
        let split = frontmatter::split(&text).map_err(|e| invalid(&doc_path, e))?;
        let doc: CollectionDoc =
            serde_saphyr::from_str(split.yaml).map_err(|e| invalid(&doc_path, e))?;
        let path = dir
            .strip_prefix(root)
            .unwrap_or(dir)
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        found.push(Collection { path, doc });
    }
    let items = described_items(dir)?;
    for child in read_dir_sorted(dir)? {
        let name = child
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if child.is_dir() && !items.contains(&name) {
            walk_collections(root, &child, found)?;
        }
    }
    Ok(())
}

/// YAML frontmatter for an archive document.
fn to_frontmatter(value: &impl serde::Serialize) -> Result<String, ArchiveError> {
    serde_saphyr::to_string(value).map_err(|e| ArchiveError::Invalid {
        path: PathBuf::new(),
        message: format!("couldn't write YAML: {e}"),
    })
}

/// A new archive's `Active/_index.md`.
const ACTIVE_INDEX: &str = "# Active\n\nNothing is checked out or on the desk.\n";
/// A new archive's `.obsidian/app.json`: use the system trash, and skip `_System/`.
const OBSIDIAN_APP: &str =
    "{\n  \"trashOption\": \"system\",\n  \"userIgnoreFilters\": [\"_System/\"]\n}\n";

/// The body of a new `ARCHIVE.md`: a placeholder purpose, conventions, and a managed
/// `collections` section linking the Holding collection.
fn archive_body(name: &str) -> String {
    format!(
        "\n# {name}\n\nWhat this archive is for, in a sentence or two.\n\n## Conventions\n\nCollection and file names are plain words. Dates in file names are written `YYYY-MM-DD`.\n\n<!-- carrel:collections -->\n## Collections\n\n| Collection | Items | Description |\n| --- | --- | --- |\n| [Holding](Holding/_collection.md) | 0 | Items waiting for a better home |\n<!-- /carrel:collections -->\n"
    )
}

/// The body of a new `Holding/_collection.md`, linking back to `ARCHIVE.md`.
fn holding_body(name: &str) -> String {
    format!(
        "\n# Holding\n\n## Purpose and scope\n\nItems ingest could not place with confidence. Each waits here until it is filed or a restructure gives it a home.\n\n<!-- carrel:listing -->\n## Contents\n\nPart of [{name}](../ARCHIVE.md)\n<!-- /carrel:listing -->\n",
        name = escape_link_text(name)
    )
}

/// Escapes text for use inside `[...]` in a Markdown link, so brackets and backslashes in
/// names can't end the link early or unbalance it.
#[must_use]
pub fn escape_link_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '\\' | '[' | ']') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// A new archive's `README.md`, explaining the layout to someone without Carrel.
fn readme(name: &str) -> String {
    format!(
        "# {name}\n\nThis folder is a Carrel archive: an ordinary folder you can read with Finder and any Markdown reader.\n\n- Start at [ARCHIVE.md](ARCHIVE.md), which links every collection.\n- Each **collection** is a folder with a `_collection.md` explaining what belongs in it.\n- Each **item** has a **card** beside it: `Paper.pdf` is described by `Paper.pdf.md`.\n- `Active/` shows what is checked out, `Journal/` logs work sessions, and `Holding/` has items waiting to be filed.\n- `_System/` holds data for programs, including the format specification (`_System/FORMAT.md`).\n"
    )
}
