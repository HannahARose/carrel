//! Where Carrel keeps data that belongs to this device rather than to an archive: the
//! device's identity and preferences, and downloaded models.

use std::path::PathBuf;

/// The configuration folder: `CARREL_CONFIG_DIR` if set, otherwise
/// `~/Library/Application Support/Carrel` on macOS and `$XDG_CONFIG_HOME/carrel` (or
/// `~/.config/carrel`) elsewhere. `None` if there is no home folder to put it in.
#[must_use]
pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CARREL_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    if cfg!(target_os = "macos") {
        return Some(home.join("Library/Application Support/Carrel"));
    }
    Some(
        std::env::var_os("XDG_CONFIG_HOME")
            .map_or_else(|| home.join(".config"), PathBuf::from)
            .join("carrel"),
    )
}

/// Where local models live: `CARREL_MODELS_DIR` if set, otherwise `models` in the
/// configuration folder.
#[must_use]
pub fn models_dir() -> Option<PathBuf> {
    std::env::var_os("CARREL_MODELS_DIR")
        .map(PathBuf::from)
        .or_else(|| config_dir().map(|dir| dir.join("models")))
}
