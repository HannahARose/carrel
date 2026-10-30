//! Creating collections: a folder with a design document (FORMAT.md §6) that says what belongs
//! in it, which every model reads before filing anything there.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::archive::{Archive, ArchiveError, ROOT_RESERVED, escape_link_text};
use crate::card::{Id, Tag};
use crate::coordinator::WriterLease;
use crate::history::{Change, History, HistoryError, Operation};

/// What a new collection's design document starts with, besides its path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewCollection {
    /// One line saying what belongs in it, used in listings and read by models.
    pub description: Option<String>,
    /// Its tag vocabulary: each tag and what it means.
    pub tags: Vec<(String, String)>,
}

/// A collection that was created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    /// Its folder, relative to the archive root.
    pub path: String,
    /// Its identifier.
    pub id: Id,
    /// Its design document, relative to the archive root.
    pub design_document: String,
    /// The history commit that records it, if one was made.
    pub commit: Option<String>,
}

/// Why a collection couldn't be created.
#[derive(Debug)]
pub enum CollectionError {
    /// The path is empty, has an empty part, or starts with `.` or `_`.
    BadPath(String),
    /// A part of the path won't work on every system.
    NotPortable(Vec<String>),
    /// The name is reserved at the archive root (FORMAT.md §2.7).
    Reserved(String),
    /// Something already exists at the path.
    Exists(String),
    /// The folder it would go in isn't a collection.
    NoParent(String),
    /// A tag isn't lowercase words joined by hyphens.
    BadTag(String),
    /// Writing failed.
    Io {
        /// The path.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The archive couldn't be read.
    Archive(ArchiveError),
    /// The history couldn't be committed.
    History(HistoryError),
}

impl std::fmt::Display for CollectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadPath(path) => write!(
                f,
                "“{path}” isn't a collection path: give folder names separated by /, such as Finances or Finances/Tax"
            ),
            Self::NotPortable(problems) => write!(f, "the name {}", problems.join(" and ")),
            Self::Reserved(name) => write!(f, "{name} is reserved at the archive's top level"),
            Self::Exists(path) => write!(f, "{path} already exists"),
            Self::NoParent(parent) => write!(
                f,
                "{parent} isn't a collection; create it first, or put the new one at the top level"
            ),
            Self::BadTag(tag) => write!(
                f,
                "the tag “{tag}” must be lowercase letters and digits, joined by hyphens"
            ),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Archive(e) => write!(f, "{e}"),
            Self::History(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CollectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Archive(e) => Some(e),
            Self::History(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ArchiveError> for CollectionError {
    fn from(e: ArchiveError) -> Self {
        Self::Archive(e)
    }
}

impl From<HistoryError> for CollectionError {
    fn from(e: HistoryError) -> Self {
        Self::History(e)
    }
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> CollectionError + '_ {
    move |source| CollectionError::Io {
        path: path.to_owned(),
        source,
    }
}

/// Creates the collection at `path` (such as `Finances` or `Finances/Tax`) with a design
/// document to fill in, links it from its parent and `ARCHIVE.md`, and records it in history,
/// holding `lease`.
///
/// # Errors
///
/// A [`CollectionError`] if the path can't be used, or writing or committing fails.
pub fn create(
    archive: &Archive,
    path: &str,
    new: &NewCollection,
    lease: &WriterLease,
) -> Result<Created, CollectionError> {
    let path = path.trim().trim_matches('/');
    let parts: Vec<&str> = path.split('/').collect();
    if path.is_empty()
        || parts
            .iter()
            .any(|p| p.trim().is_empty() || p.starts_with('.') || p.starts_with('_'))
    {
        return Err(CollectionError::BadPath(path.to_owned()));
    }
    for part in &parts {
        let problems = crate::names::problems(part);
        if !problems.is_empty() {
            return Err(CollectionError::NotPortable(problems));
        }
    }
    if ROOT_RESERVED
        .iter()
        .any(|r| r.eq_ignore_ascii_case(parts[0]))
        && parts.len() == 1
    {
        return Err(CollectionError::Reserved(parts[0].to_owned()));
    }
    let collections = archive.collections()?;
    let parent = parts[..parts.len() - 1].join("/");
    if !parent.is_empty() && !collections.iter().any(|c| c.path == parent) {
        return Err(CollectionError::NoParent(parent));
    }
    for (tag, _) in &new.tags {
        if !Tag(tag.clone()).is_well_formed() {
            return Err(CollectionError::BadTag(tag.clone()));
        }
    }
    let folder = archive.root().join(path);
    // Case-insensitive disks would treat "finances" and "Finances" as one folder.
    let taken = std::fs::read_dir(folder.parent().unwrap_or(archive.root()))
        .map_err(io(&folder))?
        .filter_map(Result::ok)
        .any(|e| {
            e.file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(parts[parts.len() - 1])
        });
    if taken {
        return Err(CollectionError::Exists(path.to_owned()));
    }

    let title = parts[parts.len() - 1];
    let id = Id::new_v7();
    std::fs::create_dir(&folder).map_err(io(&folder))?;
    let document = folder.join("_collection.md");
    std::fs::write(&document, design_document(archive, id, title, &parent, new))
        .map_err(io(&document))?;
    let changelog = folder.join("CHANGELOG.md");
    std::fs::write(&changelog, "# Changelog\n").map_err(io(&changelog))?;
    crate::listing::refresh(archive)?;

    let change = Change::new(
        Operation::Restructure,
        format!("Create the collection “{path}”"),
        lease.info().person,
        lease.info().device,
    );
    let commit = History::open(archive)?.commit(archive, &change, lease)?;
    Ok(Created {
        path: path.to_owned(),
        id,
        design_document: format!("{path}/_collection.md"),
        commit,
    })
}

/// A new design document: frontmatter, then the sections models read, with guidance for the
/// owner to replace, and the managed listing.
fn design_document(
    archive: &Archive,
    id: Id,
    title: &str,
    parent: &str,
    new: &NewCollection,
) -> String {
    let quoted = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""));
    let mut text = format!(
        "---\ncarrel: collection\nformat: \"{}\"\nid: {id}\ntitle: {}\n",
        crate::FORMAT_VERSION,
        quoted(title)
    );
    if let Some(description) = new.description.as_deref().filter(|d| !d.trim().is_empty()) {
        writeln!(text, "description: {}", quoted(description.trim())).unwrap();
    }
    if !new.tags.is_empty() {
        text.push_str("tags:\n");
        for (tag, meaning) in &new.tags {
            writeln!(
                text,
                "  - name: {tag}\n    description: {}",
                quoted(meaning)
            )
            .unwrap();
        }
    }
    let purpose = new
        .description
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map_or_else(
            || "<!-- What belongs here, in a sentence or two. Carrel's models read this section before filing anything here. -->".to_owned(),
            |d| {
                if d.ends_with(['.', '!', '?']) {
                    d.to_owned()
                } else {
                    format!("{d}.")
                }
            },
        );
    let warrant = archive.doc().min_collection_items.unwrap_or(5);
    let (up, up_link) = if parent.is_empty() {
        (archive.doc().name.clone(), "../ARCHIVE.md".to_owned())
    } else {
        let name = parent.rsplit('/').next().unwrap_or(parent).to_owned();
        (name, "../_collection.md".to_owned())
    };
    write!(
        text,
        "---\n\n# {title}\n\n## Purpose and scope\n\n{purpose}\n\n## Not here\n\n\
         <!-- What looks as if it belongs here but doesn't, and where it goes instead. -->\n\n\
         ## Organization\n\nFlat until a topic has at least {warrant} items, then a subcollection per topic.\n\n\
         <!-- carrel:listing -->\n## Contents\n\nPart of [{}]({up_link})\n<!-- /carrel:listing -->\n",
        escape_link_text(&up)
    )
    .unwrap();
    text
}
