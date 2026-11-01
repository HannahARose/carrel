//! What happens to a checkout's working copy once it's returned, by `checkout.after_return`
//! (FORMAT.md §7.2, `docs/design/checkout.md`): it goes to the system Trash, never deleted
//! outright, it's kept where it is, or the owner is asked.

use std::path::Path;

use crate::documents::Settings;

/// What `checkout.after_return` says to do with a returned working copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterReturn {
    /// Ask the owner each time.
    Ask,
    /// Put it in the system Trash.
    Trash,
    /// Leave it where it is.
    Keep,
}

impl AfterReturn {
    /// What `settings` says. An unknown value asks, which changes nothing without the owner.
    #[must_use]
    pub fn of(settings: &Settings) -> Self {
        match settings.checkout.after_return.as_str() {
            "trash" => Self::Trash,
            "keep" => Self::Keep,
            _ => Self::Ask,
        }
    }
}

/// Why a working copy couldn't go to the Trash.
#[derive(Debug)]
pub enum TrashError {
    /// It's the archive, or inside it, which the Trash never takes from.
    InArchive,
    /// It isn't there any more.
    Missing,
    /// The Trash refused it.
    Io(std::io::Error),
}

impl std::fmt::Display for TrashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InArchive => f.write_str("the working copy is inside the archive, so it's kept"),
            Self::Missing => f.write_str("the working copy isn't there any more"),
            Self::Io(e) => write!(f, "couldn't put the working copy in the Trash: {e}"),
        }
    }
}

impl std::error::Error for TrashError {}

/// Puts the returned working copy at `working` in the system Trash, where it can be
/// recovered. Never anything inside the archive at `archive_root`.
///
/// # Errors
///
/// [`TrashError`] if it's inside the archive, missing, or the Trash refuses it.
pub fn trash_working_copy(archive_root: &Path, working: &Path) -> Result<(), TrashError> {
    if !working.exists() {
        return Err(TrashError::Missing);
    }
    let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_owned());
    let (root, working_full) = (canonical(archive_root), canonical(working));
    if working_full.starts_with(&root) || root.starts_with(&working_full) {
        return Err(TrashError::InArchive);
    }
    crate::trash::put(working).map_err(TrashError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_chooses_and_the_trash_never_takes_from_the_archive() {
        let mut settings = Settings::default();
        assert_eq!(AfterReturn::of(&settings), AfterReturn::Ask);
        settings.checkout.after_return = "trash".into();
        assert_eq!(AfterReturn::of(&settings), AfterReturn::Trash);
        settings.checkout.after_return = "keep".into();
        assert_eq!(AfterReturn::of(&settings), AfterReturn::Keep);

        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("trash");
        crate::trash::redirect(Some(bin.clone()));
        let archive = dir.path().join("archive");
        let inside = archive.join("Projects/thing");
        std::fs::create_dir_all(&inside).unwrap();
        assert!(matches!(
            trash_working_copy(&archive, &inside),
            Err(TrashError::InArchive)
        ));
        assert!(matches!(
            trash_working_copy(&inside, &archive),
            Err(TrashError::InArchive)
        ));
        let working = dir.path().join("Desktop/thing");
        std::fs::create_dir_all(&working).unwrap();
        std::fs::write(working.join("a.txt"), "a").unwrap();
        trash_working_copy(&archive, &working).unwrap();
        assert!(!working.exists());
        assert!(bin.join("thing/a.txt").exists());
        assert!(matches!(
            trash_working_copy(&archive, &working),
            Err(TrashError::Missing)
        ));
        crate::trash::redirect(None);
    }
}
