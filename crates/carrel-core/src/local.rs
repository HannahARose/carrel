//! Where Carrel keeps data that belongs to this device rather than to an archive: the
//! device's identity and preferences, and downloaded models.

use std::path::{Path, PathBuf};

use crate::card::{Extra, Id};
use crate::coordinator::Holder;

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

/// Why this device's identity or preferences couldn't be read or written.
#[derive(Debug)]
pub enum LocalError {
    /// There's no home folder, so no configuration folder.
    NoHome,
    /// A file couldn't be read or written.
    Io {
        /// The file or folder.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
    /// A file isn't valid JSON of the expected shape.
    Invalid {
        /// The file.
        path: PathBuf,
        /// What's wrong with it.
        message: String,
    },
}

impl std::fmt::Display for LocalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoHome => f.write_str("can't find your home folder; set CARREL_CONFIG_DIR"),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Invalid { path, message } => write!(f, "{}: {message}", path.display()),
        }
    }
}

impl std::error::Error for LocalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// The configuration folder ([`config_dir`]), or why there's none.
///
/// # Errors
///
/// [`LocalError::NoHome`] if there's no folder to put it in.
pub fn config_folder() -> Result<PathBuf, LocalError> {
    config_dir().ok_or(LocalError::NoHome)
}

/// Who is making changes from this device, recorded in every history commit
/// (`identity.json` in the configuration folder).
///
/// Created on first use. Until shared archives arrive there is one person, the owner; later
/// versions will share the person's identifier between their devices.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    ///
    /// # Errors
    ///
    /// [`LocalError`] if it can't be read or saved.
    pub fn load_or_create() -> Result<Self, LocalError> {
        Self::load_or_create_in(&config_folder()?)
    }

    /// The identity kept in the configuration folder `dir`, created and saved if there isn't
    /// one yet.
    ///
    /// # Errors
    ///
    /// [`LocalError`] if it can't be read or saved.
    pub fn load_or_create_in(dir: &Path) -> Result<Self, LocalError> {
        let path = dir.join("identity.json");
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
    ///
    /// # Errors
    ///
    /// [`LocalError`] if it can't be read.
    pub fn load() -> Result<Option<Self>, LocalError> {
        read(&config_folder()?.join("identity.json"))
    }

    /// Who takes the writer lease from this device.
    #[must_use]
    pub fn holder(&self) -> Holder {
        Holder {
            person: self.person,
            device: self.device,
        }
    }
}

/// This device's preferences (`cli.json` in the configuration folder), shared by the CLI and
/// the apps.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
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
    /// The archives opened or created on this device, most recent first, at most
    /// [`RECENT_ARCHIVES`], so the interactive mode and the apps can offer them again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent: Vec<PathBuf>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

/// How many archives [`Prefs::recent`] remembers.
pub const RECENT_ARCHIVES: usize = 10;

impl Prefs {
    /// Notes that the archive at `root` was opened or created, putting it first among the
    /// recent archives.
    pub fn note_archive(&mut self, root: &Path) {
        self.recent.retain(|r| r != root);
        self.recent.insert(0, root.to_owned());
        self.recent.truncate(RECENT_ARCHIVES);
    }

    /// The archives to offer: the one chosen with `carrel open`, then the recent ones, each
    /// once, leaving out those no longer there.
    #[must_use]
    pub fn known_archives(&self) -> Vec<PathBuf> {
        let mut known: Vec<PathBuf> = Vec::new();
        for root in self.archive.iter().chain(&self.recent) {
            if !known.contains(root) && root.join("_System/archive.json").is_file() {
                known.push(root.clone());
            }
        }
        known
    }

    /// The saved preferences, or the defaults.
    ///
    /// # Errors
    ///
    /// [`LocalError`] if they can't be read.
    pub fn load() -> Result<Self, LocalError> {
        Self::load_from(&config_folder()?)
    }

    /// The preferences saved in the configuration folder `dir`, or the defaults.
    ///
    /// # Errors
    ///
    /// [`LocalError`] if they can't be read.
    pub fn load_from(dir: &Path) -> Result<Self, LocalError> {
        Ok(read(&dir.join("cli.json"))?.unwrap_or_default())
    }

    /// Saves the preferences.
    ///
    /// # Errors
    ///
    /// [`LocalError`] if they can't be written.
    pub fn save(&self) -> Result<(), LocalError> {
        self.save_to(&config_folder()?)
    }

    /// Saves the preferences in the configuration folder `dir`.
    ///
    /// # Errors
    ///
    /// [`LocalError`] if they can't be written.
    pub fn save_to(&self, dir: &Path) -> Result<(), LocalError> {
        write(&dir.join("cli.json"), self)
    }
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, LocalError> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| LocalError::Invalid {
                path: path.to_owned(),
                message: e.to_string(),
            }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(LocalError::Io {
            path: path.to_owned(),
            source,
        }),
    }
}

fn write(path: &Path, value: &impl serde::Serialize) -> Result<(), LocalError> {
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| LocalError::Io { path, source }
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io(parent))?;
    }
    let text = serde_json::to_string_pretty(value).expect("settings always serialize") + "\n";
    std::fs::write(path, text).map_err(io(path))
}

#[cfg(test)]
mod tests {
    use super::{Prefs, RECENT_ARCHIVES};

    #[test]
    fn recent_archives_are_kept_newest_first_and_offered_while_they_exist() {
        let dir = tempfile::tempdir().unwrap();
        let archive = |name: &str| {
            let root = dir.path().join(name);
            std::fs::create_dir_all(root.join("_System")).unwrap();
            std::fs::write(root.join("_System/archive.json"), "{}").unwrap();
            root
        };
        let (a, b) = (archive("a"), archive("b"));
        let mut prefs = Prefs::default();
        prefs.note_archive(&a);
        prefs.note_archive(&b);
        prefs.note_archive(&a);
        assert_eq!(prefs.recent, vec![a.clone(), b.clone()]);

        // The one chosen with `carrel open` comes first; gone ones are left out.
        prefs.archive = Some(b.clone());
        prefs.note_archive(&dir.path().join("gone"));
        assert_eq!(prefs.known_archives(), vec![b.clone(), a.clone()]);

        for i in 0..RECENT_ARCHIVES + 2 {
            prefs.note_archive(&dir.path().join(i.to_string()));
        }
        assert_eq!(prefs.recent.len(), RECENT_ARCHIVES);

        // Saved and read back with the other preferences.
        prefs.save_to(dir.path()).unwrap();
        assert_eq!(Prefs::load_from(dir.path()).unwrap().recent, prefs.recent);
    }
}
