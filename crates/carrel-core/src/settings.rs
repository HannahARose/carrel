//! Changing an archive's shared settings (`_System/settings.json`, FORMAT.md §7.2) one at a
//! time, each as a history change so it can be undone (`docs/design/tui-ingest.md`).

use std::path::{Path, PathBuf};

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
    /// Whether the librarian may make the changes it's sure of without asking.
    LibrarianChanges,
    /// How sure the librarian must be to make a change on its own.
    LibrarianConfidence,
    /// Where items are checked out to when no folder is given.
    CheckoutDestination,
    /// What happens to a working copy once it's returned.
    CheckoutAfterReturn,
    /// How many minutes without changes end a work session.
    IdleMinutes,
    /// Which reminders are on.
    Reminders,
}

/// The embedding models an archive can choose: the catalog's, the default first.
const EMBEDDING_MODELS: [&str; 3] = [
    crate::models::DEFAULT_EMBEDDING_MODEL,
    "qwen3-embedding-0.6b",
    "apple-nl-sentence-en",
];

/// The reminders an archive can turn on (FORMAT.md §7.2), in the order they're written.
pub const REMINDERS: [&str; 4] = ["idle", "weekly", "menu-bar", "due-date"];

/// What `checkout.destination` says for the Desktop.
const DESKTOP: &str = "desktop";

