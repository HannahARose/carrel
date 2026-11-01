//! The Backups screen: whether backups are set up, the snapshots taken with each change, and
//! taking, checking and restoring from them, the way `carrel backup` does.
//!
//! The list comes from history, which records each snapshot with its change, so the screen
//! shows without the backup drive; taking, verifying and restoring need it.

use carrel_core::backup::snapshot_entries;
use carrel_core::history::Entry;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::{App, Effect, step};
use super::review::Handled;
use super::screens::{Input, InputPurpose};

/// How much of the files' contents `v` reads back, as `carrel backup verify` does by default.
pub const VERIFY_PERCENT: u8 = 1;

/// A password, kept out of debug output and logs.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(pub String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(…)")
    }
}

impl App {
    /// The history entries that took a snapshot, newest first.
    pub fn snapshots(&self) -> Vec<&Entry> {
        snapshot_entries(&self.history).collect()
    }

    /// Whether this archive has a backup repository set up.
    pub fn backups_set_up(&self) -> bool {
        self.archive.settings().backups.is_some()
    }

    /// Moves the choice among snapshots.
    pub(super) fn backups_move(&mut self, delta: i32) {
        self.backup_chosen = step(self.backup_chosen, delta, self.snapshots().len());
    }

    /// Handles a key on the Backups screen.
    pub(super) fn backups_key(&mut self, key: KeyEvent) -> Handled {
        let set_up = self.backups_set_up();
        Handled::Yes(match key.code {
            KeyCode::Char('s') => {
                self.picking = super::app::Picking::BackupFolder;
                self.message = Some(
                    "Choose the folder for the backups: an empty one for new backups, or an existing repository".into(),
                );
                self.start_picker()
            }
            KeyCode::Char('n' | 'v' | 'r') if !set_up => {
                self.message = Some("Backups aren't set up; press s to set them up".into());
                None
            }
            KeyCode::Char('n') => Some(Effect::BackupNow),
            KeyCode::Char('v') => Some(Effect::BackupVerify),
            KeyCode::Char('r') => {
                match self
                    .snapshots()
                    .get(self.backup_chosen)
                    .map(|e| e.id.clone())
                {
                    Some(at) => {
                        self.input = Some(Input {
                            purpose: InputPurpose::RestoreItem { at },
                            text: String::new(),
                        });
                    }
                    None => self.message = Some("No snapshot to restore from yet".into()),
                }
                None
            }
            _ => return Handled::No,
        })
    }

    /// Opens the line to type or paste the backup folder into.
    pub(super) fn ask_for_backup_folder(&mut self) {
        self.input = Some(Input {
            purpose: InputPurpose::BackupRepository,
            text: String::new(),
        });
    }

    /// Goes on to the password for the folder chosen, typed or picked: the first of `paths`,
    /// or the folder a picked file is in.
    pub(super) fn backup_folder_chosen(&mut self, paths: &[std::path::PathBuf]) {
        let Some(first) = paths.first() else {
            self.message = Some("No folder chosen; nothing set up".into());
            return;
        };
        let folder = if first.is_file() {
            first
                .parent()
                .map_or_else(|| first.clone(), std::path::Path::to_owned)
        } else {
            first.clone()
        };
        let new = !crate::backup::exists(&folder);
        self.input = Some(Input {
            purpose: InputPurpose::BackupPassword {
                repository: folder,
                new,
                first: None,
            },
            text: String::new(),
        });
    }

    /// What happens when a line typed for backups is entered: the repository, then the
    /// password (twice for a new repository), then setting up; or the item to restore.
    pub(super) fn backup_input(&mut self, purpose: InputPurpose, text: String) -> Option<Effect> {
        match purpose {
            InputPurpose::BackupRepository => {
                self.backup_folder_chosen(&crate::paths::split_paths(&text));
                None
            }
            InputPurpose::BackupPassword {
                repository,
                new,
                first,
            } => {
                if text.is_empty() {
                    self.message = Some("The password can't be empty; nothing set up".into());
                    return None;
                }
                match first {
                    None if new => {
                        self.input = Some(Input {
                            purpose: InputPurpose::BackupPassword {
                                repository,
                                new,
                                first: Some(Secret(text)),
                            },
                            text: String::new(),
                        });
                        None
                    }
                    Some(Secret(first)) if first != text => {
                        self.message = Some("The passwords don't match; nothing was set up".into());
                        None
                    }
                    _ => Some(Effect::BackupSetup {
                        repository,
                        password: Secret(text),
                    }),
                }
            }
            InputPurpose::RestoreItem { at } if !text.is_empty() => {
                Some(Effect::BackupRestore { item: text, at })
            }
            InputPurpose::RestoreItem { .. } => {
                self.message = Some("No item given; nothing restored".into());
                None
            }
            _ => None,
        }
    }
}
