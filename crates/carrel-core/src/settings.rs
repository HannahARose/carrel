//! Changing an archive's shared settings (`_System/settings.json`, FORMAT.md §7.2) one at a
//! time, each as a history change so it can be undone (`docs/design/tui-ingest.md`).

use crate::archive::{Archive, ArchiveError};
use crate::coordinator::WriterLease;
use crate::documents::{Originals, OutsideEdits, Settings};
use crate::history::{Change, History, HistoryError, Operation};

/// A setting the owner can change from Carrel's interfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// What happens to an item's original when its proposal is approved.
    Originals,
    /// How many loose items make a collection crowded, as a restructure signal.
    CrowdedItems,
    /// How changes made outside Carrel are handled.
    OutsideEdits,
}

impl Key {
    /// Every setting, in the order they're shown.
    pub const ALL: [Self; 3] = [Self::Originals, Self::CrowdedItems, Self::OutsideEdits];

    /// Its key in `settings.json`, as FORMAT.md §7.2 writes it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Originals => "ingest.originals",
            Self::CrowdedItems => "signals.crowded_items",
            Self::OutsideEdits => "outside_edits",
        }
    }

    /// The setting with this key.
    #[must_use]
    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == key)
    }

    /// What it does, in words.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Originals => {
                "What happens to an item's original when it's approved: moved into the archive, or copied and kept where it was"
            }
            Self::CrowdedItems => {
                "How many loose items make a collection with no subcollections crowded, a sign it may want splitting"
            }
            Self::OutsideEdits => {
                "What to do with changes made outside Carrel: flag them for review, record them as edits, or offer to undo them"
            }
        }
    }

    /// The values it can take, when they're a fixed list.
    #[must_use]
    pub fn choices(self) -> Option<&'static [&'static str]> {
        match self {
            Self::Originals => Some(&["move", "keep"]),
            Self::OutsideEdits => Some(&["flag", "reconcile", "drift"]),
            Self::CrowdedItems => None,
        }
    }

    /// Its value in `settings`.
    #[must_use]
    pub fn get(self, settings: &Settings) -> String {
        match self {
            Self::Originals => match settings.ingest.originals {
                Originals::Move => "move".into(),
                Originals::Keep => "keep".into(),
            },
            Self::CrowdedItems => settings.signals.crowded_items.to_string(),
            Self::OutsideEdits => match settings.outside_edits {
                OutsideEdits::Flag => "flag".into(),
                OutsideEdits::Reconcile => "reconcile".into(),
                OutsideEdits::Drift => "drift".into(),
            },
        }
    }

    /// Sets it in `settings` to `value`.
    ///
    /// # Errors
    ///
    /// A message saying what values it takes, if `value` isn't one.
    pub fn set(self, settings: &mut Settings, value: &str) -> Result<(), String> {
        let value = value.trim();
        let wrong = || match self.choices() {
            Some(choices) => format!("{} is one of {}", self.as_str(), choices.join(", ")),
            None => format!("{} is a whole number, at least 1", self.as_str()),
        };
        match self {
            Self::Originals => {
                settings.ingest.originals = match value {
                    "move" => Originals::Move,
                    "keep" => Originals::Keep,
                    _ => return Err(wrong()),
                };
            }
            Self::CrowdedItems => {
                settings.signals.crowded_items =
                    value.parse().ok().filter(|n| *n >= 1).ok_or_else(wrong)?;
            }
            Self::OutsideEdits => {
                settings.outside_edits = match value {
                    "flag" => OutsideEdits::Flag,
                    "reconcile" => OutsideEdits::Reconcile,
                    "drift" => OutsideEdits::Drift,
                    _ => return Err(wrong()),
                };
            }
        }
        Ok(())
    }
}

/// Why a setting couldn't be changed.
#[derive(Debug)]
pub enum SettingsError {
    /// The value isn't one the setting takes.
    Invalid(String),
    /// `settings.json` couldn't be written.
    Io(std::io::Error),
    /// The archive couldn't be read again.
    Archive(ArchiveError),
    /// The change couldn't be recorded.
    History(HistoryError),
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => f.write_str(message),
            Self::Io(e) => write!(f, "_System/settings.json: {e}"),
            Self::Archive(e) => write!(f, "{e}"),
            Self::History(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SettingsError {}

/// A setting that was changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    /// The setting.
    pub key: Key,
    /// Its value before.
    pub from: String,
    /// Its value now.
    pub to: String,
    /// The history commit that records it, if anything changed.
    pub commit: Option<String>,
}

/// Sets `key` to `value` in `archive`'s settings, holding `lease`, and records it in history.
/// Setting the value it already has changes nothing.
///
/// # Errors
///
/// [`SettingsError`] if the value is wrong, or writing or committing fails.
pub fn change(
    archive: &Archive,
    key: Key,
    value: &str,
    lease: &WriterLease,
) -> Result<Changed, SettingsError> {
    let mut settings = archive.settings().clone();
    let from = key.get(&settings);
    key.set(&mut settings, value)
        .map_err(SettingsError::Invalid)?;
    let to = key.get(&settings);
    if from == to {
        return Ok(Changed {
            key,
            from,
            to,
            commit: None,
        });
    }
    let path = archive.root().join("_System/settings.json");
    let text = serde_json::to_string_pretty(&settings)
        .map_err(std::io::Error::other)
        .map_err(SettingsError::Io)?;
    std::fs::write(&path, text + "\n").map_err(SettingsError::Io)?;
    let change = Change::new(
        Operation::Edit,
        format!("Set {} to {to}", key.as_str()),
        lease.info().person,
        lease.info().device,
    );
    let reopened = Archive::open(archive.root()).map_err(SettingsError::Archive)?;
    let commit = History::open(&reopened)
        .map_err(SettingsError::History)?
        .commit(&reopened, &change, lease)
        .map_err(SettingsError::History)?;
    Ok(Changed {
        key,
        from,
        to,
        commit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_read_and_take_only_their_values() {
        let mut settings = Settings::default();
        assert_eq!(Key::Originals.get(&settings), "move");
        Key::Originals.set(&mut settings, "keep").unwrap();
        assert_eq!(Key::Originals.get(&settings), "keep");
        assert!(
            Key::Originals
                .set(&mut settings, "copy")
                .unwrap_err()
                .contains("move, keep")
        );
        Key::CrowdedItems.set(&mut settings, " 40 ").unwrap();
        assert_eq!(settings.signals.crowded_items, 40);
        assert!(Key::CrowdedItems.set(&mut settings, "0").is_err());
        Key::OutsideEdits.set(&mut settings, "drift").unwrap();
        assert_eq!(Key::parse("outside_edits"), Some(Key::OutsideEdits));
        assert_eq!(Key::parse("nope"), None);
    }
}
