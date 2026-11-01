//! `carrel expand ITEM`: expanding a compressed file that's already filed, on the owner's
//! request, with the same checks as filing, as one undoable change
//! (`docs/design/compressed.md`).

use std::path::{Path, PathBuf};

use super::{Error, Limits, plan};
use crate::archive::{Archive, ArchiveError, ItemRef, escape_link_text};
use crate::card::{Checksum, ExpandedFrom, Extra, Id, Kind, ManifestRef};
use crate::card_file::{CardFile, CardFileError};
use crate::coordinator::WriterLease;
use crate::history::{Change, History, HistoryError, Move, Operation};
use crate::manifest::{Manifest, ManifestError};

/// What expanding a filed item did.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpandedItem {
    /// The compressed file's path before, relative to the archive root.
    pub from: String,
    /// What it held, now the item, relative to the archive root.
    pub to: String,
    /// The card's path after.
    pub card: String,
    /// What the item is now.
    pub kind: Kind,
    /// How many files were written.
    pub files: usize,
    /// How many bytes.
    pub size: u64,
    /// Why the compressed file couldn't go to the Trash, when it couldn't; it's then left
    /// beside the item.
    pub trash_problem: Option<String>,
    /// The compressed file, kept in the holding folder instead of the Trash on a device
    /// without Kopia, until a snapshot has it ([`crate::holding`]).
    pub held: Option<crate::holding::Held>,
    /// The history commit.
    pub commit: Option<String>,
}

/// Why a filed item couldn't be expanded. Nothing in the archive has changed.
#[derive(Debug)]
pub enum ExpandError {
    /// The item isn't a compressed file.
    NotCompressed(String),
    /// The item is checked out, so its contents aren't where the card says.
    CheckedOut,
    /// There's a reason to keep it compressed; `anyway` overrides it when it can be.
    Kept {
        /// Why.
        reason: String,
        /// Whether asking again with `anyway` would expand it.
        overridable: bool,
    },
    /// Expanding failed.
    Expand(Error),
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
    /// The manifest couldn't be built.
    Manifest(ManifestError),
    /// The history couldn't be committed.
    History(HistoryError),
}

