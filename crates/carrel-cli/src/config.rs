//! What the CLI keeps on this device, outside any archive: who is using it and which archive
//! to use by default.
//!
//! Files live in the configuration folder: `CARREL_CONFIG_DIR` if set, otherwise
//! `~/Library/Application Support/Carrel` on macOS and `$XDG_CONFIG_HOME/carrel` (or
//! `~/.config/carrel`) elsewhere. The apps share them, so they're read and written in
//! [`carrel_core::local`].

use std::path::PathBuf;

pub use carrel_core::local::{Identity, Prefs};

use crate::Failure;

/// The configuration folder ([`carrel_core::local::config_dir`]).
pub fn dir() -> Result<PathBuf, Failure> {
    Ok(carrel_core::local::config_folder()?)
}
