//! Checking items out and back in (`docs/design/checkout.md`): what an item looked like when
//! it went out, how its working copy differs now, and the lock that keeps the archive copy
//! from changing meanwhile.
//!
//! The lock is the files' own permissions: while an item is out, its archive copy is
//! read-only, so an editor opened on it by mistake can't save, and anything (Finder, `chmod`)
//! can undo it if something goes wrong. Carrel itself also refuses to change an item whose
//! card says it's checked out.

use std::path::{Path, PathBuf};

use crate::card::{Checksum, Id};
use crate::detect::Rules;
use crate::manifest::{Diff, Entry, Manifest, ManifestError};

/// Why an item couldn't be compared or locked.
#[derive(Debug)]
pub enum CheckoutError {
    /// Reading the item failed.
    Manifest(ManifestError),
    /// A file operation failed.
    Io {
        /// The path concerned.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The working copy isn't there.
    Missing(PathBuf),
}

impl std::fmt::Display for CheckoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manifest(e) => write!(f, "{e}"),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Missing(path) => write!(
                f,
                "the working copy isn't at {}; give its new place with --from",
                path.display()
            ),
        }
    }
}

impl std::error::Error for CheckoutError {}

impl From<ManifestError> for CheckoutError {
    fn from(e: ManifestError) -> Self {
        Self::Manifest(e)
    }
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> CheckoutError + '_ {
    move |source| CheckoutError::Io {
        path: path.to_owned(),
        source,
    }
}

/// What `path` holds now, as a manifest: every file in a folder or bundle, leaving out what
/// `rules` call regenerable, or a single file as one entry named after it.
///
/// # Errors
///
/// [`CheckoutError::Missing`] if nothing is at `path`, or an error reading it.
pub fn snapshot(path: &Path, item: Id, rules: &Rules) -> Result<Manifest, CheckoutError> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(CheckoutError::Missing(path.to_owned()));
        }
        Err(e) => return Err(io(path)(e)),
    };
    if meta.is_dir() {
        return Ok(Manifest::build(path, item, rules, None)?);
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let modified = meta.modified().map_err(io(path))?;
    Ok(Manifest {
        item,
        entries: vec![Entry {
            path: name,
            checksum: Checksum::of_file(path).map_err(io(path))?,
            size: meta.len(),
            modified: crate::manifest::timestamp(modified),
        }],
    })
}

/// How the working copy at `working` differs from the item as it was checked out
/// (`at_checkout`). For a single file, a different name counts as the same file changed.
///
/// # Errors
///
/// As [`snapshot`].
pub fn compare(
    at_checkout: &Manifest,
    working: &Path,
    rules: &Rules,
) -> Result<Diff, CheckoutError> {
    let mut now = snapshot(working, at_checkout.item, rules)?;
    if let ([then], [entry]) = (at_checkout.entries.as_slice(), now.entries.as_mut_slice()) {
        // A file checked out and renamed on the desk is still the same item.
        if !working.is_dir() {
            entry.path.clone_from(&then.path);
        }
    }
    Ok(at_checkout.diff(&now))
}

/// Makes the item at `path` read-only: every file, and for a folder every folder inside it
/// too, so nothing can be added, changed or removed. Symbolic links are left as they are.
///
/// # Errors
///
/// An error reading the item or changing a permission.
pub fn lock(path: &Path) -> Result<(), CheckoutError> {
    // Files first, then folders from the deepest up, so each step can still reach the next.
    let (dirs, files) = walk(path)?;
    for file in &files {
        set_writable(file, false)?;
    }
    for dir in dirs.iter().rev() {
        set_writable(dir, false)?;
    }
    Ok(())
}

/// Undoes [`lock`]: the owner can write to the item again.
///
/// # Errors
///
/// An error reading the item or changing a permission.
pub fn unlock(path: &Path) -> Result<(), CheckoutError> {
    // Folders first, from the top down, so their contents can be listed and changed.
    if path.is_dir() {
        set_writable(path, true)?;
    }
    let (dirs, files) = walk(path)?;
    for dir in &dirs {
        set_writable(dir, true)?;
    }
    for file in &files {
        set_writable(file, true)?;
    }
    Ok(())
}

/// Whether the item at `path` is locked: its top file or folder isn't writable.
///
/// # Errors
///
/// An error reading its metadata.
pub fn is_locked(path: &Path) -> Result<bool, CheckoutError> {
    let meta = std::fs::metadata(path).map_err(io(path))?;
    Ok(meta.permissions().readonly())
}

