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
    /// How many days a skipped question rests before it's asked again.
    SkipDays,
    /// How changes made outside Carrel are handled.
    OutsideEdits,
    /// What happens to compressed files at ingest.
    CompressedFiles,
    /// The model every device embeds with for search. It can be chosen once; switching
    /// models means re-embedding the archive (`docs/design/search.md`).
    EmbeddingModel,
}

/// The embedding models an archive can choose: the catalog's, the default first.
const EMBEDDING_MODELS: [&str; 3] = [
    crate::models::DEFAULT_EMBEDDING_MODEL,
    "qwen3-embedding-0.6b",
    "apple-nl-sentence-en",
];

impl Key {
    /// Every setting, in the order they're shown.
    pub const ALL: [Self; 6] = [
        Self::Originals,
        Self::CompressedFiles,
        Self::CrowdedItems,
        Self::SkipDays,
        Self::OutsideEdits,
        Self::EmbeddingModel,
    ];

    /// Its key in `settings.json`, as FORMAT.md §7.2 writes it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Originals => "ingest.originals",
            Self::CrowdedItems => "signals.crowded_items",
            Self::SkipDays => "questions.skip_days",
            Self::OutsideEdits => "outside_edits",
            Self::CompressedFiles => "compressed_files",
            Self::EmbeddingModel => "embedding.model",
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
            Self::SkipDays => "How many days a skipped question rests before Carrel asks it again",
            Self::OutsideEdits => {
                "What to do with changes made outside Carrel: flag them for review, record them as edits, or offer to undo them"
            }
            Self::CompressedFiles => {
                "What happens to zip and other compressed files when they're approved: expanded so what's inside can be browsed, kept compressed, or asked about each time"
            }
            Self::EmbeddingModel => {
                "The model every device uses to search by meaning; chosen once, as switching re-embeds the archive"
            }
        }
    }

    /// The values it can take, when they're a fixed list.
    #[must_use]
    pub fn choices(self) -> Option<&'static [&'static str]> {
        match self {
            Self::Originals => Some(&["move", "keep"]),
            Self::OutsideEdits => Some(&["flag", "reconcile", "drift"]),
            Self::CompressedFiles => Some(&["expand", "keep", "ask"]),
            Self::EmbeddingModel => Some(&EMBEDDING_MODELS),
            Self::CrowdedItems | Self::SkipDays => None,
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
            Self::SkipDays => settings.questions.skip_days.to_string(),
            Self::OutsideEdits => match settings.outside_edits {
                OutsideEdits::Flag => "flag".into(),
                OutsideEdits::Reconcile => "reconcile".into(),
                OutsideEdits::Drift => "drift".into(),
            },
            Self::CompressedFiles => settings.compressed_files.as_str().into(),
            Self::EmbeddingModel => settings.embedding_model().unwrap_or("none").to_owned(),
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
            Self::SkipDays => {
                settings.questions.skip_days =
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
            Self::CompressedFiles => {
                settings.compressed_files =
                    crate::compressed::plan::Choice::parse(value).ok_or_else(wrong)?;
            }
            Self::EmbeddingModel => {
                if !EMBEDDING_MODELS.contains(&value) {
                    return Err(wrong());
                }
                match settings.embedding_model() {
                    Some(chosen) if chosen != value => {
                        return Err(format!(
                            "this archive is searched with {chosen}; switching models re-embeds the archive, so use `carrel index model {value}`"
                        ));
                    }
                    _ => settings.set_embedding_model(value),
                }
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
    let commit = save(
        archive,
        &settings,
        &format!("Set {} to {to}", key.as_str()),
        lease,
    )?;
    Ok(Changed {
        key,
        from,
        to,
        commit,
    })
}

/// Writes `settings` as `archive`'s and records it in history as `summary`, holding `lease`.
/// Returns the history commit, if anything changed.
///
/// # Errors
///
/// [`SettingsError`] if writing or committing fails.
pub fn save(
    archive: &Archive,
    settings: &Settings,
    summary: &str,
    lease: &WriterLease,
) -> Result<Option<String>, SettingsError> {
    let path = archive.root().join("_System/settings.json");
    let text = serde_json::to_string_pretty(settings)
        .map_err(std::io::Error::other)
        .map_err(SettingsError::Io)?;
    std::fs::write(&path, text + "\n").map_err(SettingsError::Io)?;
    let change = Change::new(
        Operation::Edit,
        summary,
        lease.info().person,
        lease.info().device,
    );
    let reopened = Archive::open(archive.root()).map_err(SettingsError::Archive)?;
    History::open(&reopened)
        .map_err(SettingsError::History)?
        .commit(&reopened, &change, lease)
        .map_err(SettingsError::History)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedding_models_are_the_catalogs() {
        let catalog: Vec<&str> = crate::models::embedding_models().map(|m| m.name).collect();
        assert_eq!(catalog, EMBEDDING_MODELS);
    }

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
        assert_eq!(Key::EmbeddingModel.get(&settings), "none");
        assert!(Key::EmbeddingModel.set(&mut settings, "word2vec").is_err());
        Key::EmbeddingModel
            .set(&mut settings, "qwen3-embedding-0.6b")
            .unwrap();
        assert_eq!(settings.embedding_model(), Some("qwen3-embedding-0.6b"));
        Key::EmbeddingModel
            .set(&mut settings, "qwen3-embedding-0.6b")
            .unwrap();
        let switch = Key::EmbeddingModel
            .set(&mut settings, "embeddinggemma-300m")
            .unwrap_err();
        assert!(switch.contains("re-embeds"), "{switch}");
        assert_eq!(Key::parse("embedding.model"), Some(Key::EmbeddingModel));
        assert_eq!(Key::parse("outside_edits"), Some(Key::OutsideEdits));
        assert_eq!(Key::parse("nope"), None);
    }
}
