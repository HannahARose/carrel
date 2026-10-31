//! Putting files in the system Trash, where they can be recovered, rather than deleting them
//! outright: on macOS with the system's `trash` program, elsewhere in the freedesktop.org
//! Trash in the home folder.
//!
//! `CARREL_TRASH_DIR`, or [`redirect`], sends them to a folder instead, so tests and sandboxes
//! never touch the person's own Trash.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The folder [`redirect`] chose, if any.
static REDIRECTED: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Sends everything put in the Trash from now on to `folder` instead, or back to the system
/// Trash with `None`. For tests and sandboxes.
pub fn redirect(folder: Option<PathBuf>) {
    *REDIRECTED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = folder;
}

/// Where things go instead of the system Trash, if somewhere: [`redirect`]'s folder, then
/// `CARREL_TRASH_DIR`.
fn redirected() -> Option<PathBuf> {
    REDIRECTED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .or_else(|| std::env::var_os("CARREL_TRASH_DIR").map(PathBuf::from))
}

/// Puts the file or folder at `path` in the Trash.
///
/// # Errors
///
/// Any error moving it, or [`std::io::ErrorKind::Unsupported`] where there's no Trash to put
/// it in. It's left where it was.
pub fn put(path: &Path) -> std::io::Result<()> {
    if let Some(folder) = redirected() {
        return move_into(path, &folder);
    }
    system(path)
}

#[cfg(target_os = "macos")]
fn system(path: &Path) -> std::io::Result<()> {
    let program = Path::new("/usr/bin/trash");
    if !program.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "this version of macOS has no trash program",
        ));
    }
    let output = std::process::Command::new(program).arg(path).output()?;
    if output.status.success() && std::fs::symlink_metadata(path).is_err() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "trash couldn't move it: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "ios")))]
fn system(path: &Path) -> std::io::Result<()> {
    let trash = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| crate::local::home_dir().map(|h| h.join(".local/share")))
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "no home folder for a Trash",
            )
        })?
        .join("Trash");
    let files = trash.join("files");
    let info = trash.join("info");
    std::fs::create_dir_all(&files)?;
    std::fs::create_dir_all(&info)?;
    let name = free_name(&files, path)?;
    let absolute = std::fs::canonicalize(path)?;
    let when = jiff::Zoned::now().strftime("%Y-%m-%dT%H:%M:%S").to_string();
    std::fs::write(
        info.join(format!("{name}.trashinfo")),
        format!(
            "[Trash Info]\nPath={}\nDeletionDate={when}\n",
            encode(&absolute.to_string_lossy())
        ),
    )?;
    move_to(path, &files.join(&name))
}

#[cfg(not(any(target_os = "macos", all(unix, not(target_os = "ios")))))]
fn system(_path: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Carrel can't use the Trash on this system yet",
    ))
}

/// Percent-encodes a path for a `.trashinfo` file.
#[cfg(all(unix, not(target_os = "macos"), not(target_os = "ios")))]
fn encode(path: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            write!(out, "%{byte:02X}").expect("writing to a string");
        }
    }
    out
}

/// Moves `path` into `folder` under a name not yet taken there.
fn move_into(path: &Path, folder: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)?;
    let name = free_name(folder, path)?;
    move_to(path, &folder.join(name))
}

/// `path`'s name, or with ` 2`, ` 3`… added, whichever `folder` doesn't have yet.
fn free_name(folder: &Path, path: &Path) -> std::io::Result<String> {
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("no name to put in the Trash"))?
        .to_string_lossy()
        .into_owned();
    let mut candidate = name.clone();
    let mut n = 2;
    while std::fs::symlink_metadata(folder.join(&candidate)).is_ok() {
        candidate = format!("{name} {n}");
        n += 1;
    }
    Ok(candidate)
}

/// Moves `from` to `to`, copying and then removing when they're on different volumes.
fn move_to(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            copy(from, to)?;
            if std::fs::symlink_metadata(from)?.is_dir() {
                std::fs::remove_dir_all(from)
            } else {
                std::fs::remove_file(from)
            }
        }
        result => result,
    }
}

fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(from)?.is_dir() {
        std::fs::create_dir(to)?;
        for child in std::fs::read_dir(from)? {
            let child = child?;
            copy(&child.path(), &to.join(child.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_redirected_trash_keeps_both_of_two_same_named_files() {
        let dir = tempfile::tempdir().unwrap();
        let trash = dir.path().join("Trash");
        for _ in 0..2 {
            let file = dir.path().join("Export.zip");
            std::fs::write(&file, "zip").unwrap();
            move_into(&file, &trash).unwrap();
            assert!(!file.exists());
        }
        assert!(trash.join("Export.zip").is_file());
        assert!(trash.join("Export.zip 2").is_file());
    }
}
