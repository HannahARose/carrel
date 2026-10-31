//! Switching archives in the interactive mode: the recent ones, one by its path, as
//! `carrel open` does, or a new one, as `carrel init` does. `Ctrl-O` opens the switcher on
//! any screen, and so does the Archive row in Settings.

use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;

use super::app::{App, Effect, Preferences};
use super::review::{Picker, Purpose};
use super::screens::{Input, InputPurpose};
use super::settings::{Row, Shown};
use crate::Failure;

/// A choice in the archive switcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Open the archive at this root.
    Open(PathBuf),
    /// Type the path of an archive to open.
    Path,
    /// Type the path of a new archive to create.
    Create,
}

impl App {
    /// The archives to offer, other than this one, with their names: the one chosen with
    /// `carrel open`, then those opened or created lately.
    #[must_use]
    pub fn other_archives(&self) -> Vec<(String, PathBuf)> {
        let here = std::fs::canonicalize(self.archive.root()).ok();
        self.known_archives
            .iter()
            .filter(|root| here.as_ref() != Some(*root))
            .filter_map(|root| Some((Archive::open(root).ok()?.doc().name.clone(), root.clone())))
            .collect()
    }

    /// The choices in the archive switcher, each with its words.
    #[must_use]
    pub fn archive_choices(&self) -> Vec<(String, Choice)> {
        let mut choices: Vec<(String, Choice)> = self
            .other_archives()
            .into_iter()
            .map(|(name, root)| (format!("{name}  {}", root.display()), Choice::Open(root)))
            .collect();
        choices.push(("Open an archive by its path…".into(), Choice::Path));
        choices.push(("Create a new archive…".into(), Choice::Create));
        choices
    }

    /// Opens the archive switcher.
    pub(super) fn archive_picker(&mut self) {
        let choices = self.archive_choices();
        self.picker = Some(Picker {
            collections: choices.iter().map(|(words, _)| words.clone()).collect(),
            filter: String::new(),
            chosen: 0,
            purpose: Purpose::Archive(choices),
        });
    }

    /// What choosing `words` in the archive switcher does.
    pub(super) fn archive_chosen(
        &mut self,
        choices: &[(String, Choice)],
        words: &str,
    ) -> Option<Effect> {
        match choices.iter().find(|(w, _)| w == words).map(|(_, c)| c)? {
            Choice::Open(root) => return Some(Effect::OpenArchive(root.clone())),
            Choice::Path => {
                self.input = Some(Input {
                    purpose: InputPurpose::OpenArchive,
                    text: String::new(),
                });
            }
            Choice::Create => {
                self.input = Some(Input {
                    purpose: InputPurpose::CreateArchive,
                    text: String::new(),
                });
            }
        }
        None
    }

    /// The Settings row for the archive shown, which opens the switcher.
    pub(super) fn archive_row(&self) -> Shown {
        let mut detail = vec![
            format!("{}", self.archive.root().display()),
            String::new(),
            "Enter, or Ctrl-O on any screen, to open another archive or create one. Commands \
             outside an archive then use it too, as after `carrel open`."
                .into(),
        ];
        let others = self.other_archives();
        if !others.is_empty() {
            detail.push(String::new());
            detail.push("Recent archives:".into());
            detail.extend(
                others
                    .iter()
                    .map(|(name, root)| format!("  {name}  {}", root.display())),
            );
        }
        Shown {
            row: Row::Archive,
            label: "Archive".into(),
            value: self.archive.doc().name.clone(),
            detail,
        }
    }

    /// Shows the archive at `path` instead of this one, as `carrel open` does: commands
    /// outside an archive use it too, unless the preferences are only kept in memory.
    ///
    /// # Errors
    ///
    /// [`Failure`] if `path` isn't in an archive or it can't be read; this one stays shown.
    pub fn switch_archive(&mut self, path: &Path) -> Result<String, Failure> {
        let (archive, root) = crate::commands::open_archive(path)?;
        let message = match self.preferences {
            Preferences::Saved => {
                let report = crate::commands::open(Some(&root))?;
                report.text.trim().to_owned()
            }
            Preferences::InMemory => format!("Opened “{}”", archive.doc().name),
        };
        let mut app = Self::new(archive)?;
        app.preferences = self.preferences;
        if self.preferences == Preferences::InMemory {
            // Kept as `carrel open` would keep them, without writing them.
            let mut known = self.known_archives.clone();
            known.retain(|r| *r != root);
            known.insert(0, root);
            app.known_archives = known;
        }
        app.here = self.here.take();
        app.tab = self.tab;
        app.reload()?;
        *self = app;
        Ok(message)
    }

    /// Creates an archive at `path`, as `carrel init PATH` does, and shows it.
    ///
    /// # Errors
    ///
    /// [`Failure`] if it can't be created; this one stays shown.
    pub fn create_archive(&mut self, path: &Path) -> Result<String, Failure> {
        let report = crate::commands::init(
            Some(path),
            None,
            false,
            carrel_core::models::DEFAULT_EMBEDDING_MODEL,
        )?;
        let switched = self.switch_archive(path)?;
        let created = report.text.lines().next().unwrap_or_default().to_owned();
        Ok(format!("{created}; {switched}"))
    }
}

/// The archive path typed into `text`: the first path, with quotes from dragging and `~`
/// handled as for ingest.
pub(super) fn typed_path(text: &str) -> Option<PathBuf> {
    crate::paths::split_paths(text).into_iter().next()
}
