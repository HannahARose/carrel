//! Switching an archive's embedding model (`carrel index model`, `docs/design/search.md`).
//!
//! The archive records the model it's switching to in its settings (`embedding.switching_to`,
//! FORMAT.md §7.2), so every device sharing it sees the switch. The new model's index is built
//! in its own folder beside the current one, while search and the worker keep using the
//! current one. Once it's complete, [`finish`] makes it the archive's model and removes the
//! old model's folder. Stopped part way, the switch carries on from what was saved when it's
//! started again.

use std::path::PathBuf;

use crate::archive::{Archive, ArchiveError};
use crate::coordinator::WriterLease;
use crate::index::{Index, IndexError};
use crate::settings::{SettingsError, save};

/// Why a switch couldn't be started, stopped or finished.
#[derive(Debug)]
pub enum SwitchError {
    /// The settings couldn't be written or recorded.
    Settings(SettingsError),
    /// The archive couldn't be read again.
    Archive(ArchiveError),
    /// The new model's index couldn't be read or removed.
    Index(IndexError),
    /// The archive isn't switching to this model (any more): another device chose another,
    /// or stopped the switch.
    NotSwitching {
        /// The model that was to be finished.
        model: String,
        /// The model it's switching to now, if any.
        now: Option<String>,
    },
    /// The new model's index isn't complete yet.
    Incomplete(String),
}

impl std::fmt::Display for SwitchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Settings(e) => e.fmt(f),
            Self::Archive(e) => e.fmt(f),
            Self::Index(e) => e.fmt(f),
            Self::NotSwitching {
                model,
                now: Some(now),
            } => write!(
                f,
                "the archive is switching to {now} now, not {model}; another device changed it"
            ),
            Self::NotSwitching { model, now: None } => write!(
                f,
                "the archive isn't switching to {model} any more; another device stopped it"
            ),
            Self::Incomplete(model) => write!(f, "{model}'s index isn't complete yet"),
        }
    }
}

impl std::error::Error for SwitchError {}

impl From<SettingsError> for SwitchError {
    fn from(e: SettingsError) -> Self {
        Self::Settings(e)
    }
}

impl From<IndexError> for SwitchError {
    fn from(e: IndexError) -> Self {
        Self::Index(e)
    }
}

impl From<ArchiveError> for SwitchError {
    fn from(e: ArchiveError) -> Self {
        Self::Archive(e)
    }
}

/// A switch of embedding model under way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switch {
    /// The archive's model until the switch finishes, if it has one.
    pub from: Option<String>,
    /// The model it's switching to.
    pub to: String,
}

/// The switch `archive` is part way through, if any.
#[must_use]
pub fn pending(archive: &Archive) -> Option<Switch> {
    let settings = archive.settings();
    let to = settings.embedding_switch()?;
    Some(Switch {
        from: settings.embedding_model().map(str::to_owned),
        to: to.to_owned(),
    })
}

/// Removes `model`'s index of `archive`, if there is one.
///
/// # Errors
///
/// An error removing the folder.
pub fn remove_index(archive: &Archive, model: &str) -> Result<(), IndexError> {
    let dir = Index::dir_of(archive, model);
    match std::fs::remove_dir_all(&dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(IndexError::Io {
            path: dir,
            source: e,
        }),
        _ => Ok(()),
    }
}

/// How [`begin`] left things.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Begun {
    /// Whether the archive was already switching to the model, so this carries on.
    pub resumed: bool,
    /// The model the archive was switching to instead, whose index was removed.
    pub replaced: Option<String>,
    /// The history commit recording the switch, if one was made.
    pub commit: Option<String>,
}

/// Records, holding `lease`, that `archive` is switching to `model`, so every device sees it,
/// and returns the index to build for it, marked as being built. A switch to another model
/// that hadn't finished is given up and its index removed. Already switching to `model`, it
/// carries on from what was saved.
///
/// # Errors
///
/// [`SwitchError`] if the settings or the index can't be read or written.
pub fn begin(
    archive: &Archive,
    model: &str,
    lease: &WriterLease,
) -> Result<(Index, Begun), SwitchError> {
    let previous = archive.settings().embedding_switch().map(str::to_owned);
    let resumed = previous.as_deref() == Some(model);
    let mut commit = None;
    if !resumed {
        let mut settings = archive.settings().clone();
        settings.set_embedding_switch(Some(model));
        let summary = match archive.settings().embedding_model() {
            Some(from) => format!("Start switching the embedding model from {from} to {model}"),
            None => format!("Start embedding with {model}"),
        };
        commit = save(archive, &settings, &summary, lease)?;
    }
    let replaced = previous.filter(|p| p != model);
    if let Some(replaced) = &replaced {
        remove_index(archive, replaced)?;
    }
    let mut index = Index::open_or_new(archive, model)?;
    index.start_building();
    index.save()?;
    Ok((
        index,
        Begun {
            resumed,
            replaced,
            commit,
        },
    ))
}

/// Stops the switch `archive` is part way through, holding `lease`, and removes the new
/// model's index. Returns the model it was switching to, if it was switching.
///
/// # Errors
///
/// [`SwitchError`] if the settings can't be written or the index can't be removed.
pub fn cancel(archive: &Archive, lease: &WriterLease) -> Result<Option<String>, SwitchError> {
    let Some(switch) = pending(archive) else {
        return Ok(None);
    };
    let mut settings = archive.settings().clone();
    settings.set_embedding_switch(None);
    save(
        archive,
        &settings,
        &format!("Stop switching the embedding model to {}", switch.to),
        lease,
    )?;
    if switch.from.as_deref() != Some(switch.to.as_str()) {
        remove_index(archive, &switch.to)?;
    }
    Ok(Some(switch.to))
}

/// How [`finish`] left things.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    /// The model the archive used before, whose index was removed.
    pub from: Option<String>,
    /// The folder the old index was in, now removed.
    pub removed: Option<PathBuf>,
    /// The history commit recording the switch.
    pub commit: Option<String>,
}

/// Makes `model`, whose index is complete, `archive`'s embedding model, holding `lease`, then
/// removes the old model's index. Search and the worker use the new one from then on.
///
/// # Errors
///
/// [`SwitchError::NotSwitching`] if the archive isn't switching to `model` any more,
/// [`SwitchError::Incomplete`] if its index isn't complete, or an error writing the settings
/// or removing the old index.
pub fn finish(
    archive: &Archive,
    model: &str,
    lease: &WriterLease,
) -> Result<Finished, SwitchError> {
    // Settings may have changed on another device since the switch began.
    let archive = Archive::open(archive.root())?;
    let now = archive.settings().embedding_switch().map(str::to_owned);
    if now.as_deref() != Some(model) {
        return Err(SwitchError::NotSwitching {
            model: model.to_owned(),
            now,
        });
    }
    if Index::open(&archive, model)?.is_none_or(|i| i.rebuilding()) {
        return Err(SwitchError::Incomplete(model.to_owned()));
    }
    let from = archive.settings().embedding_model().map(str::to_owned);
    let mut settings = archive.settings().clone();
    settings.set_embedding_model(model);
    settings.set_embedding_switch(None);
    let summary = match &from {
        Some(from) => format!("Switch the embedding model from {from} to {model}"),
        None => format!("Embed with {model}"),
    };
    let commit = save(&archive, &settings, &summary, lease)?;
    let removed = match &from {
        Some(from) if from != model => {
            remove_index(&archive, from)?;
            Some(Index::dir_of(&archive, from))
        }
        _ => None,
    };
    Ok(Finished {
        from,
        removed,
        commit,
    })
}
