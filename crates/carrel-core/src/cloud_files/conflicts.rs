//! iCloud's conflicted copies: when two devices change the same file before syncing, iCloud
//! Drive keeps both, the other as `name 2.ext` beside it (other sync services write
//! `name (… conflicted copy …).ext`). Carrel finds them and reports them for the owner to
//! choose ([`resolve`]), never picking one silently.
//!
//! With one writer and per-device inboxes they should be rare. A numbered copy only counts
//! among Carrel's own files (cards, design documents and everything in `_System`), where Carrel
//! never makes such names; in an item's own contents `photo 2.jpg` is just a photo.

use std::path::Path;

use super::{SyncError, io, relative};
use crate::archive::Archive;
use crate::coordinator::WriterLease;

/// A conflicted copy and the file it conflicts with, both relative to the archive root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictedCopy {
    /// The copy iCloud made.
    pub copy: String,
    /// The file it is a version of.
    pub original: String,
}

/// Which version the owner keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// The file at its own name; the copy goes to the Trash.
    Original,
    /// The copy, which takes the original's name; the original goes to the Trash.
    Copy,
}

/// The conflicted copies in the archive at `root`, sorted by the copy's path.
#[must_use]
pub fn conflicts(root: &Path) -> Vec<ConflictedCopy> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || super::is_local_only(&name) {
                continue;
            }
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(path);
                continue;
            }
            let rel = relative(root, &path);
            let carrels = rel.starts_with("_System/")
                || name.starts_with('_')
                || Path::new(&name)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("md"));
            let Some(original) = original_name(&name, carrels) else {
                continue;
            };
            if dir.join(&original).exists() {
                found.push(ConflictedCopy {
                    copy: rel,
                    original: relative(root, &dir.join(original)),
                });
            }
        }
    }
    found.sort_by(|a, b| a.copy.cmp(&b.copy));
    found
}

/// The name a conflicted copy named `name` is a copy of: `notes.md` for `notes 2.md` (only
/// when `numbered` copies count) or for `notes (Mac's conflicted copy 2026-11-01).md`.
fn original_name(name: &str, numbered: bool) -> Option<String> {
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    };
    if let Some(open) = stem.rfind(" (")
        && stem.ends_with(')')
        && stem[open..].to_lowercase().contains("conflicted copy")
    {
        return Some(format!("{}{ext}", &stem[..open]));
    }
    if !numbered {
        return None;
    }
    let (base, number) = stem.rsplit_once(' ')?;
    let n: u32 = number.parse().ok()?;
    (n >= 2 && !base.is_empty() && number.len() <= 3).then(|| format!("{base}{ext}"))
}

/// Resolves the conflicted copy `copy` (relative to the archive root) by keeping one version,
/// the other going to the Trash, and records it as one change, holding `lease`. Returns the
/// history commit, if anything history tracks changed.
///
/// # Errors
///
/// [`SyncError::Invalid`] if `copy` isn't a conflicted copy, or an error moving the files or
/// recording the change.
pub fn resolve(
    archive: &Archive,
    copy: &str,
    keep: Keep,
    lease: &WriterLease,
) -> Result<Option<String>, SyncError> {
    let copy = copy.trim().trim_matches('/');
    let found = conflicts(archive.root())
        .into_iter()
        .find(|c| c.copy == copy)
        .ok_or_else(|| SyncError::Invalid(format!("{copy} isn't a conflicted copy")))?;
    let root = archive.root();
    let (copy_path, original_path) = (root.join(&found.copy), root.join(&found.original));
    let summary = match keep {
        Keep::Original => {
            crate::trash::put(&copy_path).map_err(io(&copy_path))?;
            format!("Keep {} over its conflicted copy", found.original)
        }
        Keep::Copy => {
            crate::trash::put(&original_path).map_err(io(&original_path))?;
            std::fs::rename(&copy_path, &original_path).map_err(io(&copy_path))?;
            format!("Keep the conflicted copy of {}", found.original)
        }
    };
    let change = crate::history::Change::new(
        crate::history::Operation::Edit,
        summary,
        lease.info().person,
        lease.info().device,
    );
    let archive = Archive::open(root)?;
    Ok(crate::history::History::open(&archive)?.commit(&archive, &change, lease)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_of_conflicted_copies_lead_to_their_originals() {
        assert_eq!(original_name("notes 2.md", true).unwrap(), "notes.md");
        assert_eq!(original_name("a.pdf 3.md", true).unwrap(), "a.pdf.md");
        assert_eq!(original_name("photo 2.jpg", false), None);
        assert_eq!(original_name("photo 1.jpg", true), None);
        assert_eq!(original_name("Chapter 2024.md", true), None);
        assert_eq!(
            original_name("plan (Mac's conflicted copy 2026-11-01).pdf", false).unwrap(),
            "plan.pdf"
        );
    }
}
