//! Renaming an item in place, as when the owner accepts a portable name (FORMAT.md §2.5). The
//! item and its card are renamed together, other cards' links follow, and the change is one
//! history commit with a `Carrel-Move` trailer, so undo can rename it back.

use std::path::{Path, PathBuf};

use crate::archive::{Archive, ArchiveError, ItemRef, escape_link_text};
use crate::card_file::{CardFile, CardFileError};
use crate::coordinator::WriterLease;
use crate::history::{Change, History, HistoryError, Move, Operation};

/// What renaming did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renamed {
    /// The item's path before, relative to the archive root (its card's, for a card-only item).
    pub from: String,
    /// Its path after.
    pub to: String,
    /// The card's path after.
    pub card: String,
    /// Other cards whose links were updated.
    pub relinked: Vec<String>,
    /// The history commit.
    pub commit: Option<String>,
}

/// Why an item couldn't be renamed. Nothing has changed when this is returned.
#[derive(Debug)]
pub enum RenameError {
    /// The new name isn't portable.
    NotPortable(Vec<String>),
    /// The new name, or its card's name, is taken in the folder.
    Taken(String),
    /// The item is checked out, so its contents aren't where the card says.
    CheckedOut,
    /// A file operation failed.
    Io {
        /// The path concerned.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// A card couldn't be read or written.
    Card(CardFileError),
    /// The archive couldn't be read.
    Archive(ArchiveError),
    /// The history couldn't be committed.
    History(HistoryError),
}

impl std::fmt::Display for RenameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotPortable(problems) => {
                write!(f, "the new name {}", problems.join(" and "))
            }
            Self::Taken(name) => write!(f, "{name} is already taken in that folder"),
            Self::CheckedOut => f.write_str("the item is checked out; return it first"),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Card(e) => write!(f, "{e}"),
            Self::Archive(e) => write!(f, "{e}"),
            Self::History(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RenameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Card(e) => Some(e),
            Self::Archive(e) => Some(e),
            Self::History(e) => Some(e),
            _ => None,
        }
    }
}

impl From<CardFileError> for RenameError {
    fn from(e: CardFileError) -> Self {
        Self::Card(e)
    }
}

impl From<ArchiveError> for RenameError {
    fn from(e: ArchiveError) -> Self {
        Self::Archive(e)
    }
}

impl From<HistoryError> for RenameError {
    fn from(e: HistoryError) -> Self {
        Self::History(e)
    }
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> RenameError + '_ {
    move |source| RenameError::Io {
        path: path.to_owned(),
        source,
    }
}

