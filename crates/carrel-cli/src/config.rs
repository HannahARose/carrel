//! What the CLI keeps on this device, outside any archive: who is using it and which archive
//! to use by default.
//!
//! Files live in the configuration folder: `CARREL_CONFIG_DIR` if set, otherwise
//! `~/Library/Application Support/Carrel` on macOS and `$XDG_CONFIG_HOME/carrel` (or
//! `~/.config/carrel`) elsewhere.

use std::path::{Path, PathBuf};

use carrel_core::card::{Extra, Id};
use serde::{Deserialize, Serialize};

use crate::Failure;

/// The configuration folder ([`carrel_core::local::config_dir`]).
pub fn dir() -> Result<PathBuf, Failure> {
    carrel_core::local::config_dir()
        .ok_or_else(|| Failure::new("can't find your home folder; set CARREL_CONFIG_DIR"))
}

/// Who is making changes from this device, recorded in every history commit.
///
/// Created on first use. Until shared archives arrive there is one person, the owner; later
/// versions will share the person's identifier between their devices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    /// The person using this device.
    pub person: Id,
    /// This device.
    pub device: Id,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Identity {
    /// This device's identity, created and saved if there isn't one yet.
    pub fn load_or_create() -> Result<Self, Failure> {
        let path = dir()?.join("identity.json");
        if let Some(identity) = read(&path)? {
            return Ok(identity);
        }
        let identity = Self {
            person: Id::new_v7(),
            device: Id::new_v7(),
            extra: Extra::new(),
        };
        write(&path, &identity)?;
        Ok(identity)
    }

    /// This device's identity, if it has one.
    pub fn load() -> Result<Option<Self>, Failure> {
        read(&dir()?.join("identity.json"))
    }
}

/// The CLI's own preferences.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Prefs {
    /// The archive used when no other is given, set by `carrel open`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<PathBuf>,
    /// The file picker for choosing what to ingest: a known picker's name, any command
    /// (with `{out}` for the file to write paths to), or `builtin`. Unset, the first known
    /// picker installed is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picker: Option<String>,
    /// The editor for cards, before `$VISUAL` and `$EDITOR`; its words are split as a shell
    /// would.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
    /// The text model that drafts cards on this device, as `provider/offer` (such as
    /// `llama/qwen3.5-4b`). Unset, Carrel uses the best available by priority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The text model asked again when the first isn't sure where an item goes: a model
    /// name, or `off`. Unset, a recommended model that's installed, if there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_model: Option<String>,
    /// The folder files were last picked from, where picking starts next time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_folder: Option<PathBuf>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Prefs {
    /// The saved preferences, or the defaults.
    pub fn load() -> Result<Self, Failure> {
        Ok(read(&dir()?.join("cli.json"))?.unwrap_or_default())
    }

    /// Saves the preferences.
    pub fn save(&self) -> Result<(), Failure> {
        write(&dir()?.join("cli.json"), self)
    }
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, Failure> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| Failure::new(format!("{}: {e}", path.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Failure::io(path, &e)),
    }
}

fn write(path: &Path, value: &impl Serialize) -> Result<(), Failure> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Failure::io(parent, &e))?;
    }
    let text = serde_json::to_string_pretty(value).expect("settings always serialize") + "\n";
    std::fs::write(path, text).map_err(|e| Failure::io(path, &e))
}