/// Every folder (top first, parents before children) and file under `path`, or `path` alone
/// if it's a file. Symbolic links aren't followed or included.
fn walk(path: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>), CheckoutError> {
    let meta = std::fs::symlink_metadata(path).map_err(io(path))?;
    if meta.file_type().is_symlink() {
        return Ok((Vec::new(), Vec::new()));
    }
    if !meta.is_dir() {
        return Ok((Vec::new(), vec![path.to_owned()]));
    }
    let mut dirs = vec![path.to_owned()];
    let mut files = Vec::new();
    let mut i = 0;
    while let Some(dir) = dirs.get(i).cloned() {
        i += 1;
        for entry in std::fs::read_dir(&dir).map_err(io(&dir))? {
            let entry = entry.map_err(io(&dir))?;
            let kind = entry.file_type().map_err(io(&dir))?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                dirs.push(entry.path());
            } else {
                files.push(entry.path());
            }
        }
    }
    Ok((dirs, files))
}

/// Gives or takes away write permission. On Unix only the write bits change, and giving it
/// back gives it to the owner alone; on Windows, the read-only attribute.
fn set_writable(path: &Path, writable: bool) -> Result<(), CheckoutError> {
    let mut permissions = std::fs::metadata(path).map_err(io(path))?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = permissions.mode();
        permissions.set_mode(if writable {
            mode | 0o200
        } else {
            mode & !0o222
        });
    }
    #[cfg(not(unix))]
    {
        // Folders on Windows ignore the attribute; their files carry the lock.
        #[allow(clippy::permissions_set_readonly_false)] // Windows has no world-writable
        permissions.set_readonly(!writable);
    }
    std::fs::set_permissions(path, permissions).map_err(io(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Rules {
        Rules::default()
    }

    #[test]
    fn a_folder_is_locked_inside_and_unlocked_again() {
        let dir = tempfile::tempdir().unwrap();
        let item = dir.path().join("project");
        std::fs::create_dir_all(item.join("src")).unwrap();
        std::fs::write(item.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(item.join("README.md"), "hi").unwrap();

        lock(&item).unwrap();
        assert!(is_locked(&item).unwrap());
        assert!(std::fs::write(item.join("README.md"), "changed").is_err());
        #[cfg(unix)]
        assert!(
            std::fs::write(item.join("src/new.rs"), "").is_err(),
            "nothing can be added either"
        );

        unlock(&item).unwrap();
        assert!(!is_locked(&item).unwrap());
        std::fs::write(item.join("README.md"), "changed").unwrap();
        std::fs::write(item.join("src/new.rs"), "").unwrap();
    }

    #[test]
    fn a_single_file_locks_too() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.md");
        std::fs::write(&file, "x").unwrap();
        lock(&file).unwrap();
        assert!(std::fs::write(&file, "y").is_err());
        unlock(&file).unwrap();
        std::fs::write(&file, "y").unwrap();
    }

    #[test]
    fn the_working_copy_is_compared_with_the_item_as_it_went_out() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("project");
        std::fs::create_dir_all(out.join("data")).unwrap();
        std::fs::write(out.join("a.txt"), "same").unwrap();
        std::fs::write(out.join("b.txt"), "before").unwrap();
        std::fs::write(out.join("c.txt"), "moving").unwrap();
        std::fs::write(out.join("gone.txt"), "bye").unwrap();
        let then = snapshot(&out, Id::new_v7(), &rules()).unwrap();

        std::fs::write(out.join("b.txt"), "after").unwrap();
        std::fs::rename(out.join("c.txt"), out.join("data/c.txt")).unwrap();
        std::fs::remove_file(out.join("gone.txt")).unwrap();
        std::fs::write(out.join("new.txt"), "hello").unwrap();
        let diff = compare(&then, &out, &rules()).unwrap();
        assert_eq!(diff.changed, ["b.txt"]);
        assert_eq!(
            diff.renamed,
            [("c.txt".to_owned(), "data/c.txt".to_owned())]
        );
        assert_eq!(diff.removed, ["gone.txt"]);
        assert_eq!(diff.added, ["new.txt"]);

        assert!(matches!(
            compare(&then, &dir.path().join("elsewhere"), &rules()),
            Err(CheckoutError::Missing(_))
        ));
    }

    #[test]
    fn a_renamed_single_file_is_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("draft.md");
        std::fs::write(&file, "one").unwrap();
        let then = snapshot(&file, Id::new_v7(), &rules()).unwrap();
        let moved = dir.path().join("draft (2).md");
        std::fs::rename(&file, &moved).unwrap();
        assert!(compare(&then, &moved, &rules()).unwrap().is_empty());
        std::fs::write(&moved, "two").unwrap();
        assert_eq!(
            compare(&then, &moved, &rules()).unwrap().changed,
            ["draft.md"]
        );
    }
}