impl Key {
    /// Every setting, in the order they're shown.
    pub const ALL: [Self; 12] = [
        Self::Originals,
        Self::CompressedFiles,
        Self::CrowdedItems,
        Self::SkipDays,
        Self::OutsideEdits,
        Self::CheckoutDestination,
        Self::CheckoutAfterReturn,
        Self::IdleMinutes,
        Self::Reminders,
        Self::EmbeddingModel,
        Self::LibrarianChanges,
        Self::LibrarianConfidence,
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
            Self::LibrarianChanges => "librarian.changes",
            Self::LibrarianConfidence => "librarian.confidence",
            Self::CheckoutDestination => "checkout.destination",
            Self::CheckoutAfterReturn => "checkout.after_return",
            Self::IdleMinutes => "sessions.idle_minutes",
            Self::Reminders => "reminders",
        }
    }

    /// Its name, for people.
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Originals => "Originals",
            Self::CrowdedItems => "Crowded collections",
            Self::SkipDays => "Skipped questions rest",
            Self::OutsideEdits => "Changes made outside Carrel",
            Self::CompressedFiles => "Compressed files",
            Self::EmbeddingModel => "Embedding model",
            Self::LibrarianChanges => "Librarian's own changes",
            Self::LibrarianConfidence => "Librarian's confidence",
            Self::CheckoutDestination => "Check out to",
            Self::CheckoutAfterReturn => "After a return",
            Self::IdleMinutes => "Work sessions end after",
            Self::Reminders => "Reminders",
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
            Self::LibrarianChanges => {
                "Whether the librarian may refile items, correct cards, rename items and create collections on its own when it's sure, each change undoable; off, it only suggests them"
            }
            Self::LibrarianConfidence => {
                "How sure the librarian must be, from 0.5 to 1, to make a change on its own"
            }
            Self::CheckoutDestination => {
                "Where items are checked out to when no folder is given: the Desktop, or a folder"
            }
            Self::CheckoutAfterReturn => {
                "What happens to the working copy once it's returned: ask each time, move it to the trash, or keep it"
            }
            Self::IdleMinutes => {
                "How many minutes without changes end a work session, which History records as one"
            }
            Self::Reminders => {
                "Which reminders are on: when work goes idle, a weekly review, in the menu bar, and when a checkout is due back"
            }
        }
    }

    /// The values it takes, in words, as a phrase such as "one of move, keep".
    #[must_use]
    pub fn takes(self) -> String {
        match self {
            Self::Reminders => format!(
                "any of {}, separated by commas, or none",
                REMINDERS.join(", ")
            ),
            _ if self.choices().is_some() => {
                format!("one of {}", self.choices().unwrap_or_default().join(", "))
            }
            Self::LibrarianConfidence => "a number from 0.5 to 1, such as 0.9".into(),
            Self::CheckoutDestination => {
                "desktop, or a folder's full path (~ for the home folder)".into()
            }
            _ => "a whole number, at least 1".into(),
        }
    }

    /// Whether it takes several of its [`choices`](Self::choices) at once, separated by
    /// commas, rather than one.
    #[must_use]
    pub fn takes_several(self) -> bool {
        self == Self::Reminders
    }

    /// The values it can take, when they're a fixed list.
    #[must_use]
    pub fn choices(self) -> Option<&'static [&'static str]> {
        match self {
            Self::Originals => Some(&["move", "keep"]),
            Self::OutsideEdits => Some(&["flag", "reconcile", "drift"]),
            Self::CompressedFiles => Some(&["expand", "keep", "ask"]),
            Self::EmbeddingModel => Some(&EMBEDDING_MODELS),
            Self::LibrarianChanges => Some(&["off", "on"]),
            Self::CheckoutAfterReturn => Some(&["ask", "trash", "keep"]),
            Self::Reminders => Some(&REMINDERS),
            Self::CrowdedItems
            | Self::SkipDays
            | Self::LibrarianConfidence
            | Self::CheckoutDestination
            | Self::IdleMinutes => None,
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
            Self::LibrarianChanges => if settings.librarian.changes {
                "on"
            } else {
                "off"
            }
            .into(),
            Self::LibrarianConfidence => settings.librarian.confidence.to_string(),
            Self::CheckoutDestination => settings.checkout.destination.clone(),
            Self::CheckoutAfterReturn => settings.checkout.after_return.clone(),
            Self::IdleMinutes => settings.sessions.idle_minutes.to_string(),
            Self::Reminders if settings.reminders.is_empty() => "none".into(),
            Self::Reminders => settings.reminders.join(", "),
        }
    }

    /// Sets it in `settings` to `value`.
    ///
    /// # Errors
    ///
    /// A message saying what values it takes, if `value` isn't one.
    pub fn set(self, settings: &mut Settings, value: &str) -> Result<(), String> {
        let value = value.trim();
        let wrong = || format!("{} is {}", self.as_str(), self.takes());
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
            Self::LibrarianChanges => {
                settings.librarian.changes = match value {
                    "on" => true,
                    "off" => false,
                    _ => return Err(wrong()),
                };
            }
            Self::LibrarianConfidence => {
                settings.librarian.confidence = value
                    .parse::<f64>()
                    .ok()
                    .filter(|n| (0.5..=1.0).contains(n))
                    .ok_or_else(wrong)?;
            }
            Self::CheckoutDestination => {
                settings.checkout.destination = checkout_destination(value).ok_or_else(wrong)?;
            }
            Self::CheckoutAfterReturn => {
                if !self.choices().unwrap_or_default().contains(&value) {
                    return Err(wrong());
                }
                settings.checkout.after_return = value.to_owned();
            }
            Self::IdleMinutes => {
                settings.sessions.idle_minutes =
                    value.parse().ok().filter(|n| *n >= 1).ok_or_else(wrong)?;
            }
            Self::Reminders => settings.reminders = reminders(value).ok_or_else(wrong)?,
        }
        Ok(())
    }
}

/// `value` as `checkout.destination` stores it: `desktop`, or a folder's full path, which may
/// start with `~` for the home folder. `None` for anything else.
fn checkout_destination(value: &str) -> Option<String> {
    if value.eq_ignore_ascii_case(DESKTOP) {
        return Some(DESKTOP.into());
    }
    let from_home = value == "~" || value.starts_with("~/");
    (from_home || Path::new(value).is_absolute()).then(|| value.to_owned())
}

/// The reminders `value` turns on, in the order FORMAT.md writes them: names separated by
/// commas or spaces, or `none`. `None` if one isn't a reminder.
fn reminders(value: &str) -> Option<Vec<String>> {
    let named: Vec<&str> = value
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|n| !n.is_empty() && *n != "none")
        .collect();
    if named.iter().any(|n| !REMINDERS.contains(n)) {
        return None;
    }
    Some(
        REMINDERS
            .iter()
            .filter(|r| named.contains(r))
            .map(|r| (*r).to_owned())
            .collect(),
    )
}

