//! 7z and RAR, through an installed `7z` or `unar` (`docs/design/compressed.md`).

use std::path::Path;

use super::{Error, Listing};
use crate::detect::Compressed;

/// An installed program that reads 7z and RAR files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    /// Where it is.
    pub path: std::path::PathBuf,
}

/// The installed tool, if there is one.
#[must_use]
pub fn find() -> Option<Tool> {
    None
}

/// What a 7z or RAR file holds, listed by an installed tool.
///
/// # Errors
///
/// [`Error::NoReader`] when no tool is installed.
pub fn list(_path: &Path, format: Compressed) -> Result<Listing, Error> {
    Err(Error::NoReader(format))
}

/// Up to `limit` bytes of one file inside, read by an installed tool.
///
/// # Errors
///
/// [`Error::NoReader`] when no tool is installed.
pub fn read_entry(
    _path: &Path,
    format: Compressed,
    _inner: &str,
    _limit: u64,
) -> Result<Vec<u8>, Error> {
    Err(Error::NoReader(format))
}

/// Expands into `base` with an installed tool.
///
/// # Errors
///
/// [`Error::NoReader`] when no tool is installed.
pub fn expand(_path: &Path, format: Compressed, _base: &Path) -> Result<(), Error> {
    Err(Error::NoReader(format))
}
