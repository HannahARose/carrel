//! Where Carrel keeps data that belongs to this device rather than to an archive: the
//! device's identity and preferences, and downloaded models.

use std::path::PathBuf;

/// The person's home folder: `HOME`, or on Windows `USERPROFILE`.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| {
            if cfg!(windows) {
                std::env::var_os("USERPROFILE")
            } else {
                None
            }
        })
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// The configuration folder: `CARREL_CONFIG_DIR` if set, otherwise
/// `~/Library/Application Support/Carrel` on macOS, `%APPDATA%\Carrel` on Windows, and
/// `$XDG_CONFIG_HOME/carrel` (or `~/.config/carrel`) elsewhere. `None` if there is no folder
/// to put it in.
#[must_use]
pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CARREL_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    if cfg!(windows) {
        return std::env::var_os("APPDATA")
            .filter(|d| !d.is_empty())
            .map(|d| PathBuf::from(d).join("Carrel"));
    }
    let home = home_dir()?;
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