/// The folder items are checked out to when no folder is given, by `settings`'
/// `checkout.destination`: the Desktop in `home`, or the folder it names, with `~` for `home`.
/// `None` when it needs `home` and there's none.
#[must_use]
pub fn checkout_folder(settings: &Settings, home: Option<&Path>) -> Option<PathBuf> {
    let destination = settings.checkout.destination.trim();
    if destination.eq_ignore_ascii_case(DESKTOP) {
        return home.map(|h| h.join("Desktop"));
    }
    match destination.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            home.map(|h| h.join(rest.trim_start_matches('/')))
        }
        _ => Some(PathBuf::from(destination)),
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

        assert_eq!(
            Key::LibrarianChanges.get(&settings),
            "off",
            "off by default"
        );
        Key::LibrarianChanges.set(&mut settings, "on").unwrap();
        assert!(settings.librarian.changes);
        assert!(Key::LibrarianChanges.set(&mut settings, "yes").is_err());
        assert_eq!(Key::LibrarianConfidence.get(&settings), "0.9");
        Key::LibrarianConfidence.set(&mut settings, "0.75").unwrap();
        assert!(Key::LibrarianConfidence.set(&mut settings, "0.2").is_err());
        assert!(Key::LibrarianConfidence.set(&mut settings, "sure").is_err());
    }

    #[test]
    fn checkout_session_and_reminder_settings_take_only_their_values() {
        let mut settings = Settings::default();
        assert_eq!(Key::CheckoutDestination.get(&settings), "desktop");
        Key::CheckoutDestination
            .set(&mut settings, "~/Checked out")
            .unwrap();
        assert_eq!(settings.checkout.destination, "~/Checked out");
        let absolute = if cfg!(windows) { r"C:\Work" } else { "/work" };
        Key::CheckoutDestination
            .set(&mut settings, absolute)
            .unwrap();
        let relative = Key::CheckoutDestination
            .set(&mut settings, "work")
            .unwrap_err();
        assert!(relative.contains("full path"), "{relative}");
        Key::CheckoutDestination
            .set(&mut settings, "Desktop")
            .unwrap();
        assert_eq!(Key::CheckoutDestination.get(&settings), "desktop");

        assert_eq!(Key::CheckoutAfterReturn.get(&settings), "ask");
        Key::CheckoutAfterReturn
            .set(&mut settings, "trash")
            .unwrap();
        assert_eq!(settings.checkout.after_return, "trash");
        assert!(
            Key::CheckoutAfterReturn
                .set(&mut settings, "delete")
                .unwrap_err()
                .contains("ask, trash, keep")
        );

        assert_eq!(Key::IdleMinutes.get(&settings), "45");
        Key::IdleMinutes.set(&mut settings, "20").unwrap();
        assert_eq!(settings.sessions.idle_minutes, 20);
        assert!(Key::IdleMinutes.set(&mut settings, "0").is_err());
        assert!(Key::IdleMinutes.set(&mut settings, "soon").is_err());

        assert_eq!(Key::Reminders.get(&settings), "idle, weekly, menu-bar");
        Key::Reminders
            .set(&mut settings, "due-date, idle idle")
            .unwrap();
        assert_eq!(settings.reminders, ["idle", "due-date"]);
        Key::Reminders.set(&mut settings, "none").unwrap();
        assert!(settings.reminders.is_empty());
        assert_eq!(Key::Reminders.get(&settings), "none");
        let unknown = Key::Reminders.set(&mut settings, "hourly").unwrap_err();
        assert!(unknown.contains("menu-bar, due-date"), "{unknown}");
        assert!(Key::Reminders.takes_several());

        for key in Key::ALL {
            assert_eq!(Key::parse(key.as_str()), Some(key));
            assert!(!key.title().is_empty() && !key.describe().is_empty());
        }
    }

    #[test]
    fn checkouts_go_to_the_desktop_or_the_folder_chosen() {
        let home = Path::new("/home/owner");
        let mut settings = Settings::default();
        assert_eq!(
            checkout_folder(&settings, Some(home)),
            Some(home.join("Desktop"))
        );
        assert_eq!(checkout_folder(&settings, None), None);
        settings.checkout.destination = "~/Out".into();
        assert_eq!(
            checkout_folder(&settings, Some(home)),
            Some(home.join("Out"))
        );
        settings.checkout.destination = "/elsewhere".into();
        assert_eq!(
            checkout_folder(&settings, None),
            Some(PathBuf::from("/elsewhere"))
        );
    }
}