fn parent_and_name(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

/// Whether `name` is free in `folder`, compared without case (FORMAT.md §2.5). `except` is a
/// name that may be taken because it's the one being replaced.
fn is_free(folder: &Path, name: &str, except: &[&str]) -> Result<bool, RenameError> {
    let wanted = name.to_lowercase();
    for entry in std::fs::read_dir(folder).map_err(io(folder))? {
        let entry = entry.map_err(io(folder))?;
        let existing = entry.file_name().to_string_lossy().into_owned();
        if existing.to_lowercase() == wanted && !except.contains(&existing.as_str()) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Renames `item` to `new_name` in its folder, holding `lease`. For an item with contents, the
/// contents and card are renamed together and the card's `item` follows; for a card-only item,
/// just the card. Links to it from other cards are updated, the collection's changelog gets a
/// line, listings are rebuilt, and everything is one `move` commit.
///
/// # Errors
///
/// [`RenameError`]; nothing changes unless the rename itself succeeded.
pub fn rename(
    archive: &Archive,
    item: &ItemRef,
    new_name: &str,
    lease: &WriterLease,
) -> Result<Renamed, RenameError> {
    let problems = crate::names::problems(new_name);
    if !problems.is_empty() {
        return Err(RenameError::NotPortable(problems));
    }
    let root = archive.root();
    let old_card = root.join(&item.card);
    let mut file = CardFile::parse(&std::fs::read_to_string(&old_card).map_err(io(&old_card))?)?;
    if file.card().checked_out.is_some() {
        return Err(RenameError::CheckedOut);
    }
    let (folder_path, old_card_name) = parent_and_name(&item.card);
    let folder = root.join(folder_path);

    let (from, to, card_name) = names_for(&folder, folder_path, old_card_name, item, new_name)?;
    let new_card = folder.join(&card_name);
    let card_path = format!("{folder_path}/{card_name}");

    if item.item.is_some() {
        let source = root.join(&from);
        let target = root.join(&to);
        // A rename that only changes case goes through a free name on case-insensitive disks.
        if from.to_lowercase() == to.to_lowercase() {
            let step = folder.join(format!(".carrel-rename-{}", item.id));
            std::fs::rename(&source, &step).map_err(io(&source))?;
            std::fs::rename(&step, &target).map_err(io(&target))?;
        } else {
            std::fs::rename(&source, &target).map_err(io(&source))?;
        }
        file.card_mut().item = Some(new_name.to_owned());
    }
    // The owner has placed the item, so its placement is no longer in doubt (FORMAT.md §5.8).
    file.card_mut().placement = None;
    let text = file.to_text()?;
    let temp = folder.join(format!(".{card_name}.tmp-{}", item.id));
    std::fs::write(&temp, text).map_err(io(&temp))?;
    if old_card != new_card {
        std::fs::remove_file(&old_card).map_err(io(&old_card))?;
    }
    std::fs::rename(&temp, &new_card).map_err(io(&new_card))?;

    let relinked = relink(archive, item, &card_path)?;
    let title = file.card().title.clone();
    let old_name = parent_and_name(&from).1.to_owned();
    let entry = format!(
        "renamed “{}” to [{}]({})",
        old_name.replace('|', "\\|"),
        escape_link_text(&title),
        crate::listing::link_destination(&card_name)
    );
    if !folder_path.is_empty() {
        crate::listing::add_to_changelog(archive, folder_path, &entry)
            .map_err(io(&folder.join("CHANGELOG.md")))?;
    }
    crate::listing::refresh(archive)?;

    let mut change = Change::new(
        Operation::Move,
        format!("Rename “{old_name}” to “{}”", parent_and_name(&to).1),
        lease.info().person,
        lease.info().device,
    );
    change.items = vec![item.id];
    if item.item.is_some() {
        change.moves = vec![Move {
            item: item.id,
            from: from.clone(),
            to: to.clone(),
        }];
    }
    let commit = History::open(archive)?.commit(archive, &change, lease)?;
    Ok(Renamed {
        from,
        to,
        card: card_path,
        relinked,
        commit,
    })
}

/// The item's path before and after, and its card's new name.
fn names_for(
    folder: &Path,
    folder_path: &str,
    old_card_name: &str,
    item: &ItemRef,
    new_name: &str,
) -> Result<(String, String, String), RenameError> {
    if let Some(contents) = &item.item {
        let (_, old_name) = parent_and_name(contents);
        let card_name = format!("{new_name}.md");
        for name in [new_name, card_name.as_str()] {
            if !is_free(folder, name, &[old_name, old_card_name])? {
                return Err(RenameError::Taken(name.to_owned()));
            }
        }
        let to = if folder_path.is_empty() {
            new_name.to_owned()
        } else {
            format!("{folder_path}/{new_name}")
        };
        return Ok((contents.clone(), to, card_name));
    }
    let card_name = if Path::new(new_name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
    {
        new_name.to_owned()
    } else {
        format!("{new_name}.md")
    };
    if !is_free(folder, &card_name, &[old_card_name])? {
        return Err(RenameError::Taken(card_name));
    }
    Ok((
        item.card.clone(),
        format!("{folder_path}/{card_name}"),
        card_name,
    ))
}

/// Points other cards' links to `item` at its card's new path.
fn relink(archive: &Archive, item: &ItemRef, card_path: &str) -> Result<Vec<String>, RenameError> {
    let mut relinked = Vec::new();
    for other in archive.items()? {
        if other.id == item.id {
            continue;
        }
        let path = archive.root().join(&other.card);
        let text = std::fs::read_to_string(&path).map_err(io(&path))?;
        let Ok(mut file) = CardFile::parse(&text) else {
            continue;
        };
        let (from_folder, _) = parent_and_name(&other.card);
        let wanted = crate::listing::relative(from_folder, card_path);
        let mut changed = false;
        for link in &mut file.card_mut().links {
            if link.id == item.id && link.path.as_deref() != Some(wanted.as_str()) {
                link.path = Some(wanted.clone());
                changed = true;
            }
        }
        if changed {
            let updated = file.to_text()?;
            let temp = path.with_extension(format!("tmp-{}", item.id));
            std::fs::write(&temp, updated).map_err(io(&temp))?;
            std::fs::rename(&temp, &path).map_err(io(&path))?;
            relinked.push(other.card);
        }
    }
    Ok(relinked)
}