impl std::fmt::Display for ExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotCompressed(name) => write!(f, "{name} isn't a compressed file"),
            Self::CheckedOut => f.write_str("the item is checked out; return it first"),
            Self::Kept {
                reason,
                overridable,
            } => {
                write!(f, "it's kept compressed: {reason}")?;
                if *overridable {
                    f.write_str("; use --anyway to expand it all the same")?;
                }
                Ok(())
            }
            Self::Expand(e) => write!(f, "it couldn't be expanded: {e}"),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Card(e) => write!(f, "{e}"),
            Self::Archive(e) => write!(f, "{e}"),
            Self::Manifest(e) => write!(f, "{e}"),
            Self::History(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ExpandError {}

impl From<CardFileError> for ExpandError {
    fn from(e: CardFileError) -> Self {
        Self::Card(e)
    }
}

impl From<ArchiveError> for ExpandError {
    fn from(e: ArchiveError) -> Self {
        Self::Archive(e)
    }
}

impl From<ManifestError> for ExpandError {
    fn from(e: ManifestError) -> Self {
        Self::Manifest(e)
    }
}

impl From<HistoryError> for ExpandError {
    fn from(e: HistoryError) -> Self {
        Self::History(e)
    }
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ExpandError + '_ {
    move |source| ExpandError::Io {
        path: path.to_owned(),
        source,
    }
}

/// Whether the filed compressed file `item` would expand: `None` if it would as it is, or the
/// reason it's kept compressed when only expanding it all the same (`anyway`) would. Reads the
/// file's listing but changes nothing, so an interface can ask before expanding.
///
/// # Errors
///
/// [`ExpandError`] when it can't be expanded at all: it isn't a compressed file, it's checked
/// out, or it's kept compressed for a reason `anyway` doesn't override.
pub fn examine(archive: &Archive, item: &ItemRef) -> Result<Option<String>, ExpandError> {
    let card_path = archive.root().join(&item.card);
    let file = CardFile::parse(&std::fs::read_to_string(&card_path).map_err(io(&card_path))?)?;
    if file.card().checked_out.is_some() {
        return Err(ExpandError::CheckedOut);
    }
    let Some(from) = item.item.clone().filter(|_| file.card().kind == Kind::File) else {
        return Err(ExpandError::NotCompressed(item.card.clone()));
    };
    let inside = super::inspect(&archive.root().join(&from))
        .ok_or_else(|| ExpandError::NotCompressed(from.clone()))?;
    match check(&inside, false) {
        Ok(()) => Ok(None),
        Err(ExpandError::Kept {
            reason,
            overridable: true,
        }) => Ok(Some(reason)),
        Err(e) => Err(e),
    }
}

/// Expands the filed compressed file `item`, holding `lease`: what it held takes its place in
/// the collection, unwrapped as at filing, its card follows (with `expanded_from`, and a new
/// name when the item's name changes), links to it follow, and the compressed file goes to the
/// system Trash once everything is in place. It's one `edit` change: undo restores the card
/// and moves what was expanded out of the collection; the compressed file comes back from the
/// Trash.
///
/// Software, and expansions that look too big, are refused unless `anyway`; encrypted, split,
/// unreadable and unsafe archives always are.
///
/// # Errors
///
/// [`ExpandError`]; nothing in the archive has changed.
pub fn expand_item(
    archive: &Archive,
    item: &ItemRef,
    anyway: bool,
    lease: &WriterLease,
) -> Result<ExpandedItem, ExpandError> {
    let root = archive.root();
    let card_path = root.join(&item.card);
    let mut file = CardFile::parse(&std::fs::read_to_string(&card_path).map_err(io(&card_path))?)?;
    if file.card().checked_out.is_some() {
        return Err(ExpandError::CheckedOut);
    }
    let (folder_path, old_card_name) = item.card.rsplit_once('/').unwrap_or(("", &item.card));
    let Some(from) = item.item.clone().filter(|_| file.card().kind == Kind::File) else {
        return Err(ExpandError::NotCompressed(item.card.clone()));
    };
    let source = root.join(&from);
    let inside = super::inspect(&source).ok_or_else(|| ExpandError::NotCompressed(from.clone()))?;
    check(&inside, anyway)?;

    let temp = root
        .join("_System/cache.nosync/expanding")
        .join(Id::new_v7().to_string());
    std::fs::create_dir_all(&temp).map_err(io(&temp))?;
    let result = place(archive, item, &mut file, &source, &temp, anyway);
    let _ = remove(&temp);
    let (to, kind, expanded, staged) = result?;
    let name = to
        .rsplit_once('/')
        .map_or(to.as_str(), |(_, n)| n)
        .to_owned();

    let card_name = format!("{name}.md");
    let new_card_path = join(folder_path, &card_name);
    let new_card = root.join(&new_card_path);
    let text = file.to_text()?;
    std::fs::write(&new_card, text).map_err(io(&new_card))?;
    if old_card_name != card_name {
        std::fs::remove_file(&card_path).map_err(io(&card_path))?;
    }
    crate::rename::relink(archive, item, &new_card_path).map_err(|e| ExpandError::Io {
        path: new_card.clone(),
        source: std::io::Error::other(e.to_string()),
    })?;
    let old_name = from
        .rsplit_once('/')
        .map_or(from.as_str(), |(_, n)| n)
        .to_owned();
    if !folder_path.is_empty() {
        let entry = format!(
            "expanded “{}” into [{}]({})",
            old_name.replace('|', "\\|"),
            escape_link_text(&file.card().title),
            crate::listing::link_destination(&card_name)
        );
        crate::listing::add_to_changelog(archive, folder_path, &entry)
            .map_err(io(&root.join(folder_path).join("CHANGELOG.md")))?;
    }
    // Only now, with the item and card in place, does the compressed file leave: to the
    // holding folder on a device without Kopia, otherwise to the Trash.
    let holder = crate::coordinator::Holder {
        person: lease.info().person,
        device: lease.info().device,
    };
    let keeping = crate::holding::Keeping {
        item: Some(item.id),
        title: &file.card().title,
        why: crate::holding::Why::Expand,
        name: None,
    };
    let mut trash_problem = None;
    let held = crate::holding::keep_or(archive, &source, &keeping, holder, |source| {
        trash_problem = crate::trash::put(source).err().map(|e| e.to_string());
        Ok::<(), std::io::Error>(())
    })
    .unwrap_or_else(|e| {
        trash_problem = Some(e.to_string());
        None
    });
    crate::listing::refresh(archive)?;

    let mut change = Change::new(
        Operation::Edit,
        format!("Expand “{old_name}”"),
        lease.info().person,
        lease.info().device,
    );
    change.items = vec![item.id];
    change.moves = vec![Move {
        item: item.id,
        from: staged,
        to: to.clone(),
    }];
    let commit = History::open(archive)?.commit(archive, &change, lease)?;
    Ok(ExpandedItem {
        from,
        to,
        card: new_card_path,
        kind,
        files: expanded.files,
        size: expanded.size,
        trash_problem,
        held,
        commit,
    })
}

/// Refuses what can't, or shouldn't without `anyway`, be expanded.
fn check(inside: &super::Inside, anyway: bool) -> Result<(), ExpandError> {
    let kept = |reason: String, overridable: bool| ExpandError::Kept {
        reason,
        overridable,
    };
    let Some(listing) = inside.listing.as_ref().filter(|_| !inside.split) else {
        return Err(kept(
            plan::automatic(inside).unwrap_or_else(|| "it couldn't be read".into()),
            false,
        ));
    };
    if listing.encrypted() {
        return Err(kept("it's encrypted".into(), false));
    }
    if let Some(why) = listing.unsafe_entry() {
        return Err(kept(format!("expanding looks unsafe: {why}"), false));
    }
    if anyway {
        return Ok(());
    }
    if let Some(reason) = plan::automatic(inside) {
        return Err(kept(reason, true));
    }
    if let Some(why) = listing.too_big(&Limits::DEFAULT) {
        return Err(kept(format!("expanding looks too big: {why}"), true));
    }
    Ok(())
}

/// Expands into `temp` and moves what it held into the item's folder, rewriting the card for
/// it. Returns its path, its kind, what was expanded, and where it was staged (both relative
/// to the archive root).
fn place(
    archive: &Archive,
    item: &ItemRef,
    file: &mut CardFile,
    source: &Path,
    temp: &Path,
    anyway: bool,
) -> Result<(String, Kind, super::Expanded, String), ExpandError> {
    let root = archive.root();
    let format = super::inspect(source)
        .map(|i| i.format)
        .ok_or_else(|| ExpandError::NotCompressed(source.display().to_string()))?;
    let limits = (!anyway).then_some(&Limits::DEFAULT);
    let expanded = super::expand(source, format, temp, limits).map_err(ExpandError::Expand)?;
    let rules = archive.rules().map_err(|e| ExpandError::Io {
        path: root.join("_System/rules"),
        source: std::io::Error::other(e.to_string()),
    })?;
    let detected = rules.detect(&expanded.path).map_err(io(&expanded.path))?;
    let (folder_path, _) = item.card.rsplit_once('/').unwrap_or(("", &item.card));
    let folder = root.join(folder_path);
    let wanted = expanded
        .path
        .file_name()
        .map(|n| crate::names::portable(&n.to_string_lossy()))
        .unwrap_or_default();
    let name = crate::ingest::filing::free_name(&folder, &wanted).map_err(io(&folder))?;
    let target = folder.join(&name);
    let staged = relative(root, &expanded.path);
    std::fs::rename(&expanded.path, &target).map_err(io(&target))?;

    let original = std::fs::metadata(source).map_err(io(source))?;
    let card = file.card_mut();
    card.expanded_from = Some(ExpandedFrom {
        name: source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        size: original.len(),
        checksum: card
            .checksum
            .clone()
            .map_or_else(|| Checksum::of_file(source), Ok)
            .map_err(io(source))?,
        extra: Extra::new(),
    });
    card.kind = detected.kind;
    card.sealed = detected.sealed;
    card.item = Some(name.clone());
    if detected.kind == Kind::File {
        card.checksum = Some(Checksum::of_file(&target).map_err(io(&target))?);
        card.size = Some(std::fs::metadata(&target).map_err(io(&target))?.len());
        card.manifest = None;
    } else {
        let manifest = Manifest::build(&target, item.id, &rules, None)?;
        let path = format!("_System/manifests/{}.tsv", item.id);
        let written = root.join(&path);
        if let Some(parent) = written.parent() {
            std::fs::create_dir_all(parent).map_err(io(parent))?;
        }
        std::fs::write(&written, manifest.to_tsv()).map_err(io(&written))?;
        card.checksum = None;
        card.size = Some(manifest.total_size());
        card.manifest = Some(ManifestRef {
            checksum: manifest.checksum(),
            path,
            extra: Extra::new(),
        });
    }
    Ok((join(folder_path, &name), detected.kind, expanded, staged))
}

fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{folder}/{name}")
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn remove(path: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compressed::{Entry, EntryKind, Inside, Listing};
    use crate::detect::Compressed;

    fn inside(path: &str, encrypted: bool) -> Inside {
        Inside {
            format: Compressed::Zip,
            listing: Some(Listing {
                format: Compressed::Zip,
                compressed_size: 10,
                entries: vec![Entry {
                    path: path.into(),
                    kind: EntryKind::File,
                    size: Some(1),
                    modified: None,
                    executable: false,
                    crc32: None,
                    encrypted,
                }],
                truncated: false,
            }),
            unread: None,
            no_reader: false,
            split: false,
        }
    }

    #[test]
    fn software_needs_anyway_and_encrypted_archives_never_expand() {
        assert!(check(&inside("notes.txt", false), false).is_ok());
        let software = inside("Setup.exe", false);
        assert!(matches!(
            check(&software, false),
            Err(ExpandError::Kept {
                overridable: true,
                ..
            })
        ));
        assert!(check(&software, true).is_ok());
        assert!(matches!(
            check(&inside("notes.txt", true), true),
            Err(ExpandError::Kept {
                overridable: false,
                ..
            })
        ));
        assert!(matches!(
            check(&inside("../up", false), true),
            Err(ExpandError::Kept {
                overridable: false,
                ..
            })
        ));
    }
}
